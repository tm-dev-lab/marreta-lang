use super::*;

fn run_err(src: &str) -> MarretaError {
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    Interpreter::new().execute(&stmts).unwrap_err()
}

// ─── raise ────────────────────────────────────────────────────────────────

#[test]
fn test_raise_produces_raise_error() {
    let err = run_err("raise \"something went wrong\"");
    assert!(matches!(err, MarretaError::RaiseError { .. }));
}

#[test]
fn test_raise_message_is_preserved() {
    let err = run_err("raise \"custom error message\"");
    if let MarretaError::RaiseError { message } = err {
        assert_eq!(message, "custom error message");
    } else {
        panic!("expected RaiseError");
    }
}

#[test]
fn test_raise_semantic_code_is_raise_error() {
    let err = run_err("raise \"oops\"");
    assert_eq!(err.semantic_code(), "raise_error");
}

#[test]
fn test_raise_if_true_fires() {
    let err = run_err("x = true\nraise \"conditional\" if x");
    assert!(matches!(err, MarretaError::RaiseError { .. }));
}

#[test]
fn test_raise_if_false_does_not_fire() {
    let tokens = crate::lexer::Lexer::new("x = false\nraise \"never\" if x\ny = 42")
        .tokenize()
        .unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&stmts).unwrap();
    assert_eq!(interp.env.get("y"), Some(Value::Integer(42)));
}

#[test]
fn test_raise_stops_execution() {
    let tokens = crate::lexer::Lexer::new("raise \"stop\"\nx = 99")
        .tokenize()
        .unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    let _ = interp.execute(&stmts);
    // x must not have been set
    assert!(interp.env.get("x").is_none());
}

// ─── rescue — pipeline ─────────────────────────────────────────────────────

#[test]
fn test_rescue_pipeline_catches_raise_error() {
    let src = "task boom(x)\n    raise \"boom\"\n\nresult = 1 >> boom >> rescue \"caught\"";
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&stmts).unwrap();
    assert_eq!(
        interp.env.get("result"),
        Some(Value::String("caught".into()))
    );
}

#[test]
fn test_rescue_pipeline_error_map_has_message_op_code() {
    let src =
        "task boom(x)\n    raise \"domain error\"\n\nresult = 1 >> boom >> rescue error.message";
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&stmts).unwrap();
    assert_eq!(
        interp.env.get("result"),
        Some(Value::String("domain error".into()))
    );
}

#[test]
fn test_rescue_pipeline_error_code_field() {
    let src = "task boom(x)\n    raise \"x\"\n\nresult = 1 >> boom >> rescue error.code";
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&stmts).unwrap();
    assert_eq!(
        interp.env.get("result"),
        Some(Value::String("raise_error".into()))
    );
}

#[test]
fn test_rescue_pipeline_no_error_passes_through() {
    let src = "task add1(x) => x + 1\nresult = 10 >> add1 >> rescue 0";
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&stmts).unwrap();
    // No error — rescue handler should NOT execute; result = 11
    assert_eq!(interp.env.get("result"), Some(Value::Integer(11)));
}

// ─── rescue — expression modifier ─────────────────────────────────────────

#[test]
fn test_rescue_expression_catches_undefined_variable() {
    let src = "result = missing_var rescue \"default\"";
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&stmts).unwrap();
    assert_eq!(
        interp.env.get("result"),
        Some(Value::String("default".into()))
    );
}

#[test]
fn test_rescue_expression_no_error_passes_through() {
    let src = "x = 42\nresult = x rescue 0";
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&stmts).unwrap();
    assert_eq!(interp.env.get("result"), Some(Value::Integer(42)));
}

#[test]
fn test_rescue_expression_error_map_available_in_handler() {
    let src = "result = (1 / 0) rescue error.code";
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&stmts).unwrap();
    assert_eq!(
        interp.env.get("result"),
        Some(Value::String("arithmetic_error".into()))
    );
}

// ─── Pipeline map/keep/skip edge cases ────────────────────────────────────

#[test]
fn test_pipeline_map_keep_all_pass() {
    // keep EXPR inside map block keeps elements where condition is true
    let src = "result = [1, 2, 3] >> map x\n    keep x if x > 0\n";
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&stmts).unwrap();
    assert_eq!(
        interp.env.get("result"),
        Some(Value::List(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3)
        ]))
    );
}

