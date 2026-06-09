//! HTTP integration tests — start a real axum server, make real HTTP requests via reqwest.
//!
//! Each test calls `start_test_server(source)` which:
//!   1. Parses the source
//!   2. Loads routes via `route_loader::load()`
//!   3. Executes startup statements
//!   4. Binds to a random free port on 127.0.0.1
//!   5. Spawns the server in a background tokio thread
//!   6. Returns the port for the test to use

use std::sync::Arc;

use marreta::file_loader::{self, ProjectRuntime};
use marreta::interpreter::Interpreter;
use marreta::lexer::Lexer;
use marreta::parser::Parser;
use marreta::route_loader;
use marreta::server::{ServerConfig, serve};

/// Parses `source`, starts a server on a random port, returns the port.
/// Panics if the source has errors or the server fails to bind.
fn start_test_server(source: &str) -> u16 {
    let tokens = Lexer::new(source).tokenize().expect("tokenize failed");
    let program = Parser::new(tokens).parse().expect("parse failed");
    let registry = route_loader::load(program, None).expect("route_loader failed");

    let mut interp = Interpreter::new();
    interp
        .execute(&registry.startup_stmts)
        .expect("startup failed");
    let runtime = Arc::new(ProjectRuntime::single(
        interp.into_environment(),
        registry.schemas.clone(),
    ));

    // Bind port=0 to let the OS pick a free port
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener); // release so axum can bind it

    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port,
        cors_enabled: false,
        cors_origin: "*".to_string(),
        docs_enabled: false,
        docs_path: "/docs".to_string(),
        db_engine: None,
        doc_engine: None,
        queue_driver: None,
        cache_engine: None,
        http_client_driver: None,
        request_log_enabled: false,
        trace_context_enabled: false,
        startup_started_at: None,
    };

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            serve(registry, runtime, config).await.unwrap();
        });
    });

    // Wait until the server is actually accepting connections. Polling is robust
    // under parallel test load, where a fixed sleep can elapse before the
    // background thread has bound the port (causing connection-refused flakes).
    let addr = format!("127.0.0.1:{port}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if std::net::TcpStream::connect(&addr).is_ok() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "test server on port {port} did not become ready within 10s"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    port
}

// =============================================================================
// Scenario 1 — GET returns 200 with JSON body
// =============================================================================

#[test]
fn test_get_returns_200_with_json() {
    let port = start_test_server(
        r#"
route GET "/hello"
    reply 200, { message: "hello" }
"#,
    );
    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/hello", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["message"], "hello");
}

// =============================================================================
// Scenario 2 — POST /echo take payload → 201 echoing body
// =============================================================================

#[test]
fn test_post_echo_payload() {
    let port = start_test_server(
        r#"
route POST "/echo" take payload
    reply 201, payload
"#,
    );
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{}/echo", port))
        .json(&serde_json::json!({ "x": 42 }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["x"], 42);
}

// =============================================================================
// Scenario 3 — URL param :id injected as variable
// =============================================================================

#[test]
fn test_url_param_injected() {
    let port = start_test_server(
        r#"
route GET "/users/:id"
    reply 200, { id: id }
"#,
    );
    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/users/99", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["id"], 99);
}

// =============================================================================
// Scenario 4 — take query binds query string params
// =============================================================================

#[test]
fn test_take_query_binds_params() {
    let port = start_test_server(
        r#"
route GET "/search" take query
    reply 200, { q: query.q }
"#,
    );
    let resp =
        reqwest::blocking::get(format!("http://127.0.0.1:{}/search?q=marreta", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["q"], "marreta");
}

// =============================================================================
// Scenario 5 — take headers binds request headers
// =============================================================================

#[test]
fn test_take_headers_binds_headers() {
    // Note: property access uses dot notation, so header names must be valid identifiers.
    // We send "xtoken" (no hyphens) as a custom header.
    let port = start_test_server(
        r#"
route GET "/whoami" take headers
    reply 200, { token: headers.xtoken }
"#,
    );
    let client = reqwest::blocking::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/whoami", port))
        .header("xtoken", "secret123")
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["token"], "secret123");
}

// =============================================================================
// Scenario 6 — require failing → HttpError → 400
// =============================================================================

#[test]
fn test_require_failing_returns_error() {
    let port = start_test_server(
        r#"
route GET "/guarded"
    require false else fail 400, "bad request"
    reply 200, { ok: true }
"#,
    );
    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/guarded", port)).unwrap();
    assert_eq!(resp.status(), 400);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["error"], "bad request");
}

