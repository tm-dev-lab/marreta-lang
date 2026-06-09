use super::*;
use crate::feature_flags::FeatureFlags;
use crate::lexer::Lexer;
use crate::parser::Parser;
use std::collections::HashMap;

fn run_with_flags(src: &str, flags: FeatureFlags) -> Value {
    let tokens = Lexer::new(src).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    Interpreter::new()
        .with_feature_flags(flags)
        .execute(&program)
        .unwrap()
}

fn run_err(src: &str) -> MarretaError {
    let tokens = Lexer::new(src).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    Interpreter::new().execute(&program).unwrap_err()
}

#[test]
fn feature_enabled_reads_configured_flags() {
    let flags = FeatureFlags::new(HashMap::from([
        ("inventory_api".to_string(), true),
        ("low_stock_alert".to_string(), false),
    ]));

    assert_eq!(
        run_with_flags(r#"feature.enabled("inventory_api")"#, flags.clone()),
        Value::Boolean(true)
    );
    assert_eq!(
        run_with_flags(r#"feature.enabled("low_stock_alert")"#, flags.clone()),
        Value::Boolean(false)
    );
    assert_eq!(
        run_with_flags(r#"feature.enabled("missing")"#, flags),
        Value::Boolean(false)
    );
}

#[test]
fn feature_namespace_is_available_as_value() {
    assert_eq!(
        run_with_flags("feature", FeatureFlags::default()),
        Value::FeatureNamespace
    );
}

#[test]
fn feature_enabled_rejects_wrong_arity_and_type() {
    let err = run_err("feature.enabled()");
    assert!(matches!(
        err,
        MarretaError::WrongArity { task_name, expected, got, .. }
            if task_name == "feature.enabled" && expected == 1 && got == 0
    ));

    let err = run_err("feature.enabled(123)");
    assert!(matches!(
        err,
        MarretaError::TypeError { message, .. }
            if message.contains("feature.enabled()") && message.contains("String")
    ));
}

#[test]
fn feature_enabled_rejects_invalid_names() {
    let err = run_err(r#"feature.enabled("inventory__api")"#);
    assert!(matches!(
        err,
        MarretaError::RuntimeError { message, .. }
            if message.contains("invalid feature flag name")
                && message.contains("single underscores")
    ));
}

#[test]
fn feature_enabled_unknown_method_errors() {
    let err = run_err(r#"feature.disabled("x")"#);
    assert!(matches!(
        err,
        MarretaError::PropertyNotFound { object_type, property, .. }
            if object_type == "FeatureNamespace" && property == "disabled"
    ));
}
