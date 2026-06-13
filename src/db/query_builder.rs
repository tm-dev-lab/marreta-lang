use crate::db::driver::{FilterClause, FilterOp, JoinKind, QueryState};
use crate::error::{ErrorCode, MarretaError};
use crate::value::Value;
use std::collections::HashSet;

// ─── Spec 076: identifier hardening ─────────────────────────────────────────────
//
// Filter values are bound as `$1`, `$2`, ... but identifiers (order_by, select columns, filter
// column names) are concatenated into the SQL string, so a runtime-derived identifier would be a
// SQL injection vector. The guard below is the runtime defense the 071 lint only warns about.

/// Truncate an offending identifier for an error message (the value is the caller's own input,
/// never our SQL, but cap the length).
fn truncate_for_message(raw: &str) -> String {
    let s = raw.trim();
    if s.chars().count() > 60 {
        format!("{}...", s.chars().take(60).collect::<String>())
    } else {
        s.to_string()
    }
}

fn invalid_identifier(raw: &str) -> MarretaError {
    MarretaError::DbIdentifierError {
        code: ErrorCode::InvalidIdentifier,
        message: format!("invalid SQL identifier '{}'", truncate_for_message(raw)),
    }
}

/// Spec 076 floor: validate a SQL identifier (`name` or `table.name`, each segment
/// `[A-Za-z_][A-Za-z0-9_]*`) and return it double-quoted. Anything else is rejected, which makes
/// injection structurally impossible (a string that passes cannot contain a space, quote, `;`, or
/// `--`). Pure function, tested in isolation.
pub fn quote_identifier(raw: &str) -> Result<String, MarretaError> {
    fn segment_ok(seg: &str) -> bool {
        let mut chars = seg.chars();
        matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    }
    let trimmed = raw.trim();
    let segments: Vec<&str> = trimmed.split('.').collect();
    let shaped = matches!(segments.len(), 1 | 2) && segments.iter().all(|s| segment_ok(s));
    if !shaped {
        return Err(invalid_identifier(trimmed));
    }
    Ok(segments
        .iter()
        .map(|s| format!("\"{}\"", s))
        .collect::<Vec<_>>()
        .join("."))
}

/// Floor plus the optional schema layer: validate (and quote) a column identifier, and when the
/// table's known columns are present, reject an unqualified column that is not one of them. A
/// qualified `table.col` is guarded by the floor only (it may reference a joined table).
fn validate_column(raw: &str, known: &Option<HashSet<String>>) -> Result<String, MarretaError> {
    let quoted = quote_identifier(raw)?;
    if let Some(cols) = known {
        let trimmed = raw.trim();
        if !trimmed.contains('.') && !cols.contains(trimmed) {
            return Err(MarretaError::DbIdentifierError {
                code: ErrorCode::UnknownColumn,
                message: format!("unknown column '{}'", truncate_for_message(trimmed)),
            });
        }
    }
    Ok(quoted)
}

/// Spec 076: parse `order_by` into `column [asc|desc]` parts (comma-separated), validating each
/// column via the floor (plus the schema layer when present) and accepting only the asc/desc
/// keyword. Pure function, tested in isolation.
pub fn build_order_by(
    order: &str,
    known: &Option<HashSet<String>>,
) -> Result<String, MarretaError> {
    let mut rendered = Vec::new();
    for part in order.split(',') {
        let tokens: Vec<&str> = part.split_whitespace().collect();
        match tokens.as_slice() {
            [col] => rendered.push(validate_column(col, known)?),
            [col, dir] => {
                let direction = dir.to_ascii_uppercase();
                if direction != "ASC" && direction != "DESC" {
                    return Err(invalid_identifier(part));
                }
                rendered.push(format!("{} {}", validate_column(col, known)?, dir));
            }
            _ => return Err(invalid_identifier(part.trim())),
        }
    }
    if rendered.is_empty() {
        return Err(invalid_identifier(order));
    }
    Ok(rendered.join(", "))
}

