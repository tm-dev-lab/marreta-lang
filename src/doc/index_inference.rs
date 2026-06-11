//! Static inference of document indexes from the query surface (Spec 067).
//!
//! Walks the parsed program, finds `doc.<collection> >> ...` pipelines, and derives the index
//! each collection needs from the fields its queries filter and sort on, by the ESR rule
//! (Equality, Sort, Range). No declaration, no `doc:` marker, no runtime data: pure static
//! analysis of the code. The result is a per-collection plan the serve startup ensures.

use crate::ast::{
    Argument, BinaryOperator, Expression, MapStatement, PipelineStage, RescueHandler, Statement,
    TaskBody,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// A document index the runtime should ensure: a collection, its ordered keys (field,
/// ascending), and a deterministic Marreta-owned name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferredIndex {
    pub collection: String,
    pub keys: Vec<(String, bool)>,
    pub name: String,
}

/// The ESR categories collected from one query shape. `skip` marks a shape that referenced a
/// non-literal field and must be dropped rather than guessed.
#[derive(Default)]
struct Shape {
    equality: Vec<String>,
    sort: Vec<(String, bool)>,
    range: Vec<String>,
    skip: bool,
}

impl Shape {
    /// Build the composite key list by ESR: equality (canonicalized) + sort (with direction) +
    /// range. Returns `None` when the shape yields no key (e.g. a bare `fetch_all`).
    fn into_composite(mut self) -> Option<Vec<(String, bool)>> {
        if self.skip {
            return None;
        }
        // Equality fields are interchangeable in ESR, so canonicalize to one order (lexicographic)
        // so `where(a) + where(b)` and `where(b) + where(a)` collapse to the same index.
        self.equality.sort();
        self.equality.dedup();
        let mut keys: Vec<(String, bool)> = Vec::new();
        for f in self.equality {
            keys.push((f, true));
        }
        for (f, asc) in self.sort {
            if !keys.iter().any(|(k, _)| k == &f) {
                keys.push((f, asc));
            }
        }
        for f in self.range {
            if !keys.iter().any(|(k, _)| k == &f) {
                keys.push((f, true));
            }
        }
        if keys.is_empty() { None } else { Some(keys) }
    }
}

/// Infer the per-collection document indexes from the program's statements.
pub fn infer_indexes(statements: &[Statement]) -> Vec<InferredIndex> {
    let mut by_collection: BTreeMap<String, Vec<Vec<(String, bool)>>> = BTreeMap::new();
    for stmt in statements {
        walk_statement(stmt, &mut by_collection);
    }

    let mut result = Vec::new();
    for (collection, mut shapes) in by_collection {
        dedup_by_prefix(&mut shapes);
        for keys in shapes {
            let name = index_name(&collection, &keys);
            result.push(InferredIndex {
                collection: collection.clone(),
                keys,
                name,
            });
        }
    }
    result
}

/// Deterministic Marreta-owned index name. All inferred indexes are non-unique (uniqueness is a
/// domain rule, out of scope), so the prefix is always `idx_`. Falls back to a hash suffix when
/// the name would exceed MongoDB's practical limit, keeping it stable and collision-safe.
fn index_name(collection: &str, keys: &[(String, bool)]) -> String {
    let cols: Vec<String> = keys
        .iter()
        .map(|(f, asc)| {
            if *asc {
                f.clone()
            } else {
                format!("{}_desc", f)
            }
        })
        .collect();
    let base = format!("idx_{}_{}", collection, cols.join("_"));
    if base.len() <= 63 {
        return base;
    }
    let key = format!("{}|{}", collection, cols.join(","));
    let digest = Sha256::digest(key.as_bytes());
    let hash: String = digest
        .iter()
        .take(8)
        .map(|b| format!("{:02x}", b))
        .collect();
    let coll_part: String = collection.chars().take(40).collect();
    format!("idx_{}_{}", coll_part, hash)
}

/// Whether an index name belongs to Marreta's owned scheme. Marreta only ever touches indexes
/// that match this, so hand-made indexes are never disturbed.
pub fn is_owned_index_name(name: &str) -> bool {
    name.starts_with("idx_") || name.starts_with("uniq_")
}

/// Drop any key-vector that is a prefix of another for the same collection: `{a}` is redundant
/// when `{a, b}` exists, because the compound index already serves the prefix.
fn dedup_by_prefix(shapes: &mut Vec<Vec<(String, bool)>>) {
    shapes.sort();
    shapes.dedup();
    let all = shapes.clone();
    shapes.retain(|s| {
        !all.iter()
            .any(|other| other.len() > s.len() && other.starts_with(s))
    });
}

