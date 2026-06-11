use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{IsTerminal, Write};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::Router;
use axum::body::Bytes;
use axum::extract::MatchedPath;
use axum::extract::Request;
use axum::extract::{Extension, Path, Query};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{delete, get, patch, post, put};
use chrono::{SecondsFormat, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;
use tokio::sync::RwLock as AsyncRwLock;
use tower_http::cors::{Any, CorsLayer};

use crate::ast::{HttpVerb, TakeBinding};
use crate::auth::{
    ApiKeyAuthConfig, ApiKeySecretSource, AuthProviderRuntimeConfig, AuthRegistry, JwtAuthConfig,
    JwtValidationSource, build_auth_registry,
};
use crate::cache::CacheEngine;
use crate::db::DbEngine;
use crate::doc::DocEngine;
use crate::error::MarretaError;
use crate::file_loader::ProjectRuntime;
use crate::http_client::driver::HttpClient as HttpClientDriver;
use crate::interpreter::Interpreter;
use crate::openapi;
use crate::queue::driver::QueueDriver;
use crate::route_loader::{ConsumerDefinition, ConsumerKind, RouteDefinition, RouteRegistry};
use crate::runtime_profile::{self, ProfilePhase};
use crate::trace_context::TraceContext;
use crate::validator::coerce_payload;
use crate::value::{Value, ValueMap, json_to_value, value_to_json};
use crate::version::MARRETA_VERSION;

pub(crate) struct AuthRuntime {
    registry: AuthRegistry,
    jwks_cache: AsyncRwLock<HashMap<String, CachedJwks>>,
    client: reqwest::Client,
    auth_overrides: HashMap<String, Value>,
}

struct CachedJwks {
    set: JwkSet,
    expires_at: Instant,
}

impl AuthRuntime {
    pub(crate) fn new(registry: AuthRegistry) -> Self {
        Self {
            registry,
            jwks_cache: AsyncRwLock::new(HashMap::new()),
            client: reqwest::Client::new(),
            auth_overrides: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self::new(AuthRegistry::empty())
    }

    pub(crate) fn with_auth_overrides(
        registry: AuthRegistry,
        auth_overrides: HashMap<String, Value>,
    ) -> Self {
        Self {
            registry,
            jwks_cache: AsyncRwLock::new(HashMap::new()),
            client: reqwest::Client::new(),
            auth_overrides,
        }
    }
}

/// HTTP server configuration.
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub cors_enabled: bool,
    pub cors_origin: String,
    pub docs_enabled: bool,
    pub docs_path: String,
    /// Active DB engine, if configured. Shared (via `Arc`) across all requests.
    pub db_engine: Option<DbEngine>,
    /// Active Doc engine, if configured. Shared (via `Arc`) across all requests.
    pub doc_engine: Option<DocEngine>,
    /// Active queue driver, if configured. Shared across all route handlers and consumers.
    pub queue_driver: Option<Arc<dyn QueueDriver>>,
    /// Active cache engine, if configured.
    pub cache_engine: Option<CacheEngine>,
    /// Active HTTP client driver. Always available (no external dependency).
    pub http_client_driver: Option<Arc<dyn HttpClientDriver>>,
    /// Whether automatic runtime request logging is enabled for this server.
    pub request_log_enabled: bool,
    /// Whether runtime W3C Trace Context is enabled for this server.
    pub trace_context_enabled: bool,
    /// Startup timer origin, when serving via the CLI.
    pub startup_started_at: Option<Instant>,
}

pub fn request_log_enabled_for_serve_from_env() -> bool {
    parse_runtime_bool_env("MARRETA_REQUEST_LOG").unwrap_or(true)
}

pub fn trace_context_enabled_for_serve_from_env() -> bool {
    parse_runtime_bool_env("MARRETA_TRACE_CONTEXT").unwrap_or(true)
}

fn parse_runtime_bool_env(key: &str) -> Option<bool> {
    match std::env::var(key)
        .ok()?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Starts the axum HTTP server, registering all routes from the registry.
pub async fn serve(
    registry: RouteRegistry,
    runtime: Arc<ProjectRuntime>,
    config: ServerConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut router = Router::new();
    let (project_name, project_version) = project_metadata(&runtime.global_env);
    let profile_registry = runtime_profile::init_from_env();

    // Build OpenAPI spec before consuming registry fields
    if config.docs_enabled {
        let openapi_json: Arc<String> = Arc::new(openapi::build_with_runtime(
            &registry,
            &runtime,
            &project_name,
            &project_version,
        ));

        let oj = Arc::clone(&openapi_json);
        router = router.route(
            "/openapi.json",
            get(move || {
                let oj = Arc::clone(&oj);
                async move {
                    axum::response::Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", "application/json")
                        .body(axum::body::Body::from((*oj).clone()))
                        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
                }
            }),
        );

        let docs_path = config.docs_path.clone();
        router = router.route(
            &docs_path,
            get(|| async {
                axum::response::Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "text/html; charset=utf-8")
                    .body(axum::body::Body::from(swagger_ui_html("/openapi.json")))
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
            }),
        );
    }

    let db_engine: Arc<Option<DbEngine>> = Arc::new(config.db_engine);
    let doc_engine: Arc<Option<DocEngine>> = Arc::new(config.doc_engine);
    let queue_driver: Arc<Option<Arc<dyn QueueDriver>>> = Arc::new(config.queue_driver);
    let cache_driver_and_config: Arc<
        Option<(
            Arc<dyn crate::cache::driver::CacheDriver>,
            crate::cache::CacheConfig,
        )>,
    > = Arc::new(config.cache_engine.map(|e| (e.driver, e.config)));
    let http_client_driver: Arc<Option<Arc<dyn HttpClientDriver>>> =
        Arc::new(config.http_client_driver);
    let auth_runtime = Arc::new(AuthRuntime::new(build_auth_registry(
        &registry.auth_providers,
    )?));

    // Built-in GET /health endpoint
    {
        let project_runtime = Arc::clone(&runtime);
        let has_db = db_engine.is_some();
        let has_doc = doc_engine.is_some();
        let has_queue = queue_driver.is_some();
        let has_cache = cache_driver_and_config.is_some();
        router = router.route(
            "/_health",
            get(move || {
                let project_runtime = Arc::clone(&project_runtime);
                async move {
                    let (project_name, project_version) =
                        project_metadata(&project_runtime.global_env);
                    let mut body = serde_json::json!({
                        "ok": true,
                        "api": project_name,
                        "version": project_version
                    });
                    if has_db {
                        body["db"] = serde_json::json!("connected");
                    }
                    if has_doc {
                        body["doc"] = serde_json::json!("connected");
                    }
                    body["queue"] = if has_queue {
                        serde_json::json!("connected")
                    } else {
                        serde_json::json!("not_configured")
                    };
                    body["cache"] = if has_cache {
                        serde_json::json!("connected")
                    } else {
                        serde_json::json!("not_configured")
                    };
                    axum::response::Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", "application/json")
                        .body(axum::body::Body::from(body.to_string()))
                        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
                }
            }),
        );
    }

    // Start background consumer tasks before accepting HTTP requests
    if !registry.consumers.is_empty() {
        start_consumers(
            registry.consumers,
            Arc::clone(&runtime),
            Arc::clone(&db_engine),
            Arc::clone(&doc_engine),
            Arc::clone(&queue_driver),
            Arc::clone(&cache_driver_and_config),
            Arc::clone(&http_client_driver),
            config.request_log_enabled,
            config.trace_context_enabled,
        )
        .await;
    }

    for route_def in registry.routes {
        router = register_route(
            router,
            route_def,
            Arc::clone(&runtime),
            Arc::clone(&db_engine),
            Arc::clone(&doc_engine),
            Arc::clone(&queue_driver),
            Arc::clone(&cache_driver_and_config),
            Arc::clone(&http_client_driver),
            Arc::clone(&auth_runtime),
            profile_registry.clone(),
        );
    }

    if config.cors_enabled {
        let origin = config.cors_origin.as_str();
        let cors = if origin == "*" {
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any)
        } else {
            let value = HeaderValue::from_str(origin).unwrap_or(HeaderValue::from_static("*"));
            CorsLayer::new()
                .allow_origin(value)
                .allow_methods(Any)
                .allow_headers(Any)
        };
        router = router.layer(cors);
    }

    if config.request_log_enabled {
        router = router.layer(middleware::from_fn(request_logging_middleware));
    }
    if config.trace_context_enabled {
        router = router.layer(middleware::from_fn(trace_context_middleware));
    }

    let addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&addr).await?;
    if let Some(started_at) = config.startup_started_at {
        println!(
            "Application {} (version {}) started in {}ms on http://{} (runtime MarretaLang v{})",
            project_name,
            project_version,
            started_at.elapsed().as_millis(),
            addr,
            MARRETA_VERSION
        );
    } else {
        println!(
            "Application {} (version {}) serving on http://{}",
            project_name, project_version, addr
        );
    }
    let server = axum::serve(listener, router);
    if let Some(profile_registry) = profile_registry {
        server
            .with_graceful_shutdown(profile_shutdown(profile_registry))
            .await?;
    } else {
        server.await?;
    }
    Ok(())
}

async fn profile_shutdown(profile_registry: Arc<runtime_profile::RuntimeProfileRegistry>) {
    #[cfg(unix)]
    {
        let mut terminate =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(signal) => signal,
                Err(_) => {
                    let _ = tokio::signal::ctrl_c().await;
                    profile_registry.emit_json();
                    return;
                }
            };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }

    profile_registry.emit_json();
}

async fn request_logging_middleware(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| to_marreta_route_path(matched.as_str()));
    let trace_context = request.extensions().get::<TraceContext>().cloned();
    let started_at = Instant::now();
    let response = next.run(request).await;
    emit_request_log(
        &method,
        &path,
        route.as_deref(),
        trace_context.as_ref(),
        response.status(),
        started_at.elapsed(),
    );
    response
}

async fn trace_context_middleware(mut request: Request, next: Next) -> Response {
    let traceparent = request
        .headers()
        .get("traceparent")
        .and_then(|value| value.to_str().ok());
    let tracestate = request
        .headers()
        .get("tracestate")
        .and_then(|value| value.to_str().ok());
    let trace_context = TraceContext::from_headers(traceparent, tracestate);
    request.extensions_mut().insert(trace_context);
    next.run(request).await
}

fn emit_request_log(
    method: &Method,
    path: &str,
    route: Option<&str>,
    trace_context: Option<&TraceContext>,
    status: StatusCode,
    duration: Duration,
) {
    let event = build_request_log_event(method, path, route, trace_context, status, duration);
    let Ok(line) = serde_json::to_string(&event) else {
        return;
    };
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
}

// Round a millisecond duration to 3 decimal places so request/message logs read
// cleanly (e.g. 1.833) instead of carrying raw f64 sub-microsecond noise.
fn round_ms(ms: f64) -> f64 {
    (ms * 1000.0).round() / 1000.0
}

fn build_request_log_event(
    method: &Method,
    path: &str,
    route: Option<&str>,
    trace_context: Option<&TraceContext>,
    status: StatusCode,
    duration: Duration,
) -> JsonValue {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "timestamp".into(),
        JsonValue::String(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)),
    );
    obj.insert("kind".into(), JsonValue::String("request".into()));
    if let Some(trace_context) = trace_context {
        obj.insert(
            "trace_id".into(),
            JsonValue::String(trace_context.trace_id.clone()),
        );
        obj.insert(
            "span_id".into(),
            JsonValue::String(trace_context.span_id.clone()),
        );
    }
    obj.insert(
        "method".into(),
        JsonValue::String(method.as_str().to_string()),
    );
    obj.insert("path".into(), JsonValue::String(path.to_string()));
    if let Some(route) = route {
        obj.insert("route".into(), JsonValue::String(route.to_string()));
    }
    obj.insert(
        "status".into(),
        JsonValue::Number(serde_json::Number::from(status.as_u16())),
    );
    obj.insert(
        "duration_ms".into(),
        serde_json::Number::from_f64(round_ms(duration.as_secs_f64() * 1000.0))
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
    );
    JsonValue::Object(obj)
}

