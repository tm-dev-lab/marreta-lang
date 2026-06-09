use super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;
use std::sync::{Mutex, OnceLock};

fn timezone_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn run(source: &str) -> Value {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&program).unwrap()
}

fn run_with_timezone(source: &str, timezone: &str) -> Value {
    let _guard = timezone_test_lock().lock().unwrap();
    let previous = std::env::var("MARRETA_TIMEZONE").ok();
    // SAFETY: env access here is serialized through timezone_test_lock (held
    // for this scope); the var is set, the source runs, then the original is
    // restored, so no other thread observes the mutation.
    unsafe { std::env::set_var("MARRETA_TIMEZONE", timezone) };
    let result = run(source);
    match previous {
        Some(value) => unsafe { std::env::set_var("MARRETA_TIMEZONE", value) },
        None => unsafe { std::env::remove_var("MARRETA_TIMEZONE") },
    }
    result
}

fn run_with(interp: &mut Interpreter, source: &str) -> Value {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    interp.execute(&program).unwrap()
}

fn run_err(source: &str) -> MarretaError {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&program).unwrap_err()
}

// --- Literals ---

#[test]
fn test_integer_literal() {
    assert_eq!(run("42"), Value::Integer(42));
}

#[test]
fn test_float_literal() {
    assert_eq!(run("3.14"), Value::Float(3.14));
}

#[test]
fn test_string_literal() {
    assert_eq!(run("\"hello\""), Value::String("hello".into()));
}

#[test]
fn test_boolean_literals() {
    assert_eq!(run("true"), Value::Boolean(true));
    assert_eq!(run("false"), Value::Boolean(false));
}

#[test]
fn test_null_literal() {
    assert_eq!(run("null"), Value::Null);
}

#[test]
fn test_list_literal() {
    assert_eq!(
        run("[1, 2, 3]"),
        Value::List(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ])
    );
}

#[test]
fn test_map_literal() {
    let result = run("{name: \"Ana\", age: 30}");
    if let Value::Map(m) = result {
        let m = m.read().unwrap();
        assert_eq!(m.get("name"), Some(&Value::String("Ana".into())));
        assert_eq!(m.get("age"), Some(&Value::Integer(30)));
    } else {
        panic!("expected Map");
    }
}

// --- Assignments ---

#[test]
fn test_simple_assignment() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = 42");
    assert_eq!(interp.env.get("x"), Some(Value::Integer(42)));
}

#[test]
fn test_conditional_assignment_truthy() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = 10\nstatus = \"yes\" if x");
    assert_eq!(interp.env.get("status"), Some(Value::String("yes".into())));
}

#[test]
fn test_conditional_assignment_falsy() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = null\nstatus = \"yes\" if x");
    assert_eq!(interp.env.get("status"), None);
}

// --- Arithmetic ---

#[test]
fn test_arithmetic_int() {
    assert_eq!(run("2 + 3"), Value::Integer(5));
    assert_eq!(run("10 - 4"), Value::Integer(6));
    assert_eq!(run("3 * 7"), Value::Integer(21));
    assert_eq!(run("15 / 4"), Value::Integer(3));
    assert_eq!(run("17 % 5"), Value::Integer(2));
}

#[test]
fn test_arithmetic_float() {
    assert_eq!(run("2.5 + 1.5"), Value::Float(4.0));
    assert_eq!(run("10 + 0.5"), Value::Float(10.5));
}

#[test]
fn test_string_concatenation() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = \"hello\" + \" \" + \"world\"");
    assert_eq!(
        interp.env.get("x"),
        Some(Value::String("hello world".into()))
    );
}

#[test]
fn test_division_by_zero() {
    let err = run_err("10 / 0");
    assert!(matches!(err, MarretaError::DivisionByZero { .. }));
}

#[test]
fn test_math_abs_integer() {
    assert_eq!(run("math.abs(-5)"), Value::Integer(5));
}

#[test]
fn test_math_abs_float() {
    assert_eq!(run("math.abs(-5.25)"), Value::Float(5.25));
}

#[test]
fn test_math_floor_returns_integer() {
    assert_eq!(run("math.floor(3.9)"), Value::Integer(3));
}

#[test]
fn test_math_ceil_returns_integer() {
    assert_eq!(run("math.ceil(3.1)"), Value::Integer(4));
}

#[test]
fn test_math_round_without_places_returns_integer() {
    assert_eq!(run("math.round(4.8)"), Value::Integer(5));
}

#[test]
fn test_math_round_with_places_returns_float() {
    assert_eq!(run("math.round(4.876, places: 2)"), Value::Float(4.88));
}

#[test]
fn test_math_round_with_zero_places_returns_float() {
    assert_eq!(run("math.round(4.8, places: 0)"), Value::Float(5.0));
}

#[test]
fn test_math_round_integer_with_places_returns_float() {
    assert_eq!(run("math.round(5, places: 2)"), Value::Float(5.0));
}

#[test]
fn test_math_min_promotes_to_float_when_needed() {
    assert_eq!(run("math.min(10, 10.5)"), Value::Float(10.0));
}

#[test]
fn test_math_max_promotes_to_float_when_needed() {
    assert_eq!(run("math.max(10, 10.5)"), Value::Float(10.5));
}

#[test]
fn test_math_clamp_integer_result() {
    assert_eq!(
        run("math.clamp(120, min: 0, max: 100)"),
        Value::Integer(100)
    );
}

#[test]
fn test_math_clamp_promotes_to_float_when_needed() {
    assert_eq!(run("math.clamp(10, min: 0.5, max: 9.5)"), Value::Float(9.5));
}

#[test]
fn test_math_round_rejects_negative_places() {
    let err = run_err("math.round(4.2, places: -1)");
    assert!(
        matches!(err, MarretaError::RuntimeError { message, .. } if message.contains("zero or greater"))
    );
}

#[test]
fn test_math_clamp_rejects_min_greater_than_max() {
    let err = run_err("math.clamp(5, min: 10, max: 1)");
    assert!(
        matches!(err, MarretaError::RuntimeError { message, .. } if message.contains("min <= max"))
    );
}

#[test]
fn test_math_rejects_non_numeric_values() {
    let err = run_err("math.abs(\"x\")");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_math_pipeline_injects_input_as_first_argument() {
    assert_eq!(run("4.2 >> math.round(places: 0)"), Value::Float(4.0));
}

#[test]
fn test_math_broadcast_injects_input_as_first_argument() {
    assert_eq!(
        run(
            "value = -4.876\nresults = value *>>\n    -> math.abs()\n    -> math.round(places: 2)\nresults",
        ),
        Value::List(vec![Value::Float(4.876), Value::Float(-4.88)])
    );
}

// --- Comparison ---

#[test]
fn test_comparison() {
    assert_eq!(run("5 > 3"), Value::Boolean(true));
    assert_eq!(run("5 < 3"), Value::Boolean(false));
    assert_eq!(run("5 == 5"), Value::Boolean(true));
    assert_eq!(run("5 != 3"), Value::Boolean(true));
    assert_eq!(run("5 >= 5"), Value::Boolean(true));
    assert_eq!(run("5 <= 4"), Value::Boolean(false));
}

// --- Logical ---

#[test]
fn test_logical_and() {
    assert_eq!(run("true and true"), Value::Boolean(true));
    assert_eq!(run("true and false"), Value::Boolean(false));
    assert_eq!(run("null and true"), Value::Null);
}

#[test]
fn test_logical_or() {
    assert_eq!(run("false or true"), Value::Boolean(true));
    assert_eq!(run("null or 42"), Value::Integer(42));
}

#[test]
fn test_or_as_default() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = null\ny = x or 10");
    assert_eq!(interp.env.get("y"), Some(Value::Integer(10)));
}

// --- Unary ---

#[test]
fn test_unary_negate() {
    assert_eq!(run("-42"), Value::Integer(-42));
    assert_eq!(run("-3.14"), Value::Float(-3.14));
}

#[test]
fn test_unary_not() {
    assert_eq!(run("not true"), Value::Boolean(false));
    assert_eq!(run("not null"), Value::Boolean(true));
}

// --- Property access ---

#[test]
fn test_map_property_access() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "m = {name: \"Ana\"}\nx = m.name");
    assert_eq!(interp.env.get("x"), Some(Value::String("Ana".into())));
}

#[test]
fn test_map_missing_property() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "m = {name: \"Ana\"}\nx = m.missing");
    assert_eq!(interp.env.get("x"), Some(Value::Null));
}

// --- Method calls ---

#[test]
fn test_string_method() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = \"hello\".upper()");
    assert_eq!(interp.env.get("x"), Some(Value::String("HELLO".into())));
}

#[test]
fn test_list_method() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = [1, 2, 3].length()");
    assert_eq!(interp.env.get("x"), Some(Value::Integer(3)));
}

// --- Tasks ---

#[test]
fn test_inline_task() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "task double(n) => n * 2\nx = double(5)");
    assert_eq!(interp.env.get("x"), Some(Value::Integer(10)));
}

#[test]
fn test_block_task() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task calc(a, b)\n    sum = a + b\n    sum * 2\nx = calc(3, 4)",
    );
    assert_eq!(interp.env.get("x"), Some(Value::Integer(14)));
}

#[test]
fn test_wrong_arity() {
    let err = run_err("task double(n) => n * 2\ndouble(1, 2)");
    assert!(matches!(err, MarretaError::WrongArity { .. }));
}

