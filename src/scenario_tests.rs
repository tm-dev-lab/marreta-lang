use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::{Bytes, to_bytes};
use axum::http::HeaderMap;
use futures_util::stream;
use serde_json::Value as JsonValue;

use crate::ast::{Argument, Expression, HttpVerb, ScenarioStep, Statement};
use crate::auth::build_auth_registry;
use crate::cache::driver::{CacheDriver, CacheDriverError, CacheResult};
use crate::db::driver::{DbDriver, DbResult, DbRow, DbTx, FilterClause, QueryState};
use crate::db::{DbEngine, DbProvider};
use crate::doc::mongodb::{DocDriver, DocResult, DocRow};
use crate::doc::query::DocQueryState;
use crate::error::MarretaError;
use crate::file_loader::LoadedProject;
use crate::http_client::driver::{
    HttpClient, HttpClientDriverError, HttpClientResult, HttpRequest,
    HttpResponse as HttpClientResponse,
};
use crate::interpreter::Interpreter;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::queue::driver::{
    QueueDelivery, QueueDriver, QueueDriverError, QueueMessage, QueueResult,
};
use crate::route_loader::{RouteDefinition, RouteRegistry};
use crate::server::execute_route;
use crate::trace_context::TraceContext;
use crate::value::{Value, value_to_json};