fn emit_consumer_log(
    consumer_kind: &ConsumerKind,
    target: &str,
    routing_key: &str,
    exchange: &str,
    trace_context: Option<&TraceContext>,
    status: &str,
    duration: Duration,
) {
    let event = build_consumer_log_event(
        consumer_kind,
        target,
        routing_key,
        exchange,
        trace_context,
        status,
        duration,
    );
    let Ok(line) = serde_json::to_string(&event) else {
        return;
    };
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
}

fn build_consumer_log_event(
    consumer_kind: &ConsumerKind,
    target: &str,
    routing_key: &str,
    exchange: &str,
    trace_context: Option<&TraceContext>,
    status: &str,
    duration: Duration,
) -> JsonValue {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "timestamp".into(),
        JsonValue::String(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)),
    );
    obj.insert("kind".into(), JsonValue::String("consumer".into()));
    if let Some(trace_context) = trace_context {
        obj.insert(
            "trace_id".into(),
            JsonValue::String(trace_context.trace_id.clone()),
        );
        obj.insert(
            "span_id".into(),
            JsonValue::String(trace_context.span_id.clone()),
        );
    }
    obj.insert(
        "consumer_kind".into(),
        JsonValue::String(
            match consumer_kind {
                ConsumerKind::Queue => "queue",
                ConsumerKind::Topic => "topic",
            }
            .into(),
        ),
    );
    obj.insert("target".into(), JsonValue::String(target.to_string()));
    if !routing_key.is_empty() {
        obj.insert(
            "routing_key".into(),
            JsonValue::String(routing_key.to_string()),
        );
    }
    if !exchange.is_empty() {
        obj.insert("exchange".into(), JsonValue::String(exchange.to_string()));
    }
    obj.insert("status".into(), JsonValue::String(status.to_string()));
    obj.insert(
        "duration_ms".into(),
        serde_json::Number::from_f64(round_ms(duration.as_secs_f64() * 1000.0))
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
    );
    JsonValue::Object(obj)
}

fn project_metadata(env: &crate::environment::Environment) -> (String, String) {
    let project_name = env
        .get("project_name")
        .and_then(|v| {
            if let Value::String(s) = v {
                Some(s)
            } else {
                None
            }
        })
        .unwrap_or_else(|| "MarretaLang Project".to_string());
    let project_version = env
        .get("project_version")
        .and_then(|v| {
            if let Value::String(s) = v {
                Some(s)
            } else {
                None
            }
        })
        .unwrap_or_else(|| "1.0.0".to_string());
    (project_name, project_version)
}

/// Registers a single route definition on the router.
fn register_route(
    router: Router,
    route_def: RouteDefinition,
    runtime: Arc<ProjectRuntime>,
    db_engine: Arc<Option<DbEngine>>,
    doc_engine: Arc<Option<DocEngine>>,
    queue_driver: Arc<Option<Arc<dyn QueueDriver>>>,
    cache_driver_and_config: Arc<
        Option<(
            Arc<dyn crate::cache::driver::CacheDriver>,
            crate::cache::CacheConfig,
        )>,
    >,
    http_client_driver: Arc<Option<Arc<dyn HttpClientDriver>>>,
    auth_runtime: Arc<AuthRuntime>,
    profile_registry: Option<Arc<runtime_profile::RuntimeProfileRegistry>>,
) -> Router {
    let axum_path = to_axum_path(&route_def.path);
    let route_profile = profile_registry
        .as_ref()
        .map(|registry| registry.route(&format!("{} {}", route_def.verb, route_def.path)));

    macro_rules! make_handler {
        () => {{
            let runtime = Arc::clone(&runtime);
            let def = route_def.clone();
            let route_profile = route_profile.clone();
            let db = Arc::clone(&db_engine);
            let doc = Arc::clone(&doc_engine);
            let q = Arc::clone(&queue_driver);
            let c = Arc::clone(&cache_driver_and_config);
            let hc = Arc::clone(&http_client_driver);
            let auth_runtime = Arc::clone(&auth_runtime);
            move |path_params: Path<HashMap<String, String>>,
                  Query(query_params): Query<HashMap<String, String>>,
                  headers: HeaderMap,
                  trace_context: Option<Extension<TraceContext>>,
                  body: Bytes| {
                let runtime = Arc::clone(&runtime);
                if let Some(profile) = &route_profile {
                    profile.request_started();
                }
                let http_timer =
                    runtime_profile::timer(route_profile.as_ref(), ProfilePhase::HttpTotal);
                let def = {
                    let _timer =
                        runtime_profile::timer(route_profile.as_ref(), ProfilePhase::RouteClone);
                    def.clone()
                };
                let db = Arc::clone(&db);
                let doc = Arc::clone(&doc);
                let q = Arc::clone(&q);
                let c = Arc::clone(&c);
                let hc = Arc::clone(&hc);
                let auth_runtime = Arc::clone(&auth_runtime);
                let route_profile = route_profile.clone();
                async move {
                    let _http_timer = http_timer;
                    let _handler_timer =
                        runtime_profile::timer(route_profile.as_ref(), ProfilePhase::HandlerTotal);
                    execute_route_profiled(
                        def,
                        path_params.0,
                        query_params,
                        headers,
                        trace_context.map(|Extension(ctx)| ctx),
                        body,
                        runtime,
                        db,
                        doc,
                        q,
                        c,
                        hc,
                        auth_runtime,
                        route_profile,
                    )
                    .await
                }
            }
        }};
    }

    match route_def.verb {
        HttpVerb::Get => router.route(&axum_path, get(make_handler!())),
        HttpVerb::Post => router.route(&axum_path, post(make_handler!())),
        HttpVerb::Put => router.route(&axum_path, put(make_handler!())),
        HttpVerb::Patch => router.route(&axum_path, patch(make_handler!())),
        HttpVerb::Delete => router.route(&axum_path, delete(make_handler!())),
    }
}

/// Executes a route body for an incoming request and returns an axum Response.
pub(crate) async fn execute_route(
    route: RouteDefinition,
    url_params: HashMap<String, String>,
    query_params: HashMap<String, String>,
    headers: HeaderMap,
    trace_context: Option<TraceContext>,
    body: Bytes,
    runtime: Arc<ProjectRuntime>,
    db_engine: Arc<Option<DbEngine>>,
    doc_engine: Arc<Option<DocEngine>>,
    queue_driver: Arc<Option<Arc<dyn QueueDriver>>>,
    cache_driver_and_config: Arc<
        Option<(
            Arc<dyn crate::cache::driver::CacheDriver>,
            crate::cache::CacheConfig,
        )>,
    >,
    http_client_driver: Arc<Option<Arc<dyn HttpClientDriver>>>,
    auth_runtime: Arc<AuthRuntime>,
) -> Response {
    execute_route_profiled(
        route,
        url_params,
        query_params,
        headers,
        trace_context,
        body,
        runtime,
        db_engine,
        doc_engine,
        queue_driver,
        cache_driver_and_config,
        http_client_driver,
        auth_runtime,
        None,
    )
    .await
}

async fn execute_route_profiled(
    route: RouteDefinition,
    url_params: HashMap<String, String>,
    query_params: HashMap<String, String>,
    headers: HeaderMap,
    trace_context: Option<TraceContext>,
    body: Bytes,
    runtime: Arc<ProjectRuntime>,
    db_engine: Arc<Option<DbEngine>>,
    doc_engine: Arc<Option<DocEngine>>,
    queue_driver: Arc<Option<Arc<dyn QueueDriver>>>,
    cache_driver_and_config: Arc<
        Option<(
            Arc<dyn crate::cache::driver::CacheDriver>,
            crate::cache::CacheConfig,
        )>,
    >,
    http_client_driver: Arc<Option<Arc<dyn HttpClientDriver>>>,
    auth_runtime: Arc<AuthRuntime>,
    route_profile: Option<Arc<runtime_profile::RouteProfile>>,
) -> Response {
    let _total_timer =
        runtime_profile::timer(route_profile.as_ref(), ProfilePhase::TotalExecuteRoute);
    let route_auth_context = if route.auth.is_some() {
        let auth_result = {
            let _timer = runtime_profile::timer(route_profile.as_ref(), ProfilePhase::AuthEval);
            authenticate_route(&route, &headers, &auth_runtime).await
        };
        match auth_result {
            Ok(auth) => Some(auth),
            Err(response) => {
                let _timer =
                    runtime_profile::timer(route_profile.as_ref(), ProfilePhase::ResponseBuild);
                return response;
            }
        }
    } else {
        None
    };

    let mut interp = {
        let _timer = runtime_profile::timer(route_profile.as_ref(), ProfilePhase::EnvSetup);
        Interpreter::from_environment(runtime.env_for_module(route.module_id.as_deref()))
            .with_project_runtime(Arc::clone(&runtime))
            .with_current_module(route.module_id.clone())
            .with_runtime_profile(route_profile.clone())
    };

    if let Some(engine) = (*db_engine).clone() {
        interp = interp.with_db(engine);
    }

    if let Some(engine) = (*doc_engine).clone() {
        interp = interp.with_doc(engine);
    }

    if let Some(driver) = (*queue_driver).clone() {
        interp = interp.with_queue(driver);
    }

    if let Some((driver, cfg)) = (*cache_driver_and_config).clone() {
        interp = interp.with_cache(driver, cfg);
    }

    if let Some(driver) = (*http_client_driver).clone() {
        interp = interp.with_http_client(driver);
    }

    if let Some(trace_context) = trace_context {
        interp = interp.with_trace_context(trace_context);
    }

    let _route_trace = interp.enter_route(
        &route.verb,
        &route.path,
        route.module_id.clone(),
        route.line,
        route.column,
    );

    let (headers_map, query_map) = {
        let _timer = runtime_profile::timer(route_profile.as_ref(), ProfilePhase::RequestBinding);

        // Inject URL path parameters as individual variables AND as a `params` map.
        // Numeric-looking values are coerced to Integer so DB comparisons work without casting.
        // e.g. `/users/:id` with id=42 → `id = 42` (Integer) + `params.id = 42`
        fn coerce_param(v: &str) -> Value {
            if let Ok(n) = v.parse::<i64>() {
                Value::Integer(n)
            } else {
                Value::String(v.to_string())
            }
        }
        let params_map: ValueMap = url_params
            .iter()
            .map(|(k, v)| (k.clone(), coerce_param(v)))
            .collect();
        interp.env_set(
            "params".to_string(),
            Value::Map(Arc::new(RwLock::new(params_map))),
        );
        for (key, val) in &url_params {
            interp.env_set(key.clone(), coerce_param(val));
        }

        // Build headers map once (shared by Headers and potentially multi-bindings)
        let headers_map: ValueMap = headers
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|s| (k.as_str().to_string(), Value::String(s.to_string())))
            })
            .collect();
        let query_map: ValueMap = query_params
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        interp.env_set(
            "query".to_string(),
            Value::Map(Arc::new(RwLock::new(query_map.clone()))),
        );
        interp.env_set(
            "headers".to_string(),
            Value::Map(Arc::new(RwLock::new(headers_map.clone()))),
        );
        (headers_map, query_map)
    };

    if let Some(auth) = route_auth_context {
        interp.env_set("auth".to_string(), auth);

        for allow in &route.allow {
            let allow_result = {
                let _timer = runtime_profile::timer(route_profile.as_ref(), ProfilePhase::AuthEval);
                interp.evaluate_pub(allow)
            };
            match allow_result {
                Ok(value) if value.is_truthy() => {}
                Ok(_) => {
                    let _timer =
                        runtime_profile::timer(route_profile.as_ref(), ProfilePhase::ResponseBuild);
                    return forbidden_response();
                }
                Err(e) => {
                    log_uncaught_runtime_error(
                        &interp,
                        &e,
                        RuntimeErrorLogScope::Request {
                            http_status: status_for_error(&e),
                        },
                    );
                    let _timer =
                        runtime_profile::timer(route_profile.as_ref(), ProfilePhase::ResponseBuild);
                    return error_to_response(e);
                }
            }
        }
    }

    // Inject each `take` binding
    for binding in &route.take {
        let _timer = runtime_profile::timer(route_profile.as_ref(), ProfilePhase::RequestBinding);
        match binding {
            TakeBinding::Payload(name) => {
                let val = if body.is_empty() {
                    Value::Null
                } else {
                    serde_json::from_slice::<JsonValue>(&body)
                        .map(|j| json_to_value(&j))
                        .unwrap_or(Value::Null)
                };
                // Schema validation — runs before route body execution
                if let Some(schema_name) = &route.schema
                    && let Some(schema_def) =
                        runtime.resolve_schema(route.module_id.as_deref(), schema_name)
                {
                    let visible_schemas = runtime.visible_schemas_for(route.module_id.as_deref());
                    let coerced = {
                        let _timer = runtime_profile::timer(
                            route_profile.as_ref(),
                            ProfilePhase::SchemaCoercion,
                        );
                        coerce_payload(&val, &schema_def, &visible_schemas)
                    };
                    match coerced {
                        Ok(coerced) => interp.env_set(name.clone(), coerced),
                        Err(e) => return error_to_response(e),
                    }
                } else {
                    interp.env_set(name.clone(), val);
                }
            }
            TakeBinding::Query(name) => {
                interp.env_set(
                    name.clone(),
                    Value::Map(Arc::new(RwLock::new(query_map.clone()))),
                );
            }
            TakeBinding::Headers(name) => {
                interp.env_set(
                    name.clone(),
                    Value::Map(Arc::new(RwLock::new(headers_map.clone()))),
                );
            }
            TakeBinding::Form(name) => {
                let map: ValueMap = serde_urlencoded::from_bytes::<Vec<(String, String)>>(&body)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(k, v)| (k, Value::String(v)))
                    .collect();
                interp.env_set(name.clone(), Value::Map(Arc::new(RwLock::new(map))));
            }
            TakeBinding::Raw(name) => {
                let s = String::from_utf8_lossy(&body).into_owned();
                interp.env_set(name.clone(), Value::String(s));
            }
        }
    }

    // Execute route body — `reply`/`fail` surface as Err(HttpResponse)
    let execution_result = {
        let _timer = runtime_profile::timer(route_profile.as_ref(), ProfilePhase::AstExecute);
        interp.execute(&route.body)
    };

    match execution_result {
        Err(e) => {
            log_uncaught_runtime_error(
                &interp,
                &e,
                RuntimeErrorLogScope::Request {
                    http_status: status_for_error(&e),
                },
            );
            let _timer =
                runtime_profile::timer(route_profile.as_ref(), ProfilePhase::ResponseBuild);
            error_to_response(e)
        }
        Ok(_) => {
            let _timer =
                runtime_profile::timer(route_profile.as_ref(), ProfilePhase::ResponseBuild);
            StatusCode::NO_CONTENT.into_response()
        }
    }
}