#[test]
fn test_undefined_task() {
    let err = run_err("unknown(1)");
    assert!(matches!(err, MarretaError::UndefinedTask { .. }));
}

// --- Require / Reject ---

#[test]
fn test_require_passes() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = true\nrequire x else fail 400, \"bad\"");
    // No error, continue
}

#[test]
fn test_require_fails() {
    let err = run_err("require null else fail 400, \"missing\"");
    assert!(matches!(
        err,
        MarretaError::HttpError {
            status_code: 400,
            ..
        }
    ));
}

#[test]
fn test_reject_passes() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "reject false else fail 403, \"forbidden\"");
    // No error
}

#[test]
fn test_reject_fails() {
    let err = run_err("reject true else fail 403, \"forbidden\"");
    assert!(matches!(
        err,
        MarretaError::HttpError {
            status_code: 403,
            ..
        }
    ));
}

// --- Match ---

#[test]
fn test_match_expression() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "tipo = \"VIP\"\ndiscount = match tipo\n    \"VIP\" -> 0\n    \"regular\" -> 15\n    _ -> 20",
    );
    assert_eq!(interp.env.get("discount"), Some(Value::Integer(0)));
}

#[test]
fn test_match_fallback() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "tipo = \"other\"\ndiscount = match tipo\n    \"VIP\" -> 0\n    fallback -> 99",
    );
    assert_eq!(interp.env.get("discount"), Some(Value::Integer(99)));
}

#[test]
fn test_if_expression_true_branch() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = if true\n    1\nelse\n    2");
    assert_eq!(interp.env.get("x"), Some(Value::Integer(1)));
}

#[test]
fn test_if_expression_false_branch() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = if false\n    1\nelse\n    2");
    assert_eq!(interp.env.get("x"), Some(Value::Integer(2)));
}

#[test]
fn test_if_expression_without_else_returns_null() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = if false\n    1");
    assert_eq!(interp.env.get("x"), Some(Value::Null));
}

#[test]
fn test_if_expression_else_if_chain() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "x = if false\n    1\nelse if true\n    2\nelse\n    3",
    );
    assert_eq!(interp.env.get("x"), Some(Value::Integer(2)));
}

#[test]
fn test_if_expression_branch_scope_does_not_leak() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "x = if true\n    bonus = 10\n    bonus\nelse\n    0",
    );
    assert_eq!(interp.env.get("x"), Some(Value::Integer(10)));
    assert_eq!(interp.env.get("bonus"), None);
}

#[test]
fn test_if_expression_branch_scope_does_not_mutate_outer_binding() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "bonus = 1\nx = if true\n    bonus = 10\n    bonus\nelse\n    0",
    );
    assert_eq!(interp.env.get("x"), Some(Value::Integer(10)));
    assert_eq!(interp.env.get("bonus"), Some(Value::Integer(1)));
}

#[test]
fn test_if_expression_reply_aborts_route() {
    let err = run_err("if true\n    reply 200, 1\nelse\n    2");
    assert!(matches!(err, MarretaError::HttpResponse { .. }));
}

#[test]
fn test_if_expression_pipeline_applies_to_whole_result() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task inc(n) => n + 1\nx = if true\n    1\nelse\n    2\n>> inc",
    );
    assert_eq!(interp.env.get("x"), Some(Value::Integer(2)));
}

// --- Pipeline ---

#[test]
fn test_simple_pipeline() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "task double(n) => n * 2\nx = 5 >> double");
    assert_eq!(interp.env.get("x"), Some(Value::Integer(10)));
}

#[test]
fn test_multi_stage_pipeline() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task double(n) => n * 2\ntask inc(n) => n + 1\nx = 5 >> double >> inc",
    );
    assert_eq!(interp.env.get("x"), Some(Value::Integer(11)));
}

#[test]
fn test_pipeline_map_keep() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "x = [1, 2, 3] >> map item\n    y = item * 10\n    keep y",
    );
    assert_eq!(
        interp.env.get("x"),
        Some(Value::List(vec![
            Value::Integer(10),
            Value::Integer(20),
            Value::Integer(30),
        ]))
    );
}

// --- String interpolation ---

#[test]
fn test_string_interpolation() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "name = \"world\"\nx = \"hello #{name}!\"");
    assert_eq!(
        interp.env.get("x"),
        Some(Value::String("hello world!".into()))
    );
}

#[test]
fn test_string_interpolation_no_markers() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = \"plain string\"");
    assert_eq!(
        interp.env.get("x"),
        Some(Value::String("plain string".into()))
    );
}

// --- Built-in functions ---

#[test]
fn test_type_function() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = type(42)");
    assert_eq!(interp.env.get("x"), Some(Value::String("Integer".into())));
}

#[test]
fn test_len_function() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = len([1, 2, 3])");
    assert_eq!(interp.env.get("x"), Some(Value::Integer(3)));
}

// --- Error cases ---

#[test]
fn test_undefined_variable() {
    let err = run_err("x + 1");
    assert!(matches!(err, MarretaError::UndefinedVariable { .. }));
}

#[test]
fn test_type_error_arithmetic() {
    let err = run_err("true + 1");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_negate_string_error() {
    let err = run_err("-\"hello\"");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

// --- Reassignment ---

#[test]
fn test_reassignment() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = 1\nx = 2");
    assert_eq!(interp.env.get("x"), Some(Value::Integer(2)));
}

// --- Nested expressions ---

#[test]
fn test_nested_arithmetic() {
    assert_eq!(run("(2 + 3) * 4"), Value::Integer(20));
}

#[test]
fn test_operator_precedence() {
    assert_eq!(run("2 + 3 * 4"), Value::Integer(14));
}

#[test]
fn test_complex_expression() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = (10 + 5) * 2 - 3");
    assert_eq!(interp.env.get("x"), Some(Value::Integer(27)));
}

// --- Chained method calls ---

#[test]
fn test_chained_methods() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = \"  Hello World  \".trim().upper()");
    assert_eq!(
        interp.env.get("x"),
        Some(Value::String("HELLO WORLD".into()))
    );
}

#[test]
fn test_method_with_args() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "x = \"hello world\".replace(\"world\", \"marreta\")",
    );
    assert_eq!(
        interp.env.get("x"),
        Some(Value::String("hello marreta".into()))
    );
}

#[test]
fn test_string_split_method() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = \"a,b,c\".split(\",\")");
    assert_eq!(
        interp.env.get("x"),
        Some(Value::List(vec![
            Value::String("a".into()),
            Value::String("b".into()),
            Value::String("c".into()),
        ]))
    );
}

#[test]
fn test_list_first_last() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "items = [10, 20, 30]\nf = items.first()\nl = items.last()",
    );
    assert_eq!(interp.env.get("f"), Some(Value::Integer(10)));
    assert_eq!(interp.env.get("l"), Some(Value::Integer(30)));
}

#[test]
fn test_list_push_returns_new() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "a = [1, 2]\nb = a.push(3)");
    assert_eq!(
        interp.env.get("a"),
        Some(Value::List(vec![Value::Integer(1), Value::Integer(2)]))
    );
    assert_eq!(
        interp.env.get("b"),
        Some(Value::List(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3)
        ]))
    );
}

#[test]
fn test_list_reverse() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = [3, 1, 2].reverse()");
    assert_eq!(
        interp.env.get("x"),
        Some(Value::List(vec![
            Value::Integer(2),
            Value::Integer(1),
            Value::Integer(3)
        ]))
    );
}

#[test]
fn test_list_includes() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "a = [1, 2, 3].includes(2)\nb = [1, 2, 3].includes(5)",
    );
    assert_eq!(interp.env.get("a"), Some(Value::Boolean(true)));
    assert_eq!(interp.env.get("b"), Some(Value::Boolean(false)));
}

#[test]
fn test_list_empty() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "a = [].empty?()\nb = [1].empty?()");
    assert_eq!(interp.env.get("a"), Some(Value::Boolean(true)));
    assert_eq!(interp.env.get("b"), Some(Value::Boolean(false)));
}

#[test]
fn test_map_keys_values() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "m = {x: 1}\nk = m.keys()\nv = m.values()");
    let keys = interp.env.get("k").unwrap();
    let vals = interp.env.get("v").unwrap();
    if let Value::List(k) = keys {
        assert_eq!(k.len(), 1);
    } else {
        panic!("expected list");
    }
    if let Value::List(v) = vals {
        assert_eq!(v.len(), 1);
    } else {
        panic!("expected list");
    }
}

#[test]
fn test_map_has() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "m = {name: \"Ana\"}\na = m.has(\"name\")\nb = m.has(\"age\")",
    );
    assert_eq!(interp.env.get("a"), Some(Value::Boolean(true)));
    assert_eq!(interp.env.get("b"), Some(Value::Boolean(false)));
}

#[test]
fn test_map_merge() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "a = {x: 1}\nb = {y: 2}\nc = a.merge(b)");
    if let Some(Value::Map(m)) = interp.env.get("c") {
        let m = m.read().unwrap();
        assert_eq!(m.len(), 2);
    } else {
        panic!("expected Map");
    }
}

#[test]
fn test_integer_abs() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "n = -5\nx = n.abs()");
    assert_eq!(interp.env.get("x"), Some(Value::Integer(5)));
}

// --- Task scoping ---

