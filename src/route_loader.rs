use std::collections::HashMap;

use crate::ast::{
    Argument, AuthProvider, Expression, HttpVerb, MapStatement, PipelineStage, RescueHandler,
    RouteAuth, SchemaField, Statement, TakeBinding, TaskBody,
};
use crate::auth::build_auth_registry;
use crate::error::MarretaError;
use crate::persistent_schema::{collect_persistent_schemas, validate_persistent_schema_references};

/// A single route definition extracted from the AST.
#[derive(Debug, Clone)]
pub struct RouteDefinition {
    pub verb: HttpVerb,
    pub path: String,
    pub auth: Option<RouteAuth>,
    pub allow: Vec<Expression>,
    pub take: Vec<TakeBinding>,
    /// Schema name bound via `as SchemaName`, if any.
    pub schema: Option<String>,
    pub body: Vec<Statement>,
    pub line: usize,
    pub column: usize,
    /// Source file name (stem only, no extension), used as the OpenAPI tag.
    pub source_file: Option<String>,
    /// Stable module identity used at runtime for file-private lookup.
    pub module_id: Option<String>,
}

/// A resolved schema definition stored for runtime validation and OpenAPI generation.
#[derive(Debug, Clone)]
pub struct SchemaDefinition {
    pub db_table: Option<String>,
    pub fields: Vec<SchemaField>,
}

/// Whether this consumer is bound to a named queue or an exact topic.
#[derive(Debug, Clone, PartialEq)]
pub enum ConsumerKind {
    /// Point-to-point: `on queue "name"` — durable named queue.
    Queue,
    /// Pub/sub: `on topic "name"` — exact topic routing key.
    Topic,
}

/// A single consumer definition extracted from `on queue` / `on topic` statements.
#[derive(Debug, Clone)]
pub struct ConsumerDefinition {
    pub kind: ConsumerKind,
    /// Queue name or exact topic string (evaluated as a constant expression at startup).
    pub target: Expression,
    /// Variable name the delivery payload is bound to inside the handler body.
    pub binding: String,
    /// Optional schema name for payload validation.
    pub schema: Option<String>,
    pub body: Vec<Statement>,
    pub line: usize,
    pub column: usize,
    /// Source file name (stem only, no extension).
    pub source_file: Option<String>,
    /// Stable module identity used at runtime for file-private lookup.
    pub module_id: Option<String>,
}

/// The result of loading a `.marreta` program — routes, schemas, consumers, and startup statements.
#[derive(Debug)]
pub struct RouteRegistry {
    /// HTTP route declarations, validated for conflicts.
    pub routes: Vec<RouteDefinition>,
    /// Schema declarations keyed by name, used for payload validation and OpenAPI generation.
    pub schemas: HashMap<String, SchemaDefinition>,
    /// Subset of `schemas` that participate in relational persistence and migrations.
    pub persistent_schemas: HashMap<String, SchemaDefinition>,
    /// Non-route statements (task defs, constants) executed once at startup.
    pub startup_stmts: Vec<Statement>,
    /// Queue and topic consumer definitions collected from `on queue`/`on topic`.
    pub consumers: Vec<ConsumerDefinition>,
    /// Project-level auth provider declarations keyed by provider name.
    pub auth_providers: HashMap<String, AuthProvider>,
}

fn is_pascal_case_schema_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    first.is_ascii_uppercase()
        && name.chars().all(|ch| ch.is_ascii_alphanumeric())
        && !name.contains('_')
}

fn validate_schema_naming(schemas: &HashMap<String, SchemaDefinition>) -> Result<(), MarretaError> {
    for schema_name in schemas.keys() {
        if !is_pascal_case_schema_name(schema_name) {
            return Err(MarretaError::InvalidSchemaDefinition {
                schema_name: schema_name.clone(),
                message: "schema names must use PascalCase".to_string(),
            });
        }
    }

    Ok(())
}

