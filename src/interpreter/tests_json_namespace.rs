use super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;

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

#[test]
fn test_json_identifier_resolves_to_namespace() {
    assert_eq!(run("json"), Value::JsonNamespace);
}

#[test]
fn test_json_parse_accepts_all_root_json_types() {
    assert_eq!(run("json.parse(\"42\")"), Value::Integer(42));
    assert_eq!(run("json.parse(\"3.5\")"), Value::Float(3.5));
    assert_eq!(
        run(r#"json.parse("\"hello\"")"#),
        Value::String("hello".into())
    );
    assert_eq!(run("json.parse(\"true\")"), Value::Boolean(true));
    assert_eq!(run("json.parse(\"null\")"), Value::Null);
    assert_eq!(run(r#"json.parse("[1,2,3]")[1]"#), Value::Integer(2));
    assert_eq!(
        run(r#"data = json.parse("{\"customer\":{\"name\":\"Ana\"}}")
data.customer.name"#,),
        Value::String("Ana".into())
    );
}

#[test]
fn test_json_parse_rejects_non_string_input() {
    let err = run_err("json.parse(42)");
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("json.parse()") && message.contains("String"))
    );
}

#[test]
fn test_json_parse_invalid_json_is_runtime_error() {
    let err = run_err(r#"json.parse("{bad}")"#);
    assert!(
        matches!(err, MarretaError::RuntimeError { message, .. } if message.contains("json.parse() invalid JSON"))
    );
}

#[test]
fn test_json_stringify_preserves_insertion_order() {
    assert_eq!(
        run(r#"json.stringify({ id: 1, name: "Ana", active: true })"#),
        Value::String(r#"{"id":1,"name":"Ana","active":true}"#.into())
    );
}

#[test]
fn test_json_stringify_supports_temporal_values() {
    assert_eq!(
            run(
                r#"json.stringify({
    created_at: time.instant("2026-04-27T13:10:45Z"),
    billing_date: time.date("2026-04-27"),
    opens_at: time.at("09:30:00"),
    sla: time.minutes(90),
    window: time.interval(
        time.date("2026-04-27"),
        time.date("2026-04-30")
    )
})"#,
            ),
            Value::String(r#"{"created_at":"2026-04-27T13:10:45Z","billing_date":"2026-04-27","opens_at":"09:30:00","sla":"PT5400S","window":{"start":"2026-04-27","end":"2026-04-30"}}"#.into())
        );
}

#[test]
fn test_json_pretty_only_changes_whitespace() {
    assert_eq!(
        run(r#"json.pretty({ id: 1, name: "Ana" })"#),
        Value::String("{\n  \"id\": 1,\n  \"name\": \"Ana\"\n}".into())
    );
}

#[test]
fn test_json_stringify_rejects_unsupported_runtime_values() {
    let err = run_err("json.stringify(db)");
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("json.stringify()") && message.contains("DbNamespace"))
    );
}

#[test]
fn test_json_namespace_works_in_pipeline() {
    assert_eq!(
        run(r#"text = { id: 1, tags: ["a", "b"] } >> json.stringify()
parsed = text >> json.parse()
parsed.tags[1]"#,),
        Value::String("b".into())
    );
}
