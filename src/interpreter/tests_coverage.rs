use super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;

fn run(source: &str) -> Value {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&program).unwrap()
}

fn run_err(source: &str) -> MarretaError {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&program).unwrap_err()
}

// --- Arithmetic: cross-type combinations not yet covered ---

#[test]
fn test_add_float_integer() {
    assert_eq!(run("1.5 + 2"), Value::Float(3.5));
}

#[test]
fn test_add_integer_float() {
    assert_eq!(run("2 + 1.5"), Value::Float(3.5));
}

#[test]
fn test_add_float_float() {
    assert_eq!(run("1.5 + 2.5"), Value::Float(4.0));
}

#[test]
fn test_decimal_constructor_and_arithmetic_preserve_decimal_type() {
    assert_eq!(
        run("decimal(\"19.90\") + decimal(\"0.10\")"),
        Value::Decimal("20.00".parse::<Decimal>().unwrap())
    );
    assert_eq!(
        run("decimal(\"10.50\") * 2"),
        Value::Decimal("21.00".parse::<Decimal>().unwrap())
    );
}

#[test]
fn test_decimal_rejects_float_mixing() {
    let err = run_err("decimal(\"1.00\") + 1.0");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_decimal_constructor_rejects_float_scientific_notation_and_wrong_arity() {
    assert!(matches!(
        run_err("decimal(1.0)"),
        MarretaError::TypeError { .. }
    ));
    assert!(matches!(
        run_err("decimal(\"1e3\")"),
        MarretaError::TypeError { .. }
    ));
    assert!(matches!(
        run_err("decimal()"),
        MarretaError::WrongArity { .. }
    ));
}

#[test]
fn test_decimal_rejects_float_comparison_and_division_by_zero() {
    assert!(matches!(
        run_err("decimal(\"1.00\") > 1.0"),
        MarretaError::TypeError { .. }
    ));
    assert!(matches!(
        run_err("decimal(\"1.00\") / decimal(\"0\")"),
        MarretaError::DivisionByZero { .. }
    ));
}

#[test]
fn test_decimal_methods_round_half_even_and_truncate_to_integer() {
    assert_eq!(
        run("decimal(\"2.345\").round(2)"),
        Value::Decimal("2.34".parse::<Decimal>().unwrap())
    );
    assert_eq!(
        run("decimal(\"2.355\").round(2)"),
        Value::Decimal("2.36".parse::<Decimal>().unwrap())
    );
    assert_eq!(run("decimal(\"-2.9\").to_integer()"), Value::Integer(-2));
}

#[test]
fn test_add_string_concatenation_non_string_rhs() {
    assert_eq!(run("\"count: \" + 5"), Value::String("count: 5".into()));
}

#[test]
fn test_add_non_string_lhs_with_string_rhs() {
    assert_eq!(run("42 + \" items\""), Value::String("42 items".into()));
}

#[test]
fn test_add_list_concatenation() {
    assert_eq!(
        run("[1, 2] + [3, 4]"),
        Value::List(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
            Value::Integer(4),
        ])
    );
}