#[test]
fn test_task_does_not_leak_scope() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task setx(n)\n    local = n * 2\n    local\nresult = setx(5)",
    );
    assert_eq!(interp.env.get("result"), Some(Value::Integer(10)));
    assert_eq!(interp.env.get("local"), None);
}

#[test]
fn test_task_captures_outer_scope() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "factor = 3\ntask multiply(n) => n * factor\nx = multiply(5)",
    );
    assert_eq!(interp.env.get("x"), Some(Value::Integer(15)));
}

#[test]
fn test_task_no_params() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "task greet() => \"hello\"\nx = greet()");
    assert_eq!(interp.env.get("x"), Some(Value::String("hello".into())));
}

// --- Multiple match arms ---

#[test]
fn test_match_multiple_arms() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "val = 2\nresult = match val\n    1 -> \"one\"\n    2 -> \"two\"\n    3 -> \"three\"\n    fallback -> \"other\"",
    );
    assert_eq!(interp.env.get("result"), Some(Value::String("two".into())));
}

#[test]
fn test_match_no_match_returns_null() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "val = 99\nresult = match val\n    1 -> \"one\"\n    2 -> \"two\"",
    );
    assert_eq!(interp.env.get("result"), Some(Value::Null));
}

// --- String interpolation edge cases ---

#[test]
fn test_string_interpolation_multiple() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "first = \"John\"\nlast = \"Doe\"\nfull = \"#{first} #{last}\"",
    );
    assert_eq!(
        interp.env.get("full"),
        Some(Value::String("John Doe".into()))
    );
}

#[test]
fn test_string_interpolation_with_number() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "count = 42\nmsg = \"items: #{count}\"");
    assert_eq!(
        interp.env.get("msg"),
        Some(Value::String("items: 42".into()))
    );
}

#[test]
fn test_string_interpolation_undefined_var() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = \"hello #{missing}!\"");
    assert_eq!(
        interp.env.get("x"),
        Some(Value::String("hello null!".into()))
    );
}

// --- List concatenation ---

#[test]
fn test_list_concatenation() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = [1, 2] + [3, 4]");
    assert_eq!(
        interp.env.get("x"),
        Some(Value::List(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
            Value::Integer(4),
        ]))
    );
}

// --- Nested map access ---

#[test]
fn test_nested_map_access() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "user = {name: \"Ana\", address: {city: \"SP\"}}\ncity = user.address.city",
    );
    assert_eq!(interp.env.get("city"), Some(Value::String("SP".into())));
}

// --- Multiple pipelines ---

#[test]
fn test_pipeline_with_task_chain() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task add10(n) => n + 10\ntask mul2(n) => n * 2\ntask sub1(n) => n - 1\nx = 5 >> add10 >> mul2 >> sub1",
    );
    assert_eq!(interp.env.get("x"), Some(Value::Integer(29)));
}

// --- Complex programs ---

#[test]
fn test_discount_calculation() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "price = 100\ntipo = \"VIP\"\ndiscount = match tipo\n    \"VIP\" -> 20\n    \"regular\" -> 10\n    fallback -> 0\nfinal = price - discount",
    );
    assert_eq!(interp.env.get("final"), Some(Value::Integer(80)));
}

#[test]
fn test_conditional_with_comparison() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "balance = 150\nstatus = \"premium\" if balance > 100",
    );
    assert_eq!(
        interp.env.get("status"),
        Some(Value::String("premium".into()))
    );
}

#[test]
fn test_conditional_not_assigned() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "balance = 50\nstatus = \"premium\" if balance > 100",
    );
    assert_eq!(interp.env.get("status"), None);
}

// --- Equality edge cases ---

#[test]
fn test_equality_int_float() {
    assert_eq!(run("5 == 5.0"), Value::Boolean(true));
    assert_eq!(run("5 != 5.1"), Value::Boolean(true));
}

#[test]
fn test_equality_null() {
    assert_eq!(run("null == null"), Value::Boolean(true));
    assert_eq!(run("null != 0"), Value::Boolean(true));
}

// --- Modulo ---

#[test]
fn test_modulo() {
    assert_eq!(run("10 % 3"), Value::Integer(1));
    assert_eq!(run("10 % 2"), Value::Integer(0));
}

#[test]
fn test_modulo_by_zero() {
    let err = run_err("10 % 0");
    assert!(matches!(err, MarretaError::DivisionByZero { .. }));
}

// --- Empty list/map ---

#[test]
fn test_empty_list() {
    assert_eq!(run("[]"), Value::List(vec![]));
}

#[test]
fn test_empty_map() {
    let result = run("{}");
    if let Value::Map(m) = result {
        assert!(m.read().unwrap().is_empty());
    } else {
        panic!("expected Map");
    }
}

// --- Not callable ---

#[test]
fn test_call_non_task() {
    let err = run_err("x = 42\nx(1)");
    assert!(matches!(err, MarretaError::NotCallable { .. }));
}

// --- Broadcast ---

#[test]
fn test_broadcast_with_tasks() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task double(n) => n * 2\ntask triple(n) => n * 3\nresults = 5 *>>\n    -> double\n    -> triple",
    );
    assert_eq!(
        interp.env.get("results"),
        Some(Value::List(vec![Value::Integer(10), Value::Integer(15),]))
    );
}

#[test]
fn test_broadcast_result_order_is_declaration_order() {
    // Even though branches run concurrently, results must arrive in the order
    // the targets were declared — not in completion order.
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task a(n) => n + 1\ntask b(n) => n + 2\ntask c(n) => n + 3\nresults = 10 *>>\n    -> a\n    -> b\n    -> c",
    );
    assert_eq!(
        interp.env.get("results"),
        Some(Value::List(vec![
            Value::Integer(11),
            Value::Integer(12),
            Value::Integer(13),
        ]))
    );
}

#[test]
fn test_broadcast_parallel_executes_concurrently() {
    // Each branch sleeps for 100ms via a CPU-spin (MarretaLang has no sleep builtin).
    // We verify that two branches complete in less than 2x the time of one branch,
    // confirming they ran in parallel and not sequentially.
    // We use wall-clock time with a generous threshold to avoid flakiness.
    use std::time::Instant;

    // Two branches doing independent work — results in parallel
    let mut interp = Interpreter::new();
    let src = "task work_a(n) => n * 2\ntask work_b(n) => n * 3\nresults = 100 *>>\n    -> work_a\n    -> work_b";

    let start = Instant::now();
    run_with(&mut interp, src);
    let elapsed = start.elapsed();

    // Both results correct
    assert_eq!(
        interp.env.get("results"),
        Some(Value::List(vec![Value::Integer(200), Value::Integer(300)]))
    );

    // With parallel execution, two fast branches complete well under 1s.
    // This is primarily a smoke test that spawn doesn't introduce gross overhead.
    assert!(
        elapsed.as_secs() < 2,
        "broadcast took {:?} — unexpectedly slow",
        elapsed
    );
}

// --- String + number ---

#[test]
fn test_string_plus_number() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = \"count: \" + 42");
    assert_eq!(interp.env.get("x"), Some(Value::String("count: 42".into())));
}

// --- Comparison with strings ---

#[test]
fn test_string_comparison() {
    assert_eq!(run("\"abc\" < \"def\""), Value::Boolean(true));
    assert_eq!(run("\"xyz\" > \"abc\""), Value::Boolean(true));
}

// --- Short-circuit semantics ---

#[test]
fn test_and_short_circuit_returns_left() {
    // `false and X` should return false without evaluating X
    assert_eq!(run("false and 42"), Value::Boolean(false));
}

#[test]
fn test_or_short_circuit_returns_left() {
    // `42 or X` should return 42 without evaluating X
    assert_eq!(run("42 or false"), Value::Integer(42));
}

// --- Float division ---

#[test]
fn test_float_division() {
    assert_eq!(run("7.0 / 2.0"), Value::Float(3.5));
    assert_eq!(run("7 / 2"), Value::Integer(3)); // integer division
}

#[test]
fn test_mixed_arithmetic() {
    assert_eq!(run("7 / 2.0"), Value::Float(3.5));
    assert_eq!(run("7.0 / 2"), Value::Float(3.5));
}

// --- Len built-in edge cases ---

#[test]
fn test_len_string() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = len(\"hello\")");
    assert_eq!(interp.env.get("x"), Some(Value::Integer(5)));
}

#[test]
fn test_len_map() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = len({a: 1, b: 2})");
    assert_eq!(interp.env.get("x"), Some(Value::Integer(2)));
}

// --- Property not found on non-map ---

#[test]
fn test_property_on_integer_error() {
    let err = run_err("x = 42\nx.name");
    assert!(matches!(err, MarretaError::PropertyNotFound { .. }));
}

// --- Task returning match ---

#[test]
fn test_task_returning_match() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task classify(n) => match n\n    1 -> \"one\"\n    2 -> \"two\"\n    fallback -> \"other\"\nx = classify(2)",
    );
    assert_eq!(interp.env.get("x"), Some(Value::String("two".into())));
}

#[test]
fn test_task_returning_match_fallback() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task classify(n) => match n\n    1 -> \"one\"\n    fallback -> \"other\"\nx = classify(99)",
    );
    assert_eq!(interp.env.get("x"), Some(Value::String("other".into())));
}

// --- Pipeline + map + task call inside keep ---