// =============================================================================
// Scenario 7 — reject firing → 402
// =============================================================================

#[test]
fn test_reject_fires_http_error() {
    let port = start_test_server(
        r#"
route GET "/nope"
    reject true else fail 402, "payment required"
    reply 200, { ok: true }
"#,
    );
    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/nope", port)).unwrap();
    assert_eq!(resp.status(), 402);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["error"], "payment required");
}

// =============================================================================
// Scenario 8 — fail 404, "Not found" → 404 with error JSON
// =============================================================================

#[test]
fn test_fail_returns_error_json() {
    let port = start_test_server(
        r#"
route GET "/gone"
    fail 404, "not found"
"#,
    );
    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/gone", port)).unwrap();
    assert_eq!(resp.status(), 404);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["error"], "not found");
}

// =============================================================================
// Scenario 9 — Route with task call defined at top level
// =============================================================================

#[test]
fn test_startup_task_accessible_in_route() {
    let port = start_test_server(
        r#"
task double(n) => n * 2

route POST "/double" take payload
    result = double(payload.value)
    reply 200, { value: result }
"#,
    );
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{}/double", port))
        .json(&serde_json::json!({ "value": 7 }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["value"], 14);
}

// =============================================================================
// Scenario 10 — Route with match expression
// =============================================================================

#[test]
fn test_route_with_match_expression() {
    let port = start_test_server(
        r#"
route GET "/status/:code"
    label = match code
        200 -> "ok"
        404 -> "not found"
        _ -> "unknown"
    reply 200, { label: label }
"#,
    );
    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/status/404", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["label"], "not found");
}

// =============================================================================
// Scenario 11 — Route with pipeline
// =============================================================================

#[test]
fn test_route_with_pipeline() {
    let port = start_test_server(
        r#"
task double(n) => n * 2

route GET "/pipeline"
    result = [1, 2, 3] >> double
    reply 200, { values: result }
"#,
    );
    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/pipeline", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["values"], serde_json::json!([2, 4, 6]));
}

// =============================================================================
// Scenario 12 — Route with `or` default value from query
// =============================================================================

#[test]
fn test_or_default_from_query() {
    let port = start_test_server(
        r#"
route GET "/greet" take query
    name = query.name or "stranger"
    reply 200, { message: "hello #{name}" }
"#,
    );
    // With query param
    let resp =
        reqwest::blocking::get(format!("http://127.0.0.1:{}/greet?name=Thiago", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["message"], "hello Thiago");

    // Without query param — should use default
    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/greet", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["message"], "hello stranger");
}

// =============================================================================
// Scenario 13 — Unknown route → 404 (axum default)
// =============================================================================

#[test]
fn test_unknown_route_returns_404() {
    let port = start_test_server(
        r#"
route GET "/hello"
    reply 200, { ok: true }
"#,
    );
    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/unknown", port)).unwrap();
    assert_eq!(resp.status(), 404);
}

// =============================================================================
// Scenario 14 — Route with no reply → 204 No Content
// =============================================================================

#[test]
fn test_route_no_reply_returns_204() {
    let port = start_test_server(
        r#"
route GET "/silent"
    x = 1 + 1
"#,
    );
    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/silent", port)).unwrap();
    assert_eq!(resp.status(), 204);
}

// =============================================================================
// Scenario 15 — reply 200, null → 200 with null body
// =============================================================================

#[test]
fn test_reply_null_body() {
    let port = start_test_server(
        r#"
route GET "/nullbody"
    reply 200, null
"#,
    );
    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/nullbody", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert!(json.is_null());
}

// =============================================================================
// Scenario 16 — Startup constant visible inside route body
// =============================================================================

#[test]
fn test_startup_constant_visible_in_route() {
    let port = start_test_server(
        r#"
version = "2.0"

route GET "/version"
    reply 200, { version: version }
"#,
    );
    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/version", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["version"], "2.0");
}

// =============================================================================
// Scenario 17 — Duplicate exact route → RouteConflict at boot
// =============================================================================

#[test]
fn test_duplicate_route_conflict_at_boot() {
    let tokens = Lexer::new(
        r#"
route GET "/users"
    reply 200, { ok: true }

route GET "/users"
    reply 200, { ok: false }
"#,
    )
    .tokenize()
    .unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    let result = route_loader::load(program, None);
    assert!(
        matches!(
            result,
            Err(marreta::error::MarretaError::RouteConflict { .. })
        ),
        "expected RouteConflict, got {:?}",
        result
    );
}

// =============================================================================
// Scenario 18 — Same path different param names → RouteConflict at boot
// =============================================================================

#[test]
fn test_same_pattern_different_param_names_conflict() {
    let tokens = Lexer::new(
        r#"
route GET "/users/:id"
    reply 200, { id: id }

route GET "/users/:name"
    reply 200, { name: name }
"#,
    )
    .tokenize()
    .unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    let result = route_loader::load(program, None);
    assert!(matches!(
        result,
        Err(marreta::error::MarretaError::RouteConflict { .. })
    ));
}

// =============================================================================
// Scenario 19 — Literal + param same segment → allowed, both work correctly
// =============================================================================

#[test]
fn test_literal_and_param_routes_coexist() {
    let port = start_test_server(
        r#"
route GET "/users/active"
    reply 200, { kind: "active" }

route GET "/users/:id"
    reply 200, { kind: "by-id", id: id }
"#,
    );
    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/users/active", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["kind"], "active");

    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/users/42", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["kind"], "by-id");
    assert_eq!(json["id"], 42);
}

// =============================================================================
// v0.2.1 — Feature 1: Multiple take bindings
// =============================================================================

#[test]
fn test_multiple_take_payload_and_headers() {
    let port = start_test_server(
        r#"
route POST "/checkout" take payload, headers
    require headers.xtoken else fail 401, "Token ausente"
    require payload.cart else fail 400, "Carrinho vazio"
    reply 201, { ok: true, cart: payload.cart }
"#,
    );
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{}/checkout", port))
        .header("xtoken", "secret")
        .json(&serde_json::json!({ "cart": "item1" }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["cart"], "item1");
}

#[test]
fn test_multiple_take_missing_auth_returns_401() {
    let port = start_test_server(
        r#"
route POST "/secure" take payload, headers
    require headers.xtoken else fail 401, "Token ausente"
    reply 200, { ok: true }
"#,
    );
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{}/secure", port))
        .json(&serde_json::json!({ "x": 1 }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 401);
}

// =============================================================================
// v0.2.1 — Feature 3: take form / take raw
// =============================================================================

#[test]
fn test_take_form_binds_form_data() {
    let port = start_test_server(
        r#"
route POST "/contact" take form
    require form.email else fail 422, "email required"
    reply 200, { received: form.email }
"#,
    );
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{}/contact", port))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("email=test@example.com&name=Test")
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["received"], "test@example.com");
}

#[test]
fn test_take_raw_delivers_string_body() {
    let port = start_test_server(
        r#"
route POST "/webhook" take raw
    require raw else fail 400, "empty body"
    reply 200, { body: raw }
"#,
    );
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{}/webhook", port))
        .header("Content-Type", "text/plain")
        .body("payload=abc123")
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["body"], "payload=abc123");
}

// =============================================================================
// v0.2.1 — Feature 4: Response modifiers
// =============================================================================

#[test]
fn test_reply_html_returns_html_content_type() {
    let port = start_test_server(
        r#"
route GET "/page"
    reply html 200, "<h1>Hello</h1>"
"#,
    );
    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/page", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("text/html"));
    assert_eq!(resp.text().unwrap(), "<h1>Hello</h1>");
}

#[test]
fn test_reply_text_returns_plain_content_type() {
    let port = start_test_server(
        r#"
route GET "/ping"
    reply text 200, "pong"
"#,
    );
    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/ping", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("text/plain"));
    assert_eq!(resp.text().unwrap(), "pong");
}

#[test]
fn test_reply_redirect_with_location_header() {
    let port = start_test_server(
        r#"
route GET "/old"
    reply 302, null, { Location: "https://example.com/new" }
"#,
    );
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let resp = client
        .get(format!("http://127.0.0.1:{}/old", port))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 302);
    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(location, "https://example.com/new");
}

// =============================================================================
// v0.2.1 — Feature 6: 204 No Content — zero byte body
// =============================================================================

#[test]
fn test_204_reply_returns_empty_body() {
    let port = start_test_server(
        r#"
route DELETE "/item/:id"
    reply 204, null
"#,
    );
    let client = reqwest::blocking::Client::new();
    let resp = client
        .delete(format!("http://127.0.0.1:{}/item/42", port))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 204);
    assert_eq!(resp.bytes().unwrap().len(), 0);
}

