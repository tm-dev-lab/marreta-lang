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
fn test_base64_identifier_resolves_to_namespace() {
    assert_eq!(run("base64"), Value::Base64Namespace);
}

#[test]
fn test_base64_encode_decode_standard() {
    assert_eq!(
        run(r#"base64.encode("client:secret")"#),
        Value::String("Y2xpZW50OnNlY3JldA==".into())
    );
    assert_eq!(
        run(r#"base64.decode("Y2xpZW50OnNlY3JldA==")"#),
        Value::String("client:secret".into())
    );
}

#[test]
fn test_base64_encode_decode_url_safe() {
    assert_eq!(
        run(r#"base64.encode("???", url_safe: true)"#),
        Value::String("Pz8_".into())
    );
    assert_eq!(
        run(r#"base64.decode("Pz8_", url_safe: true)"#),
        Value::String("???".into())
    );
}

#[test]
fn test_base64_decode_accepts_missing_padding() {
    assert_eq!(run(r#"base64.decode("aGk")"#), Value::String("hi".into()));
}

#[test]
fn test_base64_decode_rejects_wrong_alphabet_for_mode() {
    let err = run_err(r#"base64.decode("Pz8_")"#);
    assert!(
        matches!(err, MarretaError::RuntimeError { message, .. } if message.contains("base64.decode() invalid Base64"))
    );
}

#[test]
fn test_base64_rejects_non_string_input() {
    let err = run_err("base64.encode(42)");
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("base64.encode()") && message.contains("String"))
    );
}

#[test]
fn test_base64_decode_invalid_input_is_runtime_error() {
    let err = run_err(r#"base64.decode("%%%")"#);
    assert!(
        matches!(err, MarretaError::RuntimeError { message, .. } if message.contains("base64.decode() invalid Base64"))
    );
}

#[test]
fn test_base64_decode_invalid_utf8_is_runtime_error() {
    let err = run_err(r#"base64.decode("//79")"#);
    assert!(
        matches!(err, MarretaError::RuntimeError { message, .. } if message.contains("not valid UTF-8"))
    );
}

#[test]
fn test_base64_namespace_works_in_pipeline() {
    assert_eq!(
        run(r#"token = "client:secret" >> base64.encode()
decoded = token >> base64.decode()
decoded"#),
        Value::String("client:secret".into())
    );
}