/// Splits a parsed program into route declarations and startup statements,
/// validating for conflicting route patterns.
///
/// `source_file` is the stem of the file being loaded (e.g. `"users"` for `routes/users.marreta`).
pub fn load(
    program: Vec<Statement>,
    source_file: Option<String>,
) -> Result<RouteRegistry, MarretaError> {
    let mut routes: Vec<RouteDefinition> = Vec::new();
    let mut schemas: HashMap<String, SchemaDefinition> = HashMap::new();
    let mut startup_stmts: Vec<Statement> = Vec::new();
    let mut consumers: Vec<ConsumerDefinition> = Vec::new();
    let mut auth_providers: HashMap<String, AuthProvider> = HashMap::new();

    for stmt in program {
        match stmt {
            // Exported schemas are registered just like regular schemas.
            // The export flag is meaningful to the multi-file loader (Phase 2), not here.
            Statement::Export(ref inner) => {
                if let Statement::Schema {
                    name,
                    db_table,
                    fields,
                    ..
                } = inner.as_ref()
                {
                    schemas.insert(
                        name.clone(),
                        SchemaDefinition {
                            db_table: db_table.clone(),
                            fields: fields.clone(),
                        },
                    );
                }
                startup_stmts.push(stmt);
            }
            Statement::Schema {
                name,
                db_table,
                fields,
                ..
            } => {
                schemas.insert(name, SchemaDefinition { db_table, fields });
            }
            Statement::Route {
                verb,
                path,
                auth,
                allow,
                take,
                schema,
                body,
                line,
                column,
            } => {
                // Spec 068: a route path parameter is a binder (it binds a name into the route
                // scope), but the name lives inside the route string literal, so the lexer never
                // emits a reserved-word token there. Reject a reserved word as a path param here,
                // at load, with the same dedicated message the parser uses at every other binder.
                validate_path_params(&path, line, column)?;

                // Check for conflicts against already-registered routes
                let new_pattern = path_pattern(&path);
                for existing in &routes {
                    if existing.verb == verb && path_pattern(&existing.path) == new_pattern {
                        return Err(MarretaError::RouteConflict {
                            verb: verb.to_string(),
                            path_a: existing.path.clone(),
                            path_b: path.clone(),
                            line,
                            column,
                        });
                    }
                }
                routes.push(RouteDefinition {
                    verb,
                    path,
                    auth,
                    allow,
                    take,
                    schema,
                    body,
                    line,
                    column,
                    source_file: source_file.clone(),
                    module_id: source_file.clone(),
                });
            }
            Statement::AuthProvider { provider, .. } => {
                let name = provider.name().to_string();
                if auth_providers.insert(name.clone(), provider).is_some() {
                    return Err(MarretaError::RuntimeError {
                        message: format!("duplicate auth provider '{}'", name),
                        line: 0,
                        column: 0,
                    });
                }
            }
            Statement::OnQueue {
                queue_name,
                binding,
                schema,
                body,
                line,
                column,
            } => {
                consumers.push(ConsumerDefinition {
                    kind: ConsumerKind::Queue,
                    target: queue_name,
                    binding,
                    schema,
                    body,
                    line,
                    column,
                    source_file: source_file.clone(),
                    module_id: source_file.clone(),
                });
            }
            Statement::OnTopic {
                pattern,
                binding,
                schema,
                body,
                line,
                column,
            } => {
                consumers.push(ConsumerDefinition {
                    kind: ConsumerKind::Topic,
                    target: pattern,
                    binding,
                    schema,
                    body,
                    line,
                    column,
                    source_file: source_file.clone(),
                    module_id: source_file.clone(),
                });
            }
            other => startup_stmts.push(other),
        }
    }

    validate_schema_naming(&schemas)?;
    // Schema-reference cycle detection moved to `file_loader` (the live load path) via
    // the shared, relation-aware `schema_cycle` helper (Spec 062).
    validate_persistent_schema_references(&schemas)?;
    validate_auth_contract(&routes, &auth_providers)?;
    build_auth_registry(&auth_providers)?;
    let persistent_schemas = collect_persistent_schemas(&schemas);

    Ok(RouteRegistry {
        routes,
        schemas,
        persistent_schemas,
        startup_stmts,
        consumers,
        auth_providers,
    })
}

