use crate::ast::*;
use crate::error::MarretaError;
use crate::interpreter::{Interpreter, MarretaFrame};
use crate::queue::driver::mock::MockQueueDriver;
use crate::route_loader::SchemaDefinition;
use crate::trace_context::TraceContext;
use crate::value::Value;
use std::collections::HashMap;
use std::sync::Arc;

fn make_interp_with_queue(driver: Arc<MockQueueDriver>) -> Interpreter {
    let driver: Arc<dyn crate::queue::driver::QueueDriver> = driver;
    Interpreter::new().with_queue(driver)
}

fn parse_and_run(src: &str, interp: &mut Interpreter) -> Result<Value, MarretaError> {
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let program = crate::parser::Parser::new(tokens).parse().unwrap();
    interp.execute(&program)
}

#[test]
fn test_queue_push_records_message() {
    let driver = MockQueueDriver::new();
    let mut interp = make_interp_with_queue(Arc::clone(&driver));
    parse_and_run(
        r#"queue.push "orders", { id: 1, status: "new" }"#,
        &mut interp,
    )
    .unwrap();
    let pushed = driver.pushed.lock().unwrap();
    assert_eq!(pushed.len(), 1);
    assert_eq!(pushed[0].0, "orders");
    assert!(pushed[0].1.metadata.is_empty());
}