// ─── AST walk (total) ──────────────────────────────────────────────────────────
//
// Every container is visited and the matches are exhaustive (no silent `_ => {}`), so a new
// AST variant is a compile error here, not a query shape dropped unnoticed. This is the same
// guard the Spec 068 table-test argument made against a manual enumeration.

type Acc = BTreeMap<String, Vec<Vec<(String, bool)>>>;

fn walk_statements(statements: &[Statement], acc: &mut Acc) {
    for s in statements {
        walk_statement(s, acc);
    }
}

fn walk_statement(stmt: &Statement, acc: &mut Acc) {
    match stmt {
        Statement::Assignment { value, .. } | Statement::ConditionalAssignment { value, .. } => {
            walk_expression(value, acc)
        }
        Statement::ExpressionStatement { expression, .. } => walk_expression(expression, acc),
        Statement::Require { condition, .. } | Statement::Reject { condition, .. } => {
            walk_expression(condition, acc)
        }
        Statement::While {
            condition, body, ..
        } => {
            walk_expression(condition, acc);
            walk_statements(body, acc);
        }
        Statement::TaskDef { body, .. } => walk_task_body(body, acc),
        Statement::Route { body, allow, .. } => {
            for e in allow {
                walk_expression(e, acc);
            }
            walk_statements(body, acc);
        }
        Statement::Reply {
            status_code, body, ..
        } => {
            walk_expression(status_code, acc);
            walk_expression(body, acc);
        }
        Statement::Fail { message, .. } => walk_expression(message, acc),
        Statement::Raise {
            message, condition, ..
        } => {
            walk_expression(message, acc);
            if let Some(c) = condition {
                walk_expression(c, acc);
            }
        }
        Statement::Nack { condition, .. } => {
            if let Some(c) = condition {
                walk_expression(c, acc);
            }
        }
        Statement::Export(inner) => walk_statement(inner, acc),
        Statement::Transaction { body, .. } => walk_statements(body, acc),
        Statement::OnQueue {
            queue_name, body, ..
        } => {
            walk_expression(queue_name, acc);
            walk_statements(body, acc);
        }
        Statement::OnTopic { pattern, body, .. } => {
            walk_expression(pattern, acc);
            walk_statements(body, acc);
        }
        // Leaves carry no doc query. Scenarios are the test DSL (they run under the scenario
        // runner, never serve traffic), so they are a deliberate exclusion, not a silent skip.
        Statement::Schema { .. } | Statement::AuthProvider { .. } | Statement::Scenario { .. } => {}
    }
}

fn walk_task_body(body: &TaskBody, acc: &mut Acc) {
    match body {
        TaskBody::Inline(expr) => walk_expression(expr, acc),
        TaskBody::Block(statements, final_expression) => {
            walk_statements(statements, acc);
            walk_expression(final_expression, acc);
        }
    }
}