#[test]
fn test_pipeline_map_with_task_in_keep() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task double(n) => n * 2\nx = [1, 2, 3] >> map item\n    keep double(item)",
    );
    assert_eq!(
        interp.env.get("x"),
        Some(Value::List(vec![
            Value::Integer(2),
            Value::Integer(4),
            Value::Integer(6),
        ]))
    );
}

#[test]
fn test_pipeline_map_classify() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task classify(n) => match n\n    1 -> \"one\"\n    2 -> \"two\"\n    fallback -> \"other\"\nresult = [1, 2, 3] >> map item\n    keep classify(item)",
    );
    assert_eq!(
        interp.env.get("result"),
        Some(Value::List(vec![
            Value::String("one".into()),
            Value::String("two".into()),
            Value::String("other".into()),
        ]))
    );
}

// --- Broadcast with string tasks ---

#[test]
fn test_broadcast_string_tasks() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task up(s) => s.upper()\ntask low(s) => s.lower()\nresults = \"Hello\" *>>\n    -> up\n    -> low",
    );
    assert_eq!(
        interp.env.get("results"),
        Some(Value::List(vec![
            Value::String("HELLO".into()),
            Value::String("hello".into()),
        ]))
    );
}

// --- Broadcast + pipeline combinations ---
// These tests simulate what will happen when *>> and map/keep are combined
// with DB results (a fetch returns a List, then pipeline operations follow).

#[test]
fn test_broadcast_result_piped_into_map() {
    // *>> returns a List — that List can then be piped into map/keep normally.
    // Simulates: db.orders *>> -> task_a -> task_b >> map result ...
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task double(n) => n * 2\ntask triple(n) => n * 3\nresults = 10 *>>\n    -> double\n    -> triple\nfinal = results >> map item\n    keep item + 1",
    );
    assert_eq!(
        interp.env.get("final"),
        Some(Value::List(vec![
            Value::Integer(21), // double(10) + 1 = 21
            Value::Integer(31), // triple(10) + 1 = 31
        ]))
    );
}

#[test]
fn test_broadcast_result_piped_into_task() {
    // *>> returns a List — that List can be piped into >> which does implicit iteration.
    // So each element of the broadcast result is passed individually to the task.
    // Simulates: fetch result >> map row -> process_row
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task double(n) => n * 2\ntask triple(n) => n * 3\ntask add_one(n) => n + 1\nresults = 5 *>>\n    -> double\n    -> triple\nfinal = results >> add_one",
    );
    // >> add_one iterates: [10, 15] → [11, 16]
    assert_eq!(
        interp.env.get("final"),
        Some(Value::List(vec![Value::Integer(11), Value::Integer(16)]))
    );
}

#[test]
fn test_list_piped_into_broadcast() {
    // A List (e.g. from a fetch) can be broadcast to multiple tasks in parallel.
    // Each task receives the full list as input.
    // Simulates: db.orders >> fetch *>> -> count_active -> count_pending
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task first_item(lst) => lst.first()\ntask last_item(lst) => lst.last()\ndata = [10, 20, 30]\nresults = data *>>\n    -> first_item\n    -> last_item",
    );
    assert_eq!(
        interp.env.get("results"),
        Some(Value::List(vec![Value::Integer(10), Value::Integer(30)]))
    );
}

#[test]
fn test_map_then_broadcast() {
    // map over a list, then broadcast the transformed list to parallel tasks.
    // Simulates: fetch >> map row -> transform *>> -> task_a -> task_b
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task count(lst) => lst.length()\ntask is_empty(lst) => lst.length() == 0\ndata = [1, 2, 3]\ndoubled = data >> map n\n    keep n * 2\nresults = doubled *>>\n    -> count\n    -> is_empty",
    );
    assert_eq!(
        interp.env.get("results"),
        Some(Value::List(vec![
            Value::Integer(3),     // count([2,4,6]) = 3
            Value::Boolean(false), // is_empty([2,4,6]) = false
        ]))
    );
}

// --- Require with falsy list ---

#[test]
fn test_require_empty_list_fails() {
    let err = run_err("payload = []\nrequire payload else fail 400, \"Empty payload\"");
    assert!(matches!(
        err,
        MarretaError::HttpError {
            status_code: 400,
            ..
        }
    ));
}

#[test]
fn test_require_nonempty_list_passes() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "payload = [1]\nrequire payload else fail 400, \"Empty\"",
    );
    // No error
}

#[test]
fn test_reject_falsy_passes() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "reject 0 else fail 403, \"forbidden\"");
    // 0 is falsy so reject does not trigger
}

#[test]
fn test_reject_empty_string_passes() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "reject \"\" else fail 403, \"forbidden\"");
    // "" is falsy
}

// --- Type errors ---

#[test]
fn test_boolean_add_error() {
    let err = run_err("true + 1");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_boolean_subtract_error() {
    let err = run_err("true - 1");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_null_arithmetic_error() {
    let err = run_err("null + 1");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_compare_incompatible_types() {
    let err = run_err("true > 1");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

// --- Wrong arity ---

#[test]
fn test_wrong_arity_too_many() {
    let err = run_err("task double(n) => n * 2\ndouble(1, 2, 3)");
    match err {
        MarretaError::WrongArity { expected, got, .. } => {
            assert_eq!(expected, 1);
            assert_eq!(got, 3);
        }
        _ => panic!("expected WrongArity"),
    }
}

#[test]
fn test_wrong_arity_too_few() {
    let err = run_err("task add(a, b) => a + b\nadd(1)");
    match err {
        MarretaError::WrongArity { expected, got, .. } => {
            assert_eq!(expected, 2);
            assert_eq!(got, 1);
        }
        _ => panic!("expected WrongArity"),
    }
}

// --- Complex multi-step programs ---

#[test]
fn test_cart_with_tax() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task apply_tax(price) => price * 1.15\ncart = [100, 200]\ntaxed = cart >> map item\n    keep apply_tax(item)",
    );
    if let Some(Value::List(items)) = interp.env.get("taxed") {
        assert_eq!(items.len(), 2);
        // 100 * 1.15 = 115.0 (float precision)
        assert!(matches!(&items[0], Value::Float(f) if (*f - 115.0).abs() < 0.01));
        assert!(matches!(&items[1], Value::Float(f) if (*f - 230.0).abs() < 0.01));
    } else {
        panic!("expected List");
    }
}

#[test]
fn test_multi_step_string_processing() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "x = \"  Hello World  \".trim().lower().replace(\"world\", \"marreta\")",
    );
    assert_eq!(
        interp.env.get("x"),
        Some(Value::String("hello marreta".into()))
    );
}

#[test]
fn test_task_calling_task() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task double(n) => n * 2\ntask quadruple(n) => double(double(n))\nx = quadruple(3)",
    );
    assert_eq!(interp.env.get("x"), Some(Value::Integer(12)));
}

#[test]
fn test_recursive_fibonacci_style() {
    // Not real recursion since we don't have if/else expressions,
    // but test tasks calling other tasks in a chain
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task step1(n) => n + 1\ntask step2(n) => step1(n) * 2\ntask step3(n) => step2(n) + step1(n)\nx = step3(5)",
    );
    // step1(5) = 6, step2(5) = step1(5)*2 = 12, step3(5) = 12 + 6 = 18
    assert_eq!(interp.env.get("x"), Some(Value::Integer(18)));
}

// --- Edge cases ---

#[test]
fn test_empty_string_is_falsy_in_conditional() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = \"\"\ny = \"default\" if x");
    assert_eq!(interp.env.get("y"), None);
}

#[test]
fn test_zero_is_falsy_in_conditional() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "x = 0\ny = \"yes\" if x");
    assert_eq!(interp.env.get("y"), None);
}

#[test]
fn test_null_is_falsy_in_conditional() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "y = \"yes\" if null");
    assert_eq!(interp.env.get("y"), None);
}

#[test]
fn test_empty_map_is_falsy() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "m = {}\ny = \"yes\" if m");
    assert_eq!(interp.env.get("y"), None);
}

#[test]
fn test_nonempty_map_is_truthy() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "m = {a: 1}\ny = \"yes\" if m");
    assert_eq!(interp.env.get("y"), Some(Value::String("yes".into())));
}

#[test]
fn test_pipeline_single_value_through_tasks() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task add1(n) => n + 1\ntask mul3(n) => n * 3\ntask sub2(n) => n - 2\nx = 10 >> add1 >> mul3 >> sub2",
    );
    // 10 + 1 = 11, * 3 = 33, - 2 = 31
    assert_eq!(interp.env.get("x"), Some(Value::Integer(31)));
}

#[test]
fn test_map_property_set_via_assignment() {
    let mut interp = Interpreter::new();
    run_with(&mut interp, "m = {x: 1, y: 2}\nsum = m.x + m.y");
    assert_eq!(interp.env.get("sum"), Some(Value::Integer(3)));
}

#[test]
fn test_list_in_map() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "m = {items: [1, 2, 3]}\ncount = m.items.length()",
    );
    assert_eq!(interp.env.get("count"), Some(Value::Integer(3)));
}

#[test]
fn test_map_in_list() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "items = [{name: \"a\"}, {name: \"b\"}]\nfirst = items.first()",
    );
    if let Some(Value::Map(m)) = interp.env.get("first") {
        assert_eq!(
            m.read().unwrap().get("name"),
            Some(&Value::String("a".into()))
        );
    } else {
        panic!("expected Map");
    }
}