#[test]
fn test_pipeline_map_keep_none_pass() {
    let src = "result = [1, 2, 3] >> map x\n    keep x if x > 100\n";
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&stmts).unwrap();
    assert_eq!(interp.env.get("result"), Some(Value::List(vec![])));
}

#[test]
fn test_pipeline_map_skip_all() {
    let src = "result = [1, 2, 3] >> map x\n    skip if x > 0\n    keep x\n";
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&stmts).unwrap();
    assert_eq!(interp.env.get("result"), Some(Value::List(vec![])));
}

// ─── Task mutual calls ─────────────────────────────────────────────────────

#[test]
fn test_task_calls_another_task() {
    let src = "task double(n) => n * 2\ntask quad(n) => double(double(n))\nresult = quad(3)";
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&stmts).unwrap();
    assert_eq!(interp.env.get("result"), Some(Value::Integer(12)));
}

#[test]
fn test_wrong_arity_error_carries_task_name() {
    let src = "task add(a, b) => a + b\nresult = add(1)";
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let err = Interpreter::new().execute(&stmts).unwrap_err();
    if let MarretaError::WrongArity {
        task_name,
        expected,
        got,
        ..
    } = err
    {
        assert_eq!(task_name, "add");
        assert_eq!(expected, 2);
        assert_eq!(got, 1);
    } else {
        panic!("expected WrongArity, got: {:?}", err);
    }
}

#[test]
fn test_undefined_task_error_carries_name() {
    let src = "result = nonexistent(1)";
    let err = run_err(src);
    if let MarretaError::UndefinedTask { name, .. } = err {
        assert_eq!(name, "nonexistent");
    } else {
        panic!("expected UndefinedTask");
    }
}

// ─── rescue trace-frame hygiene ───────────────────────────────────────────
//
// Regression guard: when rescue catches an error, frames accumulated inside
// the failed branch must not linger on the stack. A subsequent genuinely
// uncaught error would otherwise render stale "ghost" frames.

#[test]
fn test_rescue_expression_clears_frames_from_failed_branch() {
    let src = "\
task boom(x)\n    raise \"oops\"\n\nval = boom(1) rescue \"fallback\"\n";
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&stmts).unwrap();
    assert!(
        interp.trace_frames.is_empty(),
        "expected no lingering frames after rescue, got {:?}",
        interp.trace_frames
    );
}

#[test]
fn test_rescue_pipeline_clears_frames_from_failed_branch() {
    let src = "\
task boom(x)\n    raise \"oops\"\n\nval = 1 >> boom >> rescue \"fallback\"\n";
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    interp.execute(&stmts).unwrap();
    assert!(
        interp.trace_frames.is_empty(),
        "expected no lingering frames after pipeline rescue, got {:?}",
        interp.trace_frames
    );
}

#[test]
fn test_rescued_error_does_not_pollute_next_uncaught_trace() {
    // Simulate a route-level scenario: one task errs and is rescued, then a
    // subsequent task errs uncaught. The uncaught trace must only carry the
    // outer route frame + the second task, not the first (rescued) task.
    use crate::ast::HttpVerb;

    let src = "\
task first(x)\n    raise \"first failure\"\n\n\
task second(x)\n    raise \"second failure\"\n\n\
val = first(1) rescue \"handled\"\nsecond(2)\n";
    let tokens = crate::lexer::Lexer::new(src).tokenize().unwrap();
    let stmts = crate::parser::Parser::new(tokens).parse().unwrap();
    let mut interp = Interpreter::new();
    let route_trace = interp.enter_route(&HttpVerb::Get, "/t", Some("routes/t".into()), 1, 1);
    route_trace.preserve();
    let err = interp.execute(&stmts).unwrap_err();
    let trace = interp.uncaught_trace_lines(&err);

    let joined = trace.join("\n");
    assert!(
        joined.contains("route GET /t"),
        "missing route frame: {}",
        joined
    );
    assert!(
        joined.contains("task second"),
        "missing second task frame: {}",
        joined
    );
    assert!(
        !joined.contains("task first"),
        "stale frame from rescued branch leaked into trace: {}",
        joined
    );
}