fn walk_expression(expr: &Expression, acc: &mut Acc) {
    match expr {
        Expression::Integer(_)
        | Expression::Float(_)
        | Expression::StringLiteral(_)
        | Expression::Boolean(_)
        | Expression::Null
        | Expression::Identifier(_)
        | Expression::TaskCall { .. } => {}
        Expression::List(items) => {
            for e in items {
                walk_expression(e, acc);
            }
        }
        Expression::MapLiteral(entries)
        | Expression::SchemaConstructor {
            fields: entries, ..
        } => {
            for (_, v) in entries {
                walk_expression(v, acc);
            }
        }
        Expression::BinaryOp { left, right, .. } => {
            walk_expression(left, acc);
            walk_expression(right, acc);
        }
        Expression::UnaryOp { operand, .. } => walk_expression(operand, acc),
        Expression::PropertyAccess { object, .. } => walk_expression(object, acc),
        Expression::MethodCall {
            object, arguments, ..
        } => {
            walk_expression(object, acc);
            walk_arguments(arguments, acc);
        }
        Expression::HttpClientResponseSchema { call, .. } => walk_expression(call, acc),
        Expression::FunctionCall { arguments, .. } => walk_arguments(arguments, acc),
        Expression::Match { subject, arms } => {
            walk_expression(subject, acc);
            for arm in arms {
                walk_expression(&arm.value, acc);
            }
        }
        Expression::If {
            condition,
            then_branch,
            else_branch,
        } => {
            walk_expression(condition, acc);
            walk_task_body(then_branch, acc);
            if let Some(e) = else_branch {
                walk_task_body(e, acc);
            }
        }
        Expression::Subscript { object, key } => {
            walk_expression(object, acc);
            walk_expression(key, acc);
        }
        Expression::Pipeline { input, stages } => {
            // The shape lives on a pipeline whose input is a doc collection.
            if let Some(collection) = doc_collection_of(input) {
                let mut shape = Shape::default();
                for stage in stages {
                    if let PipelineStage::Expression(Expression::FunctionCall { name, arguments }) =
                        stage
                    {
                        classify_stage(name, arguments, &mut shape);
                    }
                }
                if let Some(keys) = shape.into_composite() {
                    acc.entry(collection).or_default().push(keys);
                }
            }
            walk_expression(input, acc);
            for stage in stages {
                walk_pipeline_stage(stage, acc);
            }
        }
        Expression::Broadcast { input, targets } => {
            walk_expression(input, acc);
            for t in targets {
                walk_expression(t, acc);
            }
        }
        Expression::Rescue { expr, handler } => {
            walk_expression(expr, acc);
            walk_expression(handler, acc);
        }
        Expression::QueuePush {
            queue_name,
            payload,
            ..
        } => {
            walk_expression(queue_name, acc);
            if let Some(p) = payload {
                walk_expression(p, acc);
            }
        }
        Expression::TopicPublish { topic, payload, .. } => {
            walk_expression(topic, acc);
            if let Some(p) = payload {
                walk_expression(p, acc);
            }
        }
    }
}

fn walk_pipeline_stage(stage: &PipelineStage, acc: &mut Acc) {
    match stage {
        PipelineStage::Expression(e) => walk_expression(e, acc),
        PipelineStage::Map { body, .. } => {
            for ms in body {
                match ms {
                    MapStatement::Statement(s) => walk_statement(s, acc),
                    MapStatement::Keep { value, condition } => {
                        walk_expression(value, acc);
                        if let Some(c) = condition {
                            walk_expression(c, acc);
                        }
                    }
                    MapStatement::Skip { condition } => walk_expression(condition, acc),
                }
            }
        }
        PipelineStage::Reduce { initial, body, .. } => {
            walk_expression(initial, acc);
            walk_task_body(body, acc);
        }
        PipelineStage::Rescue { handler } => match handler {
            RescueHandler::Inline(e) => walk_expression(e, acc),
            RescueHandler::Block(stmts) => walk_statements(stmts, acc),
        },
    }
}

fn walk_arguments(arguments: &[Argument], acc: &mut Acc) {
    for arg in arguments {
        match arg {
            Argument::Positional(e) | Argument::Named { value: e, .. } => walk_expression(e, acc),
        }
    }
}

