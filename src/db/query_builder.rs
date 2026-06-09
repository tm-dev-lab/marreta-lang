use crate::db::driver::{FilterClause, FilterOp, JoinKind, QueryState};
use crate::value::Value;

/// Builds a SELECT SQL string + ordered parameter list from a `QueryState`.
///
/// Returns `(sql, params)` where `params` are the values to bind as `$1`, `$2`, ...
pub fn build_select(q: &QueryState) -> (String, Vec<Value>) {
    let mut params: Vec<Value> = Vec::new();

    // SELECT clause
    let select = if q.select_cols.is_empty() {
        "*".to_string()
    } else {
        q.select_cols.join(", ")
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
                        return format!("{} IN ({})", f.column, placeholders.join(", "));
                    }
                    // Fallback: single value
                    params.push(f.value.clone());
                    format!("{} IN (${})", f.column, param_n)
                } else {
                    params.push(f.value.clone());
                    format!("{} {} ${}", f.column, f.op.to_sql(), param_n)
                }
            })
            .collect();
        sql.push_str(&format!(" WHERE {}", clauses.join(" AND ")));
    }

    // ORDER BY
    if let Some(order) = &q.order_by {
        sql.push_str(&format!(" ORDER BY {}", order));
    }

    // LIMIT / OFFSET
    if let Some(limit) = q.limit {
        sql.push_str(&format!(" LIMIT {}", limit));
    }
    if let Some(offset) = q.offset {
        sql.push_str(&format!(" OFFSET {}", offset));
    }

    (sql, params)
}

/// Builds a `UPDATE table SET col=$1 WHERE ...` string.
pub fn build_update(
    table: &str,
    data_keys: &[String],
    filters: &[FilterClause],
) -> (String, usize) {
    let set_clauses: Vec<String> = data_keys
        .iter()
        .enumerate()
        .map(|(i, k)| format!("{} = ${}", k, i + 1))
        .collect();

    let base_param = data_keys.len() + 1;
    let where_clauses: Vec<String> = filters
        .iter()
        .enumerate()
        .map(|(i, f)| format!("{} {} ${}", f.column, f.op.to_sql(), base_param + i))
        .collect();

    let mut sql = format!("UPDATE {} SET {}", table, set_clauses.join(", "));
    if !where_clauses.is_empty() {
        sql.push_str(&format!(" WHERE {}", where_clauses.join(" AND ")));
    }

    (sql, base_param)
}