/// Builds a SELECT SQL string + ordered parameter list from a `QueryState`.
///
/// Returns `(sql, params)` where `params` are the values to bind as `$1`, `$2`, ...
/// Identifiers (select columns, filter columns, order_by) are validated and quoted (Spec 076).
pub fn build_select(q: &QueryState) -> Result<(String, Vec<Value>), MarretaError> {
    let mut params: Vec<Value> = Vec::new();

    // SELECT clause
    let select = if q.count {
        "COUNT(*) AS count".to_string()
    } else if q.select_cols.is_empty() {
        "*".to_string()
    } else {
        q.select_cols
            .iter()
            .map(|c| validate_column(c, &q.known_columns))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    };

    let mut sql = format!("SELECT {} FROM {}", select, q.table);

    // JOINs
    for join in &q.joins {
        let kind = match join.kind {
            JoinKind::Inner => "INNER JOIN",
            JoinKind::Left => "LEFT JOIN",
        };
        // Convention: left.fk = right.id
        sql.push_str(&format!(
            " {} {} ON {}.{} = {}.id",
            kind, join.table, q.table, join.on, join.table
        ));
    }

    // WHERE
    if !q.filters.is_empty() {
        let clauses: Vec<String> = q
            .filters
            .iter()
            .map(|f| {
                let param_n = params.len() + 1;
                let column = validate_column(&f.column, &q.known_columns)?;

                if f.op == FilterOp::In {
                    // IN expects a list — expand inline
                    if let Value::List(items) = &f.value {
                        let placeholders: Vec<String> = items
                            .iter()
                            .enumerate()
                            .map(|(i, v)| {
                                params.push(v.clone());
                                format!("${}", param_n + i)
                            })
                            .collect();
                        return Ok(format!("{} IN ({})", column, placeholders.join(", ")));
                    }
                    // Fallback: single value
                    params.push(f.value.clone());
                    Ok(format!("{} IN (${})", column, param_n))
                } else {
                    params.push(f.value.clone());
                    Ok(format!("{} {} ${}", column, f.op.to_sql(), param_n))
                }
            })
            .collect::<Result<Vec<_>, MarretaError>>()?;
        sql.push_str(&format!(" WHERE {}", clauses.join(" AND ")));
    }

    // ORDER BY
    if let Some(order) = &q.order_by {
        sql.push_str(&format!(
            " ORDER BY {}",
            build_order_by(order, &q.known_columns)?
        ));
    }

    // LIMIT / OFFSET
    if let Some(limit) = q.limit {
        sql.push_str(&format!(" LIMIT {}", limit));
    }
    if let Some(offset) = q.offset {
        sql.push_str(&format!(" OFFSET {}", offset));
    }

    Ok((sql, params))
}

/// Builds a `UPDATE table SET col=$1 WHERE ...` string. SET columns and filter columns are
/// validated and quoted (Spec 076).
pub fn build_update(
    table: &str,
    data_keys: &[String],
    filters: &[FilterClause],
    known: &Option<HashSet<String>>,
) -> Result<(String, usize), MarretaError> {
    let set_clauses: Vec<String> = data_keys
        .iter()
        .enumerate()
        .map(|(i, k)| Ok(format!("{} = ${}", validate_column(k, known)?, i + 1)))
        .collect::<Result<Vec<_>, MarretaError>>()?;

    let base_param = data_keys.len() + 1;
    let where_clauses: Vec<String> = filters
        .iter()
        .enumerate()
        .map(|(i, f)| {
            Ok(format!(
                "{} {} ${}",
                validate_column(&f.column, known)?,
                f.op.to_sql(),
                base_param + i
            ))
        })
        .collect::<Result<Vec<_>, MarretaError>>()?;

    let mut sql = format!("UPDATE {} SET {}", table, set_clauses.join(", "));
    if !where_clauses.is_empty() {
        sql.push_str(&format!(" WHERE {}", where_clauses.join(" AND ")));
    }

    Ok((sql, base_param))
}

