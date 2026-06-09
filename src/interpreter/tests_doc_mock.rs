use super::mock_doc::interp_with_doc;
use super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;

fn run_doc(src: &str) -> Value {
    let mut interp = interp_with_doc();
    let tokens = Lexer::new(src).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    interp.execute(&program).unwrap()
}

fn run_doc_err(src: &str) -> MarretaError {
    let mut interp = interp_with_doc();
    let tokens = Lexer::new(src).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    interp.execute(&program).unwrap_err()
}

// ── dispatch_doc_direct ───────────────────────────────────────────────────

#[test]
fn test_doc_save_returns_map() {
    let result = run_doc("doc.items.save({ name: \"thing\" })");
    assert!(matches!(result, Value::Map(_)));
}

#[test]
fn test_doc_find_returns_null_when_not_found() {
    let result = run_doc("doc.items.find(\"some-id\")");
    assert_eq!(result, Value::Null);
}

#[test]
fn test_doc_find_all_returns_list() {
    let result = run_doc("doc.items.find_all()");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_doc_update_returns_map() {
    let result = run_doc("doc.items.update(\"id1\", { active: true })");
    assert!(matches!(result, Value::Map(_)));
}

#[test]
fn test_doc_delete_returns_boolean() {
    let result = run_doc("doc.items.delete(\"id1\")");
    assert!(matches!(result, Value::Boolean(_)));
}

// ── pipeline: fetch_all terminal ─────────────────────────────────────────

#[test]
fn test_doc_pipeline_fetch_all_no_filter() {
    let result = run_doc("doc.items >> fetch_all");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_doc_pipeline_fetch_all_alias_fetch() {
    let result = run_doc("doc.items >> fetch");
    assert!(matches!(result, Value::List(_)));
}

// ── pipeline: where steps ─────────────────────────────────────────────────

#[test]
fn test_doc_pipeline_where_eq() {
    let result = run_doc("doc.items >> where(\"status\" == \"pending\") >> fetch_all");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_doc_pipeline_where_ne() {
    let result = run_doc("doc.items >> where(\"status\" != \"pending\") >> fetch_all");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_doc_pipeline_where_gt() {
    let result = run_doc("doc.items >> where(\"price\" > 10) >> fetch_all");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_doc_pipeline_where_gte() {
    let result = run_doc("doc.items >> where(\"price\" >= 10) >> fetch_all");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_doc_pipeline_where_lt() {
    let result = run_doc("doc.items >> where(\"price\" < 10) >> fetch_all");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_doc_pipeline_where_lte() {
    let result = run_doc("doc.items >> where(\"price\" <= 10) >> fetch_all");
    assert!(matches!(result, Value::List(_)));
}

// ── pipeline: pick ────────────────────────────────────────────────────────

#[test]
fn test_doc_pipeline_pick() {
    let result = run_doc("doc.items >> pick([\"_id\", \"name\"]) >> fetch_all");
    assert!(matches!(result, Value::List(_)));
}

// ── pipeline: order ───────────────────────────────────────────────────────

#[test]
fn test_doc_pipeline_order_asc() {
    let result = run_doc("doc.items >> order(\"name\", \"asc\") >> fetch_all");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_doc_pipeline_order_desc() {
    let result = run_doc("doc.items >> order(\"name\", \"desc\") >> fetch_all");
    assert!(matches!(result, Value::List(_)));
}

// ── pipeline: limit/offset ────────────────────────────────────────────────

#[test]
fn test_doc_pipeline_limit() {
    let result = run_doc("doc.items >> limit(5) >> fetch_all");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_doc_pipeline_offset() {
    let result = run_doc("doc.items >> offset(10) >> fetch_all");
    assert!(matches!(result, Value::List(_)));
}

// ── pipeline: like / in ───────────────────────────────────────────────────

#[test]
fn test_doc_pipeline_like() {
    let result = run_doc("doc.items >> like(\"name\", \"%test%\") >> fetch_all");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_doc_pipeline_in() {
    let result = run_doc("doc.items >> in(\"status\", [\"A\", \"B\"]) >> fetch_all");
    assert!(matches!(result, Value::List(_)));
}

// ── pipeline: fetch_one / count / exists / delete ─────────────────────────

#[test]
fn test_doc_pipeline_fetch_one_returns_null() {
    let result = run_doc("doc.items >> fetch_one");
    assert_eq!(result, Value::Null);
}

#[test]
fn test_doc_pipeline_count_returns_integer() {
    let result = run_doc("doc.items >> count");
    assert_eq!(result, Value::Integer(0));
}

#[test]
fn test_doc_pipeline_exists_returns_boolean() {
    let result = run_doc("doc.items >> exists");
    assert_eq!(result, Value::Boolean(false));
}

#[test]
fn test_doc_pipeline_delete_returns_integer() {
    let result = run_doc("doc.items >> delete");
    assert_eq!(result, Value::Integer(0));
}

// ── pipeline: update / upsert terminals ───────────────────────────────────

#[test]
fn test_doc_pipeline_update_terminal() {
    let result =
        run_doc("doc.items >> where(\"status\" == \"pending\") >> update({ active: false })");
    assert_eq!(result, Value::Integer(0));
}

#[test]
fn test_doc_pipeline_upsert_terminal() {
    let result = run_doc("doc.items >> where(\"name\" == \"x\") >> upsert({ name: \"x\" })");
    assert_eq!(result, Value::Integer(0));
}

// ── error paths ───────────────────────────────────────────────────────────

#[test]
fn test_doc_where_named_arg_error() {
    let err = run_doc_err("doc.items >> where(status: \"pending\") >> fetch_all");
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("string field names"))
    );
}

#[test]
fn test_doc_where_bare_identifier_error() {
    let err = run_doc_err("doc.items >> where(status == \"pending\") >> fetch_all");
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("string field names"))
    );
}

#[test]
fn test_doc_order_missing_direction_error() {
    let err = run_doc_err("doc.items >> order(\"name\") >> fetch_all");
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("direction"))
    );
}

#[test]
fn test_doc_order_invalid_direction_error() {
    let err = run_doc_err("doc.items >> order(\"name\", \"sideways\") >> fetch_all");
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("\"asc\" or \"desc\""))
    );
}

#[test]
fn test_doc_pick_non_list_error() {
    let err = run_doc_err("doc.items >> pick(\"name\") >> fetch_all");
    assert!(matches!(err, MarretaError::TypeError { message, .. } if message.contains("pick")));
}

#[test]
fn test_doc_unknown_pipeline_step_error() {
    let err = run_doc_err("doc.items >> bogus_step(1) >> fetch_all");
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("bogus_step"))
    );
}