#[test]
fn test_queue_push_propagates_trace_context_metadata() {
    let driver = MockQueueDriver::new();
    let q_driver: Arc<dyn crate::queue::driver::QueueDriver> = driver.clone();
    let mut interp = Interpreter::new()
        .with_queue(q_driver)
        .with_trace_context(TraceContext {
            trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".to_string(),
            span_id: "00f067aa0ba902b7".to_string(),
            trace_flags: "01".to_string(),
            tracestate: Some("rojo=00f067aa0ba902b7".to_string()),
        });

    parse_and_run(r#"queue.push "orders", { id: 1 }"#, &mut interp).unwrap();

    let pushed = driver.pushed.lock().unwrap();
    let metadata = &pushed[0].1.metadata;
    let traceparent = metadata.get("traceparent").unwrap();
    assert!(traceparent.starts_with("00-4bf92f3577b34da6a3ce929d0e0e4736-"));
    assert!(traceparent.ends_with("-01"));
    assert!(!traceparent.contains("00f067aa0ba902b7"));
    assert_eq!(
        metadata.get("tracestate").map(String::as_str),
        Some("rojo=00f067aa0ba902b7")
    );
}

#[test]
fn test_queue_push_no_driver_returns_error() {
    let mut interp = Interpreter::new();
    let result = parse_and_run(r#"queue.push "orders", { id: 1 }"#, &mut interp);
    assert!(matches!(result, Err(MarretaError::QueueError { .. })));
}

#[test]
fn test_queue_push_non_string_queue_name_errors() {
    let driver = MockQueueDriver::new();
    let mut interp = make_interp_with_queue(Arc::clone(&driver));
    let result = parse_and_run(r#"queue.push 42, { id: 1 }"#, &mut interp);
    assert!(matches!(result, Err(MarretaError::TypeError { .. })));
}

#[test]
fn test_queue_push_failure_returns_queue_error() {
    let driver = MockQueueDriver::new();
    *driver.fail_publish.lock().unwrap() = true;
    let mut interp = make_interp_with_queue(Arc::clone(&driver));
    let result = parse_and_run(r#"queue.push "orders", { id: 1 }"#, &mut interp);
    assert!(matches!(result, Err(MarretaError::QueueError { .. })));
}

#[test]
fn test_topic_publish_records_message() {
    let driver = MockQueueDriver::new();
    let mut interp = make_interp_with_queue(Arc::clone(&driver));
    parse_and_run(
        r#"topic.publish "payments.approved", { amount: 100 }"#,
        &mut interp,
    )
    .unwrap();
    let published = driver.published.lock().unwrap();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].0, "payments.approved");
    assert!(published[0].1.metadata.is_empty());
}

#[test]
fn test_topic_publish_no_driver_returns_error() {
    let mut interp = Interpreter::new();
    let result = parse_and_run(
        r#"topic.publish "payments.approved", { amount: 100 }"#,
        &mut interp,
    );
    assert!(matches!(result, Err(MarretaError::QueueError { .. })));
}

#[test]
fn test_topic_publish_non_string_topic_errors() {
    let driver = MockQueueDriver::new();
    let mut interp = make_interp_with_queue(Arc::clone(&driver));
    let result = parse_and_run(r#"topic.publish 42, { amount: 100 }"#, &mut interp);
    assert!(matches!(result, Err(MarretaError::TypeError { .. })));
}

#[test]
fn test_queue_push_with_schema_filters_fields() {
    use crate::ast::{SchemaField, SchemaType};
    let driver = MockQueueDriver::new();
    let mut schemas = HashMap::new();
    schemas.insert(
        "OrderEvent".to_string(),
        SchemaDefinition {
            db_table: None,
            fields: vec![SchemaField {
                name: "id".into(),
                field_type: SchemaType::IntegerType,
                optional: false,
            }],
        },
    );
    let q_driver: Arc<dyn crate::queue::driver::QueueDriver> = driver.clone();
    let mut interp = Interpreter::new()
        .with_queue(q_driver)
        .with_schemas(Arc::new(schemas));
    // Payload has extra field 'secret' which should be stripped by schema
    parse_and_run(
        r#"queue.push "orders" as OrderEvent, { id: 1, secret: "shh" }"#,
        &mut interp,
    )
    .unwrap();
    let pushed = driver.pushed.lock().unwrap();
    assert_eq!(pushed.len(), 1);
    // Only 'id' should be in the pushed value
    match &pushed[0].1.payload {
        Value::Map(m) => {
            let m = m.read().unwrap();
            assert!(m.contains_key("id"));
            assert!(!m.contains_key("secret"));
        }
        other => panic!("expected Map, got {:?}", other),
    }
}

#[test]
fn test_topic_publish_with_schema_filters_fields() {
    use crate::ast::{SchemaField, SchemaType};
    let driver = MockQueueDriver::new();
    let mut schemas = HashMap::new();
    schemas.insert(
        "PaymentEvent".to_string(),
        SchemaDefinition {
            db_table: None,
            fields: vec![SchemaField {
                name: "amount".into(),
                field_type: SchemaType::FloatType,
                optional: false,
            }],
        },
    );
    let q_driver: Arc<dyn crate::queue::driver::QueueDriver> = driver.clone();
    let mut interp = Interpreter::new()
        .with_queue(q_driver)
        .with_schemas(Arc::new(schemas));
    parse_and_run(
        r#"topic.publish "payments.approved" as PaymentEvent, { amount: 99.9, internal_id: 42 }"#,
        &mut interp,
    )
    .unwrap();
    let published = driver.published.lock().unwrap();
    match &published[0].1.payload {
        Value::Map(m) => {
            let m = m.read().unwrap();
            assert!(m.contains_key("amount"));
            assert!(!m.contains_key("internal_id"));
        }
        other => panic!("expected Map, got {:?}", other),
    }
}

#[test]
fn test_nack_signal_propagates() {
    let mut interp = Interpreter::new();
    let tokens = crate::lexer::Lexer::new("nack").tokenize().unwrap();
    let program = crate::parser::Parser::new(tokens).parse().unwrap();
    let result = interp.execute(&program);
    assert!(matches!(
        result,
        Err(MarretaError::NackSignal { requeue: false })
    ));
}

#[test]
fn test_nack_requeue_signal_propagates() {
    let mut interp = Interpreter::new();
    let tokens = crate::lexer::Lexer::new("nack requeue").tokenize().unwrap();
    let program = crate::parser::Parser::new(tokens).parse().unwrap();
    let result = interp.execute(&program);
    assert!(matches!(
        result,
        Err(MarretaError::NackSignal { requeue: true })
    ));
}

// ─── Cache module tests (v0.9) ───────────────────────────────────────────

fn make_cache_interp() -> Interpreter {
    use crate::cache::driver::mock::MockCacheDriver;
    let driver = MockCacheDriver::new();
    let config = crate::cache::CacheConfig {
        url: String::new(),
        prefix: String::new(),
        default_ttl: None,
        pool_size: 1,
        connect_timeout: std::time::Duration::from_secs(1),
        operation_timeout: std::time::Duration::from_secs(1),
        reconnect_max_retries: 0,
    };
    Interpreter::new().with_cache(driver, config)
}

fn run_cache(src: &str) -> Value {
    let mut interp = make_cache_interp();
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let program = crate::parser::Parser::new(tokens).parse().unwrap();
    interp.execute(&program).unwrap()
}

fn run_cache_err(src: &str) -> MarretaError {
    let mut interp = make_cache_interp();
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let program = crate::parser::Parser::new(tokens).parse().unwrap();
    interp.execute(&program).unwrap_err()
}

#[test]
fn test_cache_set_and_get() {
    let result = run_cache("cache.set(\"k1\", 42)\ncache.get(\"k1\")");
    assert_eq!(result, Value::Integer(42));
}

#[test]
fn test_cache_get_miss_returns_null() {
    let result = run_cache("cache.get(\"missing\")");
    assert_eq!(result, Value::Null);
}

#[test]
fn test_cache_set_returns_value() {
    let result = run_cache("cache.set(\"k\", \"hello\")");
    assert_eq!(result, Value::String("hello".into()));
}

#[test]
fn test_cache_set_only_if_absent_first_wins() {
    let result = run_cache(
        "cache.set(\"k\", 1, only_if_absent: true)\ncache.set(\"k\", 2, only_if_absent: true)",
    );
    // Second set returns null because key exists
    assert_eq!(result, Value::Null);
}

#[test]
fn test_cache_delete_returns_bool() {
    let result = run_cache("cache.set(\"k\", 1)\ncache.delete(\"k\")");
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn test_cache_delete_nonexistent_returns_false() {
    let result = run_cache("cache.delete(\"nope\")");
    assert_eq!(result, Value::Boolean(false));
}

#[test]
fn test_cache_exists() {
    let result = run_cache("cache.set(\"k\", 1)\ncache.exists(\"k\")");
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn test_cache_exists_missing() {
    let result = run_cache("cache.exists(\"nope\")");
    assert_eq!(result, Value::Boolean(false));
}

#[test]
fn test_cache_incr_default_by_1() {
    let result = run_cache("cache.incr(\"counter\")");
    assert_eq!(result, Value::Integer(1));
}

#[test]
fn test_cache_incr_by_custom() {
    let result = run_cache("cache.incr(\"counter\", by: 5)");
    assert_eq!(result, Value::Integer(5));
}

#[test]
fn test_cache_decr() {
    let result = run_cache("cache.incr(\"c\", by: 10)\ncache.decr(\"c\", by: 3)");
    assert_eq!(result, Value::Integer(7));
}

#[test]
fn test_cache_no_driver_returns_error() {
    let mut interp = Interpreter::new();
    let tokens = crate::lexer::Lexer::new("cache.get(\"k\")")
        .tokenize()
        .unwrap();
    let program = crate::parser::Parser::new(tokens).parse().unwrap();
    let err = interp.execute(&program).unwrap_err();
    assert!(matches!(err, MarretaError::CacheError { .. }));
    assert!(err.to_string().contains("no cache is configured"));
}

#[test]
fn test_cache_unknown_method_errors() {
    let err = run_cache_err("cache.bogus(\"k\")");
    assert!(matches!(err, MarretaError::CacheError { .. }));
    assert!(err.to_string().contains("unknown cache operation"));
}

#[test]
fn test_cache_set_with_ttl() {
    // TTL is stored in mock — just verify it doesn't error
    let result = run_cache("cache.set(\"k\", 42, ttl: 60)");
    assert_eq!(result, Value::Integer(42));
}

#[test]
fn test_cache_get_many() {
    let result = run_cache(
        "cache.set(\"a\", 1)\ncache.set(\"b\", 2)\ncache.get_many([\"a\", \"b\", \"c\"])",
    );
    if let Value::Map(m) = result {
        let map = m.read().unwrap();
        assert_eq!(map.get("a"), Some(&Value::Integer(1)));
        assert_eq!(map.get("b"), Some(&Value::Integer(2)));
        assert_eq!(map.get("c"), Some(&Value::Null));
    } else {
        panic!("expected Map");
    }
}

#[test]
fn test_cache_set_many() {
    let result = run_cache("cache.set_many({ x: 10, y: 20 })\ncache.get(\"x\")");
    assert_eq!(result, Value::Integer(10));
}

#[test]
fn test_uncaught_trace_lines_render_outer_to_inner_with_operation() {
    let mut interp = Interpreter::new();
    let route_trace = interp.enter_route(
        &HttpVerb::Post,
        "/checkout",
        Some("routes/checkout".into()),
        3,
        1,
    );
    route_trace.preserve();
    interp.trace_frames.push(MarretaFrame::new(
        "task create_order",
        Some("tasks/orders".into()),
        Some(12),
        Some(1),
    ));

    let err = MarretaError::DbError {
        message: "record not found".into(),
        operation: "db.orders.save".into(),
    };

    let lines = interp.uncaught_trace_lines(&err);
    assert_eq!(
        lines[0],
        "at route POST /checkout (routes/checkout.marreta:3)"
    );
    assert_eq!(lines[1], "at task create_order (tasks/orders.marreta:12)");
    assert_eq!(lines[2], "at db.orders.save");
}

#[test]
fn test_trace_frame_guard_cleans_successful_scope() {
    let mut interp = Interpreter::new();
    {
        let _guard = interp.enter_route(
            &HttpVerb::Get,
            "/health",
            Some("routes/health".into()),
            2,
            1,
        );
        assert_eq!(interp.trace_frames.len(), 1);
    }

    assert!(interp.trace_frames.is_empty());
}

#[test]
fn test_trace_frame_guard_can_preserve_error_scope() {
    let mut interp = Interpreter::new();
    {
        let guard = interp.enter_route(
            &HttpVerb::Get,
            "/health",
            Some("routes/health".into()),
            2,
            1,
        );
        guard.preserve();
    }

    assert_eq!(interp.trace_frames.len(), 1);
    assert_eq!(
        interp.trace_frames[0].render(),
        "at route GET /health (routes/health.marreta:2)"
    );
}