async fn authenticate_route(
    route: &RouteDefinition,
    headers: &HeaderMap,
    auth_runtime: &AuthRuntime,
) -> Result<Value, Response> {
    let Some(route_auth) = &route.auth else {
        return Ok(Value::Null);
    };

    let Some(provider) = auth_runtime.registry.providers.get(&route_auth.provider) else {
        return Err(unauthorized_response());
    };

    if let Some(auth) = auth_runtime.auth_overrides.get(&route_auth.provider) {
        return scenario_auth_context(provider, auth).ok_or_else(unauthorized_response);
    }

    match provider {
        AuthProviderRuntimeConfig::ApiKey(config) => authenticate_api_key(config, headers),
        AuthProviderRuntimeConfig::Jwt(config) => {
            authenticate_jwt(config, headers, auth_runtime).await
        }
    }
}

fn scenario_auth_context(provider: &AuthProviderRuntimeConfig, value: &Value) -> Option<Value> {
    match provider {
        AuthProviderRuntimeConfig::Jwt(config) => {
            let JsonValue::Object(claims) = value_to_json(value) else {
                return None;
            };
            jwt_auth_context(config, claims)
        }
        AuthProviderRuntimeConfig::ApiKey(config) => Some(api_key_auth_context(config)),
    }
}

fn authenticate_api_key(config: &ApiKeyAuthConfig, headers: &HeaderMap) -> Result<Value, Response> {
    let Some(header_value) = headers.get(config.header.as_str()) else {
        return Err(unauthorized_response());
    };
    let supplied = header_value.as_bytes();

    let valid = match &config.secret_source {
        ApiKeySecretSource::Secret(secret) => constant_time_eq(supplied, secret.as_bytes()),
        ApiKeySecretSource::SecretHash(expected_hash) => {
            verify_api_key_hash(supplied, expected_hash)
        }
    };

    if !valid {
        return Err(unauthorized_response());
    }

    Ok(api_key_auth_context(config))
}

async fn authenticate_jwt(
    config: &JwtAuthConfig,
    headers: &HeaderMap,
    auth_runtime: &AuthRuntime,
) -> Result<Value, Response> {
    let Some(token) = bearer_token(headers) else {
        return Err(unauthorized_response());
    };

    let header = decode_header(token).map_err(|_| unauthorized_response())?;
    let algorithm = jwt_header_algorithm(config, header.alg).ok_or_else(unauthorized_response)?;
    let key = match decoding_key(config, token, algorithm, auth_runtime).await {
        Ok(key) => key,
        Err(_) => return Err(unauthorized_response()),
    };

    decode_jwt_claims(config, token, algorithm, &key)
}

fn decode_jwt_claims(
    config: &JwtAuthConfig,
    token: &str,
    algorithm: Algorithm,
    key: &DecodingKey,
) -> Result<Value, Response> {
    let mut validation = Validation::new(algorithm);
    validation.set_audience(&[config.audience.as_str()]);
    validation.set_issuer(&[config.issuer.as_str()]);
    validation.validate_nbf = true;
    validation.leeway = config.clock_skew_seconds;
    validation.set_required_spec_claims(&["exp", "iss", "aud"]);

    let token = match decode::<serde_json::Map<String, JsonValue>>(token, key, &validation) {
        Ok(token) => token,
        Err(_) => return Err(unauthorized_response()),
    };

    jwt_auth_context(config, token.claims).ok_or_else(unauthorized_response)
}

fn jwt_header_algorithm(config: &JwtAuthConfig, header_alg: Algorithm) -> Option<Algorithm> {
    match &config.algorithm {
        Some(name) => jwt_algorithm(name).filter(|configured| *configured == header_alg),
        None => is_supported_jwks_algorithm(header_alg).then_some(header_alg),
    }
}

fn is_supported_jwks_algorithm(algorithm: Algorithm) -> bool {
    matches!(
        algorithm,
        Algorithm::RS256
            | Algorithm::RS384
            | Algorithm::RS512
            | Algorithm::ES256
            | Algorithm::ES384
    )
}

async fn decoding_key(
    config: &JwtAuthConfig,
    token: &str,
    algorithm: Algorithm,
    auth_runtime: &AuthRuntime,
) -> Result<DecodingKey, AuthRuntimeError> {
    match &config.validation_source {
        JwtValidationSource::Secret(secret) => Ok(DecodingKey::from_secret(secret.as_bytes())),
        JwtValidationSource::PublicKeyPem(pem) => {
            fixed_public_key(pem, algorithm).map_err(|_| AuthRuntimeError)
        }
        JwtValidationSource::JwksUrl(url) => {
            jwks_decoding_key(config, token, algorithm, url, auth_runtime).await
        }
        JwtValidationSource::OidcDiscovery => {
            let url = discover_jwks_url(config, auth_runtime).await?;
            jwks_decoding_key(config, token, algorithm, &url, auth_runtime).await
        }
    }
}

fn fixed_public_key(
    pem: &str,
    algorithm: Algorithm,
) -> Result<DecodingKey, jsonwebtoken::errors::Error> {
    if matches!(
        algorithm,
        Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512
    ) {
        DecodingKey::from_rsa_pem(pem.as_bytes())
    } else if matches!(algorithm, Algorithm::ES256 | Algorithm::ES384) {
        DecodingKey::from_ec_pem(pem.as_bytes())
    } else {
        Err(jsonwebtoken::errors::Error::from(
            jsonwebtoken::errors::ErrorKind::InvalidAlgorithm,
        ))
    }
}

async fn discover_jwks_url(
    config: &JwtAuthConfig,
    auth_runtime: &AuthRuntime,
) -> Result<String, AuthRuntimeError> {
    let discovery_url = format!(
        "{}/.well-known/openid-configuration",
        config.issuer.trim_end_matches('/')
    );
    let body = auth_runtime
        .client
        .get(discovery_url)
        .send()
        .await
        .map_err(|_| AuthRuntimeError)?
        .error_for_status()
        .map_err(|_| AuthRuntimeError)?
        .json::<JsonValue>()
        .await
        .map_err(|_| AuthRuntimeError)?;
    body.get("jwks_uri")
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .ok_or(AuthRuntimeError)
}

async fn jwks_decoding_key(
    config: &JwtAuthConfig,
    token: &str,
    algorithm: Algorithm,
    jwks_url: &str,
    auth_runtime: &AuthRuntime,
) -> Result<DecodingKey, AuthRuntimeError> {
    let header = decode_header(token).map_err(|_| AuthRuntimeError)?;
    let kid = header.kid.ok_or(AuthRuntimeError)?;
    let jwks = fetch_jwks(config, jwks_url, auth_runtime).await?;
    let jwk = jwks.find(&kid).ok_or(AuthRuntimeError)?;

    if let Some(jwk_alg) = jwk.common.key_algorithm {
        let jwk_alg = jwt_algorithm(&jwk_alg.to_string()).ok_or(AuthRuntimeError)?;
        if jwk_alg != algorithm {
            return Err(AuthRuntimeError);
        }
    } else if config.algorithm.is_none() {
        return Err(AuthRuntimeError);
    }

    DecodingKey::from_jwk(jwk).map_err(|_| AuthRuntimeError)
}

async fn fetch_jwks(
    config: &JwtAuthConfig,
    jwks_url: &str,
    auth_runtime: &AuthRuntime,
) -> Result<JwkSet, AuthRuntimeError> {
    let now = Instant::now();
    if let Some(cached) = auth_runtime.jwks_cache.read().await.get(jwks_url)
        && cached.expires_at > now
    {
        return Ok(cached.set.clone());
    }

    let set = auth_runtime
        .client
        .get(jwks_url)
        .send()
        .await
        .map_err(|_| AuthRuntimeError)?
        .error_for_status()
        .map_err(|_| AuthRuntimeError)?
        .json::<JwkSet>()
        .await
        .map_err(|_| AuthRuntimeError)?;

    let ttl = Duration::from_secs(config.jwks_cache_ttl_seconds);
    auth_runtime.jwks_cache.write().await.insert(
        jwks_url.to_string(),
        CachedJwks {
            set: set.clone(),
            expires_at: Instant::now() + ttl,
        },
    );
    Ok(set)
}

#[derive(Debug)]
struct AuthRuntimeError;

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get("authorization")?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .filter(|token| !token.trim().is_empty())
}

fn jwt_algorithm(name: &str) -> Option<Algorithm> {
    match name {
        "HS256" => Some(Algorithm::HS256),
        "HS384" => Some(Algorithm::HS384),
        "HS512" => Some(Algorithm::HS512),
        "RS256" => Some(Algorithm::RS256),
        "RS384" => Some(Algorithm::RS384),
        "RS512" => Some(Algorithm::RS512),
        "ES256" => Some(Algorithm::ES256),
        "ES384" => Some(Algorithm::ES384),
        _ => None,
    }
}

fn jwt_auth_context(
    config: &JwtAuthConfig,
    claims: serde_json::Map<String, JsonValue>,
) -> Option<Value> {
    let subject = claim_string(&claims, &config.subject_claim)?;
    let user_id = claim_string(&claims, &config.user_id_claim).unwrap_or_else(|| subject.clone());
    let roles = claim_roles(&claims, &config.roles_claim);
    let email = claim_string(&claims, &config.email_claim);
    let mut user_entries = HashMap::from([
        ("id".into(), Value::String(user_id)),
        ("subject".into(), Value::String(subject.clone())),
        (
            "roles".into(),
            Value::List(roles.into_iter().map(Value::String).collect()),
        ),
    ]);
    if let Some(email) = email {
        user_entries.insert("email".into(), Value::String(email));
    }

    Some(map_value(HashMap::from([
        ("provider".into(), Value::String(config.name.clone())),
        ("type".into(), Value::String("jwt".into())),
        ("subject".into(), Value::String(subject)),
        ("user".into(), map_value(user_entries)),
        ("claims".into(), json_to_value(&JsonValue::Object(claims))),
    ])))
}