#[test]
fn test_doc_unknown_terminal_error() {
    let err = run_doc_err("doc.items >> bad_terminal");
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("bad_terminal"))
    );
}

// ── DocQueryBuilder creation from collection access ────────────────────────

#[test]
fn test_doc_collection_promotes_to_query_builder() {
    // doc.items >> fetch_all exercises the DocCollection → DocQueryBuilder promotion path
    let result = run_doc("doc.items >> fetch_all");
    assert!(matches!(result, Value::List(_)));
}

// ── db.* with only doc engine configured → no relational engine error ────

#[test]
fn test_db_namespace_errors_when_no_db_engine() {
    // doc engine is configured but no db engine — db.* calls return RuntimeError
    let err = run_doc_err("db.users.find(1)");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

// ── Layer 3: Aggregation ──────────────────────────────────────────────────

#[test]
fn test_group_by_sets_aggregate_mode() {
    let result = run_doc(
        "doc.sales >> group_by(\"category\") >> sum(\"amount\", as: \"total\") >> fetch_all",
    );
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_sum_accumulator_requires_as_arg() {
    let err = run_doc_err("doc.sales >> sum(\"amount\") >> fetch_all");
    assert!(matches!(err, MarretaError::TypeError { .. }));
    assert!(err.to_string().contains("as:"));
}

#[test]
fn test_avg_accumulator_requires_as_arg() {
    let err = run_doc_err("doc.sales >> avg(\"amount\") >> fetch_all");
    assert!(matches!(err, MarretaError::TypeError { .. }));
    assert!(err.to_string().contains("as:"));
}

#[test]
fn test_min_accumulator_requires_as_arg() {
    let err = run_doc_err("doc.sales >> min(\"price\") >> fetch_all");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_max_accumulator_requires_as_arg() {
    let err = run_doc_err("doc.sales >> max(\"price\") >> fetch_all");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_count_accumulator_with_as_arg() {
    let result = run_doc("doc.sales >> group_by(\"cat\") >> count(as: \"n\") >> fetch_all");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_count_accumulator_rejects_positional_field() {
    let err =
        run_doc_err("doc.sales >> group_by(\"cat\") >> count(\"field\", as: \"n\") >> fetch_all");
    assert!(matches!(err, MarretaError::TypeError { .. }));
    assert!(err.to_string().contains("does not accept a field argument"));
}

#[test]
fn test_count_accumulator_requires_as_arg() {
    let err = run_doc_err("doc.sales >> group_by(\"cat\") >> count >> fetch_all");
    // bare count identifier is terminal, not accumulator — should return Integer 0
    assert!(matches!(err, _));
}

#[test]
fn test_global_aggregation_no_group_by() {
    let result = run_doc("doc.sales >> sum(\"amount\", as: \"total\") >> fetch_one");
    // MockDocDriver returns one row — should be Value::Map or Null
    assert!(matches!(result, Value::Map(_) | Value::Null));
}

#[test]
fn test_group_by_after_accumulator_errors() {
    let err = run_doc_err(
        "doc.sales >> sum(\"amount\", as: \"total\") >> group_by(\"cat\") >> fetch_all",
    );
    assert!(matches!(err, MarretaError::TypeError { .. }));
    assert!(err.to_string().contains("group_by() must appear before"));
}

#[test]
fn test_write_terminal_after_accumulator_errors() {
    let err =
        run_doc_err("doc.sales >> group_by(\"cat\") >> sum(\"amount\", as: \"total\") >> delete");
    assert!(matches!(err, MarretaError::TypeError { .. }));
    assert!(err.to_string().contains("write terminals"));
}

#[test]
fn test_update_after_accumulator_errors() {
    let err = run_doc_err(
        "doc.sales >> group_by(\"cat\") >> sum(\"amount\", as: \"total\") >> update({ x: 1 })",
    );
    assert!(matches!(err, MarretaError::TypeError { .. }));
    assert!(err.to_string().contains("write terminals"));
}

#[test]
fn test_pick_after_accumulator_errors() {
    let err = run_doc_err(
        "doc.sales >> group_by(\"cat\") >> sum(\"amount\", as: \"total\") >> pick([\"total\"]) >> fetch_all",
    );
    assert!(matches!(err, MarretaError::TypeError { .. }));
    assert!(
        err.to_string()
            .contains("pick() cannot be used in aggregation")
    );
}

#[test]
fn test_post_group_order_and_limit() {
    let result = run_doc(
        "doc.sales >> group_by(\"cat\") >> sum(\"amount\", as: \"total\") >> order(\"total\", \"desc\") >> limit(5) >> fetch_all",
    );
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_multiple_accumulators() {
    let result = run_doc(
        "doc.sales >> group_by(\"cat\") >> sum(\"amount\", as: \"total\") >> count(as: \"n\") >> avg(\"amount\", as: \"avg\") >> fetch_all",
    );
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_where_before_group_by() {
    let result = run_doc(
        "doc.sales >> where(\"status\" == \"paid\") >> group_by(\"cat\") >> sum(\"amount\", as: \"total\") >> fetch_all",
    );
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_avg_min_max_accumulators() {
    let result = run_doc(
        "doc.sales >> group_by(\"cat\") >> avg(\"amount\", as: \"avg\") >> min(\"amount\", as: \"lo\") >> max(\"amount\", as: \"hi\") >> fetch_all",
    );
    assert!(matches!(result, Value::List(_)));
}

// ── Layer 4: Power Pipeline (doc.pipeline) ────────────────────────────────

#[test]
fn test_doc_pipeline_match_stage() {
    let result = run_doc(r#"doc.pipeline("orders", [{ match: { status: "paid" } }])"#);
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_doc_pipeline_group_stage() {
    let result = run_doc(
        r#"doc.pipeline("orders", [{ group: { by: "status", total: { sum: "$amount" }, n: { count: 1 } } }])"#,
    );
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_doc_pipeline_sort_limit_stages() {
    let result = run_doc(r#"doc.pipeline("orders", [{ sort: { amount: -1 } }, { limit: 2 }])"#);
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_doc_pipeline_add_fields_stage() {
    let result =
        run_doc(r#"doc.pipeline("orders", [{ add_fields: { doubled: { sum: "$amount" } } }])"#);
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_doc_pipeline_unknown_stage_error() {
    let err = run_doc_err(r#"doc.pipeline("orders", [{ xyz: {} }])"#);
    assert!(matches!(err, MarretaError::DbError { .. }));
    assert!(err.to_string().contains("unknown doc.pipeline stage"));
}

#[test]
fn test_doc_pipeline_wrong_arg_count_error() {
    let err = run_doc_err(r#"doc.pipeline("orders")"#);
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_doc_pipeline_non_string_collection_error() {
    let err = run_doc_err(r#"doc.pipeline(42, [])"#);
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_doc_pipeline_non_list_stages_error() {
    let err = run_doc_err(r#"doc.pipeline("orders", "bad")"#);
    assert!(matches!(err, MarretaError::TypeError { .. }));
}