/// Validates route-level auth declarations after all providers and routes are known.
pub fn validate_auth_contract(
    routes: &[RouteDefinition],
    auth_providers: &HashMap<String, AuthProvider>,
) -> Result<(), MarretaError> {
    for route in routes {
        match &route.auth {
            Some(auth) => {
                if !auth_providers.contains_key(&auth.provider) {
                    return Err(MarretaError::RuntimeError {
                        message: format!(
                            "route {} {} requires unknown auth provider '{}'",
                            route.verb, route.path, auth.provider
                        ),
                        line: auth.line,
                        column: auth.column,
                    });
                }
            }
            None => {
                if !route.allow.is_empty() {
                    return Err(MarretaError::RuntimeError {
                        message: format!(
                            "route {} {} declares allow without require auth",
                            route.verb, route.path
                        ),
                        line: route.line,
                        column: route.column,
                    });
                }
                if statements_reference_auth(&route.body) {
                    return Err(MarretaError::RuntimeError {
                        message: format!(
                            "public route {} {} cannot access auth; add `require auth <provider>` first",
                            route.verb, route.path
                        ),
                        line: route.line,
                        column: route.column,
                    });
                }
            }
        }
    }

    Ok(())
}

fn statements_reference_auth(statements: &[Statement]) -> bool {
    statements.iter().any(statement_references_auth)
}

fn statement_references_auth(statement: &Statement) -> bool {
    match statement {
        Statement::Assignment { value, .. } => expression_references_auth(value),
        Statement::ConditionalAssignment {
            value, condition, ..
        } => expression_references_auth(value) || expression_references_auth(condition),
        Statement::Require { condition, .. } | Statement::Reject { condition, .. } => {
            expression_references_auth(condition)
        }
        Statement::While {
            condition, body, ..
        } => expression_references_auth(condition) || statements_reference_auth(body),
        Statement::TaskDef { body, .. } => task_body_references_auth(body),
        Statement::ExpressionStatement { expression, .. } => expression_references_auth(expression),
        Statement::Route {
            auth, allow, body, ..
        } => {
            auth.is_some()
                || allow.iter().any(expression_references_auth)
                || statements_reference_auth(body)
        }
        Statement::Schema { .. } | Statement::AuthProvider { .. } | Statement::Scenario { .. } => {
            false
        }
        Statement::Reply {
            status_code,
            body,
            extra_headers,
            ..
        } => {
            expression_references_auth(status_code)
                || expression_references_auth(body)
                || extra_headers
                    .as_ref()
                    .is_some_and(expression_references_auth)
        }
        Statement::Fail { message, .. } => expression_references_auth(message),
        Statement::Raise {
            message, condition, ..
        } => {
            expression_references_auth(message)
                || condition.as_ref().is_some_and(expression_references_auth)
        }
        Statement::Export(inner) => statement_references_auth(inner),
        Statement::Transaction { body, .. } => statements_reference_auth(body),
        Statement::OnQueue {
            queue_name, body, ..
        } => expression_references_auth(queue_name) || statements_reference_auth(body),
        Statement::OnTopic { pattern, body, .. } => {
            expression_references_auth(pattern) || statements_reference_auth(body)
        }
        Statement::Nack { condition, .. } => {
            condition.as_ref().is_some_and(expression_references_auth)
        }
    }
}

fn task_body_references_auth(body: &TaskBody) -> bool {
    match body {
        TaskBody::Inline(expr) => expression_references_auth(expr),
        TaskBody::Block(statements, expr) => {
            statements_reference_auth(statements) || expression_references_auth(expr)
        }
    }
}

fn argument_references_auth(argument: &Argument) -> bool {
    match argument {
        Argument::Positional(expr) => expression_references_auth(expr),
        Argument::Named { value, .. } => expression_references_auth(value),
    }
}