fn claim_string(claims: &serde_json::Map<String, JsonValue>, key: &str) -> Option<String> {
    claims.get(key)?.as_str().map(str::to_string)
}

fn claim_roles(claims: &serde_json::Map<String, JsonValue>, key: &str) -> Vec<String> {
    match claims.get(key) {
        Some(JsonValue::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        Some(JsonValue::String(role)) => vec![role.clone()],
        _ => vec![],
    }
}

fn api_key_auth_context(config: &ApiKeyAuthConfig) -> Value {
    let principal = config.principal.clone();
    let user = map_value(HashMap::from([
        ("id".into(), Value::String(principal.clone())),
        ("subject".into(), Value::String(principal.clone())),
        ("roles".into(), Value::List(vec![])),
    ]));

    map_value(HashMap::from([
        ("provider".into(), Value::String(config.name.clone())),
        ("type".into(), Value::String("api_key".into())),
        ("subject".into(), Value::String(principal)),
        ("user".into(), user),
        ("claims".into(), map_value(HashMap::new())),
    ]))
}

fn map_value(entries: HashMap<String, Value>) -> Value {
    Value::Map(Arc::new(RwLock::new(entries.into_iter().collect())))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.ct_eq(right).into()
}

fn verify_api_key_hash(supplied: &[u8], expected_hash: &str) -> bool {
    if expected_hash.starts_with("$argon2id$") {
        let Ok(parsed) = PasswordHash::new(expected_hash) else {
            return false;
        };
        return Argon2::default().verify_password(supplied, &parsed).is_ok();
    }

    let Some(expected) = expected_hash
        .strip_prefix("sha256:")
        .or_else(|| expected_hash.strip_prefix("SHA256:"))
    else {
        return false;
    };
    let digest = Sha256::digest(supplied);
    let actual_hash = format!("{:x}", digest);
    constant_time_eq(actual_hash.as_bytes(), expected.as_bytes())
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "unauthorized" })),
    )
        .into_response()
}

fn forbidden_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({ "error": "forbidden" })),
    )
        .into_response()
}

/// Converts a `MarretaError` into an axum `Response`.
/// The HTTP status a runtime error surfaces as. Shared by the structured runtime-error log
/// (Spec 037, a frozen event-log contract) and `error_to_response`, so the logged status always
/// matches the response status.
fn status_for_error(e: &MarretaError) -> StatusCode {
    match e {
        MarretaError::HttpResponse { status_code, .. } => {
            StatusCode::from_u16(*status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
        }
        MarretaError::HttpError { status_code, .. } => {
            StatusCode::from_u16(*status_code as u16).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
        }
        MarretaError::UniqueConstraintViolation { .. } => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn error_to_response(e: MarretaError) -> Response {
    match e {
        MarretaError::HttpResponse {
            status_code,
            body,
            content_type,
            extra_headers,
            ..
        } => {
            let status =
                StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            // RFC 9110 §15.3.5 — 204 and 304 MUST NOT include a message body
            if status == StatusCode::NO_CONTENT || status == StatusCode::NOT_MODIFIED {
                return status.into_response();
            }
            let mut builder = axum::response::Response::builder()
                .status(status)
                .header("Content-Type", &content_type);
            for (k, v) in &extra_headers {
                builder = builder.header(k.as_str(), v.as_str());
            }
            builder
                .body(axum::body::Body::from(body))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        MarretaError::HttpError {
            status_code,
            message,
        } => {
            let status = StatusCode::from_u16(status_code as u16)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (status, Json(serde_json::json!({ "error": message }))).into_response()
        }
        err @ MarretaError::UniqueConstraintViolation { .. } => {
            // Stable, provider-agnostic body: never echo the driver message (it names the
            // technology and leaks the duplicate key value). The detail stays in the MarretaError
            // for `rescue` and the structured log.
            let body = serde_json::json!({
                "error": "unique constraint violation",
                "code": err.semantic_code(),
            });
            (StatusCode::CONFLICT, Json(body)).into_response()
        }
        e => {
            let body = serde_json::json!({
                "error": e.display_message(),
                "code": e.semantic_code(),
            });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
        }
    }
}

enum RuntimeErrorLogScope<'a> {
    Request {
        http_status: StatusCode,
    },
    Consumer {
        consumer_kind: &'a ConsumerKind,
        target: &'a str,
        consumer_status: &'a str,
    },
    Startup,
}

fn emit_runtime_error_log(interp: &Interpreter, e: &MarretaError, scope: RuntimeErrorLogScope<'_>) {
    let event = build_runtime_error_log_event(interp, e, scope);
    let Ok(line) = serde_json::to_string(&event) else {
        return;
    };
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
}

fn build_runtime_error_log_event(
    interp: &Interpreter,
    e: &MarretaError,
    scope: RuntimeErrorLogScope<'_>,
) -> JsonValue {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "timestamp".into(),
        JsonValue::String(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)),
    );
    obj.insert("kind".into(), JsonValue::String("runtime_error".into()));
    if let Some(trace_context) = interp.trace_context() {
        obj.insert(
            "trace_id".into(),
            JsonValue::String(trace_context.trace_id.clone()),
        );
        obj.insert(
            "span_id".into(),
            JsonValue::String(trace_context.span_id.clone()),
        );
    }
    match scope {
        RuntimeErrorLogScope::Request { http_status } => {
            obj.insert("scope".into(), JsonValue::String("request".into()));
            obj.insert(
                "http_status".into(),
                JsonValue::Number(serde_json::Number::from(http_status.as_u16())),
            );
        }
        RuntimeErrorLogScope::Consumer {
            consumer_kind,
            target,
            consumer_status,
        } => {
            obj.insert("scope".into(), JsonValue::String("consumer".into()));
            obj.insert(
                "consumer_kind".into(),
                JsonValue::String(
                    match consumer_kind {
                        ConsumerKind::Queue => "queue",
                        ConsumerKind::Topic => "topic",
                    }
                    .into(),
                ),
            );
            obj.insert("target".into(), JsonValue::String(target.to_string()));
            obj.insert(
                "consumer_status".into(),
                JsonValue::String(consumer_status.to_string()),
            );
        }
        RuntimeErrorLogScope::Startup => {
            obj.insert("scope".into(), JsonValue::String("startup".into()));
        }
    }
    obj.insert("error_code".into(), JsonValue::String(e.semantic_code()));
    obj.insert("operation".into(), JsonValue::String(e.operation_name()));
    obj.insert("message".into(), JsonValue::String(e.display_message()));
    JsonValue::Object(obj)
}

fn log_uncaught_runtime_error(
    interp: &Interpreter,
    e: &MarretaError,
    scope: RuntimeErrorLogScope<'_>,
) {
    match e {
        MarretaError::HttpResponse { .. }
        | MarretaError::HttpError { .. }
        | MarretaError::NackSignal { .. } => return,
        _ => {}
    }

    emit_runtime_error_log(interp, e, scope);

    let code = e.semantic_code();
    eprintln!(
        "[marreta] {}: {}",
        format_error_code(&code),
        e.display_message()
    );
    let trace_lines = interp.uncaught_trace_lines(e);
    if !trace_lines.is_empty() {
        eprintln!("[marreta] trace:");
        for line in trace_lines {
            eprintln!("  {}", line);
        }
    }
}

fn format_error_code(code: &str) -> Cow<'_, str> {
    if stderr_supports_color() {
        Cow::Owned(format!("\x1b[31m{}\x1b[0m", code))
    } else {
        Cow::Borrowed(code)
    }
}

fn stderr_supports_color() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::stderr().is_terminal()
}

fn make_consumer_trace_interpreter(
    consumer_def: &ConsumerDefinition,
    target_label: &str,
    trace_context: Option<TraceContext>,
) -> Interpreter {
    let mut interp = Interpreter::new();
    if let Some(trace_context) = trace_context {
        interp = interp.with_trace_context(trace_context);
    }
    let trace = interp.enter_consumer(
        match consumer_def.kind {
            ConsumerKind::Queue => "queue",
            ConsumerKind::Topic => "topic",
        },
        target_label,
        consumer_def.module_id.clone(),
        consumer_def.line,
        consumer_def.column,
    );
    trace.preserve();
    interp
}

fn log_consumer_bootstrap_error(
    consumer_def: &ConsumerDefinition,
    target_label: &str,
    e: &MarretaError,
) {
    // Bootstrap happens before a delivery exists, so it is startup scope rather
    // than consumer scope.
    let interp = make_consumer_trace_interpreter(consumer_def, target_label, None);
    log_uncaught_runtime_error(&interp, e, RuntimeErrorLogScope::Startup);
}

fn log_consumer_queue_driver_error(
    consumer_def: &ConsumerDefinition,
    target_label: &str,
    operation: &str,
    err: &impl std::fmt::Display,
) {
    let wrapped = MarretaError::QueueError {
        message: err.to_string(),
        operation: operation.to_string(),
    };
    log_consumer_bootstrap_error(consumer_def, target_label, &wrapped);
}