/// Builds a `DELETE FROM table WHERE ...` string. Filter columns are validated and quoted
/// (Spec 076).
pub fn build_delete(
    table: &str,
    filters: &[FilterClause],
    known: &Option<HashSet<String>>,
) -> Result<(String, usize), MarretaError> {
    let mut sql = format!("DELETE FROM {}", table);
    if !filters.is_empty() {
        let clauses: Vec<String> = filters
            .iter()
            .enumerate()
            .map(|(i, f)| {
                Ok(format!(
                    "{} {} ${}",
                    validate_column(&f.column, known)?,
                    f.op.to_sql(),
                    i + 1
                ))
            })
            .collect::<Result<Vec<_>, MarretaError>>()?;
        sql.push_str(&format!(" WHERE {}", clauses.join(" AND ")));
    }
    Ok((sql, 1))
}

/// Extracts `FilterClause` list from a `Vec<(key, value)>` equality map
/// (used by `db.TABLE.find_all(key: val)` direct calls).
pub fn filters_from_equality_map(pairs: Vec<(String, Value)>) -> Vec<FilterClause> {
    pairs
        .into_iter()
        .map(|(col, val)| FilterClause {
            column: col,
            op: FilterOp::Eq,
            value: val,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::driver::{JoinClause, JoinKind};

    fn eq(col: &str, val: Value) -> FilterClause {
        FilterClause {
            column: col.to_string(),
            op: FilterOp::Eq,
            value: val,
        }
    }

    fn filter(col: &str, op: FilterOp, val: Value) -> FilterClause {
        FilterClause {
            column: col.to_string(),
            op,
            value: val,
        }
    }

    fn inner_join(table: &str, on: &str) -> JoinClause {
        JoinClause {
            kind: JoinKind::Inner,
            table: table.to_string(),
            on: on.to_string(),
        }
    }

    fn left_join(table: &str, on: &str) -> JoinClause {
        JoinClause {
            kind: JoinKind::Left,
            table: table.to_string(),
            on: on.to_string(),
        }
    }

    // ─── build_select ──────────────────────────────────────────────────────────

    #[test]
    fn test_select_all_no_filters() {
        let q = QueryState::new("users");
        let (sql, params) = build_select(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM users");
        assert!(params.is_empty());
    }

    #[test]
    fn test_select_with_equality_filter() {
        let mut q = QueryState::new("users");
        q.filters = vec![eq("status", Value::String("active".to_string()))];
        let (sql, params) = build_select(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM users WHERE \"status\" = $1");
        assert_eq!(params, vec![Value::String("active".to_string())]);
    }

    #[test]
    fn test_select_with_multiple_filters_and_clauses() {
        let mut q = QueryState::new("orders");
        q.filters = vec![
            filter("total", FilterOp::Gt, Value::Integer(1000)),
            eq("status", Value::String("paid".to_string())),
        ];
        let (sql, params) = build_select(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM orders WHERE \"total\" > $1 AND \"status\" = $2"
        );
        assert_eq!(params[0], Value::Integer(1000));
        assert_eq!(params[1], Value::String("paid".to_string()));
    }

    #[test]
    fn test_select_all_comparison_operators() {
        let cases = vec![
            (FilterOp::Gt, ">"),
            (FilterOp::Gte, ">="),
            (FilterOp::Lt, "<"),
            (FilterOp::Lte, "<="),
            (FilterOp::Ne, "!="),
        ];
        for (op, expected_op) in cases {
            let mut q = QueryState::new("orders");
            q.filters = vec![filter("total", op, Value::Integer(500))];
            let (sql, _) = build_select(&q).unwrap();
            assert!(
                sql.contains(&format!("\"total\" {} $1", expected_op)),
                "expected '{}' in '{}'",
                expected_op,
                sql
            );
        }
    }

    #[test]
    fn test_select_with_like_filter() {
        let mut q = QueryState::new("users");
        q.filters = vec![filter(
            "name",
            FilterOp::Like,
            Value::String("Ana%".to_string()),
        )];
        let (sql, params) = build_select(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM users WHERE \"name\" LIKE $1");
        assert_eq!(params[0], Value::String("Ana%".to_string()));
    }

    #[test]
    fn test_select_with_in_filter_list() {
        let mut q = QueryState::new("orders");
        q.filters = vec![filter(
            "status",
            FilterOp::In,
            Value::List(vec![
                Value::String("paid".to_string()),
                Value::String("pending".to_string()),
                Value::String("shipped".to_string()),
            ]),
        )];
        let (sql, params) = build_select(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM orders WHERE \"status\" IN ($1, $2, $3)");
        assert_eq!(params.len(), 3);
        assert_eq!(params[0], Value::String("paid".to_string()));
        assert_eq!(params[2], Value::String("shipped".to_string()));
    }

    #[test]
    fn test_select_with_in_filter_single_value_fallback() {
        let mut q = QueryState::new("orders");
        q.filters = vec![filter(
            "status",
            FilterOp::In,
            Value::String("paid".to_string()),
        )];
        let (sql, params) = build_select(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM orders WHERE \"status\" IN ($1)");
        assert_eq!(params[0], Value::String("paid".to_string()));
    }

    #[test]
    fn test_select_with_inner_join() {
        let mut q = QueryState::new("orders");
        q.joins = vec![inner_join("users", "user_id")];
        let (sql, params) = build_select(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM orders INNER JOIN users ON orders.user_id = users.id"
        );
        assert!(params.is_empty());
    }

    #[test]
    fn test_select_with_left_join() {
        let mut q = QueryState::new("orders");
        q.joins = vec![left_join("users", "user_id")];
        let (sql, _) = build_select(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM orders LEFT JOIN users ON orders.user_id = users.id"
        );
    }

    #[test]
    fn test_select_join_then_filter() {
        let mut q = QueryState::new("orders");
        q.joins = vec![inner_join("users", "user_id")];
        q.filters = vec![eq("status", Value::String("paid".to_string()))];
        let (sql, params) = build_select(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM orders INNER JOIN users ON orders.user_id = users.id WHERE \"status\" = $1"
        );
        assert_eq!(params[0], Value::String("paid".to_string()));
    }

    #[test]
    fn test_select_with_order_by() {
        let mut q = QueryState::new("products");
        q.order_by = Some("price desc".to_string());
        let (sql, _) = build_select(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM products ORDER BY \"price\" desc");
    }

    #[test]
    fn test_select_with_limit_and_offset() {
        let mut q = QueryState::new("products");
        q.limit = Some(10);
        q.offset = Some(20);
        let (sql, _) = build_select(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM products LIMIT 10 OFFSET 20");
    }

    #[test]
    fn test_select_with_explicit_columns() {
        let mut q = QueryState::new("users");
        q.select_cols = vec!["id".to_string(), "name".to_string(), "email".to_string()];
        let (sql, _) = build_select(&q).unwrap();
        assert_eq!(sql, "SELECT \"id\", \"name\", \"email\" FROM users");
    }

    #[test]
    fn test_select_full_pipeline() {
        let mut q = QueryState::new("orders");
        q.joins = vec![inner_join("users", "user_id")];
        q.filters = vec![
            filter("total", FilterOp::Gte, Value::Integer(500)),
            eq("users.status", Value::String("active".to_string())),
        ];
        q.order_by = Some("total desc".to_string());
        q.limit = Some(5);
        q.offset = Some(10);
        let (sql, params) = build_select(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM orders INNER JOIN users ON orders.user_id = users.id \
             WHERE \"total\" >= $1 AND \"users\".\"status\" = $2 ORDER BY \"total\" desc LIMIT 5 OFFSET 10"
        );
        assert_eq!(params[0], Value::Integer(500));
        assert_eq!(params[1], Value::String("active".to_string()));
    }

    #[test]
    fn test_select_in_filter_param_numbering_after_previous_filter() {
        // IN filter: params should continue from where the previous filter left off
        let mut q = QueryState::new("orders");
        q.filters = vec![
            eq("user_id", Value::Integer(42)),
            filter(
                "status",
                FilterOp::In,
                Value::List(vec![
                    Value::String("paid".to_string()),
                    Value::String("shipped".to_string()),
                ]),
            ),
        ];
        let (sql, params) = build_select(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM orders WHERE \"user_id\" = $1 AND \"status\" IN ($2, $3)"
        );
        assert_eq!(params[0], Value::Integer(42));
        assert_eq!(params[1], Value::String("paid".to_string()));
        assert_eq!(params[2], Value::String("shipped".to_string()));
    }

    // ─── build_update ──────────────────────────────────────────────────────────

    #[test]
    fn test_update_single_field_no_filter() {
        let (sql, base) = build_update("users", &["name".to_string()], &[], &None).unwrap();
        assert_eq!(sql, "UPDATE users SET \"name\" = $1");
        assert_eq!(base, 2);
    }

    #[test]
    fn test_update_multiple_fields_with_filter() {
        let filters = vec![eq("id", Value::Integer(1))];
        let (sql, base) = build_update(
            "users",
            &["name".to_string(), "email".to_string()],
            &filters,
            &None,
        )
        .unwrap();
        assert_eq!(
            sql,
            "UPDATE users SET \"name\" = $1, \"email\" = $2 WHERE \"id\" = $3"
        );
        assert_eq!(base, 3);
    }

    #[test]
    fn test_update_base_param_offset_is_data_len_plus_one() {
        // 3 data fields → base_param = 4 (filters start at $4)
        let filters = vec![
            eq("status", Value::String("active".to_string())),
            filter("total", FilterOp::Gt, Value::Integer(0)),
        ];
        let keys: Vec<String> = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let (sql, base) = build_update("orders", &keys, &filters, &None).unwrap();
        assert_eq!(
            sql,
            "UPDATE orders SET \"a\" = $1, \"b\" = $2, \"c\" = $3 WHERE \"status\" = $4 AND \"total\" > $5"
        );
        assert_eq!(base, 4);
    }

    // ─── build_delete ──────────────────────────────────────────────────────────

    #[test]
    fn test_delete_no_filters() {
        let (sql, _) = build_delete("logs", &[], &None).unwrap();
        assert_eq!(sql, "DELETE FROM logs");
    }

    #[test]
    fn test_delete_with_single_filter() {
        let filters = vec![eq("id", Value::Integer(99))];
        let (sql, _) = build_delete("orders", &filters, &None).unwrap();
        assert_eq!(sql, "DELETE FROM orders WHERE \"id\" = $1");
    }

    #[test]
    fn test_delete_with_multiple_filters() {
        let filters = vec![
            eq("user_id", Value::Integer(5)),
            eq("status", Value::String("cancelled".to_string())),
        ];
        let (sql, _) = build_delete("orders", &filters, &None).unwrap();
        assert_eq!(
            sql,
            "DELETE FROM orders WHERE \"user_id\" = $1 AND \"status\" = $2"
        );
    }

    // ─── filters_from_equality_map ─────────────────────────────────────────────

    #[test]
    fn test_filters_from_equality_map_produces_eq_clauses() {
        let pairs = vec![
            ("status".to_string(), Value::String("active".to_string())),
            ("role".to_string(), Value::String("admin".to_string())),
        ];
        let filters = filters_from_equality_map(pairs);
        assert_eq!(filters.len(), 2);
        assert_eq!(filters[0].column, "status");
        assert_eq!(filters[0].op, FilterOp::Eq);
        assert_eq!(filters[1].column, "role");
    }

    // ─── Edge cases ───────────────────────────────────────────────────────────

    #[test]
    fn test_select_no_filters_no_clauses_is_clean() {
        let q = QueryState::new("events");
        let (sql, params) = build_select(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM events");
        assert!(params.is_empty());
        assert!(!sql.contains("WHERE"));
        assert!(!sql.contains("ORDER"));
        assert!(!sql.contains("LIMIT"));
    }

    #[test]
    fn test_select_limit_without_offset() {
        let mut q = QueryState::new("users");
        q.limit = Some(5);
        let (sql, _) = build_select(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM users LIMIT 5");
        assert!(!sql.contains("OFFSET"));
    }

    #[test]
    fn test_select_offset_without_limit() {
        let mut q = QueryState::new("users");
        q.offset = Some(10);
        let (sql, _) = build_select(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM users OFFSET 10");
        assert!(!sql.contains("LIMIT"));
    }

    #[test]
    fn test_select_ne_filter() {
        let mut q = QueryState::new("users");
        q.filters = vec![filter(
            "status",
            FilterOp::Ne,
            Value::String("banned".to_string()),
        )];
        let (sql, params) = build_select(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM users WHERE \"status\" != $1");
        assert_eq!(params[0], Value::String("banned".to_string()));
    }

    #[test]
    fn test_select_lte_filter() {
        let mut q = QueryState::new("orders");
        q.filters = vec![filter("total", FilterOp::Lte, Value::Integer(100))];
        let (sql, _) = build_select(&q).unwrap();
        assert!(sql.contains("\"total\" <= $1"));
    }

    #[test]
    fn test_select_empty_in_list_fallback_to_single() {
        // An empty List for IN falls back to IN ($1) with the empty list value
        let mut q = QueryState::new("orders");
        q.filters = vec![filter("status", FilterOp::In, Value::List(vec![]))];
        let (sql, params) = build_select(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM orders WHERE \"status\" IN ()");
        assert!(params.is_empty());
    }

    #[test]
    fn test_select_two_joins() {
        let mut q = QueryState::new("orders");
        q.joins = vec![
            inner_join("users", "user_id"),
            left_join("products", "product_id"),
        ];
        let (sql, _) = build_select(&q).unwrap();
        assert!(sql.contains("INNER JOIN users ON orders.user_id = users.id"));
        assert!(sql.contains("LEFT JOIN products ON orders.product_id = products.id"));
    }

    #[test]
    fn test_select_explicit_columns_with_filter() {
        let mut q = QueryState::new("users");
        q.select_cols = vec!["id".to_string(), "email".to_string()];
        q.filters = vec![eq("active", Value::Boolean(true))];
        let (sql, params) = build_select(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT \"id\", \"email\" FROM users WHERE \"active\" = $1"
        );
        assert_eq!(params[0], Value::Boolean(true));
    }

    #[test]
    fn test_select_float_value_in_filter() {
        let mut q = QueryState::new("products");
        q.filters = vec![filter("price", FilterOp::Gt, Value::Float(9.99))];
        let (sql, params) = build_select(&q).unwrap();
        assert!(sql.contains("\"price\" > $1"));
        assert_eq!(params[0], Value::Float(9.99));
    }

    #[test]
    fn test_select_null_value_in_filter() {
        let mut q = QueryState::new("users");
        q.filters = vec![eq("deleted_at", Value::Null)];
        let (sql, params) = build_select(&q).unwrap();
        assert!(sql.contains("\"deleted_at\" = $1"));
        assert_eq!(params[0], Value::Null);
    }

    #[test]
    fn test_update_no_filters_no_where_clause() {
        let (sql, _) = build_update("settings", &["value".to_string()], &[], &None).unwrap();
        assert!(!sql.contains("WHERE"));
        assert_eq!(sql, "UPDATE settings SET \"value\" = $1");
    }

    #[test]
    fn test_update_three_fields_param_numbering() {
        let (sql, base) = build_update(
            "users",
            &["a".to_string(), "b".to_string(), "c".to_string()],
            &[eq("id", Value::Integer(1))],
            &None,
        )
        .unwrap();
        assert_eq!(
            sql,
            "UPDATE users SET \"a\" = $1, \"b\" = $2, \"c\" = $3 WHERE \"id\" = $4"
        );
        assert_eq!(base, 4);
    }

    #[test]
    fn test_delete_with_ne_filter() {
        let filters = vec![filter(
            "status",
            FilterOp::Ne,
            Value::String("active".to_string()),
        )];
        let (sql, _) = build_delete("sessions", &filters, &None).unwrap();
        assert_eq!(sql, "DELETE FROM sessions WHERE \"status\" != $1");
    }

    #[test]
    fn test_filters_from_equality_map_empty_input() {
        let filters = filters_from_equality_map(vec![]);
        assert!(filters.is_empty());
    }

    #[test]
    fn test_filters_from_equality_map_single_entry() {
        let pairs = vec![("id".to_string(), Value::Integer(7))];
        let filters = filters_from_equality_map(pairs);
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].column, "id");
        assert_eq!(filters[0].op, FilterOp::Eq);
        assert_eq!(filters[0].value, Value::Integer(7));
    }

    // ─── Spec 076: identifier hardening (pure classifiers, tested in isolation) ───

    fn is_invalid(e: &MarretaError) -> bool {
        matches!(
            e,
            MarretaError::DbIdentifierError {
                code: ErrorCode::InvalidIdentifier,
                ..
            }
        )
    }

    fn is_unknown(e: &MarretaError) -> bool {
        matches!(
            e,
            MarretaError::DbIdentifierError {
                code: ErrorCode::UnknownColumn,
                ..
            }
        )
    }

    #[test]
    fn quote_identifier_floor() {
        assert_eq!(quote_identifier("price").unwrap(), "\"price\"");
        assert_eq!(quote_identifier("orders.id").unwrap(), "\"orders\".\"id\"");
        assert_eq!(quote_identifier("  status  ").unwrap(), "\"status\"");
        for bad in [
            "price; drop table users",
            "price'",
            "a--b",
            "count(*)",
            "total * 0.9",
            "a b",
            "",
            "1col",
            "a.b.c",
            "price)",
        ] {
            assert!(
                is_invalid(&quote_identifier(bad).unwrap_err()),
                "should reject: {bad}"
            );
        }
    }

    #[test]
    fn order_by_parser() {
        let none = None;
        assert_eq!(build_order_by("price", &none).unwrap(), "\"price\"");
        assert_eq!(
            build_order_by("price desc", &none).unwrap(),
            "\"price\" desc"
        );
        assert_eq!(
            build_order_by("price DESC", &none).unwrap(),
            "\"price\" DESC"
        );
        assert_eq!(
            build_order_by("a, b asc", &none).unwrap(),
            "\"a\", \"b\" asc"
        );
        for bad in ["price; drop", "price sideways", "price asc desc", "(price)"] {
            assert!(
                is_invalid(&build_order_by(bad, &none).unwrap_err()),
                "should reject: {bad}"
            );
        }
    }

    #[test]
    fn schema_layer_rejects_unknown_column_but_floor_only_on_qualified() {
        let known: Option<HashSet<String>> = Some(
            ["id".to_string(), "status".to_string()]
                .into_iter()
                .collect(),
        );
        // known column passes
        assert!(validate_column("status", &known).is_ok());
        // floor-passing but unknown column is rejected
        assert!(is_unknown(&validate_column("secret", &known).unwrap_err()));
        // qualified column bypasses the schema layer (floor only), so a joined column is allowed
        assert!(validate_column("other.col", &known).is_ok());
        // an illegal form is still an invalid_identifier, not unknown_column
        assert!(is_invalid(
            &validate_column("status; drop", &known).unwrap_err()
        ));
    }

    #[test]
    fn build_select_rejects_injection_in_each_surface() {
        // order_by injection
        let mut q = QueryState::new("products");
        q.order_by = Some("price; DROP TABLE products".to_string());
        assert!(is_invalid(&build_select(&q).unwrap_err()));

        // select column injection (a computed expression is not a bare identifier)
        let mut q = QueryState::new("orders");
        q.select_cols = vec!["total * 0.9".to_string()];
        assert!(is_invalid(&build_select(&q).unwrap_err()));

        // filter column injection (like / in / eq all flow through f.column)
        let mut q = QueryState::new("users");
        q.filters = vec![filter(
            "name); DROP TABLE users; --",
            FilterOp::Like,
            Value::String("x".to_string()),
        )];
        assert!(is_invalid(&build_select(&q).unwrap_err()));
    }

    #[test]
    fn build_select_count_renders_trusted_aggregate() {
        let mut q = QueryState::new("users");
        q.count = true;
        // select_cols is ignored when count is set, so a stray value cannot reach the SQL
        q.select_cols = vec!["ignored".to_string()];
        let (sql, _) = build_select(&q).unwrap();
        assert_eq!(sql, "SELECT COUNT(*) AS count FROM users");
    }
}