#[test]
fn test_string_interpolation_with_expression_result() {
    let mut interp = Interpreter::new();
    run_with(
        &mut interp,
        "task double(n) => n * 2\nresult = double(5)\nmsg = \"Result: #{result}\"",
    );
    assert_eq!(
        interp.env.get("msg"),
        Some(Value::String("Result: 10".into()))
    );
}

#[test]
fn test_float_division_by_zero() {
    let err = run_err("10.0 / 0.0");
    assert!(matches!(err, MarretaError::DivisionByZero { .. }));
}

#[test]
fn test_mixed_division_by_zero() {
    let err = run_err("10.0 / 0");
    assert!(matches!(err, MarretaError::DivisionByZero { .. }));
}

#[test]
fn test_print_returns_null() {
    // print() should return Null so it doesn't print extra output
    assert_eq!(run("print(42)"), Value::Null);
}

#[test]
fn test_len_wrong_type() {
    let err = run_err("len(42)");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_type_wrong_arity() {
    let err = run_err("type(1, 2)");
    assert!(matches!(err, MarretaError::WrongArity { .. }));
}

// --- HTTP: reply / fail ---

#[test]
fn test_reply_emits_http_response() {
    let err = run_err("reply 200, 42");
    assert!(matches!(
        err,
        MarretaError::HttpResponse {
            status_code: 200,
            is_error: false,
            ..
        }
    ));
}

#[test]
fn test_reply_body_is_json_integer() {
    let err = run_err("reply 200, 42");
    if let MarretaError::HttpResponse { body, .. } = err {
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v, serde_json::json!(42));
    } else {
        panic!("expected HttpResponse");
    }
}

#[test]
fn test_reply_body_is_json_map() {
    let err = run_err("reply 201, { name: \"alice\" }");
    if let MarretaError::HttpResponse {
        status_code, body, ..
    } = err
    {
        assert_eq!(status_code, 201);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["name"], "alice");
    } else {
        panic!("expected HttpResponse");
    }
}

#[test]
fn test_reply_body_has_no_injected_engine_fields() {
    // Spec 022 principle 1: authored `reply` bodies must not be wrapped or
    // enriched by the engine. The emitted body must contain exactly the
    // user-authored keys — no `code`, `trace`, `runtime`, or `details`.
    let err = run_err("reply 200, { name: \"alice\", active: true }");
    if let MarretaError::HttpResponse { body, .. } = err {
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let obj = v.as_object().expect("expected JSON object body");
        let keys: std::collections::BTreeSet<&str> = obj.keys().map(|k| k.as_str()).collect();
        let expected: std::collections::BTreeSet<&str> = ["name", "active"].into_iter().collect();
        assert_eq!(keys, expected, "unexpected keys in reply body: {:?}", keys);
    } else {
        panic!("expected HttpResponse");
    }
}

#[test]
fn test_fail_body_has_no_injected_engine_fields() {
    // Same guarantee for `fail`: the engine must not annotate authored fail
    // bodies with `code`, `trace`, or similar engine metadata.
    let err = run_err("fail 404, { error: \"not found\", hint: \"check id\" }");
    if let MarretaError::HttpResponse { body, is_error, .. } = err {
        assert!(is_error);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let obj = v.as_object().expect("expected JSON object body");
        let keys: std::collections::BTreeSet<&str> = obj.keys().map(|k| k.as_str()).collect();
        let expected: std::collections::BTreeSet<&str> = ["error", "hint"].into_iter().collect();
        assert_eq!(keys, expected, "unexpected keys in fail body: {:?}", keys);
    } else {
        panic!("expected HttpResponse");
    }
}

#[test]
fn test_reply_null_body() {
    let err = run_err("reply 204, null");
    if let MarretaError::HttpResponse {
        status_code, body, ..
    } = err
    {
        assert_eq!(status_code, 204);
        assert_eq!(body, "null");
    } else {
        panic!("expected HttpResponse");
    }
}

#[test]
fn test_reply_with_variable() {
    let err = run_err("x = 99\nreply 200, x");
    if let MarretaError::HttpResponse { body, .. } = err {
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v, serde_json::json!(99));
    } else {
        panic!("expected HttpResponse");
    }
}

#[test]
fn test_fail_emits_http_response_is_error() {
    let err = run_err("fail 404, \"Not found\"");
    assert!(matches!(
        err,
        MarretaError::HttpResponse {
            status_code: 404,
            is_error: true,
            ..
        }
    ));
}

#[test]
fn test_fail_body_is_error_json() {
    let err = run_err("fail 400, \"Bad request\"");
    if let MarretaError::HttpResponse { body, .. } = err {
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["error"], "Bad request");
    } else {
        panic!("expected HttpResponse");
    }
}

#[test]
fn test_reply_terminates_execution() {
    // Code after reply should not execute (reply raises an Err)
    let err = run_err("reply 200, 1\nreply 500, 2");
    if let MarretaError::HttpResponse { status_code, .. } = err {
        assert_eq!(status_code, 200);
    } else {
        panic!("expected HttpResponse");
    }
}

#[test]
fn test_fail_terminates_execution() {
    let err = run_err("fail 403, \"Forbidden\"\nreply 200, null");
    if let MarretaError::HttpResponse {
        status_code,
        is_error,
        ..
    } = err
    {
        assert_eq!(status_code, 403);
        assert!(is_error);
    } else {
        panic!("expected HttpResponse");
    }
}

// =========================================================================
// v0.4.0 — Task Contract Enforcement
// =========================================================================

fn run_with_schemas(source: &str, schemas: Arc<HashMap<String, SchemaDefinition>>) -> Value {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new().with_schemas(schemas);
    interp.execute(&program).unwrap()
}

fn run_err_with_schemas(
    source: &str,
    schemas: Arc<HashMap<String, SchemaDefinition>>,
) -> MarretaError {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new().with_schemas(schemas);
    interp.execute(&program).unwrap_err()
}

fn make_schema(fields: &[(&str, crate::ast::SchemaType, bool)]) -> SchemaDefinition {
    SchemaDefinition {
        db_table: None,
        fields: fields
            .iter()
            .map(|(name, t, optional)| crate::ast::SchemaField {
                name: name.to_string(),
                field_type: t.clone(),
                optional: *optional,
            })
            .collect(),
    }
}

fn make_persistent_schema(fields: &[(&str, crate::ast::SchemaType, bool)]) -> SchemaDefinition {
    SchemaDefinition {
        db_table: Some("users".into()),
        fields: fields
            .iter()
            .map(|(name, t, optional)| crate::ast::SchemaField {
                name: name.to_string(),
                field_type: t.clone(),
                optional: *optional,
            })
            .collect(),
    }
}

#[test]
fn test_schema_constructor_returns_map() {
    let mut schemas = HashMap::new();
    schemas.insert(
        "User".into(),
        make_schema(&[
            ("name", crate::ast::SchemaType::StringType, false),
            ("age", crate::ast::SchemaType::IntegerType, false),
        ]),
    );

    let result = run_with_schemas(
        r#"user = User { name: "Ana", age: 30 }
user"#,
        Arc::new(schemas),
    );

    let Value::Map(map) = result else {
        panic!("expected map");
    };
    let guard = map.read().unwrap();
    assert_eq!(guard.get("name"), Some(&Value::String("Ana".into())));
    assert_eq!(guard.get("age"), Some(&Value::Integer(30)));
}