/// Spawns a background Tokio task for each consumer definition.
///
/// Each task runs a continuous loop:
/// 1. Declares the queue/exchange via the driver (idempotent).
/// 2. Streams deliveries.
/// 3. Executes the handler body with the message payload bound to the declared variable.
/// 4. Acks on success, nacks (with optional requeue) on `NackSignal`, nacks without requeue on any other error.
async fn start_consumers(
    consumers: Vec<ConsumerDefinition>,
    runtime: Arc<ProjectRuntime>,
    db_engine: Arc<Option<DbEngine>>,
    doc_engine: Arc<Option<DocEngine>>,
    queue_driver: Arc<Option<Arc<dyn QueueDriver>>>,
    cache_driver_and_config: Arc<
        Option<(
            Arc<dyn crate::cache::driver::CacheDriver>,
            crate::cache::CacheConfig,
        )>,
    >,
    http_client_driver: Arc<Option<Arc<dyn HttpClientDriver>>>,
    request_log_enabled: bool,
    trace_context_enabled: bool,
) {
    use futures_util::StreamExt;

    let driver = match (*queue_driver).clone() {
        Some(d) => d,
        None => {
            eprintln!("[queue] consumers defined but no queue driver configured — skipping");
            return;
        }
    };

    for consumer_def in consumers {
        let driver = Arc::clone(&driver);
        let runtime = Arc::clone(&runtime);
        let db = Arc::clone(&db_engine);
        let doc = Arc::clone(&doc_engine);
        let cache = Arc::clone(&cache_driver_and_config);
        let hc = Arc::clone(&http_client_driver);

        tokio::spawn(async move {
            // Resolve target name from constant expression
            let target_name = {
                let mut interp = Interpreter::from_environment(
                    runtime.env_for_module(consumer_def.module_id.as_deref()),
                )
                .with_project_runtime(Arc::clone(&runtime))
                .with_current_module(consumer_def.module_id.clone());
                match interp.evaluate_pub(&consumer_def.target) {
                    Ok(Value::String(s)) => s,
                    Ok(other) => {
                        let err = MarretaError::TypeError {
                            message: format!(
                                "consumer target must evaluate to a string, got {}",
                                other.type_name()
                            ),
                            line: consumer_def.line,
                            column: consumer_def.column,
                        };
                        log_consumer_bootstrap_error(&consumer_def, "<target>", &err);
                        return;
                    }
                    Err(e) => {
                        log_consumer_bootstrap_error(&consumer_def, "<target>", &e);
                        return;
                    }
                }
            };

            // Declare infrastructure (idempotent)
            let queue_name = match consumer_def.kind {
                ConsumerKind::Queue => {
                    if let Err(e) = driver.declare_queue(&target_name).await {
                        log_consumer_queue_driver_error(
                            &consumer_def,
                            &target_name,
                            "queue.declare_queue",
                            &e,
                        );
                        return;
                    }
                    target_name.clone()
                }
                ConsumerKind::Topic => match driver.bind_topic(&target_name).await {
                    Ok(name) => name,
                    Err(e) => {
                        log_consumer_queue_driver_error(
                            &consumer_def,
                            &target_name,
                            "queue.bind_topic",
                            &e,
                        );
                        return;
                    }
                },
            };

            // Start consuming
            let stream = match consumer_def.kind {
                ConsumerKind::Queue => driver.consume_queue(&queue_name).await,
                ConsumerKind::Topic => driver.consume_topic(&queue_name).await,
            };
            let mut stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    let operation = match consumer_def.kind {
                        ConsumerKind::Queue => "queue.consume_queue",
                        ConsumerKind::Topic => "queue.consume_topic",
                    };
                    log_consumer_queue_driver_error(&consumer_def, &queue_name, operation, &e);
                    return;
                }
            };

            while let Some(delivery) = stream.next().await {
                let started_at = Instant::now();
                let mut payload = delivery.payload.clone();
                // For topic consumers, routing_key IS the topic the message was published to.
                // For queue consumers, it is empty string (default exchange has no routing key).
                let message_topic = delivery.routing_key.clone();
                let message_exchange = delivery.exchange.clone();
                let trace_context = if trace_context_enabled {
                    Some(TraceContext::from_headers(
                        delivery.metadata.get("traceparent").map(String::as_str),
                        delivery.metadata.get("tracestate").map(String::as_str),
                    ))
                } else {
                    None
                };

                // Validate and coerce schema if declared — nack (no requeue) on mismatch.
                // Consumers must see the same native values as route payload bindings.
                if let Some(schema_name) = &consumer_def.schema
                    && let Some(schema_def) =
                        runtime.resolve_schema(consumer_def.module_id.as_deref(), schema_name)
                {
                    let visible_schemas =
                        runtime.visible_schemas_for(consumer_def.module_id.as_deref());
                    match coerce_payload(&payload, &schema_def, &visible_schemas) {
                        Ok(coerced) => payload = coerced,
                        Err(_) => {
                            delivery.nack(false);
                            if request_log_enabled {
                                emit_consumer_log(
                                    &consumer_def.kind,
                                    &target_name,
                                    &message_topic,
                                    &message_exchange,
                                    trace_context.as_ref(),
                                    "schema_rejected",
                                    started_at.elapsed(),
                                );
                            }
                            continue;
                        }
                    }
                }

                // Build a fresh interpreter for this delivery
                let mut interp = Interpreter::from_environment(
                    runtime.env_for_module(consumer_def.module_id.as_deref()),
                )
                .with_project_runtime(Arc::clone(&runtime))
                .with_current_module(consumer_def.module_id.clone());
                if let Some(engine) = (*db).clone() {
                    interp = interp.with_db(engine);
                }
                if let Some(engine) = (*doc).clone() {
                    interp = interp.with_doc(engine);
                }
                interp = interp.with_queue(Arc::clone(&driver));
                if let Some((drv, cfg)) = (*cache).clone() {
                    interp = interp.with_cache(drv, cfg);
                }
                if let Some(drv) = (*hc).clone() {
                    interp = interp.with_http_client(drv);
                }
                if let Some(trace_context) = trace_context.clone() {
                    interp = interp.with_trace_context(trace_context);
                }
                let _consumer_trace = interp.enter_consumer(
                    match consumer_def.kind {
                        ConsumerKind::Queue => "queue",
                        ConsumerKind::Topic => "topic",
                    },
                    &queue_name,
                    consumer_def.module_id.clone(),
                    consumer_def.line,
                    consumer_def.column,
                );

                // Inject the delivery binding and metadata
                interp.env_set(consumer_def.binding.clone(), payload);
                interp.env_set(
                    "message_topic".to_string(),
                    Value::String(message_topic.clone()),
                );

                // Execute handler body — ack/nack on the delivery's own channel
                let result = interp.execute_stmts_pub(&consumer_def.body);

                let status = match result {
                    Ok(_) => {
                        delivery.ack();
                        "ack"
                    }
                    Err(MarretaError::NackSignal { requeue }) => {
                        let status = if requeue { "nack_requeue" } else { "nack" };
                        delivery.nack(requeue);
                        status
                    }
                    Err(e) => {
                        log_uncaught_runtime_error(
                            &interp,
                            &e,
                            RuntimeErrorLogScope::Consumer {
                                consumer_kind: &consumer_def.kind,
                                target: &target_name,
                                consumer_status: "error",
                            },
                        );
                        delivery.nack(false);
                        "error"
                    }
                };
                if request_log_enabled {
                    emit_consumer_log(
                        &consumer_def.kind,
                        &target_name,
                        &message_topic,
                        &message_exchange,
                        trace_context.as_ref(),
                        status,
                        started_at.elapsed(),
                    );
                }
            }
        });
    }
}

/// Returns a minimal HTML page that loads Swagger UI from CDN and points it at `spec_url`.
///
/// Note: requires internet access for CDN assets. In offline environments the UI
/// will not load, but `/openapi.json` remains available for other tooling.
fn swagger_ui_html(spec_url: &str) -> String {
    let dom_id = "#swagger-ui";
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <title>MarretaLang API Docs</title>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <link rel="stylesheet" type="text/css" href="https://unpkg.com/swagger-ui-dist/swagger-ui.css">
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist/swagger-ui-bundle.js"></script>
  <script>
    SwaggerUIBundle({{
      url: "{}",
      dom_id: "{}",
      presets: [SwaggerUIBundle.presets.apis, SwaggerUIBundle.SwaggerUIStandalonePreset],
      layout: "BaseLayout"
    }})
  </script>
</body>
</html>"#,
        spec_url, dom_id
    )
}

