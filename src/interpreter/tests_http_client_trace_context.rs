use super::*;
use crate::ast::{SchemaField, SchemaType};
use crate::http_client::driver::mock::MockHttpClient;
use crate::lexer::Lexer;
use crate::parser::Parser;

fn execute_with_mock(src: &str, mock: Arc<MockHttpClient>) -> Value {
    let tokens = Lexer::new(src).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new()
        .with_http_client(mock.clone())
        .with_trace_context(TraceContext {
            trace_id: "0af7651916cd43dd8448eb211c80319c".into(),
            span_id: "b7ad6b7169203331".into(),
            trace_flags: "01".into(),
            tracestate: Some("rojo=00f067aa0ba902b7".into()),
        });
    interp.execute(&program).unwrap()
}

fn execute_with_mock_and_schemas(
    src: &str,
    mock: Arc<MockHttpClient>,
    schemas: Arc<HashMap<String, SchemaDefinition>>,
) -> Result<Value, MarretaError> {
    let tokens = Lexer::new(src).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new()
        .with_http_client(mock.clone())
        .with_schemas(schemas);
    interp.execute(&program)
}

fn schema(fields: &[(&str, SchemaType, bool)]) -> SchemaDefinition {
    SchemaDefinition {
        db_table: None,
        fields: fields
            .iter()
            .map(|(name, field_type, optional)| SchemaField {
                name: name.to_string(),
                field_type: field_type.clone(),
                optional: *optional,
            })
            .collect(),
    }
}

fn map_value(pairs: Vec<(&str, Value)>) -> Value {
    Value::Map(Arc::new(RwLock::new(
        pairs
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )))
}

fn enqueue_ok(mock: &MockHttpClient) {
    mock.enqueue_response(HttpClientResponse {
        status: 200,
        body: Value::Null,
        headers: HashMap::new(),
    });
}

#[test]
fn test_http_client_propagates_trace_context() {
    let mock = MockHttpClient::new();
    enqueue_ok(&mock);

    execute_with_mock(
        r#"http_client.get("https://api.example.test")"#,
        mock.clone(),
    );

    let captured = mock.captured_requests();
    let request = captured.first().unwrap();
    let traceparent = request.headers.get("traceparent").unwrap();
    assert!(traceparent.starts_with("00-0af7651916cd43dd8448eb211c80319c-"));
    assert!(traceparent.ends_with("-01"));
    assert!(!traceparent.contains("b7ad6b7169203331"));
    assert_eq!(
        request.headers.get("tracestate").map(String::as_str),
        Some("rojo=00f067aa0ba902b7")
    );
}

#[test]
fn test_http_client_explicit_trace_headers_win() {
    let mock = MockHttpClient::new();
    enqueue_ok(&mock);

    execute_with_mock(
        r#"http_client.get("https://api.example.test", headers: {
    traceparent: "00-11111111111111111111111111111111-2222222222222222-00",
    tracestate: "vendor=value"
})"#,
        mock.clone(),
    );

    let captured = mock.captured_requests();
    let request = captured.first().unwrap();
    assert_eq!(
        request.headers.get("traceparent").map(String::as_str),
        Some("00-11111111111111111111111111111111-2222222222222222-00")
    );
    assert_eq!(
        request.headers.get("tracestate").map(String::as_str),
        Some("vendor=value")
    );
}

#[test]
fn test_http_client_response_schema_validates_body() {
    let mock = MockHttpClient::new();
    mock.enqueue_response(HttpClientResponse {
        status: 200,
        body: map_value(vec![
            ("name", Value::String("Ana".into())),
            (
                "extra",
                Value::String("kept like route payload ingress".into()),
            ),
        ]),
        headers: HashMap::from([("x-source".to_string(), "mock".to_string())]),
    });
    let mut schemas = HashMap::new();
    schemas.insert(
        "UserProfile".into(),
        schema(&[("name", SchemaType::StringType, false)]),
    );

    let result = execute_with_mock_and_schemas(
        r#"response = http_client.get("https://api.example.test/users/1") as UserProfile
{
    status: response.status,
    name: response.body.name,
    extra: response.body.extra,
    source: response.headers["x-source"]
}"#,
        mock,
        Arc::new(schemas),
    )
    .unwrap();

    let Value::Map(map) = result else {
        panic!("expected map");
    };
    let guard = map.read().unwrap();
    assert_eq!(guard.get("status"), Some(&Value::Integer(200)));
    assert_eq!(guard.get("name"), Some(&Value::String("Ana".into())));
    assert_eq!(
        guard.get("extra"),
        Some(&Value::String("kept like route payload ingress".into()))
    );
    assert_eq!(guard.get("source"), Some(&Value::String("mock".into())));
}

#[test]
fn test_http_client_response_schema_error_keeps_operation() {
    let mock = MockHttpClient::new();
    mock.enqueue_response(HttpClientResponse {
        status: 200,
        body: map_value(vec![("name", Value::Integer(42))]),
        headers: HashMap::new(),
    });
    let mut schemas = HashMap::new();
    schemas.insert(
        "UserProfile".into(),
        schema(&[("name", SchemaType::StringType, false)]),
    );

    let err = execute_with_mock_and_schemas(
        r#"http_client.get("https://api.example.test/users/1") as UserProfile"#,
        mock,
        Arc::new(schemas),
    )
    .unwrap_err();

    match err {
        MarretaError::HttpClientError {
            operation, message, ..
        } => {
            assert_eq!(operation, "http_client.get");
            assert!(message.contains("does not match schema 'UserProfile'"));
            assert!(message.contains("expected string"));
        }
        other => panic!("expected HttpClientError, got {:?}", other),
    }
}