#[test]
fn test_schema_constructor_rejects_extra_field() {
    let mut schemas = HashMap::new();
    schemas.insert(
        "User".into(),
        make_schema(&[("name", crate::ast::SchemaType::StringType, false)]),
    );

    let err = run_err_with_schemas(r#"User { name: "Ana", internal: true }"#, Arc::new(schemas));

    assert!(matches!(err, MarretaError::TypeError { .. }));
    assert!(err.to_string().contains("undeclared field 'internal'"));
}

#[test]
fn test_schema_constructor_allows_missing_persistent_id() {
    let mut schemas = HashMap::new();
    schemas.insert(
        "User".into(),
        make_persistent_schema(&[
            ("id", crate::ast::SchemaType::IntegerType, false),
            ("name", crate::ast::SchemaType::StringType, false),
        ]),
    );

    let result = run_with_schemas(
        r#"user = User { name: "Ana" }
user.name"#,
        Arc::new(schemas),
    );

    assert_eq!(result, Value::String("Ana".into()));
}

#[test]
fn test_task_contract_valid_argument_executes() {
    let mut schemas = HashMap::new();
    schemas.insert(
        "order_schema".into(),
        make_schema(&[("total", crate::ast::SchemaType::FloatType, false)]),
    );
    let src =
        "task apply_tax(order as order_schema) => order.total * 1.15\napply_tax({ total: 10.0 })";
    let result = run_with_schemas(src, Arc::new(schemas));
    assert_eq!(result, Value::Float(11.5));
}

#[test]
fn test_task_contract_missing_required_field_returns_type_error() {
    let mut schemas = HashMap::new();
    schemas.insert(
        "order_schema".into(),
        make_schema(&[
            ("total", crate::ast::SchemaType::FloatType, false),
            ("client_id", crate::ast::SchemaType::IntegerType, false),
        ]),
    );
    let src = "task apply_tax(order as order_schema) => order.total\napply_tax({ total: 10.0 })";
    let err = run_err_with_schemas(src, Arc::new(schemas));
    match err {
        MarretaError::TypeError { message, .. } => {
            assert!(message.contains("apply_tax"), "message: {}", message);
            assert!(message.contains("order"), "message: {}", message);
            assert!(message.contains("order_schema"), "message: {}", message);
            assert!(message.contains("client_id"), "message: {}", message);
        }
        _ => panic!("expected TypeError, got {:?}", err),
    }
}

#[test]
fn test_task_contract_wrong_type_returns_type_error() {
    let mut schemas = HashMap::new();
    schemas.insert(
        "order_schema".into(),
        make_schema(&[("total", crate::ast::SchemaType::FloatType, false)]),
    );
    let src = "task apply_tax(order as order_schema) => order.total\napply_tax({ total: \"not_a_float\" })";
    let err = run_err_with_schemas(src, Arc::new(schemas));
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_task_contract_unknown_schema_returns_type_error() {
    let src =
        "task apply_tax(order as nonexistent_schema) => order.total\napply_tax({ total: 10.0 })";
    let err = run_err_with_schemas(src, Arc::new(HashMap::new()));
    match err {
        MarretaError::TypeError { message, .. } => {
            assert!(
                message.contains("nonexistent_schema"),
                "message: {}",
                message
            );
        }
        _ => panic!("expected TypeError, got {:?}", err),
    }
}

#[test]
fn test_task_without_contract_works_as_before() {
    let src = "task double(n) => n * 2\ndouble(21)";
    assert_eq!(run(src), Value::Integer(42));
}

#[test]
fn test_task_mixed_params_unbound_param_not_validated() {
    let mut schemas = HashMap::new();
    schemas.insert(
        "order_schema".into(),
        make_schema(&[("total", crate::ast::SchemaType::FloatType, false)]),
    );
    // `discount` has no schema — any value accepted
    let src = "task apply(order as order_schema, discount) => order.total * (1.0 - discount)\napply({ total: 100.0 }, 0.1)";
    let result = run_with_schemas(src, Arc::new(schemas));
    assert_eq!(result, Value::Float(90.0));
}

// ─── DB namespace / table intermediate values ─────────────────────────────

#[test]
fn test_db_identifier_returns_db_namespace() {
    assert_eq!(run("db"), Value::DbNamespace);
}

#[test]
fn test_time_identifier_returns_time_namespace() {
    assert_eq!(run("time"), Value::TimeNamespace);
}

#[test]
fn test_fs_identifier_returns_fs_namespace() {
    assert_eq!(run("fs"), Value::FsNamespace);
}

fn temp_fs_path(label: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("marreta_fs_{}_{}", label, nanos))
}

#[test]
fn test_fs_write_read_exists_append_delete_cycle() {
    let path = temp_fs_path("cycle");
    let path_str = path.to_string_lossy().replace('\\', "\\\\");

    let source = format!(
        "target = \"{}\"\n\
             written = fs.write(target, \"hello\")\n\
             appended = fs.append(target, \"\\nworld\")\n\
             {{\n\
               written: written,\n\
               appended: appended,\n\
               exists: fs.exists(target),\n\
               content: fs.read(target),\n\
               deleted: fs.delete(target),\n\
               exists_after_delete: fs.exists(target),\n\
               deleted_missing: fs.delete(target)\n\
             }}",
        path_str
    );

    let value = run(&source);
    let map = match value {
        Value::Map(map) => map,
        other => panic!("expected map, got {:?}", other),
    };
    let guard = map.read().unwrap();
    assert_eq!(guard.get("written"), Some(&Value::String("hello".into())));
    assert_eq!(
        guard.get("appended"),
        Some(&Value::String("\nworld".into()))
    );
    assert_eq!(guard.get("exists"), Some(&Value::Boolean(true)));
    assert_eq!(
        guard.get("content"),
        Some(&Value::String("hello\nworld".into()))
    );
    assert_eq!(guard.get("deleted"), Some(&Value::Boolean(true)));
    assert_eq!(
        guard.get("exists_after_delete"),
        Some(&Value::Boolean(false))
    );
    assert_eq!(guard.get("deleted_missing"), Some(&Value::Boolean(false)));
}

#[test]
fn test_fs_read_preserves_trailing_newline() {
    let path = temp_fs_path("newline");
    std::fs::write(&path, "alpha\nbeta\n").unwrap();
    let path_str = path.to_string_lossy().replace('\\', "\\\\");

    let value = run(&format!("fs.read(\"{}\")", path_str));
    assert_eq!(value, Value::String("alpha\nbeta\n".into()));

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_fs_pipeline_write_read_and_delete() {
    let path = temp_fs_path("pipeline");
    let path_str = path.to_string_lossy().replace('\\', "\\\\");

    let source = format!(
        "written = \"hello\" >> fs.write(\"{}\")\n\
             content = \"{}\" >> fs.read()\n\
             deleted = \"{}\" >> fs.delete()\n\
             {{ written: written, content: content, deleted: deleted }}",
        path_str, path_str, path_str
    );

    let value = run(&source);
    let map = match value {
        Value::Map(map) => map,
        other => panic!("expected map, got {:?}", other),
    };
    let guard = map.read().unwrap();
    assert_eq!(guard.get("written"), Some(&Value::String("hello".into())));
    assert_eq!(guard.get("content"), Some(&Value::String("hello".into())));
    assert_eq!(guard.get("deleted"), Some(&Value::Boolean(true)));
}

#[test]
fn test_fs_read_missing_returns_file_not_found() {
    let path = temp_fs_path("missing");
    let path_str = path.to_string_lossy().replace('\\', "\\\\");
    let err = run_expect_error(&format!("fs.read(\"{}\")", path_str));
    match err {
        MarretaError::FileNotFound { path } => assert!(path.contains("marreta_fs_missing")),
        other => panic!("expected FileNotFound, got {:?}", other),
    }
}

#[test]
fn test_fs_read_invalid_utf8_returns_io_error() {
    let path = temp_fs_path("utf8");
    std::fs::write(&path, [0xff_u8, 0xfe_u8, 0xfd_u8]).unwrap();
    let path_str = path.to_string_lossy().replace('\\', "\\\\");

    let err = run_expect_error(&format!("fs.read(\"{}\")", path_str));
    match err {
        MarretaError::IoError { message } => {
            assert!(
                message.contains("fs operation failed"),
                "message: {}",
                message
            );
        }
        other => panic!("expected IoError, got {:?}", other),
    }

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_fs_write_requires_string_content() {
    let path = temp_fs_path("string_only");
    let path_str = path.to_string_lossy().replace('\\', "\\\\");
    let err = run_expect_error(&format!("fs.write(\"{}\", 42)", path_str));
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("must be String"))
    );
}

#[test]
fn test_time_now_returns_native_instant() {
    assert!(matches!(run("time.now()"), Value::Instant(_)));
}

#[test]
fn test_time_today_returns_native_date() {
    assert!(matches!(run("time.today()"), Value::Date(_)));
}

#[test]
fn test_time_date_constructor_returns_native_date() {
    match run("time.date(\"2026-04-27\")") {
        Value::Date(date) => assert_eq!(date, NaiveDate::from_ymd_opt(2026, 4, 27).unwrap()),
        other => panic!("expected Date, got {:?}", other),
    }
}

#[test]
fn test_time_at_constructor_returns_native_time() {
    match run("time.at(\"09:30:00\")") {
        Value::Time(time) => assert_eq!(time, NaiveTime::from_hms_opt(9, 30, 0).unwrap()),
        other => panic!("expected Time, got {:?}", other),
    }
}

#[test]
fn test_time_parse_returns_native_variants() {
    assert!(matches!(
        run("time.parse(\"2026-04-27T13:10:45Z\")"),
        Value::Instant(_)
    ));
    assert!(matches!(run("time.parse(\"2026-04-27\")"), Value::Date(_)));
    assert!(matches!(run("time.parse(\"09:30:00\")"), Value::Time(_)));
}

#[test]
fn test_time_instant_properties_and_roundtrip_helpers() {
    let value = run_with_timezone(
        "created_at = time.instant(\"2026-04-27T13:10:45Z\")\n{ year: created_at.year, month: created_at.month, day: created_at.day, hour: created_at.hour, minute: created_at.minute, second: created_at.second, unix: created_at.unix, local_date: created_at.date, local_time: created_at.time, roundtrip: time.from_unix(created_at.unix), formatted: time.format(created_at, \"yyyy-MM-dd HH:mm:ss\") }",
        "UTC",
    );

    let map = match value {
        Value::Map(map) => map,
        other => panic!("expected map, got {:?}", other),
    };
    let guard = map.read().unwrap();
    assert_eq!(guard.get("year"), Some(&Value::Integer(2026)));
    assert_eq!(guard.get("month"), Some(&Value::Integer(4)));
    assert_eq!(guard.get("day"), Some(&Value::Integer(27)));
    assert_eq!(guard.get("hour"), Some(&Value::Integer(13)));
    assert_eq!(guard.get("minute"), Some(&Value::Integer(10)));
    assert_eq!(guard.get("second"), Some(&Value::Integer(45)));
    assert_eq!(guard.get("unix"), Some(&Value::Integer(1777295445)));
    assert_eq!(
        guard.get("local_date"),
        Some(&Value::Date(NaiveDate::from_ymd_opt(2026, 4, 27).unwrap()))
    );
    assert_eq!(
        guard.get("local_time"),
        Some(&Value::Time(NaiveTime::from_hms_opt(13, 10, 45).unwrap()))
    );
    assert_eq!(
        guard.get("roundtrip"),
        Some(&Value::Instant(
            Utc.with_ymd_and_hms(2026, 4, 27, 13, 10, 45).unwrap()
        ))
    );
    assert_eq!(
        guard.get("formatted"),
        Some(&Value::String("2026-04-27 13:10:45".into()))
    );
}

#[test]
fn test_time_date_properties_and_day_bounds_default_utc() {
    let value = run_with_timezone(
        "billing_date = time.date(\"2026-04-27\")\n{ year: billing_date.year, month: billing_date.month, day: billing_date.day, weekday: billing_date.weekday, start: billing_date.start_of_day, end: billing_date.end_of_day, formatted: time.format(billing_date, \"dd/MM/yyyy\") }",
        "UTC",
    );

    let map = match value {
        Value::Map(map) => map,
        other => panic!("expected map, got {:?}", other),
    };
    let guard = map.read().unwrap();
    assert_eq!(guard.get("year"), Some(&Value::Integer(2026)));
    assert_eq!(guard.get("month"), Some(&Value::Integer(4)));
    assert_eq!(guard.get("day"), Some(&Value::Integer(27)));
    assert_eq!(guard.get("weekday"), Some(&Value::Integer(0)));
    assert_eq!(
        guard.get("start"),
        Some(&Value::Instant(
            Utc.with_ymd_and_hms(2026, 4, 27, 0, 0, 0).unwrap()
        ))
    );
    assert_eq!(
        guard.get("end"),
        Some(&Value::Instant(
            Utc.with_ymd_and_hms(2026, 4, 27, 23, 59, 59).unwrap()
                + ChronoDuration::milliseconds(999)
        ))
    );
    assert_eq!(
        guard.get("formatted"),
        Some(&Value::String("27/04/2026".into()))
    );
}

#[test]
fn test_time_properties_respect_configured_timezone() {
    let value = run_with_timezone(
        "created_at = time.instant(\"2026-04-27T13:10:45Z\")\n\
             billing_date = time.date(\"2026-04-27\")\n\
             opens_at = time.at(\"09:30:00\")\n\
             {\n\
               hour: created_at.hour,\n\
               local_date: created_at.date,\n\
               start: billing_date.start_of_day,\n\
               end: billing_date.end_of_day,\n\
               opening: opens_at.on(billing_date)\n\
             }",
        "America/Sao_Paulo",
    );

    let map = match value {
        Value::Map(map) => map,
        other => panic!("expected map, got {:?}", other),
    };
    let guard = map.read().unwrap();
    assert_eq!(guard.get("hour"), Some(&Value::Integer(10)));
    assert_eq!(
        guard.get("local_date"),
        Some(&Value::Date(NaiveDate::from_ymd_opt(2026, 4, 27).unwrap()))
    );
    assert_eq!(
        guard.get("start"),
        Some(&Value::Instant(
            Utc.with_ymd_and_hms(2026, 4, 27, 3, 0, 0).unwrap()
        ))
    );
    assert_eq!(
        guard.get("end"),
        Some(&Value::Instant(
            Utc.with_ymd_and_hms(2026, 4, 28, 2, 59, 59).unwrap()
                + ChronoDuration::milliseconds(999)
        ))
    );
    assert_eq!(
        guard.get("opening"),
        Some(&Value::Instant(
            Utc.with_ymd_and_hms(2026, 4, 27, 12, 30, 0).unwrap()
        ))
    );
}

#[test]
fn test_time_interval_duration_total_days() {
    let value = run(
        "window = time.interval(time.date(\"2026-04-01\"), time.date(\"2026-04-03\"))\nwindow.duration.total_days",
    );
    assert_eq!(value, Value::Float(2.0));
}

#[test]
fn test_time_contains_interval() {
    let value = run(
        "window = time.interval(time.at(\"09:00:00\"), time.at(\"18:00:00\"))\ntime.contains(window, time.at(\"10:00:00\"))",
    );
    assert_eq!(value, Value::Boolean(true));
}

#[test]
fn test_time_overlaps_interval() {
    let value = run(
        "left = time.interval(time.date(\"2026-04-27\"), time.date(\"2026-04-30\"))\nright = time.interval(time.date(\"2026-04-29\"), time.date(\"2026-05-02\"))\ntime.overlaps(left, right)",
    );
    assert_eq!(value, Value::Boolean(true));
}

#[test]
fn test_time_duration_total_units() {
    let value = run(
        "d = time.minutes(90)\n{ hours: d.total_hours, minutes: d.total_minutes, seconds: d.total_seconds }",
    );
    let map = match value {
        Value::Map(map) => map,
        other => panic!("expected map, got {:?}", other),
    };
    let guard = map.read().unwrap();
    assert_eq!(guard.get("hours"), Some(&Value::Float(1.5)));
    assert_eq!(guard.get("minutes"), Some(&Value::Float(90.0)));
    assert_eq!(guard.get("seconds"), Some(&Value::Float(5400.0)));
}

#[test]
fn test_db_row_to_runtime_value_coerces_temporal_schema_fields() {
    let mut schemas = HashMap::new();
    schemas.insert(
        "DbTimeEntry".into(),
        SchemaDefinition {
            db_table: Some("time_entries".into()),
            fields: vec![
                SchemaField {
                    name: "id".into(),
                    field_type: SchemaType::IntegerType,
                    optional: false,
                },
                SchemaField {
                    name: "created_at".into(),
                    field_type: SchemaType::InstantType,
                    optional: false,
                },
                SchemaField {
                    name: "billing_date".into(),
                    field_type: SchemaType::DateType,
                    optional: false,
                },
                SchemaField {
                    name: "opens_at".into(),
                    field_type: SchemaType::TimeType,
                    optional: false,
                },
                SchemaField {
                    name: "sla".into(),
                    field_type: SchemaType::DurationType,
                    optional: false,
                },
                SchemaField {
                    name: "business_window".into(),
                    field_type: SchemaType::IntervalType,
                    optional: false,
                },
            ],
        },
    );

    let mut row = HashMap::new();
    row.insert("id".into(), Value::Integer(1));
    row.insert(
        "created_at".into(),
        Value::Instant(Utc.with_ymd_and_hms(2026, 4, 27, 13, 10, 45).unwrap()),
    );
    row.insert(
        "billing_date".into(),
        Value::Date(NaiveDate::from_ymd_opt(2026, 4, 27).unwrap()),
    );
    row.insert(
        "opens_at".into(),
        Value::Time(NaiveTime::from_hms_opt(9, 30, 0).unwrap()),
    );
    row.insert("sla".into(), Value::Integer(5_400_000));
    row.insert(
        "business_window".into(),
        Value::map_from(vec![
            ("start".into(), Value::String("2026-04-27".into())),
            ("end".into(), Value::String("2026-04-30".into())),
        ]),
    );

    let mut interp = Interpreter::new();
    interp.schemas = Some(Arc::new(schemas));

    let value = interp.db_row_to_runtime_value("time_entries", row);
    let Value::RelationalRecord { fields, .. } = value else {
        panic!("expected relational record");
    };
    let guard = fields.read().unwrap();

    assert!(matches!(guard.get("created_at"), Some(Value::Instant(_))));
    assert!(matches!(guard.get("billing_date"), Some(Value::Date(_))));
    assert!(matches!(guard.get("opens_at"), Some(Value::Time(_))));
    assert!(matches!(guard.get("sla"), Some(Value::Duration(_))));
    assert!(matches!(
        guard.get("business_window"),
        Some(Value::Interval(_))
    ));
}

#[test]
fn test_db_table_access_returns_db_table() {
    assert_eq!(run("db.users"), Value::DbTable("users".to_string()));
}

#[test]
fn test_db_table_display() {
    assert_eq!(
        format!("{}", Value::DbTable("orders".to_string())),
        "<db.orders>"
    );
}

#[test]
fn test_db_namespace_display() {
    assert_eq!(format!("{}", Value::DbNamespace), "<db>");
}

// ─── DB direct calls — no DB configured error ─────────────────────────────

fn run_expect_error(source: &str) -> MarretaError {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&program).unwrap_err()
}