/// Builds a `DELETE FROM table WHERE ...` string.
pub fn build_delete(table: &str, filters: &[FilterClause]) -> (String, usize) {
    let mut sql = format!("DELETE FROM {}", table);
    if !filters.is_empty() {
        let clauses: Vec<String> = filters
            .iter()
            .enumerate()
            .map(|(i, f)| format!("{} {} ${}", f.column, f.op.to_sql(), i + 1))
            .collect();
        sql.push_str(&format!(" WHERE {}", clauses.join(" AND ")));
    }
    (sql, 1)
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
        let (sql, params) = build_select(&q);
        assert_eq!(sql, "SELECT * FROM users");
        assert!(params.is_empty());
    }

    #[test]
    fn test_select_with_equality_filter() {
        let mut q = QueryState::new("users");
        q.filters = vec![eq("status", Value::String("active".to_string()))];
        let (sql, params) = build_select(&q);
        assert_eq!(sql, "SELECT * FROM users WHERE status = $1");
        assert_eq!(params, vec![Value::String("active".to_string())]);
    }

    #[test]
    fn test_select_with_multiple_filters_and_clauses() {
        let mut q = QueryState::new("orders");
        q.filters = vec![
            filter("total", FilterOp::Gt, Value::Integer(1000)),
            eq("status", Value::String("paid".to_string())),
        ];
        let (sql, params) = build_select(&q);
        assert_eq!(sql, "SELECT * FROM orders WHERE total > $1 AND status = $2");
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
            let (sql, _) = build_select(&q);
            assert!(
                sql.contains(&format!("total {} $1", expected_op)),
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
        let (sql, params) = build_select(&q);
        assert_eq!(sql, "SELECT * FROM users WHERE name LIKE $1");
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
        let (sql, params) = build_select(&q);
        assert_eq!(sql, "SELECT * FROM orders WHERE status IN ($1, $2, $3)");
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
        let (sql, params) = build_select(&q);
        assert_eq!(sql, "SELECT * FROM orders WHERE status IN ($1)");
        assert_eq!(params[0], Value::String("paid".to_string()));
    }

    #[test]
    fn test_select_with_inner_join() {
        let mut q = QueryState::new("orders");
        q.joins = vec![inner_join("users", "user_id")];
        let (sql, params) = build_select(&q);
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
        let (sql, _) = build_select(&q);
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
        let (sql, params) = build_select(&q);
        assert_eq!(
            sql,
            "SELECT * FROM orders INNER JOIN users ON orders.user_id = users.id WHERE status = $1"
        );
        assert_eq!(params[0], Value::String("paid".to_string()));
    }

    #[test]
    fn test_select_with_order_by() {
        let mut q = QueryState::new("products");
        q.order_by = Some("price desc".to_string());
        let (sql, _) = build_select(&q);
        assert_eq!(sql, "SELECT * FROM products ORDER BY price desc");
    }

    #[test]
    fn test_select_with_limit_and_offset() {
        let mut q = QueryState::new("products");
        q.limit = Some(10);
        q.offset = Some(20);
        let (sql, _) = build_select(&q);
        assert_eq!(sql, "SELECT * FROM products LIMIT 10 OFFSET 20");
    }

    #[test]
    fn test_select_with_explicit_columns() {
        let mut q = QueryState::new("users");
        q.select_cols = vec!["id".to_string(), "name".to_string(), "email".to_string()];
        let (sql, _) = build_select(&q);
        assert_eq!(sql, "SELECT id, name, email FROM users");
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
        let (sql, params) = build_select(&q);
        assert_eq!(
            sql,
            "SELECT * FROM orders INNER JOIN users ON orders.user_id = users.id \
             WHERE total >= $1 AND users.status = $2 ORDER BY total desc LIMIT 5 OFFSET 10"
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
        let (sql, params) = build_select(&q);
        assert_eq!(
            sql,
            "SELECT * FROM orders WHERE user_id = $1 AND status IN ($2, $3)"
        );
        assert_eq!(params[0], Value::Integer(42));
        assert_eq!(params[1], Value::String("paid".to_string()));
        assert_eq!(params[2], Value::String("shipped".to_string()));
    }

    // ─── build_update ──────────────────────────────────────────────────────────

    #[test]
    fn test_update_single_field_no_filter() {
        let (sql, base) = build_update("users", &["name".to_string()], &[]);
        assert_eq!(sql, "UPDATE users SET name = $1");
        assert_eq!(base, 2);
    }

    #[test]
    fn test_update_multiple_fields_with_filter() {
        let filters = vec![eq("id", Value::Integer(1))];
        let (sql, base) = build_update(
            "users",
            &["name".to_string(), "email".to_string()],
            &filters,
        );
        assert_eq!(sql, "UPDATE users SET name = $1, email = $2 WHERE id = $3");
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
        let (sql, base) = build_update("orders", &keys, &filters);
        assert_eq!(
            sql,
            "UPDATE orders SET a = $1, b = $2, c = $3 WHERE status = $4 AND total > $5"
        );
        assert_eq!(base, 4);
    }

    // ─── build_delete ──────────────────────────────────────────────────────────

    #[test]
    fn test_delete_no_filters() {
        let (sql, _) = build_delete("logs", &[]);
        assert_eq!(sql, "DELETE FROM logs");
    }

    #[test]
    fn test_delete_with_single_filter() {
        let filters = vec![eq("id", Value::Integer(99))];
        let (sql, _) = build_delete("orders", &filters);
        assert_eq!(sql, "DELETE FROM orders WHERE id = $1");
    }

    #[test]
    fn test_delete_with_multiple_filters() {
        let filters = vec![
            eq("user_id", Value::Integer(5)),
            eq("status", Value::String("cancelled".to_string())),
        ];
        let (sql, _) = build_delete("orders", &filters);
        assert_eq!(sql, "DELETE FROM orders WHERE user_id = $1 AND status = $2");
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
        let (sql, params) = build_select(&q);
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
        let (sql, _) = build_select(&q);
        assert_eq!(sql, "SELECT * FROM users LIMIT 5");
        assert!(!sql.contains("OFFSET"));
    }

    #[test]
    fn test_select_offset_without_limit() {
        let mut q = QueryState::new("users");
        q.offset = Some(10);
        let (sql, _) = build_select(&q);
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
        let (sql, params) = build_select(&q);
        assert_eq!(sql, "SELECT * FROM users WHERE status != $1");
        assert_eq!(params[0], Value::String("banned".to_string()));
    }

    #[test]
    fn test_select_lte_filter() {
        let mut q = QueryState::new("orders");
        q.filters = vec![filter("total", FilterOp::Lte, Value::Integer(100))];
        let (sql, _) = build_select(&q);
        assert!(sql.contains("total <= $1"));
    }

    #[test]
    fn test_select_empty_in_list_fallback_to_single() {
        // An empty List for IN falls back to IN ($1) with the empty list value
        let mut q = QueryState::new("orders");
        q.filters = vec![filter("status", FilterOp::In, Value::List(vec![]))];
        let (sql, params) = build_select(&q);
        assert_eq!(sql, "SELECT * FROM orders WHERE status IN ()");
        assert!(params.is_empty());
    }

    #[test]
    fn test_select_two_joins() {
        let mut q = QueryState::new("orders");
        q.joins = vec![
            inner_join("users", "user_id"),
            left_join("products", "product_id"),
        ];
        let (sql, _) = build_select(&q);
        assert!(sql.contains("INNER JOIN users ON orders.user_id = users.id"));
        assert!(sql.contains("LEFT JOIN products ON orders.product_id = products.id"));
    }

    #[test]
    fn test_select_explicit_columns_with_filter() {
        let mut q = QueryState::new("users");
        q.select_cols = vec!["id".to_string(), "email".to_string()];
        q.filters = vec![eq("active", Value::Boolean(true))];
        let (sql, params) = build_select(&q);
        assert_eq!(sql, "SELECT id, email FROM users WHERE active = $1");
        assert_eq!(params[0], Value::Boolean(true));
    }

    #[test]
    fn test_select_float_value_in_filter() {
        let mut q = QueryState::new("products");
        q.filters = vec![filter("price", FilterOp::Gt, Value::Float(9.99))];
        let (sql, params) = build_select(&q);
        assert!(sql.contains("price > $1"));
        assert_eq!(params[0], Value::Float(9.99));
    }

    #[test]
    fn test_select_null_value_in_filter() {
        let mut q = QueryState::new("users");
        q.filters = vec![eq("deleted_at", Value::Null)];
        let (sql, params) = build_select(&q);
        assert!(sql.contains("deleted_at = $1"));
        assert_eq!(params[0], Value::Null);
    }

    #[test]
    fn test_update_no_filters_no_where_clause() {
        let (sql, _) = build_update("settings", &["value".to_string()], &[]);
        assert!(!sql.contains("WHERE"));
        assert_eq!(sql, "UPDATE settings SET value = $1");
    }

    #[test]
    fn test_update_three_fields_param_numbering() {
        let (sql, base) = build_update(
            "users",
            &["a".to_string(), "b".to_string(), "c".to_string()],
            &[eq("id", Value::Integer(1))],
        );
        assert_eq!(sql, "UPDATE users SET a = $1, b = $2, c = $3 WHERE id = $4");
        assert_eq!(base, 4);
    }

    #[test]
    fn test_delete_with_ne_filter() {
        let filters = vec![filter(
            "status",
            FilterOp::Ne,
            Value::String("active".to_string()),
        )];
        let (sql, _) = build_delete("sessions", &filters);
        assert_eq!(sql, "DELETE FROM sessions WHERE status != $1");
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
}