// =============================================================================
// v0.3.2 — Scope Isolation: multi-file server helper + tests
// =============================================================================

/// Writes files from a slice of (relative_path, content) pairs into a TempDir,
/// loads the project via `file_loader::load_project`, starts a server, returns port.
fn start_project_server(files: &[(&str, &str)]) -> (u16, tempfile::TempDir) {
    let dir = tempfile::TempDir::new().unwrap();
    for (rel, content) in files {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
    }

    let entrypoint = dir.path().join("app.marreta");
    let loaded = file_loader::load_project(&entrypoint).expect("load_project failed");
    let registry = loaded.registry;
    let runtime = Arc::new(loaded.runtime);

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port,
        cors_enabled: false,
        cors_origin: "*".to_string(),
        docs_enabled: false,
        docs_path: "/docs".to_string(),
        db_engine: None,
        doc_engine: None,
        queue_driver: None,
        cache_engine: None,
        http_client_driver: None,
        request_log_enabled: false,
        trace_context_enabled: false,
        startup_started_at: None,
    };

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            serve(registry, runtime, config).await.unwrap();
        });
    });

    // Wait until the server is accepting connections (robust under parallel load).
    let addr = format!("127.0.0.1:{port}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if std::net::TcpStream::connect(&addr).is_ok() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "test server on port {port} did not become ready within 10s"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    (port, dir)
}