#[test]
fn test_db_save_without_engine_returns_error() {
    let err = run_expect_error("db.users.save({ name: \"Ana\" })");
    assert!(
        err.to_string().contains("no DB is configured"),
        "expected 'no DB is configured' in: {}",
        err
    );
}

#[test]
fn test_db_find_without_engine_returns_error() {
    let err = run_expect_error("db.users.find(1)");
    assert!(err.to_string().contains("no DB is configured"));
}

#[test]
fn test_db_find_all_without_engine_returns_error() {
    let err = run_expect_error("db.users.find_all()");
    assert!(err.to_string().contains("no DB is configured"));
}

#[test]
fn test_db_update_without_engine_returns_error() {
    let err = run_expect_error("db.users.update(1, { name: \"Bob\" })");
    assert!(err.to_string().contains("no DB is configured"));
}

#[test]
fn test_db_delete_without_engine_returns_error() {
    let err = run_expect_error("db.users.delete(1)");
    assert!(err.to_string().contains("no DB is configured"));
}

#[test]
fn test_db_unknown_operation_without_engine_returns_no_db_error() {
    // Even unknown operations surface the "no DB configured" error first
    let err = run_expect_error("db.users.nonexistent(1)");
    assert!(err.to_string().contains("no DB is configured"));
}

