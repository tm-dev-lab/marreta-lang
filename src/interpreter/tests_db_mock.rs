use super::mock_db::{interp_with_mock, seed_row};
use super::*;
use crate::ast::SchemaField;
use crate::lexer::Lexer;
use crate::parser::Parser;
use std::sync::{Arc, RwLock};

fn run_with_mock(src: &str) -> Value {
    let mut interp = interp_with_mock(seed_row());
    let tokens = Lexer::new(src).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    interp.execute(&program).unwrap()
}

fn run_err_with_mock(src: &str) -> MarretaError {
    let mut interp = interp_with_mock(seed_row());
    let tokens = Lexer::new(src).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    interp.execute(&program).unwrap_err()
}

fn make_db_schema(
    table: &str,
    fields: &[(&str, crate::ast::SchemaType, bool)],
) -> SchemaDefinition {
    SchemaDefinition {
        db_table: Some(table.to_string()),
        fields: fields
            .iter()
            .map(|(name, t, optional)| SchemaField {
                name: name.to_string(),
                field_type: t.clone(),
                optional: *optional,
            })
            .collect(),
    }
}

// ── db.TABLE.save ─────────────────────────────────────────────────────────

#[test]
fn test_db_save_returns_map() {
    let result = run_with_mock("db.users.save({ name: \"Alice\" })");
    assert!(matches!(result, Value::Map(_)));
}

#[test]
fn test_db_save_no_arg_error() {
    let err = run_err_with_mock("db.users.save()");
    assert!(matches!(err, MarretaError::TypeError { message, .. } if message.contains("save")));
}

#[test]
fn test_db_save_non_map_arg_error() {
    let err = run_err_with_mock("db.users.save(42)");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

// ── db.TABLE.find ─────────────────────────────────────────────────────────

#[test]
fn test_db_find_returns_map() {
    let result = run_with_mock("db.users.find(1)");
    assert!(matches!(result, Value::Map(_)));
}

#[test]
fn test_db_save_rejects_user_supplied_id_for_db_schema() {
    let mut schemas = HashMap::new();
    schemas.insert(
        "User".into(),
        make_db_schema(
            "users",
            &[
                ("id", crate::ast::SchemaType::IntegerType, false),
                ("name", crate::ast::SchemaType::StringType, false),
            ],
        ),
    );

    let driver = super::mock_db::interp_with_mock(seed_row());
    let mut interp = driver.with_schemas(Arc::new(schemas));
    let tokens = Lexer::new("db.users.save({ id: 1, name: \"Alice\" })")
        .tokenize()
        .unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    let err = interp.execute(&program).unwrap_err();
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("must not include `id`"))
    );
}

#[test]
fn test_db_find_no_arg_error() {
    let err = run_err_with_mock("db.users.find()");
    assert!(matches!(err, MarretaError::TypeError { message, .. } if message.contains("find")));
}

// ── db.TABLE.find_all ─────────────────────────────────────────────────────

