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

fn uuid_text(value: Value) -> String {
    match value {
        Value::String(text) => text,
        other => panic!("expected UUID string, got {:?}", other),
    }
}

fn assert_canonical_uuid(text: &str, version: char) {
    assert_eq!(text.len(), 36, "uuid must be 36 chars");
    assert_eq!(&text[8..9], "-");
    assert_eq!(&text[13..14], "-");
    assert_eq!(&text[18..19], "-");
    assert_eq!(&text[23..24], "-");
    assert_eq!(text.chars().nth(14), Some(version));
    assert!(matches!(text.chars().nth(19), Some('8' | '9' | 'a' | 'b')));
    assert_eq!(text, text.to_lowercase());
    for (idx, ch) in text.chars().enumerate() {
        if matches!(idx, 8 | 13 | 18 | 23) {
            assert_eq!(ch, '-');
        } else {
            assert!(ch.is_ascii_hexdigit());
            assert!(!ch.is_ascii_uppercase());
        }
    }
}

#[test]
fn test_uuid_identifier_resolves_to_namespace() {
    assert_eq!(run("uuid"), Value::UuidNamespace);
}

#[test]
fn test_uuid_v4_returns_canonical_string() {
    let text = uuid_text(run("uuid.v4()"));
    assert_canonical_uuid(&text, '4');
}

#[test]
fn test_uuid_v7_returns_canonical_string() {
    let text = uuid_text(run("uuid.v7()"));
    assert_canonical_uuid(&text, '7');
}

#[test]
fn test_uuid_generators_return_distinct_values() {
    let first = uuid_text(run("uuid.v4()"));
    let second = uuid_text(run("uuid.v4()"));
    assert_ne!(first, second);
}

#[test]
fn test_uuid_v7_is_lexicographically_orderable() {
    let values: Vec<String> = (0..16).map(|_| uuid_text(run("uuid.v7()"))).collect();
    for pair in values.windows(2) {
        assert!(
            pair[0] <= pair[1],
            "expected v7 UUIDs to be non-decreasing lexicographically"
        );
    }
}

#[test]
fn test_uuid_rejects_arguments() {
    let err = run_err(r#"uuid.v4("x")"#);
    assert!(matches!(
        err,
        MarretaError::WrongArity { task_name, expected: 0, got: 1, .. }
        if task_name == "uuid.v4"
    ));

    let err = run_err(r#"uuid.v7("x")"#);
    assert!(matches!(
        err,
        MarretaError::WrongArity { task_name, expected: 0, got: 1, .. }
        if task_name == "uuid.v7"
    ));
}