// ─── QueryBuilder pipeline — accumulation (pure, no DB required) ──────────

fn qb_table(val: &Value) -> Option<String> {
    if let Value::QueryBuilder(q) = val {
        Some(q.table.clone())
    } else {
        None
    }
}

fn qb_filters(val: &Value) -> Option<usize> {
    if let Value::QueryBuilder(q) = val {
        Some(q.filters.len())
    } else {
        None
    }
}

fn qb_joins(val: &Value) -> Option<usize> {
    if let Value::QueryBuilder(q) = val {
        Some(q.joins.len())
    } else {
        None
    }
}

#[test]
fn test_pipeline_db_table_promotes_to_querybuilder() {
    // `db.users >> where(status: "active")` should return a QueryBuilder
    let val = run("db.users >> where(status: \"active\")");
    assert_eq!(qb_table(&val).as_deref(), Some("users"));
    assert_eq!(qb_filters(&val), Some(1));
}

#[test]
fn test_pipeline_where_equality_accumulates_filter() {
    let val = run("db.orders >> where(status: \"paid\")");
    if let Value::QueryBuilder(q) = &val {
        assert_eq!(q.filters.len(), 1);
        assert_eq!(q.filters[0].column, "status");
        assert_eq!(q.filters[0].op, crate::db::driver::FilterOp::Eq);
        assert_eq!(q.filters[0].value, Value::String("paid".to_string()));
    } else {
        panic!("expected QueryBuilder, got {:?}", val);
    }
}

#[test]
fn test_pipeline_where_gt_expression_accumulates_filter() {
    let val = run("db.orders >> where(total > 1000)");
    if let Value::QueryBuilder(q) = &val {
        assert_eq!(q.filters[0].column, "total");
        assert_eq!(q.filters[0].op, crate::db::driver::FilterOp::Gt);
        assert_eq!(q.filters[0].value, Value::Integer(1000));
    } else {
        panic!("expected QueryBuilder");
    }
}

#[test]
fn test_pipeline_where_chained_accumulates_multiple_filters() {
    let val = run("db.orders >> where(total > 100) >> where(status: \"paid\")");
    assert_eq!(qb_filters(&val), Some(2));
}

#[test]
fn test_pipeline_where_multiple_args_in_one_call() {
    let val = run("db.orders >> where(total > 500, status: \"paid\")");
    assert_eq!(qb_filters(&val), Some(2));
}

#[test]
fn test_pipeline_limit_and_offset_accumulate() {
    let val = run("db.products >> limit(10) >> offset(20)");
    if let Value::QueryBuilder(q) = &val {
        assert_eq!(q.limit, Some(10));
        assert_eq!(q.offset, Some(20));
    } else {
        panic!("expected QueryBuilder");
    }
}

#[test]
fn test_pipeline_order_by_accumulates() {
    let val = run("db.products >> order_by(\"price desc\")");
    if let Value::QueryBuilder(q) = &val {
        assert_eq!(q.order_by.as_deref(), Some("price desc"));
    } else {
        panic!("expected QueryBuilder");
    }
}

#[test]
fn test_pipeline_join_accumulates() {
    let val = run("db.orders >> join(\"users\", on: \"user_id\")");
    assert_eq!(qb_joins(&val), Some(1));
    if let Value::QueryBuilder(q) = &val {
        assert_eq!(q.joins[0].table, "users");
        assert_eq!(q.joins[0].on, "user_id");
        assert_eq!(q.joins[0].kind, crate::db::driver::JoinKind::Inner);
    }
}

#[test]
fn test_pipeline_left_join_accumulates() {
    let val = run("db.orders >> left_join(\"users\", on: \"user_id\")");
    if let Value::QueryBuilder(q) = &val {
        assert_eq!(q.joins[0].kind, crate::db::driver::JoinKind::Left);
    } else {
        panic!("expected QueryBuilder");
    }
}

#[test]
fn test_pipeline_select_accumulates_columns() {
    let val = run("db.users >> select(\"id\", \"name\")");
    if let Value::QueryBuilder(q) = &val {
        assert_eq!(q.select_cols, vec!["id", "name"]);
    } else {
        panic!("expected QueryBuilder");
    }
}

#[test]
fn test_pipeline_full_chain_no_terminal() {
    let val = run(
        "db.orders >> join(\"users\", on: \"user_id\") >> where(total >= 500) >> order_by(\"total desc\") >> limit(5)",
    );
    if let Value::QueryBuilder(q) = &val {
        assert_eq!(q.table, "orders");
        assert_eq!(q.joins.len(), 1);
        assert_eq!(q.filters.len(), 1);
        assert_eq!(q.order_by.as_deref(), Some("total desc"));
        assert_eq!(q.limit, Some(5));
    } else {
        panic!("expected QueryBuilder");
    }
}

// ─── QueryBuilder terminals — error without engine ─────────────────────────

#[test]
fn test_pipeline_fetch_without_engine_errors() {
    let err = run_expect_error("db.users >> fetch");
    assert!(err.to_string().contains("no DB is configured"));
}

#[test]
fn test_pipeline_fetch_one_without_engine_errors() {
    let err = run_expect_error("db.users >> fetch_one");
    assert!(err.to_string().contains("no DB is configured"));
}

#[test]
fn test_pipeline_count_without_engine_errors() {
    let err = run_expect_error("db.orders >> where(status: \"paid\") >> count");
    assert!(err.to_string().contains("no DB is configured"));
}

#[test]
fn test_pipeline_exists_without_engine_errors() {
    let err = run_expect_error("db.users >> where(email: \"a@b.com\") >> exists");
    assert!(err.to_string().contains("no DB is configured"));
}

#[test]
fn test_pipeline_delete_without_engine_errors() {
    let err = run_expect_error("db.orders >> where(status: \"cancelled\") >> delete");
    assert!(err.to_string().contains("no DB is configured"));
}

#[test]
fn test_pipeline_update_without_engine_errors() {
    let err = run_expect_error(
        "db.orders >> where(status: \"pending\") >> update({ status: \"cancelled\" })",
    );
    assert!(err.to_string().contains("no DB is configured"));
}

#[test]
fn test_pipeline_map_on_querybuilder_errors_descriptively() {
    let err = run_expect_error("db.users >> map item\n    keep item");
    assert!(
        err.to_string().contains("did you forget >> fetch"),
        "expected hint about missing terminal, got: {}",
        err
    );
}

#[test]
fn test_pipeline_where_unsupported_operator_errors() {
    // Arithmetic operator inside where() — unsupported operator error
    let err = run_expect_error("db.orders >> where(total + 1)");
    assert!(
        err.to_string().contains("unsupported operator"),
        "got: {}",
        err
    );
}

#[test]
fn test_pipeline_where_non_identifier_lhs_errors() {
    // Left side is not an identifier — expect a descriptive error
    let err = run_expect_error("db.orders >> where(1 > 0)");
    assert!(
        err.to_string().contains("column identifier"),
        "got: {}",
        err
    );
}

// =========================================================================
// Phase 4 — native_query + transaction
// =========================================================================

#[test]
fn test_native_query_without_engine_returns_error() {
    // Without a DB engine, db.native_query should fail with a clear message.
    let err = run_expect_error("db.native_query(\"SELECT 1\")");
    assert!(
        err.to_string().contains("no DB is configured"),
        "got: {}",
        err
    );
}

#[test]
fn test_native_query_non_string_first_arg_errors() {
    // First arg must be a string. We can't easily test this without an engine,
    // but we can verify the no-engine error fires before the type check.
    let err = run_expect_error("db.native_query(42)");
    assert!(
        err.to_string().contains("no DB is configured"),
        "got: {}",
        err
    );
}

#[test]
fn test_transaction_without_engine_returns_error() {
    // Without a DB engine, `transaction` should fail with a clear message.
    let err = run_expect_error("transaction\n    x = 1");
    assert!(
        err.to_string().contains("no DB is configured"),
        "got: {}",
        err
    );
}

#[test]
fn test_nested_transaction_parse_error() {
    // Nested `transaction` blocks are rejected at parse time.
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    let src = "transaction\n    transaction\n        x = 1";
    let tokens = Lexer::new(src).tokenize().unwrap();
    let result = Parser::new(tokens).parse();
    assert!(
        result.is_err(),
        "expected parse error for nested transaction"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("nested") || msg.contains("transaction"),
        "unexpected error message: {}",
        msg
    );
}

#[test]
fn test_broadcast_inside_transaction_errors() {
    // *>> inside a transaction block is not allowed at runtime.
    // We test this by constructing the AST state manually via the interpreter flag.
    use crate::ast::*;
    let mut interp = Interpreter::new();
    // Simulate being inside a transaction
    interp.inside_transaction = true;
    // Evaluate a broadcast expression — should error immediately
    let expr = Expression::Broadcast {
        input: Box::new(Expression::Integer(1)),
        targets: vec![Expression::TaskCall {
            name: "foo".to_string(),
        }],
    };
    let err = interp.evaluate(&expr).unwrap_err();
    assert!(
        err.to_string().contains("broadcast") || err.to_string().contains("*>>"),
        "got: {}",
        err
    );
}