#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioDefinition {
    pub name: String,
    pub steps: Vec<ScenarioStep>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioFile {
    pub path: PathBuf,
    pub scenarios: Vec<ScenarioDefinition>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioRun {
    pub file: PathBuf,
    pub name: String,
    pub passed: bool,
    pub error: Option<String>,
    pub route_verb: Option<HttpVerb>,
    pub route_path: Option<String>,
    pub assertion_count: usize,
    pub given_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct ScenarioPlan {
    route_verb: Option<HttpVerb>,
    route_path: Option<String>,
    assertion_count: usize,
    given_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct GivenKey {
    namespace: String,
    resource: Option<String>,
    method: String,
}

impl GivenKey {
    fn label(&self) -> String {
        match &self.resource {
            Some(resource) => format!("{}.{}.{}", self.namespace, resource, self.method),
            None => format!("{}.{}", self.namespace, self.method),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ValueMatcher {
    Anything,
    Exact(Value),
    Map(HashMap<String, ValueMatcher>),
    List(Vec<ValueMatcher>),
}

impl ValueMatcher {
    fn matches(&self, actual: &Value) -> bool {
        match self {
            Self::Anything => true,
            Self::Exact(expected) => expected == actual,
            Self::Map(expected) => {
                let Value::Map(actual_map) = actual else {
                    return false;
                };
                let actual_guard = actual_map.read().expect("scenario actual map poisoned");
                expected.iter().all(|(key, matcher)| {
                    actual_guard
                        .get(key)
                        .is_some_and(|value| matcher.matches(value))
                })
            }
            Self::List(expected) => {
                let Value::List(actual_items) = actual else {
                    return false;
                };
                expected.len() == actual_items.len()
                    && expected
                        .iter()
                        .zip(actual_items)
                        .all(|(matcher, value)| matcher.matches(value))
            }
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::Anything => "anything".to_string(),
            Self::Exact(value) => value.to_string(),
            Self::Map(values) => {
                let mut entries = values
                    .iter()
                    .map(|(key, value)| format!("{key}: {}", value.describe()))
                    .collect::<Vec<_>>();
                entries.sort();
                format!("{{ {} }}", entries.join(", "))
            }
            Self::List(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(ValueMatcher::describe)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    fn specificity(&self) -> usize {
        match self {
            Self::Anything => 0,
            Self::Exact(_) => 4,
            Self::Map(values) => {
                2 + values
                    .values()
                    .map(ValueMatcher::specificity)
                    .sum::<usize>()
            }
            Self::List(values) => 2 + values.iter().map(ValueMatcher::specificity).sum::<usize>(),
        }
    }
}

#[derive(Debug, Clone)]
struct GivenCall {
    key: GivenKey,
    args: Vec<ValueMatcher>,
    returns: Value,
    matched: bool,
}

impl GivenCall {
    fn describe(&self) -> String {
        format!(
            "{}({})",
            self.key.label(),
            self.args
                .iter()
                .map(ValueMatcher::describe)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    fn specificity(&self) -> usize {
        self.args.iter().map(ValueMatcher::specificity).sum()
    }
}

#[derive(Debug, Default)]
struct ScenarioMockState {
    calls: Mutex<VecDeque<GivenCall>>,
}

impl ScenarioMockState {
    fn register(&self, call: GivenCall) {
        self.calls
            .lock()
            .expect("scenario mock registry poisoned")
            .push_back(call);
    }

    fn take(&self, key: GivenKey, args: Vec<Value>) -> Result<Value, String> {
        let mut calls = self.calls.lock().expect("scenario mock registry poisoned");
        let mut best_match = None;
        for (index, call) in calls.iter().enumerate() {
            if call.key != key {
                continue;
            }
            if call.args.len() != args.len() {
                continue;
            }
            if call
                .args
                .iter()
                .zip(&args)
                .all(|(matcher, actual)| matcher.matches(actual))
            {
                let specificity = call.specificity();
                if best_match.is_none_or(|(_, best_specificity)| specificity > best_specificity) {
                    best_match = Some((index, specificity));
                }
            }
        }
        if let Some((index, _)) = best_match {
            let call = calls
                .get_mut(index)
                .expect("scenario mock registry index disappeared");
            call.matched = true;
            return Ok(call.returns.clone());
        }
        Err(format!(
            "unconfigured call: {}({})",
            key.label(),
            args.iter()
                .map(|arg| arg.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }

    fn assert_all_consumed(&self) -> Result<(), String> {
        let calls = self.calls.lock().expect("scenario mock registry poisoned");
        let unused = calls
            .iter()
            .filter(|call| !call.matched)
            .map(GivenCall::describe)
            .collect::<Vec<_>>();
        if unused.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "unused given: {}\n    The scenario declared this external call, but the route did not execute it.",
                unused.join(", ")
            ))
        }
    }
}

pub fn discover_scenario_files(project_root: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let tests_dir = project_root.join("tests");
    if !tests_dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    collect_scenario_files(&tests_dir, &mut files)?;
    files.sort();
    Ok(files)
}

pub fn load_scenario_file(path: &Path) -> Result<ScenarioFile, MarretaError> {
    let source = std::fs::read_to_string(path).map_err(|err| MarretaError::RuntimeError {
        message: format!("failed to read scenario file '{}': {err}", path.display()),
        line: 0,
        column: 0,
    })?;
    let tokens = Lexer::new(&source).tokenize()?;
    let program = Parser::new(tokens).parse()?;
    let scenarios = collect_scenarios(path, &program)?;

    Ok(ScenarioFile {
        path: path.to_path_buf(),
        scenarios,
    })
}

pub fn load_scenario_files(paths: &[PathBuf]) -> Result<Vec<ScenarioFile>, MarretaError> {
    paths.iter().map(|path| load_scenario_file(path)).collect()
}

fn collect_scenario_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_scenario_files(&path, out)?;
        } else if is_auto_discovered_scenario_file(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn is_auto_discovered_scenario_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with("_test.marreta"))
}

fn collect_scenarios(
    path: &Path,
    program: &[Statement],
) -> Result<Vec<ScenarioDefinition>, MarretaError> {
    let mut seen = HashSet::new();
    let mut scenarios = Vec::new();

    for stmt in program {
        if let Statement::Scenario {
            name,
            steps,
            line,
            column,
        } = stmt
        {
            if !seen.insert(name.clone()) {
                return Err(MarretaError::RuntimeError {
                    message: format!("duplicate scenario '{}' in '{}'", name, path.display()),
                    line: *line,
                    column: *column,
                });
            }
            scenarios.push(ScenarioDefinition {
                name: name.clone(),
                steps: steps.clone(),
                line: *line,
                column: *column,
            });
        }
    }

    Ok(scenarios)
}

pub async fn run_scenarios(
    loaded: &LoadedProject,
    files: &[ScenarioFile],
    filter: Option<&str>,
) -> Vec<ScenarioRun> {
    let mut results = Vec::new();
    let runtime = Arc::new(loaded.runtime.clone());

    for file in files {
        for scenario in &file.scenarios {
            if filter.is_some_and(|needle| !scenario.name.contains(needle)) {
                continue;
            }
            let result =
                run_single_scenario(&loaded.registry, Arc::clone(&runtime), file, scenario).await;
            results.push(result);
        }
    }

    results
}

async fn run_single_scenario(
    registry: &RouteRegistry,
    runtime: Arc<crate::file_loader::ProjectRuntime>,
    file: &ScenarioFile,
    scenario: &ScenarioDefinition,
) -> ScenarioRun {
    let plan = scenario_plan(&registry.routes, scenario);
    match execute_scenario(registry, runtime, scenario).await {
        Ok(()) => ScenarioRun {
            file: file.path.clone(),
            name: scenario.name.clone(),
            passed: true,
            error: None,
            route_verb: plan.route_verb,
            route_path: plan.route_path,
            assertion_count: plan.assertion_count,
            given_count: plan.given_count,
        },
        Err(error) => ScenarioRun {
            file: file.path.clone(),
            name: scenario.name.clone(),
            passed: false,
            error: Some(error),
            route_verb: plan.route_verb,
            route_path: plan.route_path,
            assertion_count: plan.assertion_count,
            given_count: plan.given_count,
        },
    }
}

fn scenario_plan(routes: &[RouteDefinition], scenario: &ScenarioDefinition) -> ScenarioPlan {
    let assertion_count = scenario
        .steps
        .iter()
        .filter(|step| {
            matches!(
                step,
                ScenarioStep::ThenStatus { .. } | ScenarioStep::ThenResponse { .. }
            )
        })
        .count();
    let given_count = scenario
        .steps
        .iter()
        .filter(|step| matches!(step, ScenarioStep::Given { .. }))
        .count();
    let route = scenario.steps.iter().find_map(|step| {
        if let ScenarioStep::When { verb, path, .. } = step {
            let (path, _) = split_path_and_query(path);
            find_route(routes, verb, &path)
                .map(|(route, _)| (route.verb.clone(), route.path.clone()))
        } else {
            None
        }
    });
    let (route_verb, route_path) = route
        .map(|(verb, path)| (Some(verb), Some(path)))
        .unwrap_or((None, None));

    ScenarioPlan {
        route_verb,
        route_path,
        assertion_count,
        given_count,
    }
}

/// Statically resolve which declared route a scenario targets, as a `route_key`
/// (`"VERB /path"`), reusing the same matching the runner uses.
///
/// Returns `None` when the scenario has no `when` step or its path resolves to
/// no declared route. This is the single public entry point for static
/// test-presence (used by `marreta doctor`); it never executes the scenario.
pub fn plan_scenario_route_presence(
    routes: &[RouteDefinition],
    scenario: &ScenarioDefinition,
) -> Option<String> {
    let plan = scenario_plan(routes, scenario);
    match (plan.route_verb, plan.route_path) {
        (Some(verb), Some(path)) => Some(crate::coverage::route_key(&verb, &path)),
        _ => None,
    }
}

async fn execute_scenario(
    registry: &RouteRegistry,
    runtime: Arc<crate::file_loader::ProjectRuntime>,
    scenario: &ScenarioDefinition,
) -> Result<(), String> {
    let mocks = Arc::new(ScenarioMockState::default());
    register_givens(&mocks, Arc::clone(&runtime), scenario)?;

    let when = scenario
        .steps
        .iter()
        .find_map(|step| {
            if let ScenarioStep::When {
                verb,
                path,
                body,
                headers,
                ..
            } = step
            {
                Some((verb, path, body, headers))
            } else {
                None
            }
        })
        .ok_or_else(|| "scenario has no when request".to_string())?;

    let (verb, raw_path, body_expr, headers_expr) = when;
    let (path, query_params) = split_path_and_query(raw_path);
    let (route, path_params) = find_route(&registry.routes, verb, &path)
        .ok_or_else(|| format!("route not found: {} {}", verb, path))?;

    let mut expr_interp =
        Interpreter::from_environment(runtime.env_for_module(route.module_id.as_deref()))
            .with_project_runtime(Arc::clone(&runtime))
            .with_current_module(route.module_id.clone());
    let body = match body_expr {
        Some(expr) => {
            let value = expr_interp
                .evaluate_pub(expr)
                .map_err(|err| err.display_message())?;
            Bytes::from(value_to_json(&value).to_string())
        }
        None => Bytes::new(),
    };
    let headers = match headers_expr {
        Some(expr) => expression_to_headers(&mut expr_interp, expr)?,
        None => HeaderMap::new(),
    };
    let trace_context = scenario_trace_context(&headers);

    let auth_runtime = scenario_auth_runtime(registry, route, &mocks)?;

    let response = execute_route(
        route.clone(),
        path_params,
        query_params,
        headers,
        trace_context,
        body,
        Arc::clone(&runtime),
        Arc::new(Some(DbEngine {
            driver: Arc::new(ScenarioDbDriver::new(Arc::clone(&mocks))),
            provider: DbProvider::Postgres,
        })),
        Arc::new(Some(crate::doc::mongodb::DocEngine {
            driver: Arc::new(ScenarioDocDriver::new(Arc::clone(&mocks))),
        })),
        Arc::new(Some(Arc::new(ScenarioQueueDriver::new(Arc::clone(&mocks))))),
        Arc::new(Some((
            Arc::new(ScenarioCacheDriver::new(Arc::clone(&mocks))),
            scenario_cache_config(),
        ))),
        Arc::new(Some(Arc::new(ScenarioHttpClient::new(Arc::clone(&mocks))))),
        auth_runtime,
    )
    .await;

    let status = response.status().as_u16() as i64;
    let headers = response.headers().clone();
    let body_bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .map_err(|err| format!("failed to read response body: {err}"))?;
    let body_json = if body_bytes.is_empty() {
        JsonValue::Null
    } else {
        serde_json::from_slice::<JsonValue>(&body_bytes).unwrap_or_else(|_| {
            JsonValue::String(String::from_utf8_lossy(&body_bytes).into_owned())
        })
    };

    for step in &scenario.steps {
        match step {
            ScenarioStep::ThenStatus {
                status: expected, ..
            } => {
                let expected = eval_integer(&mut expr_interp, expected)?;
                if status != expected {
                    return Err(format!("expected status {}, got {}", expected, status));
                }
            }
            ScenarioStep::ThenResponse { expected, .. } => {
                let actual = response_json(status, &headers, body_json.clone());
                assert_response_matches(expected, &actual)?;
            }
            _ => {}
        }
    }
    mocks.assert_all_consumed()?;

    Ok(())
}

fn scenario_trace_context(headers: &HeaderMap) -> Option<TraceContext> {
    if !headers.contains_key("traceparent") && !headers.contains_key("tracestate") {
        return None;
    }

    Some(TraceContext::from_headers(
        headers
            .get("traceparent")
            .and_then(|value| value.to_str().ok()),
        headers
            .get("tracestate")
            .and_then(|value| value.to_str().ok()),
    ))
}

fn register_givens(
    mocks: &Arc<ScenarioMockState>,
    runtime: Arc<crate::file_loader::ProjectRuntime>,
    scenario: &ScenarioDefinition,
) -> Result<(), String> {
    let mut interp =
        Interpreter::from_environment(runtime.env_for_module(None)).with_project_runtime(runtime);
    let mut registered = Vec::new();
    for step in &scenario.steps {
        let ScenarioStep::Given {
            target, returns, ..
        } = step
        else {
            continue;
        };
        let (key, args) = given_target_to_call(&mut interp, target)?;
        if registered.iter().any(|(registered_key, registered_args)| {
            registered_key == &key && registered_args == &args
        }) {
            return Err(format!(
                "duplicate given: {}({})",
                key.label(),
                args.iter()
                    .map(ValueMatcher::describe)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if matches!(returns, Expression::Identifier(name) if name == "anything") {
            return Err("anything is a matcher; it cannot be used as a return value".to_string());
        }
        let returns = interp
            .evaluate_pub(returns)
            .map_err(|err| err.display_message())?;
        mocks.register(GivenCall {
            key: key.clone(),
            args: args.clone(),
            returns,
            matched: false,
        });
        registered.push((key, args));
    }
    Ok(())
}

fn scenario_auth_runtime(
    registry: &RouteRegistry,
    route: &RouteDefinition,
    mocks: &Arc<ScenarioMockState>,
) -> Result<Arc<crate::server::AuthRuntime>, String> {
    let auth_registry =
        build_auth_registry(&registry.auth_providers).map_err(|err| err.display_message())?;
    let Some(route_auth) = &route.auth else {
        return Ok(Arc::new(crate::server::AuthRuntime::new(auth_registry)));
    };

    let key = GivenKey {
        namespace: "auth".to_string(),
        resource: None,
        method: route_auth.provider.clone(),
    };
    let overrides = match mocks.take(key, Vec::new()) {
        Ok(auth) => HashMap::from([(route_auth.provider.clone(), auth)]),
        Err(_) => HashMap::new(),
    };

    if overrides.is_empty() {
        Ok(Arc::new(crate::server::AuthRuntime::new(auth_registry)))
    } else {
        Ok(Arc::new(crate::server::AuthRuntime::with_auth_overrides(
            auth_registry,
            overrides,
        )))
    }
}

fn given_target_to_call(
    interp: &mut Interpreter,
    target: &Expression,
) -> Result<(GivenKey, Vec<ValueMatcher>), String> {
    match target {
        Expression::MethodCall {
            object,
            method,
            arguments,
        } => {
            let (namespace, resource) = given_object_path(object)?;
            let args = arguments_to_matchers(interp, arguments)?;
            Ok((
                GivenKey {
                    namespace,
                    resource,
                    method: method.clone(),
                },
                args,
            ))
        }
        Expression::QueuePush {
            queue_name,
            payload,
            ..
        } => {
            let mut args = vec![expression_to_matcher(interp, queue_name)?];
            if let Some(payload) = payload {
                args.push(expression_to_matcher(interp, payload)?);
            }
            Ok((
                GivenKey {
                    namespace: "queue".to_string(),
                    resource: None,
                    method: "push".to_string(),
                },
                args,
            ))
        }
        Expression::TopicPublish { topic, payload, .. } => {
            let mut args = vec![expression_to_matcher(interp, topic)?];
            if let Some(payload) = payload {
                args.push(expression_to_matcher(interp, payload)?);
            }
            Ok((
                // `topic.publish` is dispatched through the queue driver, which
                // registers the mock call under the "queue" namespace (method
                // "publish"). Match that here so `given topic.publish ...` resolves,
                // exactly as `given queue.push ...` does.
                GivenKey {
                    namespace: "queue".to_string(),
                    resource: None,
                    method: "publish".to_string(),
                },
                args,
            ))
        }
        Expression::PropertyAccess { object, property } if matches!(object.as_ref(), Expression::Identifier(name) if name == "auth") => {
            Ok((
                GivenKey {
                    namespace: "auth".to_string(),
                    resource: None,
                    method: property.clone(),
                },
                Vec::new(),
            ))
        }
        other => Err(format!("unsupported given target: {:?}", other)),
    }
}

fn given_object_path(object: &Expression) -> Result<(String, Option<String>), String> {
    match object {
        Expression::Identifier(name)
            if matches!(name.as_str(), "db" | "cache" | "http_client" | "queue") =>
        {
            Ok((name.clone(), None))
        }
        Expression::PropertyAccess { object, property } => match object.as_ref() {
            Expression::Identifier(namespace) if matches!(namespace.as_str(), "db" | "doc") => {
                Ok((namespace.clone(), Some(property.clone())))
            }
            other => Err(format!("unsupported given target object: {:?}", other)),
        },
        other => Err(format!("unsupported given target object: {:?}", other)),
    }
}

fn arguments_to_matchers(
    interp: &mut Interpreter,
    arguments: &[Argument],
) -> Result<Vec<ValueMatcher>, String> {
    arguments
        .iter()
        .map(|arg| match arg {
            Argument::Positional(expr) => expression_to_matcher(interp, expr),
            Argument::Named { name, value } => {
                let mut map = HashMap::new();
                map.insert(name.clone(), expression_to_matcher(interp, value)?);
                Ok(ValueMatcher::Map(map))
            }
        })
        .collect()
}

fn expression_to_matcher(
    interp: &mut Interpreter,
    expr: &Expression,
) -> Result<ValueMatcher, String> {
    if matches!(expr, Expression::Identifier(name) if name == "anything") {
        return Ok(ValueMatcher::Anything);
    }
    match expr {
        Expression::MapLiteral(pairs) => {
            let mut map = HashMap::new();
            for (key, value) in pairs {
                map.insert(key.clone(), expression_to_matcher(interp, value)?);
            }
            Ok(ValueMatcher::Map(map))
        }
        Expression::List(items) => {
            let mut matchers = Vec::new();
            for item in items {
                matchers.push(expression_to_matcher(interp, item)?);
            }
            Ok(ValueMatcher::List(matchers))
        }
        _ => interp
            .evaluate_pub(expr)
            .map(ValueMatcher::Exact)
            .map_err(|err| err.display_message()),
    }
}

fn expression_to_headers(interp: &mut Interpreter, expr: &Expression) -> Result<HeaderMap, String> {
    let value = interp
        .evaluate_pub(expr)
        .map_err(|err| err.display_message())?;
    let Value::Map(map) = value else {
        return Err("headers must evaluate to a map".to_string());
    };

    let mut headers = HeaderMap::new();
    for (key, value) in map.read().expect("headers map poisoned").iter() {
        let Value::String(value) = value else {
            return Err(format!("header '{}' must be a string", key));
        };
        headers.insert(
            key.parse::<axum::http::HeaderName>()
                .map_err(|err| format!("invalid header name '{}': {err}", key))?,
            value
                .parse::<axum::http::HeaderValue>()
                .map_err(|err| format!("invalid header value for '{}': {err}", key))?,
        );
    }
    Ok(headers)
}

fn eval_integer(interp: &mut Interpreter, expr: &Expression) -> Result<i64, String> {
    match interp
        .evaluate_pub(expr)
        .map_err(|err| err.display_message())?
    {
        Value::Integer(value) => Ok(value),
        other => Err(format!(
            "status assertion must evaluate to integer, got {}",
            other.type_name()
        )),
    }
}

fn response_json(status: i64, headers: &HeaderMap, body: JsonValue) -> JsonValue {
    let headers_json = headers
        .iter()
        .filter_map(|(key, value)| {
            value.to_str().ok().map(|value| {
                (
                    key.as_str().to_string(),
                    JsonValue::String(value.to_string()),
                )
            })
        })
        .collect::<serde_json::Map<String, JsonValue>>();
    serde_json::json!({
        "status": status,
        "headers": headers_json,
        "body": body,
    })
}

fn assert_response_matches(expected: &Expression, actual: &JsonValue) -> Result<(), String> {
    assert_expr_matches(expected, actual, "response")
}

fn assert_expr_matches(
    expected: &Expression,
    actual: &JsonValue,
    path: &str,
) -> Result<(), String> {
    if matches!(expected, Expression::Identifier(name) if name == "anything") {
        return Ok(());
    }

    match expected {
        Expression::Integer(value) => {
            assert_json_eq(path, &JsonValue::Number((*value).into()), actual)
        }
        Expression::Float(value) => {
            let expected = serde_json::Number::from_f64(*value)
                .map(JsonValue::Number)
                .unwrap_or(JsonValue::Null);
            assert_json_eq(path, &expected, actual)
        }
        Expression::StringLiteral(value) => {
            assert_json_eq(path, &JsonValue::String(value.clone()), actual)
        }
        Expression::Boolean(value) => assert_json_eq(path, &JsonValue::Bool(*value), actual),
        Expression::Null => assert_json_eq(path, &JsonValue::Null, actual),
        Expression::MapLiteral(pairs) => {
            let JsonValue::Object(actual_map) = actual else {
                return Err(format!("expected {} to be object, got {}", path, actual));
            };
            for (key, expected_value) in pairs {
                let actual_value = actual_map
                    .get(key)
                    .ok_or_else(|| format!("missing field {}.{}", path, key))?;
                assert_expr_matches(expected_value, actual_value, &format!("{}.{}", path, key))?;
            }
            Ok(())
        }
        Expression::List(items) => {
            let JsonValue::Array(actual_items) = actual else {
                return Err(format!("expected {} to be list, got {}", path, actual));
            };
            if items.len() != actual_items.len() {
                return Err(format!(
                    "expected {} to have {} items, got {}",
                    path,
                    items.len(),
                    actual_items.len()
                ));
            }
            for (idx, (expected_item, actual_item)) in items.iter().zip(actual_items).enumerate() {
                assert_expr_matches(expected_item, actual_item, &format!("{}[{}]", path, idx))?;
            }
            Ok(())
        }
        other => Err(format!(
            "unsupported response matcher at {}: {:?}",
            path, other
        )),
    }
}

fn assert_json_eq(path: &str, expected: &JsonValue, actual: &JsonValue) -> Result<(), String> {
    if expected == actual {
        Ok(())
    } else {
        Err(format!(
            "expected {} to be {}, got {}",
            path, expected, actual
        ))
    }
}

fn split_path_and_query(raw: &str) -> (String, HashMap<String, String>) {
    let (path, query) = raw.split_once('?').unwrap_or((raw, ""));
    let query_params = query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            (key.to_string(), value.to_string())
        })
        .collect();
    (path.to_string(), query_params)
}

fn find_route<'a>(
    routes: &'a [RouteDefinition],
    verb: &HttpVerb,
    path: &str,
) -> Option<(&'a RouteDefinition, HashMap<String, String>)> {
    for route in routes {
        if &route.verb != verb {
            continue;
        }
        if let Some(params) = match_path(&route.path, path) {
            return Some((route, params));
        }
    }
    None
}

fn match_path(pattern: &str, path: &str) -> Option<HashMap<String, String>> {
    let pattern_parts: Vec<_> = pattern
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let path_parts: Vec<_> = path
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();

    if pattern == "/" && path == "/" {
        return Some(HashMap::new());
    }
    if pattern_parts.len() != path_parts.len() {
        return None;
    }

    let mut params = HashMap::new();
    for (pattern_part, path_part) in pattern_parts.iter().zip(path_parts) {
        if let Some(name) = pattern_part.strip_prefix(':') {
            params.insert(name.to_string(), path_part.to_string());
        } else if *pattern_part != path_part {
            return None;
        }
    }
    Some(params)
}

fn scenario_cache_config() -> crate::cache::CacheConfig {
    crate::cache::CacheConfig {
        url: "scenario://cache".to_string(),
        prefix: String::new(),
        default_ttl: None,
        pool_size: 1,
        connect_timeout: Duration::from_secs(1),
        operation_timeout: Duration::from_secs(1),
        reconnect_max_retries: 0,
    }
}

fn value_to_row(value: Value, context: &str) -> Result<HashMap<String, Value>, MarretaError> {
    match value {
        Value::Map(map) => Ok(map
            .read()
            .expect("scenario row poisoned")
            .clone()
            .into_iter()
            .collect()),
        other => Err(MarretaError::TypeError {
            message: format!("{} must return a map, got {}", context, other.type_name()),
            line: 0,
            column: 0,
        }),
    }
}

fn value_to_rows(value: Value, context: &str) -> Result<Vec<HashMap<String, Value>>, MarretaError> {
    match value {
        Value::List(items) => items
            .into_iter()
            .map(|item| value_to_row(item, context))
            .collect(),
        other => Err(MarretaError::TypeError {
            message: format!("{} must return a list, got {}", context, other.type_name()),
            line: 0,
            column: 0,
        }),
    }
}

fn value_to_bool(value: Value, context: &str) -> Result<bool, MarretaError> {
    match value {
        Value::Boolean(value) => Ok(value),
        other => Err(MarretaError::TypeError {
            message: format!(
                "{} must return a boolean, got {}",
                context,
                other.type_name()
            ),
            line: 0,
            column: 0,
        }),
    }
}

fn value_to_integer(value: Value, context: &str) -> Result<i64, MarretaError> {
    match value {
        Value::Integer(value) => Ok(value),
        other => Err(MarretaError::TypeError {
            message: format!(
                "{} must return an integer, got {}",
                context,
                other.type_name()
            ),
            line: 0,
            column: 0,
        }),
    }
}

fn scenario_error(message: String, operation: &str) -> MarretaError {
    MarretaError::RuntimeError {
        message: format!("{operation}: {message}"),
        line: 0,
        column: 0,
    }
}

#[derive(Debug, Clone)]
struct ScenarioDbDriver {
    mocks: Arc<ScenarioMockState>,
}

impl ScenarioDbDriver {
    fn new(mocks: Arc<ScenarioMockState>) -> Self {
        Self { mocks }
    }

    fn call(&self, table: &str, method: &str, args: Vec<Value>) -> DbResult<Value> {
        let key = GivenKey {
            namespace: "db".to_string(),
            resource: Some(table.to_string()),
            method: method.to_string(),
        };
        self.mocks
            .take(key, args)
            .map_err(|message| scenario_error(message, &format!("db.{table}.{method}")))
    }

    fn call_namespace(&self, method: &str, args: Vec<Value>) -> DbResult<Value> {
        let key = GivenKey {
            namespace: "db".to_string(),
            resource: None,
            method: method.to_string(),
        };
        self.mocks
            .take(key, args)
            .map_err(|message| scenario_error(message, &format!("db.{method}")))
    }
}

#[async_trait]
impl DbDriver for ScenarioDbDriver {
    async fn save(&self, table: &str, data: DbRow) -> DbResult<DbRow> {
        value_to_row(
            self.call(
                table,
                "save",
                vec![Value::Map(Arc::new(RwLock::new(
                    data.into_iter().collect(),
                )))],
            )?,
            "db save given",
        )
    }

    async fn find(&self, table: &str, id: &Value) -> DbResult<Option<DbRow>> {
        match self.call(table, "find", vec![id.clone()])? {
            Value::Null => Ok(None),
            value => value_to_row(value, "db find given").map(Some),
        }
    }

    async fn find_all(&self, table: &str, _filters: Vec<FilterClause>) -> DbResult<Vec<DbRow>> {
        value_to_rows(
            self.call(table, "find_all", Vec::new())?,
            "db find_all given",
        )
    }

    async fn update_by_id(&self, table: &str, id: &Value, data: DbRow) -> DbResult<Option<DbRow>> {
        match self.call(
            table,
            "update",
            vec![
                id.clone(),
                Value::Map(Arc::new(RwLock::new(data.into_iter().collect()))),
            ],
        )? {
            Value::Null => Ok(None),
            value => value_to_row(value, "db update given").map(Some),
        }
    }

    async fn delete_by_id(&self, table: &str, id: &Value) -> DbResult<bool> {
        value_to_bool(
            self.call(table, "delete", vec![id.clone()])?,
            "db delete given",
        )
    }

    async fn query_fetch(&self, q: &QueryState) -> DbResult<Vec<DbRow>> {
        value_to_rows(self.call(&q.table, "fetch", Vec::new())?, "db fetch given")
    }

    async fn query_fetch_one(&self, q: &QueryState) -> DbResult<Option<DbRow>> {
        match self.call(&q.table, "fetch_one", Vec::new())? {
            Value::Null => Ok(None),
            value => value_to_row(value, "db fetch_one given").map(Some),
        }
    }

    async fn query_count(&self, q: &QueryState) -> DbResult<i64> {
        value_to_integer(self.call(&q.table, "count", Vec::new())?, "db count given")
    }

    async fn query_exists(&self, q: &QueryState) -> DbResult<bool> {
        value_to_bool(
            self.call(&q.table, "exists", Vec::new())?,
            "db exists given",
        )
    }

    async fn query_update(&self, q: &QueryState, _data: DbRow) -> DbResult<u64> {
        value_to_integer(
            self.call(&q.table, "update", Vec::new())?,
            "db query update given",
        )
        .map(|value| value as u64)
    }

    async fn query_delete(&self, q: &QueryState) -> DbResult<u64> {
        value_to_integer(
            self.call(&q.table, "delete", Vec::new())?,
            "db query delete given",
        )
        .map(|value| value as u64)
    }

    async fn native_query(&self, sql: &str, params: Vec<Value>) -> DbResult<Vec<DbRow>> {
        let mut args = vec![Value::String(sql.to_string())];
        args.extend(params);
        value_to_rows(
            self.call_namespace("native_query", args)?,
            "db native_query given",
        )
    }

    async fn begin(&self) -> DbResult<Box<dyn DbTx>> {
        Ok(Box::new(ScenarioDbTx::new(self.clone())))
    }
}

#[derive(Debug)]
struct ScenarioDbTx {
    driver: ScenarioDbDriver,
}

impl ScenarioDbTx {
    fn new(driver: ScenarioDbDriver) -> Self {
        Self { driver }
    }
}

#[async_trait]
impl DbTx for ScenarioDbTx {
    async fn save(&mut self, table: &str, data: DbRow) -> DbResult<DbRow> {
        self.driver.save(table, data).await
    }

    async fn find(&mut self, table: &str, id: &Value) -> DbResult<Option<DbRow>> {
        self.driver.find(table, id).await
    }

    async fn find_all(&mut self, table: &str, filters: Vec<FilterClause>) -> DbResult<Vec<DbRow>> {
        self.driver.find_all(table, filters).await
    }

    async fn update_by_id(
        &mut self,
        table: &str,
        id: &Value,
        data: DbRow,
    ) -> DbResult<Option<DbRow>> {
        self.driver.update_by_id(table, id, data).await
    }

    async fn delete_by_id(&mut self, table: &str, id: &Value) -> DbResult<bool> {
        self.driver.delete_by_id(table, id).await
    }

    async fn query_fetch(&mut self, q: &QueryState) -> DbResult<Vec<DbRow>> {
        self.driver.query_fetch(q).await
    }

    async fn query_fetch_one(&mut self, q: &QueryState) -> DbResult<Option<DbRow>> {
        self.driver.query_fetch_one(q).await
    }

    async fn query_count(&mut self, q: &QueryState) -> DbResult<i64> {
        self.driver.query_count(q).await
    }

    async fn query_exists(&mut self, q: &QueryState) -> DbResult<bool> {
        self.driver.query_exists(q).await
    }

    async fn query_update(&mut self, q: &QueryState, data: DbRow) -> DbResult<u64> {
        self.driver.query_update(q, data).await
    }

    async fn query_delete(&mut self, q: &QueryState) -> DbResult<u64> {
        self.driver.query_delete(q).await
    }

    async fn native_query(&mut self, sql: &str, params: Vec<Value>) -> DbResult<Vec<DbRow>> {
        self.driver.native_query(sql, params).await
    }

    async fn commit(self: Box<Self>) -> DbResult<()> {
        Ok(())
    }

    async fn rollback(self: Box<Self>) -> DbResult<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct ScenarioDocDriver {
    mocks: Arc<ScenarioMockState>,
}

impl ScenarioDocDriver {
    fn new(mocks: Arc<ScenarioMockState>) -> Self {
        Self { mocks }
    }

    fn call(&self, collection: &str, method: &str, args: Vec<Value>) -> DocResult<Value> {
        let key = GivenKey {
            namespace: "doc".to_string(),
            resource: Some(collection.to_string()),
            method: method.to_string(),
        };
        self.mocks
            .take(key, args)
            .map_err(|message| scenario_error(message, &format!("doc.{collection}.{method}")))
    }
}

#[async_trait]
impl DocDriver for ScenarioDocDriver {
    async fn save(&self, collection: &str, data: DocRow) -> DocResult<DocRow> {
        value_to_row(
            self.call(
                collection,
                "save",
                vec![Value::Map(Arc::new(RwLock::new(
                    data.into_iter().collect(),
                )))],
            )?,
            "doc save given",
        )
    }

    async fn find(&self, collection: &str, id: &Value) -> DocResult<Option<DocRow>> {
        match self.call(collection, "find", vec![id.clone()])? {
            Value::Null => Ok(None),
            value => value_to_row(value, "doc find given").map(Some),
        }
    }

    async fn find_all(&self, collection: &str) -> DocResult<Vec<DocRow>> {
        value_to_rows(
            self.call(collection, "find_all", Vec::new())?,
            "doc find_all given",
        )
    }

    async fn update_by_id(&self, collection: &str, id: &Value, data: DocRow) -> DocResult<DocRow> {
        value_to_row(
            self.call(
                collection,
                "update",
                vec![
                    id.clone(),
                    Value::Map(Arc::new(RwLock::new(data.into_iter().collect()))),
                ],
            )?,
            "doc update given",
        )
    }

    async fn delete_by_id(&self, collection: &str, id: &Value) -> DocResult<bool> {
        value_to_bool(
            self.call(collection, "delete", vec![id.clone()])?,
            "doc delete given",
        )
    }

    async fn query_fetch(&self, q: &DocQueryState) -> DocResult<Vec<DocRow>> {
        value_to_rows(
            self.call(&q.collection, "fetch", Vec::new())?,
            "doc fetch given",
        )
    }

    async fn query_fetch_one(&self, q: &DocQueryState) -> DocResult<Option<DocRow>> {
        match self.call(&q.collection, "fetch_one", Vec::new())? {
            Value::Null => Ok(None),
            value => value_to_row(value, "doc fetch_one given").map(Some),
        }
    }

    async fn query_count(&self, q: &DocQueryState) -> DocResult<i64> {
        value_to_integer(
            self.call(&q.collection, "count", Vec::new())?,
            "doc count given",
        )
    }

    async fn query_exists(&self, q: &DocQueryState) -> DocResult<bool> {
        value_to_bool(
            self.call(&q.collection, "exists", Vec::new())?,
            "doc exists given",
        )
    }

    async fn query_update(&self, q: &DocQueryState, _data: DocRow) -> DocResult<i64> {
        value_to_integer(
            self.call(&q.collection, "update", Vec::new())?,
            "doc update given",
        )
    }

    async fn query_upsert(&self, q: &DocQueryState, _data: DocRow) -> DocResult<i64> {
        value_to_integer(
            self.call(&q.collection, "upsert", Vec::new())?,
            "doc upsert given",
        )
    }

    async fn query_delete(&self, q: &DocQueryState) -> DocResult<i64> {
        value_to_integer(
            self.call(&q.collection, "delete", Vec::new())?,
            "doc delete given",
        )
    }

    async fn query_aggregate(&self, q: &DocQueryState) -> DocResult<Vec<DocRow>> {
        value_to_rows(
            self.call(&q.collection, "aggregate", Vec::new())?,
            "doc aggregate given",
        )
    }

    async fn raw_pipeline(&self, collection: &str, stages: &[Value]) -> DocResult<Vec<DocRow>> {
        value_to_rows(
            self.call(collection, "pipeline", vec![Value::List(stages.to_vec())])?,
            "doc pipeline given",
        )
    }
}

#[derive(Debug)]
struct ScenarioCacheDriver {
    mocks: Arc<ScenarioMockState>,
}

impl ScenarioCacheDriver {
    fn new(mocks: Arc<ScenarioMockState>) -> Self {
        Self { mocks }
    }

    fn call(&self, method: &str, args: Vec<Value>) -> CacheResult<Value> {
        let key = GivenKey {
            namespace: "cache".to_string(),
            resource: None,
            method: method.to_string(),
        };
        self.mocks
            .take(key, args)
            .map_err(CacheDriverError::OperationFailed)
    }
}

#[async_trait]
impl CacheDriver for ScenarioCacheDriver {
    async fn get(&self, key: &str) -> CacheResult<Option<Value>> {
        match self.call("get", vec![Value::String(key.to_string())])? {
            Value::Null => Ok(None),
            value => Ok(Some(value)),
        }
    }

    async fn set(
        &self,
        key: &str,
        value: &Value,
        _ttl: Option<Duration>,
        _only_if_absent: bool,
    ) -> CacheResult<Option<Value>> {
        match self.call("set", vec![Value::String(key.to_string()), value.clone()])? {
            Value::Null => Ok(None),
            value => Ok(Some(value)),
        }
    }

    async fn delete(&self, key: &str) -> CacheResult<bool> {
        value_to_bool(
            self.call("delete", vec![Value::String(key.to_string())])
                .map_err(|err| CacheDriverError::OperationFailed(err.to_string()))?,
            "cache delete given",
        )
        .map_err(|err| CacheDriverError::OperationFailed(err.display_message()))
    }

    async fn exists(&self, key: &str) -> CacheResult<bool> {
        value_to_bool(
            self.call("exists", vec![Value::String(key.to_string())])
                .map_err(|err| CacheDriverError::OperationFailed(err.to_string()))?,
            "cache exists given",
        )
        .map_err(|err| CacheDriverError::OperationFailed(err.display_message()))
    }

    async fn ttl(&self, key: &str) -> CacheResult<Option<Duration>> {
        match self.call("ttl", vec![Value::String(key.to_string())])? {
            Value::Null => Ok(None),
            Value::Integer(secs) => Ok(Some(Duration::from_secs(secs as u64))),
            other => Err(CacheDriverError::OperationFailed(format!(
                "cache ttl given must return integer or nil, got {}",
                other.type_name()
            ))),
        }
    }

    async fn expire(&self, key: &str, ttl: Duration) -> CacheResult<bool> {
        let args = vec![
            Value::String(key.to_string()),
            Value::Integer(ttl.as_secs() as i64),
        ];
        value_to_bool(self.call("expire", args)?, "cache expire given")
            .map_err(|err| CacheDriverError::OperationFailed(err.display_message()))
    }

    async fn incr(&self, key: &str, by: i64, _ttl: Option<Duration>) -> CacheResult<i64> {
        value_to_integer(
            self.call(
                "incr",
                vec![Value::String(key.to_string()), Value::Integer(by)],
            )?,
            "cache incr given",
        )
        .map_err(|err| CacheDriverError::OperationFailed(err.display_message()))
    }

    async fn decr(&self, key: &str, by: i64, _ttl: Option<Duration>) -> CacheResult<i64> {
        value_to_integer(
            self.call(
                "decr",
                vec![Value::String(key.to_string()), Value::Integer(by)],
            )?,
            "cache decr given",
        )
        .map_err(|err| CacheDriverError::OperationFailed(err.display_message()))
    }

    async fn get_many(&self, keys: &[String]) -> CacheResult<HashMap<String, Option<Value>>> {
        let result = self.call(
            "get_many",
            vec![Value::List(
                keys.iter().map(|key| Value::String(key.clone())).collect(),
            )],
        )?;
        let Value::Map(map) = result else {
            return Err(CacheDriverError::OperationFailed(
                "cache get_many given must return a map".to_string(),
            ));
        };
        Ok(map
            .read()
            .expect("scenario cache get_many map poisoned")
            .iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    if matches!(value, Value::Null) {
                        None
                    } else {
                        Some(value.clone())
                    },
                )
            })
            .collect())
    }

    async fn set_many(
        &self,
        entries: &HashMap<String, Value>,
        _ttl: Option<Duration>,
    ) -> CacheResult<()> {
        self.call(
            "set_many",
            vec![Value::Map(Arc::new(RwLock::new(
                entries.clone().into_iter().collect(),
            )))],
        )?;
        Ok(())
    }

    async fn ping(&self) -> CacheResult<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct ScenarioQueueDriver {
    mocks: Arc<ScenarioMockState>,
}

impl ScenarioQueueDriver {
    fn new(mocks: Arc<ScenarioMockState>) -> Self {
        Self { mocks }
    }

    fn call(&self, method: &str, args: Vec<Value>) -> QueueResult<Value> {
        let key = GivenKey {
            namespace: "queue".to_string(),
            resource: None,
            method: method.to_string(),
        };
        self.mocks
            .take(key, args)
            .map_err(QueueDriverError::PublishFailed)
    }
}

#[async_trait]
impl QueueDriver for ScenarioQueueDriver {
    async fn declare_queue(&self, _name: &str) -> QueueResult<()> {
        Ok(())
    }

    async fn bind_topic(&self, topic: &str) -> QueueResult<String> {
        Ok(format!("scenario.topic.{topic}"))
    }

    async fn push(&self, queue: &str, message: &QueueMessage) -> QueueResult<()> {
        self.call(
            "push",
            vec![Value::String(queue.to_string()), message.payload.clone()],
        )?;
        Ok(())
    }

    async fn publish(&self, topic: &str, message: &QueueMessage) -> QueueResult<()> {
        self.call(
            "publish",
            vec![Value::String(topic.to_string()), message.payload.clone()],
        )?;
        Ok(())
    }

    async fn consume_queue(
        &self,
        _queue: &str,
    ) -> QueueResult<futures_util::stream::BoxStream<'static, QueueDelivery>> {
        Ok(Box::pin(stream::empty()))
    }

    async fn consume_topic(
        &self,
        _queue_name: &str,
    ) -> QueueResult<futures_util::stream::BoxStream<'static, QueueDelivery>> {
        Ok(Box::pin(stream::empty()))
    }

    async fn ack(&self, _tag: u64) -> QueueResult<()> {
        Ok(())
    }

    async fn nack(&self, _tag: u64, _requeue: bool) -> QueueResult<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct ScenarioHttpClient {
    mocks: Arc<ScenarioMockState>,
}

impl ScenarioHttpClient {
    fn new(mocks: Arc<ScenarioMockState>) -> Self {
        Self { mocks }
    }
}

#[async_trait]
impl HttpClient for ScenarioHttpClient {
    async fn execute(&self, request: HttpRequest) -> HttpClientResult<HttpClientResponse> {
        let method = request.method.to_string().to_lowercase();
        let mut args = vec![Value::String(request.url)];
        if let Some(body) = request.body {
            args.push(body);
        }
        let key = GivenKey {
            namespace: "http_client".to_string(),
            resource: None,
            method: method.clone(),
        };
        let value = self
            .mocks
            .take(key, args)
            .map_err(HttpClientDriverError::RequestFailed)?;
        http_response_from_value(value).map_err(HttpClientDriverError::RequestFailed)
    }
}

fn http_response_from_value(value: Value) -> Result<HttpClientResponse, String> {
    let Value::Map(map) = value else {
        return Err("http_client given must return a response map".to_string());
    };
    let guard = map.read().expect("scenario http response poisoned");
    let status = match guard.get("status") {
        Some(Value::Integer(status)) => *status as u16,
        Some(other) => {
            return Err(format!(
                "http_client response status must be integer, got {}",
                other.type_name()
            ));
        }
        None => 200,
    };
    let body = guard.get("body").cloned().unwrap_or(Value::Null);
    let headers = match guard.get("headers") {
        Some(Value::Map(headers_map)) => headers_map
            .read()
            .expect("scenario http headers poisoned")
            .iter()
            .map(|(key, value)| (key.clone(), value.to_string()))
            .collect(),
        Some(other) => {
            return Err(format!(
                "http_client response headers must be map, got {}",
                other.type_name()
            ));
        }
        None => HashMap::new(),
    };
    Ok(HttpClientResponse {
        status,
        body,
        headers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_scenario_file_collects_scenarios() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orders_test.marreta");
        std::fs::write(
            &path,
            "scenario \"health\"\n    when GET \"/health\"\n    then status 200\n",
        )
        .unwrap();

        let file = load_scenario_file(&path).unwrap();
        assert_eq!(file.scenarios.len(), 1);
        assert_eq!(file.scenarios[0].name, "health");
    }

    #[test]
    fn discover_scenario_files_uses_suffix_convention() {
        let dir = tempfile::tempdir().unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        std::fs::write(tests.join("orders_test.marreta"), "").unwrap();
        std::fs::write(tests.join("orders.marreta"), "").unwrap();

        let files = discover_scenario_files(dir.path()).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("orders_test.marreta"));
    }

    #[test]
    fn plan_scenario_route_presence_matches_and_misses() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "presence-demo"
project_version = "1.0.0"

route GET "/orders/:id"
    reply 200, { id: id }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let path = tests.join("orders_test.marreta");
        std::fs::write(
            &path,
            r#"scenario "matches a declared route"
    when GET "/orders/42"
    then status 200

scenario "no matching route"
    when GET "/ghost"
    then status 404
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let file = load_scenario_file(&path).unwrap();

        assert_eq!(
            plan_scenario_route_presence(&loaded.registry.routes, &file.scenarios[0]),
            Some("GET /orders/:id".to_string())
        );
        assert_eq!(
            plan_scenario_route_presence(&loaded.registry.routes, &file.scenarios[1]),
            None
        );
    }

    #[test]
    fn match_path_handles_root_params_and_trailing_slashes() {
        assert_eq!(match_path("/", "/"), Some(HashMap::new()));
        assert_eq!(match_path("/orders/:id", "/orders/42").unwrap()["id"], "42");
        assert_eq!(
            match_path("/orgs/:org_id/users/:user_id", "/orgs/7/users/9").unwrap()["user_id"],
            "9"
        );
        assert!(match_path("/orders/:id", "/orders").is_none());
        assert!(match_path("/orders/:id", "/orders/42/items").is_none());
        assert!(match_path("/orders", "/orders/").is_some());
    }

    #[tokio::test]
    async fn run_scenarios_executes_http_route_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

route POST "/orders" take payload
    reply 201, { id: 10, product_id: payload.product_id, extra: true }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("orders_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "create order"
    when POST "/orders" with { product_id: 7 }
    then status 201
    then response is { body: { id: anything, product_id: 7 } }
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "{:?}", results[0].error);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_scenarios_uses_given_backed_db_driver() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

route GET "/orders/:id"
    order = db.orders.find(id)
    reply 200, { order: order }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("orders_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "get order"
    given db.orders.find(10) returns { id: 10, total: 99 }
    when GET "/orders/10"
    then status 200
    then response is { body: { order: { id: 10, total: anything } } }
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "{:?}", results[0].error);
    }

    #[tokio::test]
    async fn run_scenarios_authorizes_jwt_route_with_auth_given() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

auth jwt customer_auth {
    issuer: "https://issuer.example.test"
    audience: "scenario-api"
}

route GET "/orders"
    require auth customer_auth
    allow "admin" in auth.user.roles
    reply 200, { user_id: auth.user.id, provider: auth.provider }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("orders_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "authorized jwt route"
    given auth.customer_auth returns { sub: "user-123", roles: ["admin"] }
    when GET "/orders"
    then status 200
    then response is { body: { user_id: "user-123", provider: "customer_auth" } }
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "{:?}", results[0].error);
    }

    #[tokio::test]
    async fn run_scenarios_rejects_jwt_route_without_auth_given() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

auth jwt customer_auth {
    issuer: "https://issuer.example.test"
    audience: "scenario-api"
}

route GET "/orders"
    require auth customer_auth
    reply 200, { ok: true }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("orders_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "unauthorized jwt route"
    when GET "/orders"
    then status 401
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "{:?}", results[0].error);
    }

    #[tokio::test]
    async fn run_scenarios_forbids_jwt_route_when_allow_fails() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

auth jwt customer_auth {
    issuer: "https://issuer.example.test"
    audience: "scenario-api"
}

route GET "/orders"
    require auth customer_auth
    allow "admin" in auth.user.roles
    reply 200, { ok: true }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("orders_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "forbidden jwt route"
    given auth.customer_auth returns { sub: "user-123", roles: ["customer"] }
    when GET "/orders"
    then status 403
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "{:?}", results[0].error);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_scenarios_allows_same_given_to_match_more_than_once() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

route GET "/orders/:id"
    first = db.orders.find(id)
    second = db.orders.find(id)
    reply 200, { first: first, second: second }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("orders_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "get order twice"
    given db.orders.find(10) returns { id: 10, total: 99 }
    when GET "/orders/10"
    then status 200
    then response is { body: { first: { id: 10 }, second: { total: 99 } } }
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "{:?}", results[0].error);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_scenarios_prefers_more_specific_given_match() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

route GET "/orders"
    specific = db.orders.find(42)
    generic = db.orders.find(7)
    reply 200, { specific: specific, generic: generic }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("orders_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "specific matcher wins"
    given db.orders.find(anything) returns { id: 0, kind: "generic" }
    given db.orders.find(42) returns { id: 42, kind: "specific" }
    when GET "/orders"
    then status 200
    then response is {
        body: {
            specific: { id: 42, kind: "specific" },
            generic: { id: 0, kind: "generic" }
        }
    }
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "{:?}", results[0].error);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_scenarios_supports_transaction_with_given_backed_driver() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

route POST "/orders" take payload
    transaction
        order = db.orders.save({ product_id: payload.product_id })
    reply 201, { order: order }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("orders_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "create order transactionally"
    given db.orders.save(anything) returns { id: 99, product_id: 10 }
    when POST "/orders" with { product_id: 10 }
    then status 201
    then response is { body: { order: { id: 99, product_id: 10 } } }
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "{:?}", results[0].error);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_scenarios_rejects_anything_as_return_value() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

route GET "/orders/:id"
    order = db.orders.find(id)
    reply 200, { order: order }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("orders_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "get order"
    given db.orders.find(10) returns anything
    when GET "/orders/10"
    then status 200
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(
            results[0]
                .error
                .as_ref()
                .unwrap()
                .contains("anything is a matcher; it cannot be used as a return value")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_scenarios_rejects_duplicate_given() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

route GET "/orders/:id"
    order = db.orders.find(id)
    reply 200, { order: order }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("orders_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "get order"
    given db.orders.find(10) returns { id: 10 }
    given db.orders.find(10) returns { id: 10 }
    when GET "/orders/10"
    then status 200
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(
            results[0]
                .error
                .as_ref()
                .unwrap()
                .contains("duplicate given: db.orders.find(10)")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_scenarios_mocks_native_query_with_production_shape() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

route GET "/native"
    rows = db.native_query("SELECT 1")
    reply 200, { rows: rows }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("native_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "native query"
    given db.native_query("SELECT 1") returns [{ value: 1 }]
    when GET "/native"
    then status 200
    then response is { body: { rows: [{ value: 1 }] } }
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "{:?}", results[0].error);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_scenarios_stops_at_first_then_failure() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

route GET "/health"
    reply 200, { ok: true }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("health_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "first failure"
    when GET "/health"
    then status 201
    then response is { body: { ok: false } }
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 1);
        let error = results[0].error.as_ref().unwrap();
        assert!(error.contains("expected status 201, got 200"));
        assert!(!error.contains("ok"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_scenarios_matches_nested_anything_in_response_lists_and_maps() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

route GET "/orders"
    reply 200, { orders: [{ id: 1, meta: { token: "abc" } }] }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("orders_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "nested anything"
    when GET "/orders"
    then response is { body: { orders: [{ id: anything, meta: { token: anything } }] } }
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "{:?}", results[0].error);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_scenarios_honors_filter_by_scenario_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

route GET "/health"
    reply 200, { ok: true }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("health_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "included health"
    when GET "/health"
    then status 200

scenario "skipped health"
    when GET "/health"
    then status 500
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], Some("included")).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "included health");
        assert!(results[0].passed, "{:?}", results[0].error);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_scenarios_binds_query_payload_headers_and_raw() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

route GET "/search" take query
    reply 200, { term: query.term }

route POST "/payload" take payload
    reply 200, { name: payload.name }

route GET "/headers" take headers
    reply 200, { accept: headers.accept }

route POST "/raw" take raw
    reply 200, { raw: raw }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("bindings_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "query binding"
    when GET "/search?term=books"
    then response is { body: { term: "books" } }

scenario "payload binding"
    when POST "/payload" with { name: "Ana" }
    then response is { body: { name: "Ana" } }

scenario "headers binding"
    when GET "/headers" and headers { accept: "application/json" }
    then response is { body: { accept: "application/json" } }

scenario "raw binding"
    when POST "/raw" with "plain text"
    then response is { body: { raw: "\"plain text\"" } }
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 4);
        assert!(results.iter().all(|result| result.passed), "{results:?}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_scenarios_rejects_non_string_headers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

route GET "/headers" take headers
    reply 200, { ok: true }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("headers_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "bad headers"
    when GET "/headers" and headers { accept: 10 }
    then status 200
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(
            results[0]
                .error
                .as_ref()
                .unwrap()
                .contains("header 'accept' must be a string")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_scenarios_supports_computed_then_status() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

route GET "/created"
    reply 201, { ok: true }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("created_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "computed status"
    when GET "/created"
    then status 200 + 1
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "{:?}", results[0].error);
    }

    #[tokio::test]
    async fn run_scenarios_fails_unused_given() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.marreta"),
            r#"project_name = "scenario-demo"
project_version = "1.0.0"

route GET "/health"
    reply 200, { ok: true }
"#,
        )
        .unwrap();
        let tests = dir.path().join("tests");
        std::fs::create_dir(&tests).unwrap();
        let scenario_path = tests.join("health_test.marreta");
        std::fs::write(
            &scenario_path,
            r#"scenario "health"
    given db.orders.find(10) returns { id: 10 }
    when GET "/health"
    then status 200
"#,
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&dir.path().join("app.marreta")).unwrap();
        let scenario_file = load_scenario_file(&scenario_path).unwrap();
        let results = run_scenarios(&loaded, &[scenario_file], None).await;

        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(
            results[0]
                .error
                .as_ref()
                .unwrap()
                .contains("unused given: db.orders.find(10)")
        );
    }
}