/// The collection a pipeline input addresses, for both documented forms: the `doc.<collection>`
/// shorthand and the explicit `doc.query("collection")` (SPEC 010 Layer 2). `doc.pipeline(...)`
/// is a method call too, but only `query` matches, so the raw escape hatch is excluded by
/// construction.
fn doc_collection_of(expr: &Expression) -> Option<String> {
    match expr {
        Expression::PropertyAccess { object, property } => match object.as_ref() {
            Expression::Identifier(name) if name == "doc" => Some(property.clone()),
            _ => None,
        },
        Expression::MethodCall {
            object,
            method,
            arguments,
        } => {
            if let Expression::Identifier(name) = object.as_ref() {
                if name == "doc" && method == "query" {
                    if let Some(Argument::Positional(Expression::StringLiteral(col))) =
                        arguments.first()
                    {
                        return Some(col.clone());
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Fold one pipeline stage into the shape's ESR categories.
fn classify_stage(name: &str, arguments: &[Argument], shape: &mut Shape) {
    match name {
        "where" => {
            // `where("field" == x)` / `where("field" > x)`, and the multi-filter form
            // `where("a" == x, "b" > y)`: the runtime (`parse_doc_where_args`) iterates every
            // argument, so we must too, or a trailing filter vanishes from the shape. The field is
            // always a string literal (the DSL rejects the named-argument form and a left-hand
            // identifier at parse time), so coverage is total by construction.
            for arg in arguments {
                if let Argument::Positional(Expression::BinaryOp { left, operator, .. }) = arg {
                    match literal_str(left) {
                        Some(field) => match operator {
                            BinaryOperator::Equal => shape.equality.push(field),
                            BinaryOperator::Greater
                            | BinaryOperator::Less
                            | BinaryOperator::GreaterEqual
                            | BinaryOperator::LessEqual => shape.range.push(field),
                            _ => {}
                        },
                        None => shape.skip = true,
                    }
                }
            }
        }
        "order" => {
            // `order("field", "asc"|"desc")`. Direction is part of the index identity in a
            // composite, so a non-literal direction is dropped (skip), never guessed as asc.
            if let Some(Argument::Positional(e)) = arguments.first() {
                match literal_str(e) {
                    None => shape.skip = true,
                    Some(field) => {
                        let asc = match arguments.get(1) {
                            None => Some(true),
                            Some(Argument::Positional(d)) => match literal_str(d).as_deref() {
                                Some("desc") => Some(false),
                                Some(_) => Some(true),
                                None => None,
                            },
                            Some(_) => Some(true),
                        };
                        match asc {
                            Some(a) => shape.sort.push((field, a)),
                            None => shape.skip = true,
                        }
                    }
                }
            }
        }
        "in" => {
            // `in("field", [...])`: $in is equality alone but a range with a sort, so the
            // conservative classification is Range (after the Sort segment).
            if let Some(Argument::Positional(e)) = arguments.first() {
                match literal_str(e) {
                    Some(field) => shape.range.push(field),
                    None => shape.skip = true,
                }
            }
        }
        // `like` (regex, no usable plain index) and any other step are deliberately ignored.
        _ => {}
    }
}

fn literal_str(expr: &Expression) -> Option<String> {
    match expr {
        Expression::StringLiteral(s) => Some(s.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::HttpVerb;

    // Build the Spec 066 route body: doc.transactions >> where("account_id" == params.id)
    //   >> order("_id", "desc") >> limit(20) >> fetch_all.
    fn bench_066_statements() -> Vec<Statement> {
        let pipeline = Expression::Pipeline {
            input: Box::new(Expression::PropertyAccess {
                object: Box::new(Expression::Identifier("doc".into())),
                property: "transactions".into(),
            }),
            stages: vec![
                PipelineStage::Expression(Expression::FunctionCall {
                    name: "where".into(),
                    arguments: vec![Argument::Positional(Expression::BinaryOp {
                        left: Box::new(Expression::StringLiteral("account_id".into())),
                        operator: BinaryOperator::Equal,
                        right: Box::new(Expression::Identifier("x".into())),
                    })],
                }),
                PipelineStage::Expression(Expression::FunctionCall {
                    name: "order".into(),
                    arguments: vec![
                        Argument::Positional(Expression::StringLiteral("_id".into())),
                        Argument::Positional(Expression::StringLiteral("desc".into())),
                    ],
                }),
                PipelineStage::Expression(Expression::FunctionCall {
                    name: "fetch_all".into(),
                    arguments: vec![],
                }),
            ],
        };
        vec![Statement::Route {
            verb: HttpVerb::Get,
            path: "/accounts/:id/transactions".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![Statement::Assignment {
                target: "rows".into(),
                value: pipeline,
                line: 1,
                column: 1,
            }],
            line: 1,
            column: 1,
        }]
    }

    #[test]
    fn test_infers_esr_composite_for_bench_066() {
        let indexes = infer_indexes(&bench_066_statements());
        assert_eq!(indexes.len(), 1);
        let idx = &indexes[0];
        assert_eq!(idx.collection, "transactions");
        assert_eq!(
            idx.keys,
            vec![("account_id".to_string(), true), ("_id".to_string(), false)]
        );
        assert_eq!(idx.name, "idx_transactions_account_id__id_desc");
        assert!(is_owned_index_name(&idx.name));
    }

    #[test]
    fn test_equality_is_order_insensitive_and_dedups() {
        // where(a)+where(b) in one query, where(b)+where(a) in another -> one index {a,b}.
        let mk = |f1: &str, f2: &str| Statement::Assignment {
            target: "r".into(),
            value: Expression::Pipeline {
                input: Box::new(Expression::PropertyAccess {
                    object: Box::new(Expression::Identifier("doc".into())),
                    property: "items".into(),
                }),
                stages: vec![
                    where_eq(f1),
                    where_eq(f2),
                    PipelineStage::Expression(Expression::FunctionCall {
                        name: "fetch_all".into(),
                        arguments: vec![],
                    }),
                ],
            },
            line: 1,
            column: 1,
        };
        let indexes = infer_indexes(&[mk("a", "b"), mk("b", "a")]);
        assert_eq!(indexes.len(), 1);
        assert_eq!(
            indexes[0].keys,
            vec![("a".to_string(), true), ("b".to_string(), true)]
        );
    }

    #[test]
    fn test_prefix_dedup_drops_redundant_single_field() {
        // {account_id} is redundant once {account_id, _id} exists.
        let single = Statement::Assignment {
            target: "r".into(),
            value: Expression::Pipeline {
                input: Box::new(Expression::PropertyAccess {
                    object: Box::new(Expression::Identifier("doc".into())),
                    property: "transactions".into(),
                }),
                stages: vec![
                    where_eq("account_id"),
                    PipelineStage::Expression(Expression::FunctionCall {
                        name: "fetch_all".into(),
                        arguments: vec![],
                    }),
                ],
            },
            line: 1,
            column: 1,
        };
        let mut stmts = bench_066_statements();
        stmts.push(single);
        let indexes = infer_indexes(&stmts);
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0].keys.len(), 2);
    }

    #[test]
    fn test_like_and_dynamic_field_are_excluded() {
        // like(...) is ignored; a non-literal where field skips the shape.
        let like_only = doc_pipeline(
            "a",
            vec![PipelineStage::Expression(Expression::FunctionCall {
                name: "like".into(),
                arguments: vec![
                    Argument::Positional(Expression::StringLiteral("name".into())),
                    Argument::Positional(Expression::StringLiteral("x%".into())),
                ],
            })],
        );
        let dynamic = doc_pipeline(
            "b",
            vec![PipelineStage::Expression(Expression::FunctionCall {
                name: "where".into(),
                arguments: vec![Argument::Positional(Expression::BinaryOp {
                    left: Box::new(Expression::Identifier("dynamic".into())),
                    operator: BinaryOperator::Equal,
                    right: Box::new(Expression::Identifier("x".into())),
                })],
            })],
        );
        assert!(infer_indexes(&[like_only, dynamic]).is_empty());
    }

    #[test]
    fn test_doc_query_method_form_is_inferred() {
        // doc.query("orders") >> where("status" == "pending"): the documented Layer 2 form.
        let stmt = assign(Expression::Pipeline {
            input: Box::new(Expression::MethodCall {
                object: Box::new(Expression::Identifier("doc".into())),
                method: "query".into(),
                arguments: vec![Argument::Positional(Expression::StringLiteral(
                    "orders".into(),
                ))],
            }),
            stages: vec![where_eq("status"), fetch_all()],
        });
        let idx = infer_indexes(&[stmt]);
        assert_eq!(idx.len(), 1);
        assert_eq!(idx[0].collection, "orders");
        assert_eq!(idx[0].keys, vec![("status".to_string(), true)]);
    }

    #[test]
    fn test_doc_pipeline_escape_hatch_is_not_indexed() {
        // doc.pipeline(...) is a method call, never a `>>` input, so it yields no shape. Asserted
        // so the exclusion is an invariant, not a coincidence.
        let stmt = assign(Expression::MethodCall {
            object: Box::new(Expression::Identifier("doc".into())),
            method: "pipeline".into(),
            arguments: vec![
                Argument::Positional(Expression::StringLiteral("orders".into())),
                Argument::Positional(Expression::List(vec![])),
            ],
        });
        assert!(infer_indexes(&[stmt]).is_empty());
    }

    #[test]
    fn test_walker_reaches_every_container() {
        // A doc query planted inside each container must still be inferred. The exhaustive match
        // catches a new Statement/Expression *variant* at compile time, but NOT a new *field*
        // carrying statements (we elide the rest with `..`). So this test is the real per-field
        // guardian: when a new statement-bearing field is added to any AST node, the reflex must
        // be to plant a doc query in it here.
        let q = |c: &str| assign(doc_query_value(c));
        let stmts = vec![
            // on queue consumer (where heavy event-driven processing lives)
            Statement::OnQueue {
                queue_name: Expression::StringLiteral("orders.processing".into()),
                binding: "msg".into(),
                schema: None,
                body: vec![q("c_queue")],
                line: 1,
                column: 1,
            },
            // transaction block
            Statement::Transaction {
                body: vec![q("c_txn")],
                line: 1,
                column: 1,
            },
            // if-branch (if is an expression)
            assign(Expression::If {
                condition: Box::new(Expression::Boolean(true)),
                then_branch: Box::new(TaskBody::Inline(doc_query_value("c_if"))),
                else_branch: None,
            }),
            // broadcast target
            assign(Expression::Broadcast {
                input: Box::new(Expression::Integer(1)),
                targets: vec![doc_query_value("c_bcast")],
            }),
            // map body inside a pipeline
            assign(Expression::Pipeline {
                input: Box::new(Expression::Identifier("items".into())),
                stages: vec![PipelineStage::Map {
                    variable: "it".into(),
                    body: vec![MapStatement::Statement(q("c_map"))],
                }],
            }),
        ];
        let found: std::collections::BTreeSet<String> = infer_indexes(&stmts)
            .into_iter()
            .map(|i| i.collection)
            .collect();
        for c in ["c_queue", "c_txn", "c_if", "c_bcast", "c_map"] {
            assert!(found.contains(c), "container dropped the doc query for {c}");
        }
    }

    #[test]
    fn test_where_multi_argument_captures_all_filters() {
        // where("status" == "paid", "total" > 100): the runtime iterates every argument, so both
        // filters must reach the shape -> status (Equality) + total (Range) = {status:1, total:1}.
        let stmt = assign(Expression::Pipeline {
            input: Box::new(Expression::PropertyAccess {
                object: Box::new(Expression::Identifier("doc".into())),
                property: "orders".into(),
            }),
            stages: vec![
                PipelineStage::Expression(Expression::FunctionCall {
                    name: "where".into(),
                    arguments: vec![
                        Argument::Positional(Expression::BinaryOp {
                            left: Box::new(Expression::StringLiteral("status".into())),
                            operator: BinaryOperator::Equal,
                            right: Box::new(Expression::StringLiteral("paid".into())),
                        }),
                        Argument::Positional(Expression::BinaryOp {
                            left: Box::new(Expression::StringLiteral("total".into())),
                            operator: BinaryOperator::Greater,
                            right: Box::new(Expression::Integer(100)),
                        }),
                    ],
                }),
                fetch_all(),
            ],
        });
        let idx = infer_indexes(&[stmt]);
        assert_eq!(idx.len(), 1);
        assert_eq!(
            idx[0].keys,
            vec![("status".to_string(), true), ("total".to_string(), true)]
        );
    }

    #[test]
    fn test_non_literal_order_direction_skips_shape() {
        // order("_id", dir) with a variable direction: the direction is unknown, so the whole
        // shape is dropped rather than guessed as ascending (direction is index identity).
        let stmt = assign(Expression::Pipeline {
            input: Box::new(Expression::PropertyAccess {
                object: Box::new(Expression::Identifier("doc".into())),
                property: "t".into(),
            }),
            stages: vec![
                where_eq("account_id"),
                PipelineStage::Expression(Expression::FunctionCall {
                    name: "order".into(),
                    arguments: vec![
                        Argument::Positional(Expression::StringLiteral("_id".into())),
                        Argument::Positional(Expression::Identifier("dir".into())),
                    ],
                }),
                fetch_all(),
            ],
        });
        assert!(infer_indexes(&[stmt]).is_empty());
    }

    fn assign(value: Expression) -> Statement {
        Statement::Assignment {
            target: "r".into(),
            value,
            line: 1,
            column: 1,
        }
    }

    fn fetch_all() -> PipelineStage {
        PipelineStage::Expression(Expression::FunctionCall {
            name: "fetch_all".into(),
            arguments: vec![],
        })
    }

    fn doc_query_value(collection: &str) -> Expression {
        Expression::Pipeline {
            input: Box::new(Expression::PropertyAccess {
                object: Box::new(Expression::Identifier("doc".into())),
                property: collection.into(),
            }),
            stages: vec![where_eq("ref"), fetch_all()],
        }
    }

    fn where_eq(field: &str) -> PipelineStage {
        PipelineStage::Expression(Expression::FunctionCall {
            name: "where".into(),
            arguments: vec![Argument::Positional(Expression::BinaryOp {
                left: Box::new(Expression::StringLiteral(field.into())),
                operator: BinaryOperator::Equal,
                right: Box::new(Expression::Identifier("x".into())),
            })],
        })
    }

    fn doc_pipeline(collection: &str, stages: Vec<PipelineStage>) -> Statement {
        Statement::Assignment {
            target: "r".into(),
            value: Expression::Pipeline {
                input: Box::new(Expression::PropertyAccess {
                    object: Box::new(Expression::Identifier("doc".into())),
                    property: collection.into(),
                }),
                stages,
            },
            line: 1,
            column: 1,
        }
    }
}