fn expression_references_auth(expression: &Expression) -> bool {
    match expression {
        Expression::Identifier(name) => name == "auth",
        Expression::Integer(_)
        | Expression::Float(_)
        | Expression::StringLiteral(_)
        | Expression::Boolean(_)
        | Expression::Null
        | Expression::TaskCall { .. } => false,
        Expression::List(items) => items.iter().any(expression_references_auth),
        Expression::MapLiteral(entries) => entries
            .iter()
            .any(|(_, value)| expression_references_auth(value)),
        Expression::SchemaConstructor { fields, .. } => fields
            .iter()
            .any(|(_, value)| expression_references_auth(value)),
        Expression::BinaryOp { left, right, .. } => {
            expression_references_auth(left) || expression_references_auth(right)
        }
        Expression::UnaryOp { operand, .. } => expression_references_auth(operand),
        Expression::PropertyAccess { object, .. } => expression_references_auth(object),
        Expression::MethodCall {
            object, arguments, ..
        } => expression_references_auth(object) || arguments.iter().any(argument_references_auth),
        Expression::HttpClientResponseSchema { call, .. } => expression_references_auth(call),
        Expression::FunctionCall { arguments, .. } => {
            arguments.iter().any(argument_references_auth)
        }
        Expression::Match { subject, arms } => {
            expression_references_auth(subject)
                || arms.iter().any(|arm| {
                    (match &arm.pattern {
                        crate::ast::MatchPattern::Literal(expr) => expression_references_auth(expr),
                        crate::ast::MatchPattern::Fallback => false,
                    }) || expression_references_auth(&arm.value)
                })
        }
        Expression::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expression_references_auth(condition)
                || task_body_references_auth(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|branch| task_body_references_auth(branch))
        }
        Expression::Subscript { object, key } => {
            expression_references_auth(object) || expression_references_auth(key)
        }
        Expression::Pipeline { input, stages } => {
            expression_references_auth(input) || stages.iter().any(pipeline_stage_references_auth)
        }
        Expression::Broadcast { input, targets } => {
            expression_references_auth(input) || targets.iter().any(expression_references_auth)
        }
        Expression::Rescue { expr, handler } => {
            expression_references_auth(expr) || expression_references_auth(handler)
        }
        Expression::QueuePush {
            queue_name,
            payload,
            ..
        } => {
            expression_references_auth(queue_name)
                || payload
                    .as_ref()
                    .is_some_and(|expr| expression_references_auth(expr))
        }
        Expression::TopicPublish { topic, payload, .. } => {
            expression_references_auth(topic)
                || payload
                    .as_ref()
                    .is_some_and(|expr| expression_references_auth(expr))
        }
    }
}

fn pipeline_stage_references_auth(stage: &PipelineStage) -> bool {
    match stage {
        PipelineStage::Expression(expr) => expression_references_auth(expr),
        PipelineStage::Map { body, .. } => body.iter().any(map_statement_references_auth),
        PipelineStage::Reduce { initial, body, .. } => {
            expression_references_auth(initial) || task_body_references_auth(body)
        }
        PipelineStage::Rescue { handler } => rescue_handler_references_auth(handler),
    }
}

fn map_statement_references_auth(statement: &MapStatement) -> bool {
    match statement {
        MapStatement::Statement(stmt) => statement_references_auth(stmt),
        MapStatement::Keep { value, condition } => {
            expression_references_auth(value)
                || condition.as_ref().is_some_and(expression_references_auth)
        }
        MapStatement::Skip { condition } => expression_references_auth(condition),
    }
}

fn rescue_handler_references_auth(handler: &RescueHandler) -> bool {
    match handler {
        RescueHandler::Inline(expr) => expression_references_auth(expr),
        RescueHandler::Block(statements) => statements_reference_auth(statements),
    }
}

/// Spec 068: rejects a reserved word used as a route path parameter (`/x/:doc`). Each `:param`
/// segment binds a name into the route scope, so the same reserved-word rule that governs every
/// other binder applies - but the name is inside the string literal, so it is checked here at
/// load rather than at parse. `keyword_lookup` matching the segment means it is a reserved word.
fn validate_path_params(path: &str, line: usize, column: usize) -> Result<(), MarretaError> {
    for segment in path.split('/') {
        if let Some(param) = segment.strip_prefix(':') {
            if crate::token::keyword_lookup(param).is_some() {
                return Err(MarretaError::ReservedWord {
                    word: param.to_string(),
                    line,
                    column,
                });
            }
        }
    }
    Ok(())
}