#[test]
fn test_db_find_all_returns_list() {
    let result = run_with_mock("db.users.find_all()");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_db_find_all_with_filter_returns_list() {
    let result = run_with_mock("db.users.find_all(name: \"Alice\")");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_db_find_all_positional_arg_error() {
    let err = run_err_with_mock("db.users.find_all(1)");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

// ── db.TABLE.update ───────────────────────────────────────────────────────

#[test]
fn test_db_update_returns_map() {
    let result = run_with_mock("db.users.update(1, { name: \"Bob\" })");
    assert!(matches!(result, Value::Map(_)));
}

#[test]
fn test_db_update_missing_row_returns_null() {
    struct MissingUpdateDriver;

    #[async_trait::async_trait]
    impl crate::db::driver::DbDriver for MissingUpdateDriver {
        async fn save(
            &self,
            _t: &str,
            _d: crate::db::driver::DbRow,
        ) -> crate::db::driver::DbResult<crate::db::driver::DbRow> {
            Ok(HashMap::new())
        }
        async fn find(
            &self,
            _t: &str,
            _id: &Value,
        ) -> crate::db::driver::DbResult<Option<crate::db::driver::DbRow>> {
            Ok(None)
        }
        async fn find_all(
            &self,
            _t: &str,
            _f: Vec<FilterClause>,
        ) -> crate::db::driver::DbResult<Vec<crate::db::driver::DbRow>> {
            Ok(Vec::new())
        }
        async fn update_by_id(
            &self,
            _t: &str,
            _id: &Value,
            _d: crate::db::driver::DbRow,
        ) -> crate::db::driver::DbResult<Option<crate::db::driver::DbRow>> {
            Ok(None)
        }
        async fn delete_by_id(&self, _t: &str, _id: &Value) -> crate::db::driver::DbResult<bool> {
            Ok(false)
        }
        async fn query_fetch(
            &self,
            _q: &QueryState,
        ) -> crate::db::driver::DbResult<Vec<crate::db::driver::DbRow>> {
            Ok(Vec::new())
        }
        async fn query_fetch_one(
            &self,
            _q: &QueryState,
        ) -> crate::db::driver::DbResult<Option<crate::db::driver::DbRow>> {
            Ok(None)
        }
        async fn query_count(&self, _q: &QueryState) -> crate::db::driver::DbResult<i64> {
            Ok(0)
        }
        async fn query_exists(&self, _q: &QueryState) -> crate::db::driver::DbResult<bool> {
            Ok(false)
        }
        async fn query_update(
            &self,
            _q: &QueryState,
            _d: crate::db::driver::DbRow,
        ) -> crate::db::driver::DbResult<u64> {
            Ok(0)
        }
        async fn query_delete(&self, _q: &QueryState) -> crate::db::driver::DbResult<u64> {
            Ok(0)
        }
        async fn native_query(
            &self,
            _sql: &str,
            _params: Vec<Value>,
        ) -> crate::db::driver::DbResult<Vec<crate::db::driver::DbRow>> {
            Ok(Vec::new())
        }
        async fn begin(&self) -> crate::db::driver::DbResult<Box<dyn DbTx>> {
            unimplemented!("not needed for this test")
        }
    }

    let mut interp = Interpreter::new().with_db(DbEngine {
        driver: Arc::new(MissingUpdateDriver),
        provider: crate::db::DbProvider::Postgres,
    });
    let tokens = Lexer::new("db.users.update(1, { name: \"Bob\" })")
        .tokenize()
        .unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    let result = interp.execute(&program).unwrap();
    assert_eq!(result, Value::Null);
}

#[test]
fn test_db_update_no_id_error() {
    let err = run_err_with_mock("db.users.update()");
    assert!(matches!(err, MarretaError::TypeError { message, .. } if message.contains("update")));
}

#[test]
fn test_db_update_no_data_error() {
    let err = run_err_with_mock("db.users.update(1)");
    assert!(matches!(err, MarretaError::TypeError { message, .. } if message.contains("update")));
}

// ── db.TABLE.delete ───────────────────────────────────────────────────────

#[test]
fn test_db_delete_returns_boolean() {
    let result = run_with_mock("db.users.delete(1)");
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn test_db_delete_no_arg_error() {
    let err = run_err_with_mock("db.users.delete()");
    assert!(matches!(err, MarretaError::TypeError { message, .. } if message.contains("delete")));
}

// ── unknown direct operation ───────────────────────────────────────────────

#[test]
fn test_db_unknown_operation_error() {
    let err = run_err_with_mock("db.users.frobnicate(1)");
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("frobnicate"))
    );
}

// ── Pipeline terminals: fetch, fetch_one, count, exists, delete ───────────

#[test]
fn test_pipeline_fetch_returns_list() {
    let result = run_with_mock("db.users >> fetch");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_pipeline_fetch_one_returns_map() {
    let result = run_with_mock("db.users >> fetch_one");
    // fetch_one returns Some(row) → Map, or Null if None
    assert!(matches!(result, Value::Map(_) | Value::Null));
}

#[test]
fn test_pipeline_count_returns_integer() {
    let result = run_with_mock("db.users >> count");
    assert_eq!(result, Value::Integer(42));
}

#[test]
fn test_pipeline_exists_returns_boolean() {
    let result = run_with_mock("db.users >> exists");
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn test_pipeline_delete_returns_integer() {
    let result = run_with_mock("db.users >> delete");
    assert_eq!(result, Value::Integer(2));
}

#[test]
fn test_pipeline_update_returns_integer() {
    let result = run_with_mock("db.users >> update({ active: false })");
    assert_eq!(result, Value::Integer(3));
}

#[test]
fn test_pipeline_update_no_arg_error() {
    let err = run_err_with_mock("db.users >> update()");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_pipeline_unknown_terminal_error() {
    let err = run_err_with_mock("db.users >> nonexistent");
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("nonexistent"))
    );
}

#[test]
fn test_pipeline_unknown_step_error() {
    let err = run_err_with_mock("db.users >> unknown_step()");
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("unknown_step"))
    );
}

// ── Pipeline accumulating steps with engine ───────────────────────────────

#[test]
fn test_pipeline_where_then_fetch() {
    let result = run_with_mock("db.users >> where(active: true) >> fetch");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_pipeline_like_then_count() {
    let result = run_with_mock("db.users >> like(\"name\", \"Al%\") >> count");
    assert_eq!(result, Value::Integer(42));
}

#[test]
fn test_pipeline_like_no_col_error() {
    let err = run_err_with_mock("db.users >> like(42, \"Al%\") >> count");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_pipeline_like_no_pattern_error() {
    let err = run_err_with_mock("db.users >> like(\"name\") >> count");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_pipeline_in_then_fetch() {
    let result = run_with_mock("db.users >> in(\"role\", [\"admin\", \"mod\"]) >> fetch");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_pipeline_in_no_col_error() {
    let err = run_err_with_mock("db.users >> in(42, [\"x\"]) >> fetch");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_pipeline_in_non_list_error() {
    let err = run_err_with_mock("db.users >> in(\"role\", \"admin\") >> fetch");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_pipeline_in_missing_list_error() {
    let err = run_err_with_mock("db.users >> in(\"role\") >> fetch");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_pipeline_order_by_then_fetch() {
    let result = run_with_mock("db.users >> order_by(\"name asc\") >> fetch");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_pipeline_order_by_non_string_error() {
    let err = run_err_with_mock("db.users >> order_by(42) >> fetch");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_pipeline_limit_then_fetch() {
    let result = run_with_mock("db.users >> limit(10) >> fetch");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_pipeline_limit_non_integer_error() {
    let err = run_err_with_mock("db.users >> limit(\"ten\") >> fetch");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_pipeline_offset_then_fetch() {
    let result = run_with_mock("db.users >> offset(20) >> fetch");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_pipeline_offset_non_integer_error() {
    let err = run_err_with_mock("db.users >> offset(\"a\") >> fetch");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_pipeline_join_then_fetch() {
    let result = run_with_mock("db.users >> join(\"orders\", on: \"user_id\") >> fetch");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_pipeline_left_join_then_fetch() {
    let result = run_with_mock("db.users >> left_join(\"orders\", on: \"user_id\") >> fetch");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_pipeline_join_missing_on_error() {
    let err = run_err_with_mock("db.users >> join(\"orders\") >> fetch");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_pipeline_join_non_string_table_error() {
    let err = run_err_with_mock("db.users >> join(42, on: \"user_id\") >> fetch");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_pipeline_select_then_fetch() {
    let result = run_with_mock("db.users >> select(\"id\", \"name\") >> fetch");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_pipeline_select_non_string_error() {
    let err = run_err_with_mock("db.users >> select(42) >> fetch");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

// ── db.native_query ────────────────────────────────────────────────────────

#[test]
fn test_native_query_returns_list() {
    let result = run_with_mock("db.native_query(\"SELECT 1\")");
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_native_query_with_hash_params() {
    let result = run_with_mock(
        "user_id = 42\ndb.native_query(\"SELECT * FROM users WHERE id = #{user_id}\")",
    );
    assert!(matches!(result, Value::List(_)));
}

#[test]
fn test_native_query_no_args_error() {
    let err = run_err_with_mock("db.native_query()");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_native_query_non_string_first_arg_error() {
    let err = run_err_with_mock("db.native_query(42)");
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

// ── Transaction ───────────────────────────────────────────────────────────

#[test]
fn test_transaction_commits_on_success() {
    // If the transaction body completes without error, commit is called
    let result = run_with_mock("transaction\n    x = db.users.save({ name: \"Alice\" })\nx");
    assert!(matches!(result, Value::Map(_)));
}

#[test]
fn test_transaction_rollback_on_error() {
    // If the body raises, rollback is called and the error propagates
    let err = run_err_with_mock("transaction\n    raise \"boom\"");
    assert!(matches!(err, MarretaError::RaiseError { message } if message == "boom"));
}

// ── QueryBuilder pipeline expressions that are not FunctionCall/Identifier ─

#[test]
fn test_pipeline_querybuilder_non_fn_expr_error() {
    // An expression that's neither FunctionCall nor Identifier after a QueryBuilder
    // should produce a descriptive TypeError
    use crate::ast::*;
    let mut interp = interp_with_mock(seed_row());
    let expr2 = Expression::Pipeline {
        input: Box::new(Expression::PropertyAccess {
            object: Box::new(Expression::Identifier("db".into())),
            property: "users".into(),
        }),
        stages: vec![PipelineStage::Expression(Expression::Integer(99))],
    };
    let err = interp.evaluate(&expr2).unwrap_err();
    assert!(matches!(err, MarretaError::TypeError { .. }));
}

#[test]
fn test_relational_record_exposes_fk_and_singular_relation_handle() {
    let mut schemas = HashMap::new();
    schemas.insert(
        "User".into(),
        make_db_schema(
            "users",
            &[("id", crate::ast::SchemaType::IntegerType, false)],
        ),
    );
    schemas.insert(
        "Order".into(),
        make_db_schema(
            "orders",
            &[
                ("id", crate::ast::SchemaType::IntegerType, false),
                (
                    "customer",
                    crate::ast::SchemaType::Reference("User".into()),
                    false,
                ),
            ],
        ),
    );

    let mut interp = interp_with_mock({
        let mut row = HashMap::new();
        row.insert("id".into(), Value::Integer(9));
        row.insert("name".into(), Value::String("Alice".into()));
        row
    })
    .with_schemas(Arc::new(schemas));

    let record = Value::RelationalRecord {
        schema_name: "Order".into(),
        fields: Arc::new(RwLock::new(ValueMap::from_iter([
            ("id".into(), Value::Integer(1)),
            ("customer_id".into(), Value::Integer(9)),
        ]))),
    };

    assert_eq!(
        interp.access_property(&record, "customer_id").unwrap(),
        Value::Integer(9)
    );

    let handle = interp.access_property(&record, "customer").unwrap();
    assert!(matches!(handle, Value::RelationHandle(_)));

    let fetched = interp
        .evaluate_pipeline_stage(
            &handle,
            &PipelineStage::Expression(Expression::Identifier("fetch".into())),
        )
        .unwrap();
    assert_eq!(
        interp.access_property(&fetched, "id").unwrap(),
        Value::Integer(9)
    );
}

#[test]
fn test_singular_relation_rejects_count() {
    let mut schemas = HashMap::new();
    schemas.insert(
        "User".into(),
        make_db_schema(
            "users",
            &[("id", crate::ast::SchemaType::IntegerType, false)],
        ),
    );
    schemas.insert(
        "Order".into(),
        make_db_schema(
            "orders",
            &[
                ("id", crate::ast::SchemaType::IntegerType, false),
                (
                    "customer",
                    crate::ast::SchemaType::Reference("User".into()),
                    false,
                ),
            ],
        ),
    );

    let interp = interp_with_mock(seed_row()).with_schemas(Arc::new(schemas));
    let record = Value::RelationalRecord {
        schema_name: "Order".into(),
        fields: Arc::new(RwLock::new(ValueMap::from_iter([
            ("id".into(), Value::Integer(1)),
            ("customer_id".into(), Value::Integer(9)),
        ]))),
    };

    let handle = interp.access_property(&record, "customer").unwrap();
    let mut interp = interp;
    let err = interp
        .evaluate_pipeline_stage(
            &handle,
            &PipelineStage::Expression(Expression::Identifier("count".into())),
        )
        .unwrap_err();
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("singular relation"))
    );
}

#[test]
fn test_singular_relation_rejects_where() {
    let mut schemas = HashMap::new();
    schemas.insert(
        "User".into(),
        make_db_schema(
            "users",
            &[("id", crate::ast::SchemaType::IntegerType, false)],
        ),
    );
    schemas.insert(
        "Order".into(),
        make_db_schema(
            "orders",
            &[
                ("id", crate::ast::SchemaType::IntegerType, false),
                (
                    "customer",
                    crate::ast::SchemaType::Reference("User".into()),
                    false,
                ),
            ],
        ),
    );

    let interp = interp_with_mock(seed_row()).with_schemas(Arc::new(schemas));
    let record = Value::RelationalRecord {
        schema_name: "Order".into(),
        fields: Arc::new(RwLock::new(ValueMap::from_iter([
            ("id".into(), Value::Integer(1)),
            ("customer_id".into(), Value::Integer(9)),
        ]))),
    };

    let handle = interp.access_property(&record, "customer").unwrap();
    let mut interp = interp;
    let err = interp
        .evaluate_pipeline_stage(
            &handle,
            &PipelineStage::Expression(Expression::MethodCall {
                object: Box::new(Expression::Identifier("input".into())),
                method: "where".into(),
                arguments: vec![crate::ast::Argument::Positional(Expression::MapLiteral(
                    vec![("name".into(), Expression::StringLiteral("Alice".into()))],
                ))],
            }),
        )
        .unwrap_err();
    assert!(
        matches!(err, MarretaError::TypeError { message, .. } if message.contains("singular relation"))
    );
}

#[test]
fn test_non_relation_fetch_fails_clearly() {
    let mut interp = interp_with_mock(seed_row());
    let err = interp
        .evaluate_pipeline_stage(
            &Value::Integer(42),
            &PipelineStage::Expression(Expression::Identifier("fetch".into())),
        )
        .unwrap_err();
    assert!(matches!(
        err,
        MarretaError::TypeError { message, .. }
        if message.contains("not a relation or query")
    ));
}

#[test]
fn test_inverse_collection_fetch_returns_relational_records() {
    let mut schemas = HashMap::new();
    schemas.insert(
        "User".into(),
        make_db_schema(
            "users",
            &[
                ("id", crate::ast::SchemaType::IntegerType, false),
                (
                    "orders",
                    crate::ast::SchemaType::TypedList(Box::new(crate::ast::SchemaType::Reference(
                        "Order".into(),
                    ))),
                    false,
                ),
            ],
        ),
    );
    schemas.insert(
        "Order".into(),
        make_db_schema(
            "orders",
            &[
                ("id", crate::ast::SchemaType::IntegerType, false),
                (
                    "customer",
                    crate::ast::SchemaType::Reference("User".into()),
                    false,
                ),
            ],
        ),
    );

    let mut interp = interp_with_mock({
        let mut row = HashMap::new();
        row.insert("id".into(), Value::Integer(55));
        row.insert("customer_id".into(), Value::Integer(1));
        row
    })
    .with_schemas(Arc::new(schemas));

    let record = Value::RelationalRecord {
        schema_name: "User".into(),
        fields: Arc::new(RwLock::new(ValueMap::from_iter([(
            "id".into(),
            Value::Integer(1),
        )]))),
    };

    let handle = interp.access_property(&record, "orders").unwrap();
    let fetched = interp
        .evaluate_pipeline_stage(
            &handle,
            &PipelineStage::Expression(Expression::Identifier("fetch".into())),
        )
        .unwrap();

    let Value::List(items) = fetched else {
        panic!("expected fetched relation list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(
        interp.access_property(&items[0], "customer_id").unwrap(),
        Value::Integer(1)
    );
}

#[test]
fn test_relation_inference_uses_project_persistent_schemas_across_modules() {
    use crate::environment::Environment;
    use crate::file_loader::ProjectRuntime;

    let mut persistent_schemas = HashMap::new();
    persistent_schemas.insert(
        "User".into(),
        make_db_schema(
            "users",
            &[("id", crate::ast::SchemaType::IntegerType, false)],
        ),
    );
    persistent_schemas.insert(
        "Order".into(),
        make_db_schema(
            "orders",
            &[
                ("id", crate::ast::SchemaType::IntegerType, false),
                (
                    "customer",
                    crate::ast::SchemaType::Reference("User".into()),
                    false,
                ),
            ],
        ),
    );

    let runtime = Arc::new(ProjectRuntime {
        global_env: Environment::new(),
        modules: HashMap::from([(
            "routes/users".into(),
            crate::file_loader::ModuleRuntime {
                id: "routes/users".into(),
                env: Environment::new(),
                visible_schemas: HashMap::new(),
            },
        )]),
        public_schemas: HashMap::new(),
        persistent_schemas,
        feature_flags: FeatureFlags::default(),
        task_namespaces: HashMap::new(),
    });

    let mut interp = interp_with_mock({
        let mut row = HashMap::new();
        row.insert("id".into(), Value::Integer(9));
        row.insert("name".into(), Value::String("Alice".into()));
        row
    })
    .with_project_runtime(runtime)
    .with_current_module(Some("routes/users".into()));

    let record = Value::RelationalRecord {
        schema_name: "Order".into(),
        fields: Arc::new(RwLock::new(ValueMap::from_iter([
            ("id".into(), Value::Integer(1)),
            ("customer_id".into(), Value::Integer(9)),
        ]))),
    };

    let handle = interp.access_property(&record, "customer").unwrap();
    let fetched = interp
        .evaluate_pipeline_stage(
            &handle,
            &PipelineStage::Expression(Expression::Identifier("fetch".into())),
        )
        .unwrap();

    assert_eq!(
        interp.access_property(&fetched, "id").unwrap(),
        Value::Integer(9)
    );
}