#[test]
fn test_multifile_exported_task_callable_from_route() {
    // Spec 061: an exported task is reached cross-file via its file-namespace, `calc.double(5)`.
    let (port, _dir) = start_project_server(&[
        (
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/price"
    result = calc.double(5)
    reply 200, { value: result }
"#,
        ),
        ("tasks/calc.marreta", "export task double(x) => x * 2\n"),
    ]);

    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/price", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["value"], 10);
}

#[test]
fn test_multifile_exported_task_bare_cross_file_no_longer_resolves() {
    // Spec 061 breaking change: a bare cross-file call to an exported task is undefined;
    // the call site must name the file-namespace (`calc.double`).
    let (port, _dir) = start_project_server(&[
        (
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/price"
    result = double(5)
    reply 200, { value: result }
"#,
        ),
        ("tasks/calc.marreta", "export task double(x) => x * 2\n"),
    ]);

    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/price", port)).unwrap();
    assert_eq!(resp.status(), 500);
    let json: serde_json::Value = resp.json().unwrap();
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("task 'double' is not defined")
    );
}

#[test]
fn test_local_variable_shadows_file_namespace() {
    // Spec 061 precedence: a value bound in scope named like a file-namespace wins,
    // so `greet.length` is the string method, not a call into the `greet` namespace.
    let (port, _dir) = start_project_server(&[
        (
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/shadow"
    greet = "hello"
    reply 200, { value: greet.length() }
"#,
        ),
        ("tasks/greet.marreta", "export task length(x) => 999\n"),
    ]);

    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/shadow", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["value"], 5);
}

#[test]
fn test_multifile_private_task_callable_within_same_file() {
    let (port, _dir) = start_project_server(&[
        (
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/health"
    reply 200, { ok: true }
"#,
        ),
        (
            "routes/math.marreta",
            r#"
task double(x) => x * 2
multiplier = 3

route GET "/math/private"
    reply 200, { value: double(7), factor: multiplier }
"#,
        ),
    ]);

    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/math/private", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["value"], 14);
    assert_eq!(json["factor"], 3);
}

#[test]
fn test_multifile_private_task_not_callable_from_other_file() {
    let (port, _dir) = start_project_server(&[
        (
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/price"
    reply 200, { ok: true }
"#,
        ),
        ("tasks/math.marreta", "task double(x) => x * 2\n"),
        (
            "routes/pricing.marreta",
            r#"
route GET "/pricing/private-cross-file"
    result = double(5)
    reply 200, { value: result }
"#,
        ),
    ]);

    let resp = reqwest::blocking::get(format!(
        "http://127.0.0.1:{}/pricing/private-cross-file",
        port
    ))
    .unwrap();
    assert_eq!(resp.status(), 500);
    let json: serde_json::Value = resp.json().unwrap();
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("task 'double' is not defined")
    );
}