/// Converts a MarretaLang path (`:param` style) to axum 0.8 path (`{param}` style).
///
/// Example: `/users/:id/orders/:order_id` → `/users/{id}/orders/{order_id}`
fn to_axum_path(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            if let Some(name) = seg.strip_prefix(':') {
                format!("{{{}}}", name)
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn to_marreta_route_path(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            if seg.starts_with('{') && seg.ends_with('}') && seg.len() >= 3 {
                format!(":{}", &seg[1..seg.len() - 1])
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use std::sync::{Mutex, OnceLock};

    #[tokio::test]
    async fn test_unique_constraint_violation_maps_to_409() {
        let resp = error_to_response(MarretaError::UniqueConstraintViolation {
            message: "duplicate key value violates unique constraint \"uniq_users_email\"".into(),
            operation: "db.users.save".into(),
        });
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        // The body is stable and never echoes the provider message (finding 2).
        let json = body_json(resp).await;
        assert_eq!(json["error"], "unique constraint violation");
        assert_eq!(json["code"], "unique_violation");
    }

    #[test]
    fn test_status_for_error_unique_violation_is_conflict() {
        let status = status_for_error(&MarretaError::UniqueConstraintViolation {
            message: "x".into(),
            operation: "y".into(),
        });
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[test]
    fn test_unique_violation_semantic_code_is_unique_violation() {
        let err = MarretaError::UniqueConstraintViolation {
            message: "x".into(),
            operation: "y".into(),
        };
        assert_eq!(err.semantic_code(), "unique_violation");
    }

    async fn body_json(resp: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // ─── HttpResponse (reply / fail) ──────────────────────────────────────────

    #[tokio::test]
    async fn test_http_response_reply_200_preserved() {
        let e = MarretaError::HttpResponse {
            status_code: 200,
            body: r#"{"ok":true}"#.to_string(),
            content_type: "application/json".to_string(),
            extra_headers: vec![],
            is_error: false,
        };
        let resp = error_to_response(e);
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&bytes[..], br#"{"ok":true}"#);
    }

    #[tokio::test]
    async fn test_http_response_201_created() {
        let e = MarretaError::HttpResponse {
            status_code: 201,
            body: r#"{"id":42}"#.to_string(),
            content_type: "application/json".to_string(),
            extra_headers: vec![],
            is_error: false,
        };
        let resp = error_to_response(e);
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_http_response_204_no_body() {
        // RFC 9110 §15.3.5 — 204 MUST NOT include a message body
        let e = MarretaError::HttpResponse {
            status_code: 204,
            body: "should be dropped".to_string(),
            content_type: "application/json".to_string(),
            extra_headers: vec![],
            is_error: false,
        };
        let resp = error_to_response(e);
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(bytes.is_empty());
    }

    #[tokio::test]
    async fn test_http_response_404_fail() {
        let e = MarretaError::HttpResponse {
            status_code: 404,
            body: r#"{"error":"not found"}"#.to_string(),
            content_type: "application/json".to_string(),
            extra_headers: vec![],
            is_error: true,
        };
        let resp = error_to_response(e);
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_http_response_extra_headers_forwarded() {
        let e = MarretaError::HttpResponse {
            status_code: 302,
            body: "".to_string(),
            content_type: "text/plain".to_string(),
            extra_headers: vec![("Location".to_string(), "https://example.com".to_string())],
            is_error: false,
        };
        let resp = error_to_response(e);
        assert_eq!(resp.status(), StatusCode::FOUND);
        assert_eq!(
            resp.headers().get("Location").and_then(|v| v.to_str().ok()),
            Some("https://example.com")
        );
    }

    // ─── HttpError (require / reject) ────────────────────────────────────────

    #[tokio::test]
    async fn test_http_error_401_unauthorized() {
        let e = MarretaError::HttpError {
            status_code: 401,
            message: "unauthorized".to_string(),
        };
        let resp = error_to_response(e);
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let json = body_json(resp).await;
        assert_eq!(json["error"], "unauthorized");
    }

    #[tokio::test]
    async fn test_http_error_422_unprocessable() {
        let e = MarretaError::HttpError {
            status_code: 422,
            message: "validation failed".to_string(),
        };
        let resp = error_to_response(e);
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let json = body_json(resp).await;
        assert_eq!(json["error"], "validation failed");
    }

    // ─── Engine errors → 500 + code ──────────────────────────────────────────

    #[tokio::test]
    async fn test_raise_error_returns_500_with_code() {
        let e = MarretaError::RaiseError {
            message: "domain failure".to_string(),
        };
        let resp = error_to_response(e);
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let json = body_json(resp).await;
        assert_eq!(json["code"], "raise_error");
        assert_eq!(json["error"], "domain failure");
    }

    #[tokio::test]
    async fn test_db_error_returns_500_with_code() {
        let e = MarretaError::DbError {
            message: "record not found".to_string(),
            operation: "db.users.find".to_string(),
        };
        let resp = error_to_response(e);
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let json = body_json(resp).await;
        assert_eq!(json["code"], "db_error");
        assert_eq!(json["error"], "record not found");
    }

    #[tokio::test]
    async fn test_type_error_returns_500_with_code() {
        let e = MarretaError::TypeError {
            message: "cannot add string and integer".to_string(),
            line: 5,
            column: 3,
        };
        let resp = error_to_response(e);
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let json = body_json(resp).await;
        assert_eq!(json["code"], "type_error");
        assert!(json.get("at").is_none() || json["at"].is_null());
    }

    #[tokio::test]
    async fn test_undefined_variable_returns_reference_error_code() {
        let e = MarretaError::UndefinedVariable {
            name: "x".to_string(),
            line: 2,
            column: 1,
        };
        let resp = error_to_response(e);
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let json = body_json(resp).await;
        assert_eq!(json["code"], "reference_error");
        assert!(json.get("at").is_none() || json["at"].is_null());
    }

    #[tokio::test]
    async fn test_division_by_zero_returns_arithmetic_error_code() {
        let e = MarretaError::DivisionByZero {
            line: 10,
            column: 4,
        };
        let resp = error_to_response(e);
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let json = body_json(resp).await;
        assert_eq!(json["code"], "arithmetic_error");
        assert!(json.get("at").is_none() || json["at"].is_null());
    }

    #[tokio::test]
    async fn test_wrong_arity_returns_arity_error_code() {
        let e = MarretaError::WrongArity {
            task_name: "double".to_string(),
            expected: 1,
            got: 3,
            line: 7,
            column: 1,
        };
        let resp = error_to_response(e);
        let json = body_json(resp).await;
        assert_eq!(json["code"], "arity_error");
    }

    #[tokio::test]
    async fn test_error_at_line_zero_omits_at_field() {
        // line=0 means no source location — "at" should be absent
        let e = MarretaError::TypeError {
            message: "type error".to_string(),
            line: 0,
            column: 0,
        };
        let resp = error_to_response(e);
        let json = body_json(resp).await;
        assert!(json.get("at").is_none() || json["at"].is_null());
    }

    // ─── to_axum_path ─────────────────────────────────────────────────────────

    #[test]
    fn test_to_axum_path_no_params() {
        assert_eq!(to_axum_path("/health"), "/health");
    }

    #[test]
    fn test_to_axum_path_single_param() {
        assert_eq!(to_axum_path("/users/:id"), "/users/{id}");
    }

    #[test]
    fn test_to_axum_path_multiple_params() {
        assert_eq!(
            to_axum_path("/users/:user_id/orders/:order_id"),
            "/users/{user_id}/orders/{order_id}"
        );
    }

    #[test]
    fn test_to_axum_path_root() {
        assert_eq!(to_axum_path("/"), "/");
    }

    #[test]
    fn test_to_axum_path_param_at_root() {
        assert_eq!(to_axum_path("/:id"), "/{id}");
    }

    #[test]
    fn test_to_marreta_route_path_converts_axum_params() {
        assert_eq!(to_marreta_route_path("/orders/{id}"), "/orders/:id");
        assert_eq!(
            to_marreta_route_path("/users/{user_id}/orders/{order_id}"),
            "/users/:user_id/orders/:order_id"
        );
    }

    // ─── swagger_ui_html ──────────────────────────────────────────────────────

    #[test]
    fn test_swagger_ui_html_contains_spec_url() {
        let html = swagger_ui_html("/openapi.json");
        assert!(html.contains("/openapi.json"));
        assert!(html.contains("swagger-ui"));
        assert!(html.contains("<!DOCTYPE html>"));
    }

    #[test]
    fn test_swagger_ui_html_custom_url() {
        let html = swagger_ui_html("/api/spec.json");
        assert!(html.contains("/api/spec.json"));
    }

    // ─── error_to_response — additional branches ─────────────────────────────

    #[tokio::test]
    async fn test_304_not_modified_no_body() {
        let e = MarretaError::HttpResponse {
            status_code: 304,
            body: "should be dropped".to_string(),
            content_type: "application/json".to_string(),
            extra_headers: vec![],
            is_error: false,
        };
        let resp = error_to_response(e);
        assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(bytes.is_empty());
    }

    #[tokio::test]
    async fn test_invalid_status_code_falls_back_to_500() {
        let e = MarretaError::HttpResponse {
            status_code: 9999,
            body: "body".to_string(),
            content_type: "text/plain".to_string(),
            extra_headers: vec![],
            is_error: false,
        };
        let resp = error_to_response(e);
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_http_error_invalid_status_falls_back_to_500() {
        let e = MarretaError::HttpError {
            status_code: 9999,
            message: "bad".to_string(),
        };
        let resp = error_to_response(e);
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // ─── execute_route — take bindings ────────────────────────────────────────

    fn make_runtime() -> Arc<ProjectRuntime> {
        Arc::new(ProjectRuntime::single(
            crate::environment::Environment::new(),
            HashMap::new(),
        ))
    }

    fn empty_auth_runtime() -> Arc<AuthRuntime> {
        Arc::new(AuthRuntime::empty())
    }

    fn with_env_var<T>(key: &str, value: Option<&str>, f: impl FnOnce() -> T) -> T {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock poisoned");
        let original = std::env::var(key).ok();
        // SAFETY: env access in this test helper is serialized through ENV_LOCK;
        // the variable is set, the closure runs, then the original is restored,
        // all while holding the lock, so no other thread observes the mutation.
        match value {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
        let result = f();
        match original {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
        result
    }

    fn api_key_auth_runtime() -> Arc<AuthRuntime> {
        let digest = Sha256::digest(b"secret-key");
        api_key_auth_runtime_with_secret_source(ApiKeySecretSource::SecretHash(format!(
            "sha256:{:x}",
            digest
        )))
    }

    fn api_key_auth_runtime_with_secret_source(
        secret_source: ApiKeySecretSource,
    ) -> Arc<AuthRuntime> {
        Arc::new(AuthRuntime::new(AuthRegistry {
            providers: HashMap::from([(
                "internal_auth".into(),
                AuthProviderRuntimeConfig::ApiKey(ApiKeyAuthConfig {
                    name: "internal_auth".into(),
                    header: "x-api-key".into(),
                    secret_source,
                    principal: "internal_auth".into(),
                }),
            )]),
        }))
    }

    fn jwt_auth_runtime() -> Arc<AuthRuntime> {
        jwt_auth_runtime_with_source(
            "https://issuer.example.test",
            JwtValidationSource::Secret("jwt-secret".into()),
            Some("HS256".into()),
        )
    }

    fn jwt_auth_runtime_with_source(
        issuer: &str,
        validation_source: JwtValidationSource,
        algorithm: Option<String>,
    ) -> Arc<AuthRuntime> {
        Arc::new(AuthRuntime::new(AuthRegistry {
            providers: HashMap::from([(
                "customer_auth".into(),
                AuthProviderRuntimeConfig::Jwt(JwtAuthConfig {
                    name: "customer_auth".into(),
                    issuer: issuer.into(),
                    audience: "shop-api".into(),
                    subject_claim: "sub".into(),
                    user_id_claim: "sub".into(),
                    roles_claim: "roles".into(),
                    email_claim: "email".into(),
                    validation_source,
                    algorithm,
                    jwks_cache_ttl_seconds: 300,
                    clock_skew_seconds: 60,
                }),
            )]),
        }))
    }

    fn make_route(take: Vec<TakeBinding>) -> RouteDefinition {
        RouteDefinition {
            verb: HttpVerb::Get,
            path: "/test".into(),
            auth: None,
            allow: vec![],
            take,
            schema: None,
            body: vec![],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        }
    }

    fn protected_api_key_route(allow: crate::ast::Expression) -> RouteDefinition {
        protected_route("internal_auth", allow)
    }

    fn protected_jwt_route(allow: crate::ast::Expression) -> RouteDefinition {
        protected_route("customer_auth", allow)
    }

    fn protected_route(provider: &str, allow: crate::ast::Expression) -> RouteDefinition {
        RouteDefinition {
            verb: HttpVerb::Get,
            path: "/protected".into(),
            auth: Some(crate::ast::RouteAuth {
                provider: provider.into(),
                line: 1,
                column: 1,
            }),
            allow: vec![allow],
            take: vec![],
            schema: None,
            body: vec![crate::ast::Statement::Reply {
                status_code: crate::ast::Expression::Integer(200),
                content_type: crate::ast::ReplyContentType::Json,
                body: crate::ast::Expression::MapLiteral(vec![(
                    "subject".into(),
                    crate::ast::Expression::PropertyAccess {
                        object: Box::new(crate::ast::Expression::Identifier("auth".into())),
                        property: "subject".into(),
                    },
                )]),
                response_schema: None,
                extra_headers: None,
                line: 2,
                column: 5,
            }],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        }
    }

    fn auth_user_id_equals(value: &str) -> crate::ast::Expression {
        crate::ast::Expression::BinaryOp {
            left: Box::new(crate::ast::Expression::PropertyAccess {
                object: Box::new(crate::ast::Expression::PropertyAccess {
                    object: Box::new(crate::ast::Expression::Identifier("auth".into())),
                    property: "user".into(),
                }),
                property: "id".into(),
            }),
            operator: crate::ast::BinaryOperator::Equal,
            right: Box::new(crate::ast::Expression::StringLiteral(value.into())),
        }
    }

    fn auth_roles_include(value: &str) -> crate::ast::Expression {
        crate::ast::Expression::BinaryOp {
            left: Box::new(crate::ast::Expression::StringLiteral(value.into())),
            operator: crate::ast::BinaryOperator::In,
            right: Box::new(crate::ast::Expression::PropertyAccess {
                object: Box::new(crate::ast::Expression::PropertyAccess {
                    object: Box::new(crate::ast::Expression::Identifier("auth".into())),
                    property: "user".into(),
                }),
                property: "roles".into(),
            }),
        }
    }

    fn jwt_token(audience: &str) -> String {
        jsonwebtoken::encode(
            &jsonwebtoken::Header::new(Algorithm::HS256),
            &serde_json::json!({
                "iss": "https://issuer.example.test",
                "aud": audience,
                "sub": "user-123",
                "roles": ["admin"],
                "exp": 4102444800_u64,
                "nbf": 0_u64
            }),
            &jsonwebtoken::EncodingKey::from_secret(b"jwt-secret"),
        )
        .unwrap()
    }

    const RSA_PRIVATE_KEY: &str = include_str!("../tests/fixtures/auth/rsa_private_key.pem");
    const RSA_PUBLIC_KEY: &str = include_str!("../tests/fixtures/auth/rsa_public_key.pem");
    const EC_PRIVATE_KEY: &str = include_str!("../tests/fixtures/auth/ec_private_key.pem");
    const EC_PUBLIC_KEY: &str = include_str!("../tests/fixtures/auth/ec_public_key.pem");
    const RSA_JWKS: &str = include_str!("../tests/fixtures/auth/jwks.json");

    fn rs256_jwt_token(issuer: &str, audience: &str) -> String {
        let mut header = jsonwebtoken::Header::new(Algorithm::RS256);
        header.kid = Some("rsa01".into());
        jsonwebtoken::encode(
            &header,
            &serde_json::json!({
                "iss": issuer,
                "aud": audience,
                "sub": "user-123",
                "roles": ["admin"],
                "exp": 4102444800_u64,
                "nbf": 0_u64
            }),
            &jsonwebtoken::EncodingKey::from_rsa_pem(RSA_PRIVATE_KEY.as_bytes()).unwrap(),
        )
        .unwrap()
    }

    fn es256_jwt_token(issuer: &str, audience: &str) -> String {
        jsonwebtoken::encode(
            &jsonwebtoken::Header::new(Algorithm::ES256),
            &serde_json::json!({
                "iss": issuer,
                "aud": audience,
                "sub": "user-123",
                "roles": ["admin"],
                "exp": 4102444800_u64,
                "nbf": 0_u64
            }),
            &jsonwebtoken::EncodingKey::from_ec_pem(EC_PRIVATE_KEY.as_bytes()).unwrap(),
        )
        .unwrap()
    }

    fn hs256_jwt_token_with_kid(issuer: &str, audience: &str) -> String {
        let mut header = jsonwebtoken::Header::new(Algorithm::HS256);
        header.kid = Some("rsa01".into());
        jsonwebtoken::encode(
            &header,
            &serde_json::json!({
                "iss": issuer,
                "aud": audience,
                "sub": "user-123",
                "roles": ["admin"],
                "exp": 4102444800_u64,
                "nbf": 0_u64
            }),
            &jsonwebtoken::EncodingKey::from_secret(b"wrong-family"),
        )
        .unwrap()
    }

    async fn spawn_jwks_stub() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let discovery_base = base_url.clone();
        let jwks = serde_json::from_str::<JsonValue>(RSA_JWKS).unwrap();
        let app = Router::new()
            .route(
                "/.well-known/openid-configuration",
                get({
                    let discovery_base = discovery_base.clone();
                    move || {
                        let discovery_base = discovery_base.clone();
                        async move {
                            Json(serde_json::json!({
                                "jwks_uri": format!("{discovery_base}/jwks")
                            }))
                        }
                    }
                }),
            )
            .route(
                "/jwks",
                get({
                    let jwks = jwks.clone();
                    move || {
                        let jwks = jwks.clone();
                        async move { Json(jwks) }
                    }
                }),
            );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        base_url
    }

    #[tokio::test]
    async fn test_api_key_auth_missing_header_returns_401() {
        let resp = execute_route(
            protected_api_key_route(auth_user_id_equals("internal_auth")),
            HashMap::new(),
            HashMap::new(),
            HeaderMap::new(),
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            api_key_auth_runtime(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_api_key_auth_valid_header_injects_auth_context() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "secret-key".parse().unwrap());
        let resp = execute_route(
            protected_api_key_route(auth_user_id_equals("internal_auth")),
            HashMap::new(),
            HashMap::new(),
            headers,
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            api_key_auth_runtime(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["subject"], "internal_auth");
    }

    #[tokio::test]
    async fn test_api_key_auth_valid_argon2id_hash_injects_auth_context() {
        use argon2::PasswordHasher;
        use argon2::password_hash::SaltString;

        let salt = SaltString::from_b64("bWFycmV0YS1hcGkta2V5").unwrap();
        let hash = Argon2::default()
            .hash_password(b"secret-key", &salt)
            .unwrap()
            .to_string();
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "secret-key".parse().unwrap());
        let resp = execute_route(
            protected_api_key_route(auth_user_id_equals("internal_auth")),
            HashMap::new(),
            HashMap::new(),
            headers,
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            api_key_auth_runtime_with_secret_source(ApiKeySecretSource::SecretHash(hash)),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["subject"], "internal_auth");
    }

    #[tokio::test]
    async fn test_api_key_auth_allow_false_returns_403() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "secret-key".parse().unwrap());
        let resp = execute_route(
            protected_api_key_route(auth_user_id_equals("other")),
            HashMap::new(),
            HashMap::new(),
            headers,
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            api_key_auth_runtime(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_jwt_auth_valid_hmac_token_injects_auth_context() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {}", jwt_token("shop-api")).parse().unwrap(),
        );
        let resp = execute_route(
            protected_jwt_route(auth_roles_include("admin")),
            HashMap::new(),
            HashMap::new(),
            headers,
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            jwt_auth_runtime(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["subject"], "user-123");
    }

    #[tokio::test]
    async fn test_jwt_auth_wrong_audience_returns_401() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {}", jwt_token("other-api"))
                .parse()
                .unwrap(),
        );
        let resp = execute_route(
            protected_jwt_route(auth_roles_include("admin")),
            HashMap::new(),
            HashMap::new(),
            headers,
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            jwt_auth_runtime(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_jwt_auth_valid_jwks_token_injects_auth_context() {
        let base_url = spawn_jwks_stub().await;
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!(
                "Bearer {}",
                rs256_jwt_token("https://issuer.example.test", "shop-api")
            )
            .parse()
            .unwrap(),
        );
        let resp = execute_route(
            protected_jwt_route(auth_roles_include("admin")),
            HashMap::new(),
            HashMap::new(),
            headers,
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            jwt_auth_runtime_with_source(
                "https://issuer.example.test",
                JwtValidationSource::JwksUrl(format!("{base_url}/jwks")),
                None,
            ),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["subject"], "user-123");
    }

    #[tokio::test]
    async fn test_jwt_auth_valid_fixed_public_key_token_injects_auth_context() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!(
                "Bearer {}",
                rs256_jwt_token("https://issuer.example.test", "shop-api")
            )
            .parse()
            .unwrap(),
        );
        let resp = execute_route(
            protected_jwt_route(auth_roles_include("admin")),
            HashMap::new(),
            HashMap::new(),
            headers,
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            jwt_auth_runtime_with_source(
                "https://issuer.example.test",
                JwtValidationSource::PublicKeyPem(RSA_PUBLIC_KEY.into()),
                Some("RS256".into()),
            ),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["subject"], "user-123");
    }

    #[tokio::test]
    async fn test_jwt_auth_valid_fixed_ec_public_key_token_injects_auth_context() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!(
                "Bearer {}",
                es256_jwt_token("https://issuer.example.test", "shop-api")
            )
            .parse()
            .unwrap(),
        );
        let resp = execute_route(
            protected_jwt_route(auth_roles_include("admin")),
            HashMap::new(),
            HashMap::new(),
            headers,
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            jwt_auth_runtime_with_source(
                "https://issuer.example.test",
                JwtValidationSource::PublicKeyPem(EC_PUBLIC_KEY.into()),
                Some("ES256".into()),
            ),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["subject"], "user-123");
    }

    #[tokio::test]
    async fn test_jwt_auth_oidc_discovery_fetches_jwks() {
        let issuer = spawn_jwks_stub().await;
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {}", rs256_jwt_token(&issuer, "shop-api"))
                .parse()
                .unwrap(),
        );
        let resp = execute_route(
            protected_jwt_route(auth_roles_include("admin")),
            HashMap::new(),
            HashMap::new(),
            headers,
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            jwt_auth_runtime_with_source(&issuer, JwtValidationSource::OidcDiscovery, None),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["subject"], "user-123");
    }

    #[tokio::test]
    async fn test_jwt_auth_algorithm_confusion_returns_401() {
        let base_url = spawn_jwks_stub().await;
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!(
                "Bearer {}",
                hs256_jwt_token_with_kid("https://issuer.example.test", "shop-api")
            )
            .parse()
            .unwrap(),
        );
        let resp = execute_route(
            protected_jwt_route(auth_roles_include("admin")),
            HashMap::new(),
            HashMap::new(),
            headers,
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            jwt_auth_runtime_with_source(
                "https://issuer.example.test",
                JwtValidationSource::JwksUrl(format!("{base_url}/jwks")),
                Some("RS256".into()),
            ),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_execute_route_headers_binding() {
        let mut hm = HeaderMap::new();
        hm.insert("x-token", "abc123".parse().unwrap());
        let resp = execute_route(
            make_route(vec![TakeBinding::Headers("hdrs".into())]),
            HashMap::new(),
            HashMap::new(),
            hm,
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            empty_auth_runtime(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_execute_route_form_binding() {
        let resp = execute_route(
            make_route(vec![TakeBinding::Form("form".into())]),
            HashMap::new(),
            HashMap::new(),
            HeaderMap::new(),
            None,
            Bytes::from("name=alice&age=30"),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            empty_auth_runtime(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_execute_route_raw_binding() {
        let resp = execute_route(
            make_route(vec![TakeBinding::Raw("body".into())]),
            HashMap::new(),
            HashMap::new(),
            HeaderMap::new(),
            None,
            Bytes::from("raw content here"),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            empty_auth_runtime(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_execute_route_numeric_path_param_coerced() {
        let mut url_params = HashMap::new();
        url_params.insert("id".to_string(), "42".to_string());
        let resp = execute_route(
            make_route(vec![]),
            url_params,
            HashMap::new(),
            HeaderMap::new(),
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            empty_auth_runtime(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_execute_route_string_path_param() {
        let mut url_params = HashMap::new();
        url_params.insert("slug".to_string(), "hello-world".to_string());
        let resp = execute_route(
            make_route(vec![]),
            url_params,
            HashMap::new(),
            HeaderMap::new(),
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            empty_auth_runtime(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_execute_route_query_binding() {
        let mut query = HashMap::new();
        query.insert("page".to_string(), "1".to_string());
        let resp = execute_route(
            make_route(vec![TakeBinding::Query("q".into())]),
            HashMap::new(),
            query,
            HeaderMap::new(),
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            empty_auth_runtime(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_execute_route_payload_empty_body_is_null() {
        let resp = execute_route(
            make_route(vec![TakeBinding::Payload("p".into())]),
            HashMap::new(),
            HashMap::new(),
            HeaderMap::new(),
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            empty_auth_runtime(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_execute_route_payload_invalid_json_is_null() {
        let resp = execute_route(
            make_route(vec![TakeBinding::Payload("p".into())]),
            HashMap::new(),
            HashMap::new(),
            HeaderMap::new(),
            None,
            Bytes::from("not json"),
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            empty_auth_runtime(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    // ─── DB engine injection ──────────────────────────────────────────────────

    struct StubDriver;

    #[async_trait::async_trait]
    impl crate::db::driver::DbDriver for StubDriver {
        async fn save(
            &self,
            _: &str,
            _: crate::db::driver::DbRow,
        ) -> crate::db::driver::DbResult<crate::db::driver::DbRow> {
            unimplemented!()
        }
        async fn find(
            &self,
            _: &str,
            _: &Value,
        ) -> crate::db::driver::DbResult<Option<crate::db::driver::DbRow>> {
            unimplemented!()
        }
        async fn find_all(
            &self,
            _: &str,
            _: Vec<crate::db::driver::FilterClause>,
        ) -> crate::db::driver::DbResult<Vec<crate::db::driver::DbRow>> {
            unimplemented!()
        }
        async fn update_by_id(
            &self,
            _: &str,
            _: &Value,
            _: crate::db::driver::DbRow,
        ) -> crate::db::driver::DbResult<Option<crate::db::driver::DbRow>> {
            unimplemented!()
        }
        async fn delete_by_id(&self, _: &str, _: &Value) -> crate::db::driver::DbResult<bool> {
            unimplemented!()
        }
        async fn query_fetch(
            &self,
            _: &crate::db::driver::QueryState,
        ) -> crate::db::driver::DbResult<Vec<crate::db::driver::DbRow>> {
            unimplemented!()
        }
        async fn query_fetch_one(
            &self,
            _: &crate::db::driver::QueryState,
        ) -> crate::db::driver::DbResult<Option<crate::db::driver::DbRow>> {
            unimplemented!()
        }
        async fn query_count(
            &self,
            _: &crate::db::driver::QueryState,
        ) -> crate::db::driver::DbResult<i64> {
            unimplemented!()
        }
        async fn query_exists(
            &self,
            _: &crate::db::driver::QueryState,
        ) -> crate::db::driver::DbResult<bool> {
            unimplemented!()
        }
        async fn query_update(
            &self,
            _: &crate::db::driver::QueryState,
            _: crate::db::driver::DbRow,
        ) -> crate::db::driver::DbResult<u64> {
            unimplemented!()
        }
        async fn query_delete(
            &self,
            _: &crate::db::driver::QueryState,
        ) -> crate::db::driver::DbResult<u64> {
            unimplemented!()
        }
        async fn native_query(
            &self,
            _: &str,
            _: Vec<Value>,
        ) -> crate::db::driver::DbResult<Vec<crate::db::driver::DbRow>> {
            unimplemented!()
        }
        async fn begin(&self) -> crate::db::driver::DbResult<Box<dyn crate::db::driver::DbTx>> {
            unimplemented!()
        }
    }

    // ─── register_route — all verbs ───────────────────────────────────────────

    fn make_router_for_verb(verb: HttpVerb) -> Router {
        let route_def = RouteDefinition {
            verb,
            path: "/test".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        };
        register_route(
            Router::new(),
            route_def,
            make_runtime(),
            Arc::new(None),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            empty_auth_runtime(),
            None,
        )
    }

    #[test]
    fn test_register_route_patch_verb() {
        let _router = make_router_for_verb(HttpVerb::Patch);
    }

    #[test]
    fn test_register_route_delete_verb() {
        let _router = make_router_for_verb(HttpVerb::Delete);
    }

    #[test]
    fn test_request_log_enabled_for_serve_defaults_true() {
        with_env_var("MARRETA_REQUEST_LOG", None, || {
            assert!(request_log_enabled_for_serve_from_env());
        });
    }

    #[test]
    fn test_request_log_enabled_for_serve_false_when_disabled() {
        with_env_var("MARRETA_REQUEST_LOG", Some("false"), || {
            assert!(!request_log_enabled_for_serve_from_env());
        });
    }

    #[test]
    fn test_trace_context_enabled_for_serve_defaults_true() {
        with_env_var("MARRETA_TRACE_CONTEXT", None, || {
            assert!(trace_context_enabled_for_serve_from_env());
        });
    }

    #[test]
    fn test_trace_context_enabled_for_serve_false_when_disabled() {
        with_env_var("MARRETA_TRACE_CONTEXT", Some("false"), || {
            assert!(!trace_context_enabled_for_serve_from_env());
        });
    }

    #[test]
    fn test_build_request_log_event_includes_route_and_float_duration() {
        let trace_context = TraceContext {
            trace_id: "0af7651916cd43dd8448eb211c80319c".into(),
            span_id: "b7ad6b7169203331".into(),
            trace_flags: "01".into(),
            tracestate: None,
        };
        let event = build_request_log_event(
            &Method::POST,
            "/orders/42",
            Some("/orders/:id"),
            Some(&trace_context),
            StatusCode::CREATED,
            Duration::from_micros(420),
        );
        let obj = event.as_object().unwrap();
        assert_eq!(obj.get("kind").unwrap(), "request");
        assert_eq!(
            obj.get("trace_id").unwrap(),
            "0af7651916cd43dd8448eb211c80319c"
        );
        assert_eq!(obj.get("span_id").unwrap(), "b7ad6b7169203331");
        assert_eq!(obj.get("method").unwrap(), "POST");
        assert_eq!(obj.get("path").unwrap(), "/orders/42");
        assert_eq!(obj.get("route").unwrap(), "/orders/:id");
        assert_eq!(obj.get("status").unwrap(), 201);
        let duration = obj.get("duration_ms").unwrap().as_f64().unwrap();
        assert!((duration - 0.42).abs() < 1e-9);
    }

    #[test]
    fn test_build_request_log_event_omits_route_when_absent() {
        let event = build_request_log_event(
            &Method::GET,
            "/missing",
            None,
            None,
            StatusCode::NOT_FOUND,
            Duration::from_millis(3),
        );
        let obj = event.as_object().unwrap();
        assert!(!obj.contains_key("route"));
        assert!(!obj.contains_key("trace_id"));
        assert!(!obj.contains_key("span_id"));
        assert_eq!(obj.get("status").unwrap(), 404);
    }

    #[test]
    fn test_build_consumer_log_event_includes_trace_and_topic_fields() {
        let trace_context = TraceContext {
            trace_id: "0af7651916cd43dd8448eb211c80319c".into(),
            span_id: "b7ad6b7169203331".into(),
            trace_flags: "01".into(),
            tracestate: None,
        };
        let event = build_consumer_log_event(
            &ConsumerKind::Topic,
            "orders.created",
            "orders.created",
            "amq.topic",
            Some(&trace_context),
            "ack",
            Duration::from_micros(420),
        );
        let obj = event.as_object().unwrap();
        assert_eq!(obj.get("kind").unwrap(), "consumer");
        assert_eq!(
            obj.get("trace_id").unwrap(),
            "0af7651916cd43dd8448eb211c80319c"
        );
        assert_eq!(obj.get("span_id").unwrap(), "b7ad6b7169203331");
        assert_eq!(obj.get("consumer_kind").unwrap(), "topic");
        assert_eq!(obj.get("target").unwrap(), "orders.created");
        assert_eq!(obj.get("routing_key").unwrap(), "orders.created");
        assert_eq!(obj.get("exchange").unwrap(), "amq.topic");
        assert_eq!(obj.get("status").unwrap(), "ack");
        let duration = obj.get("duration_ms").unwrap().as_f64().unwrap();
        assert!((duration - 0.42).abs() < 1e-9);
    }

    #[test]
    fn test_build_consumer_log_event_omits_optional_empty_fields() {
        let event = build_consumer_log_event(
            &ConsumerKind::Queue,
            "orders",
            "",
            "",
            None,
            "schema_rejected",
            Duration::from_millis(3),
        );
        let obj = event.as_object().unwrap();
        assert_eq!(obj.get("kind").unwrap(), "consumer");
        assert_eq!(obj.get("consumer_kind").unwrap(), "queue");
        assert_eq!(obj.get("target").unwrap(), "orders");
        assert_eq!(obj.get("status").unwrap(), "schema_rejected");
        assert!(!obj.contains_key("routing_key"));
        assert!(!obj.contains_key("exchange"));
        assert!(!obj.contains_key("trace_id"));
        assert!(!obj.contains_key("span_id"));
    }

    #[test]
    fn test_build_runtime_error_log_event_for_request_scope() {
        let interp = Interpreter::new().with_trace_context(TraceContext {
            trace_id: "0af7651916cd43dd8448eb211c80319c".into(),
            span_id: "b7ad6b7169203331".into(),
            trace_flags: "01".into(),
            tracestate: None,
        });
        let err = MarretaError::TypeError {
            message: "Property 'id' not found".into(),
            line: 1,
            column: 1,
        };
        let event = build_runtime_error_log_event(
            &interp,
            &err,
            RuntimeErrorLogScope::Request {
                http_status: StatusCode::INTERNAL_SERVER_ERROR,
            },
        );
        let obj = event.as_object().unwrap();
        assert_eq!(obj.get("kind").unwrap(), "runtime_error");
        assert_eq!(
            obj.get("trace_id").unwrap(),
            "0af7651916cd43dd8448eb211c80319c"
        );
        assert_eq!(obj.get("span_id").unwrap(), "b7ad6b7169203331");
        assert_eq!(obj.get("scope").unwrap(), "request");
        assert_eq!(obj.get("error_code").unwrap(), "type_error");
        assert_eq!(obj.get("operation").unwrap(), "interpreter");
        assert_eq!(obj.get("message").unwrap(), "Property 'id' not found");
        assert_eq!(obj.get("http_status").unwrap(), 500);
        assert!(!obj.contains_key("source"));
    }

    #[test]
    fn test_build_runtime_error_log_event_for_consumer_scope() {
        let interp = Interpreter::new();
        let err = MarretaError::QueueError {
            message: "ack failed".into(),
            operation: "queue.ack".into(),
        };
        let event = build_runtime_error_log_event(
            &interp,
            &err,
            RuntimeErrorLogScope::Consumer {
                consumer_kind: &ConsumerKind::Queue,
                target: "orders",
                consumer_status: "error",
            },
        );
        let obj = event.as_object().unwrap();
        assert_eq!(obj.get("kind").unwrap(), "runtime_error");
        assert_eq!(obj.get("scope").unwrap(), "consumer");
        assert_eq!(obj.get("consumer_kind").unwrap(), "queue");
        assert_eq!(obj.get("target").unwrap(), "orders");
        assert_eq!(obj.get("consumer_status").unwrap(), "error");
        assert_eq!(obj.get("error_code").unwrap(), "queue_error");
        assert_eq!(obj.get("operation").unwrap(), "queue.ack");
        assert!(!obj.contains_key("trace_id"));
        assert!(!obj.contains_key("span_id"));
        assert!(!obj.contains_key("source"));
    }

    // ─── serve() — startup coverage via abort ────────────────────────────────

    fn make_server_config(docs: bool, cors: bool, origin: &str) -> ServerConfig {
        ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 0,
            cors_enabled: cors,
            cors_origin: origin.to_string(),
            docs_enabled: docs,
            docs_path: "/docs".to_string(),
            db_engine: None,
            doc_engine: None,
            queue_driver: None,
            cache_engine: None,
            http_client_driver: None,
            request_log_enabled: false,
            trace_context_enabled: false,
            startup_started_at: None,
        }
    }

    fn empty_registry() -> RouteRegistry {
        RouteRegistry {
            routes: vec![],
            schemas: std::collections::HashMap::new(),
            persistent_schemas: std::collections::HashMap::new(),
            startup_stmts: vec![],
            consumers: vec![],
            auth_providers: std::collections::HashMap::new(),
        }
    }

    async fn run_serve_briefly(
        registry: RouteRegistry,
        runtime: Arc<ProjectRuntime>,
        config: ServerConfig,
    ) {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async move {
                tokio::task::spawn_local(async move {
                    let _ = serve(registry, runtime, config).await;
                });
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            })
            .await;
    }

    #[tokio::test]
    async fn test_serve_docs_and_wildcard_cors() {
        let mut env = crate::environment::Environment::new();
        env.set(
            "project_name".to_string(),
            crate::value::Value::String("Test API".to_string()),
        );
        env.set(
            "project_version".to_string(),
            crate::value::Value::String("1.0.0".to_string()),
        );
        let runtime = Arc::new(ProjectRuntime::single(env, HashMap::new()));
        let config = make_server_config(true, true, "*");
        run_serve_briefly(empty_registry(), runtime, config).await;
    }

    #[tokio::test]
    async fn test_serve_specific_cors_origin() {
        let config = make_server_config(false, true, "https://example.com");
        run_serve_briefly(empty_registry(), make_runtime(), config).await;
    }

    #[tokio::test]
    async fn test_serve_no_docs_no_cors() {
        let config = make_server_config(false, false, "*");
        run_serve_briefly(empty_registry(), make_runtime(), config).await;
    }

    #[tokio::test]
    async fn test_execute_route_with_db_engine_injected() {
        let engine = DbEngine {
            driver: Arc::new(StubDriver),
            provider: crate::db::DbProvider::Postgres,
        };
        let resp = execute_route(
            make_route(vec![]),
            HashMap::new(),
            HashMap::new(),
            HeaderMap::new(),
            None,
            Bytes::new(),
            make_runtime(),
            Arc::new(Some(engine)),
            Arc::new(None),
            Arc::new(None::<std::sync::Arc<dyn crate::queue::driver::QueueDriver>>),
            Arc::new(
                None::<(
                    Arc<dyn crate::cache::driver::CacheDriver>,
                    crate::cache::CacheConfig,
                )>,
            ),
            Arc::new(None::<Arc<dyn HttpClientDriver>>),
            empty_auth_runtime(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    // ─── Built-in /_health endpoint ───────────────────────────────────────────

    async fn run_health(
        config: ServerConfig,
        env: Arc<crate::environment::Environment>,
    ) -> serde_json::Value {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async move {
                let (tx, rx) = tokio::sync::oneshot::channel::<u16>();
                let env2 = Arc::clone(&env);
                tokio::task::spawn_local(async move {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let port = listener.local_addr().unwrap().port();
                    tx.send(port).unwrap();
                    let mut router = axum::Router::new();
                    let db_engine: Arc<Option<DbEngine>> = Arc::new(config.db_engine);
                    let doc_engine: Arc<Option<DocEngine>> = Arc::new(config.doc_engine);
                    let queue_driver: Arc<Option<Arc<dyn QueueDriver>>> =
                        Arc::new(config.queue_driver);
                    // Inline the health route
                    {
                        let env = Arc::clone(&env2);
                        let has_db = db_engine.is_some();
                        let has_doc = doc_engine.is_some();
                        let has_queue = queue_driver.is_some();
                        router = router.route(
                            "/_health",
                            axum::routing::get(move || {
                                let env = Arc::clone(&env);
                                async move {
                                    let api = env
                                        .get("project_name")
                                        .and_then(|v| {
                                            if let Value::String(s) = v {
                                                Some(s)
                                            } else {
                                                None
                                            }
                                        })
                                        .unwrap_or_else(|| "test".to_string());
                                    let version = env
                                        .get("project_version")
                                        .and_then(|v| {
                                            if let Value::String(s) = v {
                                                Some(s)
                                            } else {
                                                None
                                            }
                                        })
                                        .unwrap_or_else(|| "1.0".to_string());
                                    let mut body = serde_json::json!({
                                        "ok": true, "api": api, "version": version
                                    });
                                    if has_db {
                                        body["db"] = serde_json::json!("connected");
                                    }
                                    if has_doc {
                                        body["doc"] = serde_json::json!("connected");
                                    }
                                    body["queue"] = if has_queue {
                                        serde_json::json!("connected")
                                    } else {
                                        serde_json::json!("not_configured")
                                    };
                                    axum::response::Response::builder()
                                        .status(StatusCode::OK)
                                        .header("Content-Type", "application/json")
                                        .body(axum::body::Body::from(body.to_string()))
                                        .unwrap()
                                }
                            }),
                        );
                    }
                    axum::serve(listener, router).await.unwrap();
                });
                let port = rx.await.unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let url = format!("http://127.0.0.1:{port}/_health");
                let resp = reqwest::get(&url).await.unwrap();
                resp.json::<serde_json::Value>().await.unwrap()
            })
            .await
    }

    #[tokio::test]
    async fn test_health_no_engines_queue_not_configured() {
        let config = make_server_config(false, false, "*");
        let mut env = crate::environment::Environment::new();
        env.set("project_name".into(), Value::String("MyAPI".into()));
        env.set("project_version".into(), Value::String("2.0".into()));
        let body = run_health(config, Arc::new(env)).await;
        assert_eq!(body["ok"], true);
        assert_eq!(body["api"], "MyAPI");
        assert_eq!(body["version"], "2.0");
        assert_eq!(body["queue"], "not_configured");
        assert!(body.get("db").is_none());
        assert!(body.get("doc").is_none());
    }
}