/// Normalizes a route path for conflict detection by replacing all `:param`
/// segments with the placeholder `:*`.
///
/// Examples:
/// - `/users/:id`           → `/users/:*`
/// - `/users/:name`         → `/users/:*`   (conflicts with above)
/// - `/users/active`        → `/users/active` (literal — no conflict)
/// - `/orders/:id/items/:n` → `/orders/:*/items/:*`
pub fn path_pattern(path: &str) -> String {
    path.split('/')
        .map(|seg| if seg.starts_with(':') { ":*" } else { seg })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{
        AuthProviderConfig, AuthProviderField, BinaryOperator, Expression, HttpVerb, ParamDef,
        RouteAuth, TaskBody,
    };

    fn make_route(verb: HttpVerb, path: &str) -> Statement {
        Statement::Route {
            verb,
            path: path.to_string(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![],
            line: 1,
            column: 1,
        }
    }

    fn make_task() -> Statement {
        Statement::TaskDef {
            name: "double".into(),
            params: vec![ParamDef {
                name: "n".into(),
                schema: None,
            }],
            body: TaskBody::Inline(Expression::Null),
            line: 1,
            column: 1,
        }
    }

    fn make_auth_provider(name: &str) -> Statement {
        Statement::AuthProvider {
            provider: AuthProvider::Jwt(AuthProviderConfig {
                name: name.to_string(),
                fields: vec![
                    AuthProviderField {
                        name: "issuer".into(),
                        value: Expression::StringLiteral("https://idp.example.test".into()),
                        line: 1,
                        column: 1,
                    },
                    AuthProviderField {
                        name: "audience".into(),
                        value: Expression::StringLiteral("shop-api".into()),
                        line: 1,
                        column: 1,
                    },
                ],
            }),
            line: 1,
            column: 1,
        }
    }

    fn protected_route(provider: &str) -> Statement {
        Statement::Route {
            verb: HttpVerb::Get,
            path: "/orders".into(),
            auth: Some(RouteAuth {
                provider: provider.into(),
                line: 2,
                column: 5,
            }),
            allow: vec![Expression::BinaryOp {
                left: Box::new(Expression::StringLiteral("admin".into())),
                operator: BinaryOperator::In,
                right: Box::new(Expression::PropertyAccess {
                    object: Box::new(Expression::PropertyAccess {
                        object: Box::new(Expression::Identifier("auth".into())),
                        property: "user".into(),
                    }),
                    property: "roles".into(),
                }),
            }],
            take: vec![],
            schema: None,
            body: vec![Statement::Reply {
                status_code: Expression::Integer(200),
                content_type: crate::ast::ReplyContentType::Json,
                body: Expression::MapLiteral(vec![("ok".into(), Expression::Boolean(true))]),
                response_schema: None,
                extra_headers: None,
                line: 3,
                column: 5,
            }],
            line: 1,
            column: 1,
        }
    }

    #[test]
    fn test_auth_provider_stored_and_route_auth_validated() {
        let registry = load(
            vec![
                make_auth_provider("customer_auth"),
                protected_route("customer_auth"),
            ],
            None,
        )
        .unwrap();
        assert!(registry.auth_providers.contains_key("customer_auth"));
        assert_eq!(
            registry.routes[0]
                .auth
                .as_ref()
                .map(|auth| auth.provider.as_str()),
            Some("customer_auth")
        );
        assert_eq!(registry.routes[0].allow.len(), 1);
    }

    #[test]
    fn test_route_auth_unknown_provider_is_error() {
        let err = load(vec![protected_route("missing_auth")], None).unwrap_err();
        match err {
            MarretaError::RuntimeError { message, line, .. } => {
                assert!(message.contains("unknown auth provider"));
                assert_eq!(line, 2);
            }
            other => panic!("expected RuntimeError, got {:?}", other),
        }
    }

    #[test]
    fn test_route_allow_without_auth_is_error() {
        let mut route = protected_route("customer_auth");
        if let Statement::Route { auth, .. } = &mut route {
            *auth = None;
        }
        let err = load(vec![make_auth_provider("customer_auth"), route], None).unwrap_err();
        match err {
            MarretaError::RuntimeError { message, .. } => {
                assert!(message.contains("allow without require auth"));
            }
            other => panic!("expected RuntimeError, got {:?}", other),
        }
    }

    #[test]
    fn test_public_route_accessing_auth_is_error() {
        let route = Statement::Route {
            verb: HttpVerb::Get,
            path: "/me".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![Statement::Reply {
                status_code: Expression::Integer(200),
                content_type: crate::ast::ReplyContentType::Json,
                body: Expression::PropertyAccess {
                    object: Box::new(Expression::Identifier("auth".into())),
                    property: "user".into(),
                },
                response_schema: None,
                extra_headers: None,
                line: 2,
                column: 5,
            }],
            line: 1,
            column: 1,
        };
        let err = load(vec![route], None).unwrap_err();
        match err {
            MarretaError::RuntimeError { message, .. } => {
                assert!(message.contains("public route"));
                assert!(message.contains("cannot access auth"));
            }
            other => panic!("expected RuntimeError, got {:?}", other),
        }
    }

    #[test]
    fn test_splits_routes_from_startup() {
        let program = vec![make_task(), make_route(HttpVerb::Get, "/hello")];
        let registry = load(program, None).unwrap();
        assert_eq!(registry.routes.len(), 1);
        assert_eq!(registry.startup_stmts.len(), 1);
    }

    #[test]
    fn test_route_path_and_verb_stored() {
        let program = vec![make_route(HttpVerb::Post, "/users")];
        let registry = load(program, None).unwrap();
        assert_eq!(registry.routes[0].verb, HttpVerb::Post);
        assert_eq!(registry.routes[0].path, "/users");
    }

    #[test]
    fn test_reserved_word_path_param_blocked_at_load() {
        // Spec 068: `:doc`/`:feature`/`:env` (and any reserved word) as a route path parameter is
        // rejected at load with the dedicated reserved-word error - the lexer never sees the name
        // because it lives inside the route string literal.
        for word in ["doc", "feature", "env", "db", "time"] {
            let program = vec![make_route(HttpVerb::Get, &format!("/x/:{word}"))];
            match load(program, None) {
                Err(MarretaError::ReservedWord { word: got, .. }) => assert_eq!(got, word),
                other => panic!("expected ReservedWord for :{word}, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_ordinary_path_param_allowed_at_load() {
        // A non-reserved path parameter still loads cleanly.
        let program = vec![make_route(HttpVerb::Get, "/users/:id")];
        let registry = load(program, None).unwrap();
        assert_eq!(registry.routes[0].path, "/users/:id");
    }

    #[test]
    fn test_exact_duplicate_route_conflicts() {
        let program = vec![
            make_route(HttpVerb::Get, "/users"),
            make_route(HttpVerb::Get, "/users"),
        ];
        assert!(matches!(
            load(program, None),
            Err(MarretaError::RouteConflict { .. })
        ));
    }

    #[test]
    fn test_same_pattern_different_param_names_conflicts() {
        let program = vec![
            make_route(HttpVerb::Get, "/users/:id"),
            make_route(HttpVerb::Get, "/users/:name"),
        ];
        assert!(matches!(
            load(program, None),
            Err(MarretaError::RouteConflict { .. })
        ));
    }

    #[test]
    fn test_same_wildcard_structure_conflicts() {
        let program = vec![
            make_route(HttpVerb::Get, "/:a/:b"),
            make_route(HttpVerb::Get, "/:x/:y"),
        ];
        assert!(matches!(
            load(program, None),
            Err(MarretaError::RouteConflict { .. })
        ));
    }

    #[test]
    fn test_literal_vs_param_allowed() {
        // axum handles this correctly — literal wins over param
        let program = vec![
            make_route(HttpVerb::Get, "/users/active"),
            make_route(HttpVerb::Get, "/users/:id"),
        ];
        assert!(load(program, None).is_ok());
    }

    #[test]
    fn test_different_verbs_same_path_allowed() {
        let program = vec![
            make_route(HttpVerb::Get, "/users/:id"),
            make_route(HttpVerb::Post, "/users/:id"),
        ];
        assert!(load(program, None).is_ok());
    }

    #[test]
    fn test_empty_program() {
        let registry = load(vec![], None).unwrap();
        assert!(registry.routes.is_empty());
        assert!(registry.startup_stmts.is_empty());
        assert!(registry.schemas.is_empty());
    }

    #[test]
    fn test_path_pattern_normalization() {
        assert_eq!(path_pattern("/users/:id"), "/users/:*");
        assert_eq!(path_pattern("/users/active"), "/users/active");
        assert_eq!(path_pattern("/:a/:b"), "/:*/:*");
        assert_eq!(path_pattern("/hello"), "/hello");
    }

    #[test]
    fn test_conflict_error_contains_both_paths() {
        let program = vec![
            make_route(HttpVerb::Get, "/users/:id"),
            make_route(HttpVerb::Get, "/users/:name"),
        ];
        match load(program, None) {
            Err(MarretaError::RouteConflict { path_a, path_b, .. }) => {
                assert_eq!(path_a, "/users/:id");
                assert_eq!(path_b, "/users/:name");
            }
            _ => panic!("expected RouteConflict"),
        }
    }

    fn make_schema(name: &str, fields: Vec<SchemaField>) -> Statement {
        Statement::Schema {
            name: name.to_string(),
            db_table: None,
            fields,
            line: 1,
            column: 1,
        }
    }

    #[test]
    fn test_schema_stored_in_registry() {
        use crate::ast::{SchemaField, SchemaType};
        let schema = make_schema(
            "UserPayload",
            vec![
                SchemaField {
                    name: "name".into(),
                    field_type: SchemaType::StringType,
                    optional: false,
                },
                SchemaField {
                    name: "age".into(),
                    field_type: SchemaType::IntegerType,
                    optional: false,
                },
            ],
        );
        let registry = load(vec![schema], None).unwrap();
        assert_eq!(registry.routes.len(), 0);
        assert_eq!(registry.startup_stmts.len(), 0);
        assert!(registry.schemas.contains_key("UserPayload"));
        assert!(!registry.persistent_schemas.contains_key("UserPayload"));
        assert_eq!(registry.schemas["UserPayload"].fields.len(), 2);
    }

    #[test]
    fn test_schema_name_must_use_pascal_case() {
        let schema = make_schema("user_payload", vec![str_field("name")]);
        let err = load(vec![schema], None).unwrap_err();
        assert!(matches!(err, MarretaError::InvalidSchemaDefinition { .. }));
    }

    #[test]
    fn test_persistent_schema_stored_in_persistent_registry() {
        let schema = Statement::Schema {
            name: "User".into(),
            db_table: Some("users".into()),
            fields: vec![int_field("id"), str_field("name")],
            line: 1,
            column: 1,
        };

        let registry = load(vec![schema], None).unwrap();
        assert!(registry.schemas.contains_key("User"));
        assert!(registry.persistent_schemas.contains_key("User"));
        assert_eq!(
            registry.persistent_schemas["User"].db_table.as_deref(),
            Some("users")
        );
    }

    #[test]
    fn test_route_schema_binding_stored() {
        let stmt = Statement::Route {
            verb: HttpVerb::Post,
            path: "/users".into(),
            auth: None,
            allow: vec![],
            take: vec![TakeBinding::Payload("payload".into())],
            schema: Some("UserPayload".into()),
            body: vec![],
            line: 1,
            column: 1,
        };
        let registry = load(vec![stmt], None).unwrap();
        assert_eq!(registry.routes[0].schema.as_deref(), Some("UserPayload"));
    }

    // --- v0.4.0: Circular reference detection tests ---

    fn make_schema_with_fields(name: &str, fields: Vec<SchemaField>) -> Statement {
        Statement::Schema {
            name: name.to_string(),
            db_table: None,
            fields,
            line: 1,
            column: 1,
        }
    }

    fn ref_field(name: &str, schema_ref: &str) -> SchemaField {
        use crate::ast::SchemaType;
        SchemaField {
            name: name.to_string(),
            field_type: SchemaType::Reference(schema_ref.to_string()),
            optional: false,
        }
    }

    fn int_field(name: &str) -> SchemaField {
        use crate::ast::SchemaType;
        SchemaField {
            name: name.to_string(),
            field_type: SchemaType::IntegerType,
            optional: false,
        }
    }

    fn str_field(name: &str) -> SchemaField {
        use crate::ast::SchemaType;
        SchemaField {
            name: name.to_string(),
            field_type: SchemaType::StringType,
            optional: false,
        }
    }

    // Schema-reference cycle detection moved to `file_loader` + the shared
    // `schema_cycle` helper (Spec 062); its tests live there and in `schema_cycle`.

    #[test]
    fn test_reference_to_unknown_schema_is_not_a_cycle() {
        // Referencing a schema not yet declared is not a cycle — it fails at runtime
        let program = vec![make_schema_with_fields(
            "User",
            vec![ref_field("billing", "nonexistent")],
        )];
        // Should succeed at load time — unknown refs are caught at validation time
        assert!(load(program, None).is_ok());
    }

    #[test]
    fn test_persistent_schema_reference_to_contract_schema_is_error() {
        let contract = make_schema_with_fields("AddressPayload", vec![str_field("city")]);
        let persistent = Statement::Schema {
            name: "User".into(),
            db_table: Some("users".into()),
            fields: vec![int_field("id"), ref_field("address", "AddressPayload")],
            line: 1,
            column: 1,
        };

        let err = load(vec![contract, persistent], None).unwrap_err();
        assert!(matches!(
            err,
            MarretaError::InvalidPersistentSchemaReference { .. }
        ));
    }

    #[test]
    fn test_schema_does_not_go_to_startup_stmts() {
        use crate::ast::{SchemaField, SchemaType};
        let schema = make_schema(
            "Payload",
            vec![SchemaField {
                name: "id".into(),
                field_type: SchemaType::IntegerType,
                optional: false,
            }],
        );
        let registry = load(vec![make_task(), schema], None).unwrap();
        // task goes to startup, schema goes to schemas map — not startup_stmts
        assert_eq!(registry.startup_stmts.len(), 1);
        assert_eq!(registry.schemas.len(), 1);
    }

    // --- v0.8.0: Consumer collection tests ---

    fn make_on_queue(queue: &str, binding: &str) -> Statement {
        Statement::OnQueue {
            queue_name: Expression::StringLiteral(queue.to_string()),
            binding: binding.to_string(),
            schema: None,
            body: vec![],
            line: 1,
            column: 1,
        }
    }

    fn make_on_queue_with_schema(queue: &str, binding: &str, schema: &str) -> Statement {
        Statement::OnQueue {
            queue_name: Expression::StringLiteral(queue.to_string()),
            binding: binding.to_string(),
            schema: Some(schema.to_string()),
            body: vec![],
            line: 1,
            column: 1,
        }
    }

    fn make_on_topic(pattern: &str, binding: &str) -> Statement {
        Statement::OnTopic {
            pattern: Expression::StringLiteral(pattern.to_string()),
            binding: binding.to_string(),
            schema: None,
            body: vec![],
            line: 1,
            column: 1,
        }
    }

    #[test]
    fn test_on_queue_collected_as_consumer() {
        let program = vec![make_on_queue("orders", "msg")];
        let registry = load(program, None).unwrap();
        assert_eq!(registry.consumers.len(), 1);
        assert_eq!(registry.consumers[0].kind, ConsumerKind::Queue);
        assert_eq!(registry.consumers[0].binding, "msg");
        assert_eq!(registry.routes.len(), 0);
        assert_eq!(registry.startup_stmts.len(), 0);
    }

    #[test]
    fn test_on_topic_collected_as_consumer() {
        let program = vec![make_on_topic("orders.created", "msg")];
        let registry = load(program, None).unwrap();
        assert_eq!(registry.consumers.len(), 1);
        assert_eq!(registry.consumers[0].kind, ConsumerKind::Topic);
        assert_eq!(registry.consumers[0].binding, "msg");
    }

    #[test]
    fn test_consumer_schema_stored() {
        let program = vec![make_on_queue_with_schema("orders", "msg", "OrderPayload")];
        let registry = load(program, None).unwrap();
        assert_eq!(
            registry.consumers[0].schema.as_deref(),
            Some("OrderPayload")
        );
    }

    #[test]
    fn test_consumer_source_file_stored() {
        let program = vec![make_on_queue("orders", "msg")];
        let registry = load(program, Some("consumers".to_string())).unwrap();
        assert_eq!(
            registry.consumers[0].source_file.as_deref(),
            Some("consumers")
        );
    }

    #[test]
    fn test_multiple_consumers_collected() {
        let program = vec![
            make_on_queue("orders", "order_msg"),
            make_on_topic("payments.approved", "payment_msg"),
            make_on_queue("notifications", "notif"),
        ];
        let registry = load(program, None).unwrap();
        assert_eq!(registry.consumers.len(), 3);
        assert_eq!(registry.consumers[0].kind, ConsumerKind::Queue);
        assert_eq!(registry.consumers[1].kind, ConsumerKind::Topic);
        assert_eq!(registry.consumers[2].kind, ConsumerKind::Queue);
    }

    #[test]
    fn test_consumers_and_routes_coexist() {
        let program = vec![
            make_route(HttpVerb::Get, "/health"),
            make_on_queue("orders", "msg"),
        ];
        let registry = load(program, None).unwrap();
        assert_eq!(registry.routes.len(), 1);
        assert_eq!(registry.consumers.len(), 1);
    }

    #[test]
    fn test_empty_program_has_no_consumers() {
        let registry = load(vec![], None).unwrap();
        assert!(registry.consumers.is_empty());
    }
}