#[test]
fn test_add_incompatible_types_error() {
    let err = run_err("true + 1");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_subtract_float_integer() {
    assert_eq!(run("5.0 - 2"), Value::Float(3.0));
}

#[test]
fn test_subtract_integer_float() {
    assert_eq!(run("5 - 1.5"), Value::Float(3.5));
}

#[test]
fn test_subtract_float_float() {
    assert_eq!(run("5.0 - 1.5"), Value::Float(3.5));
}

#[test]
fn test_subtract_incompatible_types_error() {
    let err = run_err("\"a\" - \"b\"");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_multiply_float_integer() {
    assert_eq!(run("2.0 * 3"), Value::Float(6.0));
}

#[test]
fn test_multiply_integer_float() {
    assert_eq!(run("3 * 2.0"), Value::Float(6.0));
}

#[test]
fn test_multiply_float_float() {
    assert_eq!(run("2.0 * 2.0"), Value::Float(4.0));
}

#[test]
fn test_divide_float_float() {
    assert_eq!(run("9.0 / 3.0"), Value::Float(3.0));
}

#[test]
fn test_divide_integer_float() {
    assert_eq!(run("9 / 3.0"), Value::Float(3.0));
}

#[test]
fn test_divide_float_integer() {
    assert_eq!(run("9.0 / 3"), Value::Float(3.0));
}

#[test]
fn test_divide_by_float_zero_error() {
    let err = run_err("1.0 / 0.0");
    assert!(matches!(err, MarretaError::DivisionByZero { .. }));
}

#[test]
fn test_divide_float_by_float_zero_error() {
    let err = run_err("5.0 / 0.0");
    assert!(matches!(err, MarretaError::DivisionByZero { .. }));
}

#[test]
fn test_divide_incompatible_types_error() {
    let err = run_err("\"a\" / 2");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_modulo_float_integer() {
    assert_eq!(run("7.0 % 3"), Value::Float(1.0));
}

#[test]
fn test_modulo_integer_float() {
    assert_eq!(run("7 % 3.0"), Value::Float(1.0));
}

#[test]
fn test_modulo_float_float() {
    assert_eq!(run("7.5 % 2.5"), Value::Float(0.0));
}

#[test]
fn test_modulo_incompatible_types_error() {
    let err = run_err("\"a\" % 2");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

// --- Comparison: string and float variants ---

#[test]
fn test_compare_strings_less() {
    assert_eq!(run("\"apple\" < \"banana\""), Value::Boolean(true));
}

#[test]
fn test_compare_strings_greater() {
    assert_eq!(run("\"zebra\" > \"apple\""), Value::Boolean(true));
}

#[test]
fn test_compare_float_float() {
    assert_eq!(run("1.5 < 2.5"), Value::Boolean(true));
    assert_eq!(run("3.0 >= 3.0"), Value::Boolean(true));
}

#[test]
fn test_compare_integer_float() {
    assert_eq!(run("2 < 2.5"), Value::Boolean(true));
}

#[test]
fn test_compare_float_integer() {
    assert_eq!(run("1.5 < 2"), Value::Boolean(true));
}

#[test]
fn test_compare_incompatible_types_error() {
    let err = run_err("1 < \"a\"");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

// --- Subscript: Map, List, negative index, out-of-bounds, error ---

#[test]
fn test_subscript_map_by_string_key() {
    assert_eq!(
        run("m = { name: \"alice\" }\nm[\"name\"]"),
        Value::String("alice".into())
    );
}

#[test]
fn test_subscript_map_missing_key_returns_null() {
    assert_eq!(run("m = { x: 1 }\nm[\"y\"]"), Value::Null);
}

#[test]
fn test_subscript_list_by_integer() {
    assert_eq!(run("[10, 20, 30][1]"), Value::Integer(20));
}

#[test]
fn test_subscript_list_negative_index() {
    assert_eq!(run("[10, 20, 30][-1]"), Value::Integer(30));
}

#[test]
fn test_subscript_list_out_of_bounds_returns_null() {
    assert_eq!(run("[1, 2][10]"), Value::Null);
}

#[test]
fn test_subscript_list_negative_overflow_returns_null() {
    assert_eq!(run("[1, 2][-10]"), Value::Null);
}

#[test]
fn test_subscript_invalid_type_error() {
    let err = run_err("1[\"key\"]");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

// --- Unary: negate float, not ---

#[test]
fn test_negate_float() {
    assert_eq!(run("-3.14"), Value::Float(-3.14));
}

#[test]
fn test_negate_invalid_type_error() {
    let err = run_err("-\"hello\"");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_not_truthy_value() {
    assert_eq!(run("not true"), Value::Boolean(false));
    assert_eq!(run("not 0"), Value::Boolean(true));
    assert_eq!(run("not \"hello\""), Value::Boolean(false));
}

// --- Reply/Fail edge cases ---

#[test]
fn test_reply_non_integer_status_error() {
    let err = run_err("reply \"ok\", null");
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("reply status must be Integer"))
    );
}

#[test]
fn test_reply_html_content_type() {
    let err = run_err("reply html 200, \"<h1>Hello</h1>\"");
    if let MarretaError::HttpResponse {
        content_type,
        body,
        status_code,
        ..
    } = err
    {
        assert_eq!(status_code, 200);
        assert!(content_type.contains("text/html"));
        assert_eq!(body, "<h1>Hello</h1>");
    } else {
        panic!("expected HttpResponse");
    }
}

#[test]
fn test_reply_text_content_type() {
    let err = run_err("reply text 200, \"plain text\"");
    if let MarretaError::HttpResponse {
        content_type, body, ..
    } = err
    {
        assert!(content_type.contains("text/plain"));
        assert_eq!(body, "plain text");
    } else {
        panic!("expected HttpResponse");
    }
}

#[test]
fn test_fail_map_body_is_serialized_as_json() {
    let err = run_err("fail 422, { errors: [\"name required\"] }");
    if let MarretaError::HttpResponse { body, is_error, .. } = err {
        assert!(is_error);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v.get("errors").is_some());
    } else {
        panic!("expected HttpResponse");
    }
}

#[test]
fn test_fail_list_body_is_serialized_as_json() {
    let err = run_err("fail 422, [\"error1\", \"error2\"]");
    if let MarretaError::HttpResponse { body, .. } = err {
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v.is_array());
    } else {
        panic!("expected HttpResponse");
    }
}

// --- Builtin functions ---

#[test]
fn test_builtin_type_function() {
    assert_eq!(run("type(42)"), Value::String("Integer".into()));
    assert_eq!(run("type(3.14)"), Value::String("Float".into()));
    assert_eq!(run("type(\"hi\")"), Value::String("String".into()));
    assert_eq!(run("type(true)"), Value::String("Boolean".into()));
    assert_eq!(run("type(null)"), Value::String("Null".into()));
}

#[test]
fn test_builtin_type_wrong_arity_error() {
    let err = run_err("type(1, 2)");
    assert!(matches!(err, MarretaError::WrongArity { task_name, .. } if task_name == "type"));
}

#[test]
fn test_builtin_len_string() {
    assert_eq!(run("len(\"hello\")"), Value::Integer(5));
}

#[test]
fn test_builtin_len_list() {
    assert_eq!(run("len([1, 2, 3])"), Value::Integer(3));
}

#[test]
fn test_builtin_len_map() {
    assert_eq!(run("len({ a: 1, b: 2 })"), Value::Integer(2));
}

#[test]
fn test_builtin_len_wrong_arity_error() {
    let err = run_err("len(\"a\", \"b\")");
    assert!(matches!(err, MarretaError::WrongArity { task_name, .. } if task_name == "len"));
}

#[test]
fn test_builtin_len_unsupported_type_error() {
    let err = run_err("len(42)");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_builtin_len_unsupported_type_preserves_call_site() {
    let err = run_err("x = len(42)");
    match err {
        MarretaError::TypeError { line, column, .. } => {
            assert_eq!(line, 1);
            assert!(column > 0);
        }
        other => panic!("expected TypeError, got {:?}", other),
    }
}

#[test]
fn test_method_call_type_error_preserves_call_site() {
    let err = run_err("x = \"abc\".starts_with(1)");
    match err {
        MarretaError::TypeError { line, column, .. } => {
            assert_eq!(line, 1);
            assert!(column > 0);
        }
        other => panic!("expected TypeError, got {:?}", other),
    }
}

#[test]
fn test_expression_statement_type_error_preserves_call_site() {
    let err = run_err("\"abc\".starts_with(1)");
    match err {
        MarretaError::TypeError { line, column, .. } => {
            assert_eq!(line, 1);
            assert!(column > 0);
        }
        other => panic!("expected TypeError, got {:?}", other),
    }
}

#[test]
fn test_builtin_print_returns_null() {
    assert_eq!(run("print(\"hello\")"), Value::Null);
}

// --- Pipeline map on non-list error ---

#[test]
fn test_pipeline_map_on_non_list_error() {
    let err = run_err("42 >> map x\n    keep x");
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("map requires a List"))
    );
}

// --- TaskCall in pipeline (resolves task value) ---

#[test]
fn test_task_call_in_pipeline() {
    // Task used as a pipeline stage — each element is passed to the task
    let result = run("task double(n) => n * 2\n[1, 2, 3] >> double");
    assert_eq!(
        result,
        Value::List(vec![
            Value::Integer(2),
            Value::Integer(4),
            Value::Integer(6),
        ])
    );
}

// --- Rescue: HttpResponse not caught ---

#[test]
fn test_rescue_expression_does_not_catch_http_response() {
    // fail produces HttpResponse — rescue must NOT catch these
    // Use AST directly since reply/fail are statements, not expressions
    use crate::ast::*;
    let expr = Expression::Rescue {
        expr: Box::new(Expression::FunctionCall {
            name: "__fail__".into(),
            arguments: vec![Argument::Positional(Expression::Integer(200))],
        }),
        handler: Box::new(Expression::StringLiteral("fallback".into())),
    };
    let mut interp = Interpreter::new();
    let err = interp.evaluate(&expr).unwrap_err();
    assert!(matches!(err, MarretaError::HttpResponse { .. }));
}

#[test]
fn test_rescue_pipeline_does_not_catch_http_response() {
    // HttpResponse in railway-oriented pipeline rescue pass-through
    use crate::ast::*;
    // Build: __fail__(404) >> rescue "fallback"
    let expr = Expression::Pipeline {
        input: Box::new(Expression::FunctionCall {
            name: "__fail__".into(),
            arguments: vec![
                Argument::Positional(Expression::Integer(404)),
                Argument::Positional(Expression::StringLiteral("not found".into())),
            ],
        }),
        stages: vec![PipelineStage::Rescue {
            handler: RescueHandler::Inline(Expression::StringLiteral("rescued".into())),
        }],
    };
    let mut interp = Interpreter::new();
    let err = interp.evaluate(&expr).unwrap_err();
    assert!(matches!(err, MarretaError::HttpResponse { .. }));
}

// --- Broadcast in non-tokio context (OS thread path) ---

#[test]
fn test_broadcast_in_non_tokio_context() {
    // When run outside tokio runtime, *>> uses OS threads with mpsc channels
    let result = std::thread::spawn(|| {
        run("task double(n) => n * 2\ntask triple(n) => n * 3\n5 *>>\n    -> double\n    -> triple")
    })
    .join()
    .unwrap();
    assert_eq!(
        result,
        Value::List(vec![Value::Integer(10), Value::Integer(15)])
    );
}

// --- Export statement ---

#[test]
fn test_export_task_definition_executes_inner() {
    // Export wraps a task def — the interpreter executes the inner statement
    let src = "export task greet(name) => \"hello \" + name\ngreet(\"world\")";
    assert_eq!(run(src), Value::String("hello world".into()));
}

#[test]
fn test_export_assignment_executes_inner() {
    let src = "export x = 99\nx";
    assert_eq!(run(src), Value::Integer(99));
}

// --- Route and Schema statements return Null ---

#[test]
fn test_route_statement_returns_null() {
    // Route declarations are no-ops at the interpreter level
    use crate::ast::*;
    let stmt = Statement::Route {
        verb: HttpVerb::Get,
        path: "/test".into(),
        auth: None,
        allow: vec![],
        take: vec![],
        body: vec![],
        line: 1,
        column: 1,
    };
    let mut interp = Interpreter::new();
    assert_eq!(interp.execute_statement(&stmt).unwrap(), Value::Null);
}

// --- Match expression ---

#[test]
fn test_match_fallback_arm() {
    let result = run("x = 99\nmatch x\n    1 -> \"one\"\n    fallback -> \"other\"");
    assert_eq!(result, Value::String("other".into()));
}

#[test]
fn test_match_no_matching_arm_returns_null() {
    // No fallback — returns Null when no arm matches
    let result = run("x = 99\nmatch x\n    1 -> \"one\"\n    2 -> \"two\"");
    assert_eq!(result, Value::Null);
}

#[test]
fn test_builtin_range_single_argument() {
    let result = run("range(5)");
    assert_eq!(
        result,
        Value::List(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
            Value::Integer(4),
            Value::Integer(5),
        ])
    );
}

#[test]
fn test_builtin_range_start_greater_than_end_returns_empty_list() {
    let result = run("range(5, 3)");
    assert_eq!(result, Value::List(vec![]));
}

#[test]
fn test_reduce_pipeline_accumulates_sum() {
    let result = run("[1, 2, 3, 4] >> reduce(0) acc, item\n    acc + item");
    assert_eq!(result, Value::Integer(10));
}

#[test]
fn test_reduce_pipeline_returns_initial_on_empty_list() {
    let result = run("[] >> reduce(\"start\") acc, item\n    acc + item");
    assert_eq!(result, Value::String("start".into()));
}

#[test]
fn test_while_loop_updates_outer_scope() {
    let result = run("counter = 0\nwhile counter < 3\n    counter = counter + 1\ncounter");
    assert_eq!(result, Value::Integer(3));
}

#[test]
fn test_while_loop_limit_returns_runtime_error() {
    let err = run_err("counter = 0\nwhile true\n    counter = counter + 1");
    assert!(matches!(err, MarretaError::RuntimeError { .. }));
    assert!(err.to_string().contains("10000 iterations"));
}

#[test]
fn test_recursive_task_factorial() {
    let result = run(
        "task fact(n)\n    match n <= 1\n        true -> 1\n        fallback -> n * fact(n - 1)\nfact(5)",
    );
    assert_eq!(result, Value::Integer(120));
}

#[test]
fn test_mutual_recursion() {
    let result = run(
        "task is_even(n)\n    match n == 0\n        true -> true\n        fallback -> is_odd(n - 1)\n\
task is_odd(n)\n    match n == 0\n        true -> false\n        fallback -> is_even(n - 1)\n\
is_even(6)",
    );
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn test_recursion_depth_limit_returns_runtime_error() {
    let mut interp = Interpreter::new();
    interp.max_recursion_depth = 5;
    let tokens = Lexer::new("task dive(n) => dive(n + 1)\ndive(0)")
        .tokenize()
        .unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    let err = interp.execute(&program).unwrap_err();
    assert!(matches!(err, MarretaError::RuntimeError { .. }));
    assert!(err.to_string().contains("maximum recursion depth exceeded"));
}

// --- NotCallable: calling a non-task value ---

#[test]
fn test_calling_non_task_value_error() {
    let err = run_err("x = 42\nx(1)");
    assert!(matches!(err, MarretaError::NotCallable { .. }));
}

// --- Short-circuit And/Or ---

#[test]
fn test_and_short_circuit_on_false() {
    // Left side is falsy → right side not evaluated (would error)
    let result = run("false and (1/0 > 0)");
    assert_eq!(result, Value::Boolean(false));
}

#[test]
fn test_or_short_circuit_on_true() {
    // Left side is truthy → right side not evaluated
    let result = run("true or (1/0 > 0)");
    assert_eq!(result, Value::Boolean(true));
}

// --- ConditionalAssignment ---

#[test]
fn test_conditional_assignment_condition_false_no_assign() {
    let src = "x = 1\nx = 99 if false\nx";
    assert_eq!(run(src), Value::Integer(1));
}

#[test]
fn test_conditional_assignment_condition_true_assigns() {
    let src = "x = 1\nx = 99 if true\nx";
    assert_eq!(run(src), Value::Integer(99));
}

// --- with_schemas method ---

#[test]
fn test_with_schemas_method_returns_interpreter() {
    use std::collections::HashMap;
    let schemas = Arc::new(HashMap::new());
    let interp = Interpreter::new().with_schemas(schemas);
    assert!(interp.schemas.is_some());
}

// --- environment / from_environment / into_environment ---

#[test]
fn test_from_environment_and_into_environment() {
    use crate::environment::Environment;
    let mut env = Environment::new();
    env.set("x".into(), Value::Integer(42));
    let interp = Interpreter::from_environment(env);
    assert_eq!(interp.environment().get("x"), Some(Value::Integer(42)));
    let env2 = interp.into_environment();
    assert_eq!(env2.get("x"), Some(Value::Integer(42)));
}

// --- env_set ---

#[test]
fn test_env_set_injects_variable() {
    let mut interp = Interpreter::new();
    interp.env_set("injected".into(), Value::String("hello".into()));
    let tokens = Lexer::new("injected").tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        interp.execute(&program).unwrap(),
        Value::String("hello".into())
    );
}