#[test]
fn test_namespaced_call_to_private_task_reports_qualified_name() {
    // Spec 061: a private task reached as `calc.helper` (the file is a known namespace
    // but `helper` is not exported) reports the qualified task as undefined, not a
    // misleading "variable 'calc' is not defined".
    let (port, _dir) = start_project_server(&[
        (
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/health"
    reply 200, { ok: true }
"#,
        ),
        (
            "tasks/calc.marreta",
            "task helper(x) => x + 1\nexport task double(x) => x * 2\n",
        ),
        (
            "routes/pricing.marreta",
            r#"
route GET "/pricing/private-namespaced"
    reply 200, { value: calc.helper(5) }
"#,
        ),
    ]);

    let resp = reqwest::blocking::get(format!(
        "http://127.0.0.1:{}/pricing/private-namespaced",
        port
    ))
    .unwrap();
    assert_eq!(resp.status(), 500);
    let json: serde_json::Value = resp.json().unwrap();
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("task 'calc.helper' is not defined"),
        "expected qualified undefined-task error, got {}",
        json["error"]
    );
}

#[test]
fn test_exported_task_keeps_private_helper_context() {
    // Spec 061: `calc.double` runs in its own file scope, so its bare private helper
    // `apply_rate` and the file var `base_rate` resolve from the declaring file.
    let (port, _dir) = start_project_server(&[
        (
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/pricing/helper"
    reply 200, { value: calc.double(5) }
"#,
        ),
        (
            "tasks/calc.marreta",
            r#"
base_rate = 2
task apply_rate(x) => x * base_rate
export task double(x) => apply_rate(x)
"#,
        ),
    ]);

    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/pricing/helper", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["value"], 10);
}

#[test]
fn test_multifile_routes_from_separate_files_respond() {
    let (port, _dir) = start_project_server(&[
        (
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/health"
    reply 200, { ok: true }
"#,
        ),
        (
            "routes/products.marreta",
            r#"
route GET "/products"
    reply 200, { items: [] }
"#,
        ),
    ]);

    let r1 = reqwest::blocking::get(format!("http://127.0.0.1:{}/health", port)).unwrap();
    assert_eq!(r1.status(), 200);

    let r2 = reqwest::blocking::get(format!("http://127.0.0.1:{}/products", port)).unwrap();
    assert_eq!(r2.status(), 200);
    let json: serde_json::Value = r2.json().unwrap();
    assert_eq!(json["items"], serde_json::json!([]));
}

#[test]
fn test_multifile_entrypoint_var_accessible_in_route() {
    let (port, _dir) = start_project_server(&[(
        "app.marreta",
        r#"
project_name = "my-api"
project_version = "2.0.0"
route GET "/info"
    reply 200, { name: project_name, version: project_version }
"#,
    )]);

    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/info", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["name"], "my-api");
    assert_eq!(json["version"], "2.0.0");
}

#[test]
fn test_multifile_exported_schema_validates_route() {
    let (port, _dir) = start_project_server(&[
        (
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/health"
    reply 200, { ok: true }
"#,
        ),
        (
            "schemas/payloads.marreta",
            r#"
export schema ItemPayload
    name: string
    qty: integer
"#,
        ),
        (
            "routes/items.marreta",
            r#"
route POST "/items" take payload as ItemPayload
    reply 201, { created: payload.name }
"#,
        ),
    ]);

    let client = reqwest::blocking::Client::new();

    // valid payload → 201
    let resp = client
        .post(format!("http://127.0.0.1:{}/items", port))
        .json(&serde_json::json!({ "name": "widget", "qty": 5 }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);

    // invalid payload (qty is string) → 422
    let resp = client
        .post(format!("http://127.0.0.1:{}/items", port))
        .json(&serde_json::json!({ "name": "widget", "qty": "five" }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 422);
}

#[test]
fn test_multifile_private_schema_validates_same_file_route() {
    let (port, _dir) = start_project_server(&[
        (
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/health"
    reply 200, { ok: true }
"#,
        ),
        (
            "routes/items.marreta",
            r#"
schema ItemPayload
    name: string
    qty: integer

route POST "/items/private-schema" take payload as ItemPayload
    reply 201, { created: payload.name, qty: payload.qty }
"#,
        ),
    ]);

    let client = reqwest::blocking::Client::new();
    let ok = client
        .post(format!("http://127.0.0.1:{}/items/private-schema", port))
        .json(&serde_json::json!({ "name": "Widget", "qty": 2 }))
        .send()
        .unwrap();
    assert_eq!(ok.status(), 201);

    let bad = client
        .post(format!("http://127.0.0.1:{}/items/private-schema", port))
        .json(&serde_json::json!({ "name": "Widget", "qty": "two" }))
        .send()
        .unwrap();
    assert_eq!(bad.status(), 422);
}
