/// A program is a list of statements.
pub type Program = Vec<Statement>;

/// Statements — instructions that don't directly produce a value.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// `name = expression`
    Assignment {
        target: String,
        value: Expression,
        line: usize,
        column: usize,
    },

    /// `name = expression if condition`
    ConditionalAssignment {
        target: String,
        value: Expression,
        condition: Expression,
        line: usize,
        column: usize,
    },

    /// `require EXPR else fail CODE, MSG`
    Require {
        condition: Expression,
        error_code: i64,
        error_message: String,
        line: usize,
        column: usize,
    },

    /// `reject EXPR else fail CODE, MSG`
    Reject {
        condition: Expression,
        error_code: i64,
        error_message: String,
        line: usize,
        column: usize,
    },

    /// `while CONDITION\n  body`
    While {
        condition: Expression,
        body: Vec<Statement>,
        line: usize,
        column: usize,
    },

    /// `task name(params) => expr` or `task name(params)\n  body`
    TaskDef {
        name: String,
        params: Vec<ParamDef>,
        body: TaskBody,
        line: usize,
        column: usize,
    },

    /// An expression used as a statement (e.g., function call).
    ExpressionStatement {
        expression: Expression,
        line: usize,
        column: usize,
    },

    /// `route GET "/path" [take BINDING [as Schema] [, ...]]\n  body`, or the multi-line form with
    /// leading indented `take` lines. Per-binding `as Schema` lives on each `TakeBinding` (Spec 077).
    Route {
        verb: HttpVerb,
        path: String,
        /// Optional route-level authentication provider from `require auth provider_name`.
        auth: Option<RouteAuth>,
        /// Route-level authorization expressions from `allow expr`.
        allow: Vec<Expression>,
        /// Zero or more request bindings (empty = no take). A `take payload as Schema` carries the
        /// payload schema on the binding itself (Spec 077; previously a route-level `schema` field).
        take: Vec<TakeBinding>,
        body: Vec<Statement>,
        line: usize,
        column: usize,
    },

    /// `schema Name { field: type\n  ... }`
    Schema {
        name: String,
        db_table: Option<String>,
        fields: Vec<SchemaField>,
        line: usize,
        column: usize,
    },

    /// `auth jwt name { ... }` or `auth api_key name { ... }`.
    AuthProvider {
        provider: AuthProvider,
        line: usize,
        column: usize,
    },

    /// `reply [html|text] CODE [as schema_name], expr [, headers_map]` — terminates route with HTTP response
    Reply {
        /// Status code — may be any expression that evaluates to Integer at runtime
        status_code: Expression,
        content_type: ReplyContentType,
        body: Expression,
        /// Optional schema name for response serialization: `reply 201 as order_result, value`
        response_schema: Option<String>,
        /// Optional extra response headers map, e.g. `{ Location: "..." }` for redirects
        extra_headers: Option<Expression>,
        line: usize,
        column: usize,
    },

    /// `fail CODE, expr` — terminates route with HTTP error response
    Fail {
        status_code: i64,
        message: Expression,
        line: usize,
        column: usize,
    },

    /// `raise MSG [if CONDITION]` — signals a domain error; propagates as RaiseError (v0.6.0)
    Raise {
        message: Expression,
        condition: Option<Expression>,
        line: usize,
        column: usize,
    },

    /// `export task|schema|assignment` — marks a symbol as globally visible across files (v0.3.2)
    Export(Box<Statement>),

    /// `transaction\n  body` — atomic sequential block; rolls back all DB ops on error.
    /// Nesting is a parse-time error. `*>>` inside is a runtime error.
    Transaction {
        body: Vec<Statement>,
        line: usize,
        column: usize,
    },

    // ── Queue (v0.8) ──────────────────────────────────────────────────────────
    /// `on queue "name" take binding [as schema]\n  body`
    /// Point-to-point consumer. Runs continuously in the background.
    OnQueue {
        queue_name: Expression,
        binding: String,
        schema: Option<String>,
        body: Vec<Statement>,
        line: usize,
        column: usize,
    },

    /// `on topic "name" take binding [as schema]\n  body`
    /// Pub/sub consumer bound to an exact topic string.
    OnTopic {
        pattern: Expression,
        binding: String,
        schema: Option<String>,
        body: Vec<Statement>,
        line: usize,
        column: usize,
    },

    /// `nack` or `nack requeue`, optionally guarded via `require X else nack [requeue]`.
    /// Explicitly rejects a message inside an `on queue/topic` handler.
    Nack {
        requeue: bool,
        /// When set, nack only fires if this expression is truthy (used by
        /// `require X else nack` — the expression is NOT(original condition)).
        condition: Option<Expression>,
        line: usize,
        column: usize,
    },

    /// `scenario "name"\n  given ...\n  when ...\n  then ...`
    /// API scenario test block executed by `marreta test`.
    Scenario {
        name: String,
        steps: Vec<ScenarioStep>,
        line: usize,
        column: usize,
    },
}

/// Route-level authentication requirement.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteAuth {
    pub provider: String,
    pub line: usize,
    pub column: usize,
}

/// Project-level authentication provider declaration.
#[derive(Debug, Clone, PartialEq)]
pub enum AuthProvider {
    Jwt(AuthProviderConfig),
    ApiKey(AuthProviderConfig),
}

impl AuthProvider {
    pub fn name(&self) -> &str {
        match self {
            AuthProvider::Jwt(config) | AuthProvider::ApiKey(config) => &config.name,
        }
    }
}

/// Generic auth provider field map. Field values are expressions so env-backed
/// config remains a first-class Marreta expression.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthProviderConfig {
    pub name: String,
    pub fields: Vec<AuthProviderField>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthProviderField {
    pub name: String,
    pub value: Expression,
    pub line: usize,
    pub column: usize,
}

/// HTTP verb for route declarations.
#[derive(Debug, Clone, PartialEq)]
pub enum HttpVerb {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl std::fmt::Display for HttpVerb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpVerb::Get => write!(f, "GET"),
            HttpVerb::Post => write!(f, "POST"),
            HttpVerb::Put => write!(f, "PUT"),
            HttpVerb::Patch => write!(f, "PATCH"),
            HttpVerb::Delete => write!(f, "DELETE"),
        }
    }
}

/// A statement inside a `scenario` block.
#[derive(Debug, Clone, PartialEq)]
pub enum ScenarioStep {
    /// `given TARGET returns VALUE`
    Given {
        target: Expression,
        returns: Expression,
        line: usize,
        column: usize,
    },

    /// `when METHOD "/path" [with BODY] [and headers HEADERS]`
    When {
        verb: HttpVerb,
        path: String,
        body: Option<Expression>,
        headers: Option<Expression>,
        line: usize,
        column: usize,
    },

    /// `then status CODE`
    ThenStatus {
        status: Expression,
        line: usize,
        column: usize,
    },

    /// `then response is EXPECTED`
    ThenResponse {
        expected: Expression,
        line: usize,
        column: usize,
    },
}

/// Content type modifier for `reply` statements.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplyContentType {
    /// Default — `application/json` (body serialized via `value_to_json`)
    Json,
    /// `reply html CODE, "..."` — `text/html; charset=utf-8`
    Html,
    /// `reply text CODE, "..."` — `text/plain; charset=utf-8`
    Text,
}

/// Which request input a `take` binding captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakeKind {
    /// `take payload` — JSON request body (`application/json`) → `Value::Map`
    Payload,
    /// `take query` — query string parameters → `Value::Map`
    Query,
    /// `take headers` — request headers → `Value::Map`
    Headers,
    /// `take form` — form-encoded body (`application/x-www-form-urlencoded`) → `Value::Map`
    Form,
    /// `take raw` — raw request body bytes → `Value::String`
    Raw,
}

/// A single `take` binding: the input source, the variable name it binds to, and an optional
/// per-binding schema (`take query as SearchQuery`). Spec 077 moved `as Schema` from a route-level
/// clause (payload-only) to per-binding, so query and headers can be validated and coerced like the
/// body. `schema` is `None` for a raw bind. `Raw` never carries a schema.
#[derive(Debug, Clone, PartialEq)]
pub struct TakeBinding {
    pub kind: TakeKind,
    pub name: String,
    pub schema: Option<String>,
}

impl TakeBinding {
    /// Convenience constructor for a raw (schema-less) binding.
    pub fn raw(kind: TakeKind, name: impl Into<String>) -> Self {
        TakeBinding {
            kind,
            name: name.into(),
            schema: None,
        }
    }
}

/// The payload schema declared on a route's `take` list, if any (Spec 077: `take payload as Schema`).
/// Replaces the former route-level `schema` field; the request-body readers (payload validation in
/// the server, request body in the OpenAPI generator) resolve the payload schema through this.
pub fn payload_schema(take: &[TakeBinding]) -> Option<&str> {
    take.iter()
        .find(|b| b.kind == TakeKind::Payload)
        .and_then(|b| b.schema.as_deref())
}

/// The binding (if any) for a given input kind, used to resolve its per-binding schema.
pub fn binding_for(take: &[TakeBinding], kind: TakeKind) -> Option<&TakeBinding> {
    take.iter().find(|b| b.kind == kind)
}

/// Task body — inline single expression or block with implicit return.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskBody {
    /// `task apply_discount(value) => value * 0.90`
    Inline(Expression),

    /// ```marreta
    /// task calculate(item)
    ///     base = item.price * 1.15
    ///     base - discount
    /// ```
    /// The Vec<Statement> are the body statements, and the Expression is the
    /// final expression (implicit return).
    Block(Vec<Statement>, Expression),
}

/// Expressions — everything that produces a value.
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    // --- Literals ---
    Integer(i64),
    Float(f64),
    StringLiteral(String),
    Boolean(bool),
    Null,
    List(Vec<Expression>),
    MapLiteral(Vec<(String, Expression)>),

    // --- Schema constructor: SchemaName { field: expr, ... } ---
    SchemaConstructor {
        schema_name: String,
        fields: Vec<(String, Expression)>,
    },

    // --- Identifier ---
    Identifier(String),

    // --- Binary operation: left op right ---
    BinaryOp {
        left: Box<Expression>,
        operator: BinaryOperator,
        right: Box<Expression>,
    },

    // --- Unary operation: op operand ---
    UnaryOp {
        operator: UnaryOperator,
        operand: Box<Expression>,
    },

    // --- Property access: obj.field ---
    PropertyAccess {
        object: Box<Expression>,
        property: String,
    },

    // --- Method call: obj.method(args) ---
    MethodCall {
        object: Box<Expression>,
        method: String,
        arguments: Vec<Argument>,
    },

    // --- HTTP client response schema: http_client.get(...) as SchemaName ---
    HttpClientResponseSchema {
        call: Box<Expression>,
        schema_name: String,
    },

    // --- Function call: func(args) ---
    FunctionCall {
        name: String,
        arguments: Vec<Argument>,
    },

    // --- Task call in pipeline: task(task_name) ---
    TaskCall {
        name: String,
    },

    // --- Match expression ---
    Match {
        subject: Box<Expression>,
        arms: Vec<MatchArm>,
    },

    // --- If expression ---
    If {
        condition: Box<Expression>,
        then_branch: Box<TaskBody>,
        else_branch: Option<Box<TaskBody>>,
    },

    // --- Subscript access: expr[key] ---
    Subscript {
        object: Box<Expression>,
        key: Box<Expression>,
    },

    // --- Pipeline: expr >> stage >> stage ---
    Pipeline {
        input: Box<Expression>,
        stages: Vec<PipelineStage>,
    },

    // --- Broadcast: expr *>> [destinations] ---
    Broadcast {
        input: Box<Expression>,
        targets: Vec<Expression>,
    },

    // --- Rescue expression modifier: expr rescue handler (v0.6.0) ---
    Rescue {
        expr: Box<Expression>,
        handler: Box<Expression>,
    },

    // ── Queue producers (v0.8) ───────────────────────────────────────────────
    /// `queue.push "name" [as schema], payload` or `value >> queue.push("name")`
    /// Sends a message to a named queue (point-to-point).
    /// When `payload` is `None`, the value comes from the pipeline input.
    QueuePush {
        queue_name: Box<Expression>,
        schema: Option<String>,
        payload: Option<Box<Expression>>,
    },

    /// `topic.publish "topic" [as schema], payload` or `value >> topic.publish("topic")`
    /// Publishes a message to a topic (provider-agnostic pub/sub).
    /// When `payload` is `None`, the value comes from the pipeline input.
    TopicPublish {
        topic: Box<Expression>,
        schema: Option<String>,
        payload: Option<Box<Expression>>,
    },
}

/// Binary operators.
#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOperator {
    Add,          // +
    Subtract,     // -
    Multiply,     // *
    Divide,       // /
    Modulo,       // %
    Equal,        // ==
    NotEqual,     // !=
    Greater,      // >
    Less,         // <
    GreaterEqual, // >=
    LessEqual,    // <=
    In,           // in
    And,          // and
    Or,           // or
}

/// Unary operators.
#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOperator {
    Negate, // -
    Not,    // not
}

/// A single arm in a match expression.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub value: Expression,
}

/// Pattern in a match arm.
#[derive(Debug, Clone, PartialEq)]
pub enum MatchPattern {
    /// A literal value to compare: `"VIP" ->`, `42 ->`, `true ->`
    Literal(Expression),
    /// The default case: `fallback ->`
    Fallback,
}

/// A stage in a pipeline chain.
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineStage {
    /// A simple expression stage: `>> db.orders.save`
    Expression(Expression),

    /// A map/keep transformation block:
    /// ```marreta
    /// >> map item
    ///     item.total = item.price * 1.15
    ///     keep item
    /// ```
    Map {
        variable: String,
        body: Vec<MapStatement>,
    },

    /// A reduce accumulation block:
    /// ```marreta
    /// >> reduce(0) acc, item
    ///     acc + item
    /// ```
    Reduce {
        initial: Expression,
        accumulator: String,
        item: String,
        body: TaskBody,
    },

    /// `>> rescue [handler]` — terminal error-capture stage (v0.6.0)
    Rescue { handler: RescueHandler },
}

/// Handler for `>> rescue` pipeline stage (v0.6.0).
#[derive(Debug, Clone, PartialEq)]
pub enum RescueHandler {
    /// `>> rescue fail CODE, MSG` or `>> rescue expr` — inline expression
    Inline(Expression),
    /// `>> rescue\n  indented body` — multi-statement block
    Block(Vec<Statement>),
}

/// A statement inside a `map` block — regular statements plus keep/skip directives.
#[derive(Debug, Clone, PartialEq)]
pub enum MapStatement {
    /// A regular statement (assignment, expression, etc.)
    Statement(Statement),
    /// `keep expr [if cond]` — keep element with value; condition is optional
    Keep {
        value: Expression,
        condition: Option<Expression>,
    },
    /// `skip if cond` — drop element from result when condition is true
    Skip { condition: Expression },
}

/// A field in a schema declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaField {
    pub name: String,
    pub field_type: SchemaType,
    pub optional: bool,
}

/// Types supported in schema field declarations (v0.4.0: composition via Reference and TypedList).
#[derive(Debug, Clone, PartialEq)]
pub enum SchemaType {
    StringType,
    IntegerType,
    FloatType,
    DecimalType,
    BooleanType,
    InstantType,
    DateType,
    TimeType,
    DurationType,
    IntervalType,
    ListType,
    MapType,
    /// `status: enum ["pending", "paid"]` — inline string enum constraint.
    EnumType(Vec<String>),
    /// `billing: address` — references another declared schema by name.
    Reference(String),
    /// `items: list of order_item` — typed list of primitives or schema references.
    TypedList(Box<SchemaType>),
}

impl std::fmt::Display for SchemaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StringType => write!(f, "string"),
            Self::IntegerType => write!(f, "integer"),
            Self::FloatType => write!(f, "float"),
            Self::DecimalType => write!(f, "decimal"),
            Self::BooleanType => write!(f, "boolean"),
            Self::InstantType => write!(f, "instant"),
            Self::DateType => write!(f, "date"),
            Self::TimeType => write!(f, "time"),
            Self::DurationType => write!(f, "duration"),
            Self::IntervalType => write!(f, "interval"),
            Self::ListType => write!(f, "list"),
            Self::MapType => write!(f, "map"),
            Self::EnumType(values) => {
                let quoted = values
                    .iter()
                    .map(|value| format!("\"{}\"", value))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "enum [{}]", quoted)
            }
            Self::Reference(name) => write!(f, "{}", name),
            Self::TypedList(inner) => write!(f, "list of {}", inner),
        }
    }
}

/// A task parameter definition, optionally bound to a schema contract (v0.4.0).
///
/// `task apply_taxes(order as order_payload)` →
/// `ParamDef { name: "order", schema: Some("order_payload") }`
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDef {
    pub name: String,
    /// Schema contract — if `Some`, the argument is validated against this schema at call time.
    pub schema: Option<String>,
}

/// Function/method argument — positional or named.
#[derive(Debug, Clone, PartialEq)]
pub enum Argument {
    /// `func(value)`
    Positional(Expression),
    /// `func(limit: 10)`
    Named { name: String, value: Expression },
}

impl std::fmt::Display for BinaryOperator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Modulo => "%",
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Greater => ">",
            Self::Less => "<",
            Self::GreaterEqual => ">=",
            Self::LessEqual => "<=",
            Self::In => "in",
            Self::And => "and",
            Self::Or => "or",
        };
        write!(f, "{}", s)
    }
}

impl std::fmt::Display for UnaryOperator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Negate => "-",
            Self::Not => "not",
        };
        write!(f, "{}", s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assignment_node() {
        let stmt = Statement::Assignment {
            target: "x".into(),
            value: Expression::Integer(42),
            line: 1,
            column: 1,
        };
        if let Statement::Assignment { target, value, .. } = stmt {
            assert_eq!(target, "x");
            assert_eq!(value, Expression::Integer(42));
        } else {
            panic!("expected Assignment");
        }
    }

    #[test]
    fn test_conditional_assignment_node() {
        let stmt = Statement::ConditionalAssignment {
            target: "status".into(),
            value: Expression::StringLiteral("approved".into()),
            condition: Expression::BinaryOp {
                left: Box::new(Expression::Identifier("balance".into())),
                operator: BinaryOperator::Greater,
                right: Box::new(Expression::Integer(100)),
            },
            line: 1,
            column: 1,
        };
        if let Statement::ConditionalAssignment {
            target, condition, ..
        } = stmt
        {
            assert_eq!(target, "status");
            assert!(matches!(condition, Expression::BinaryOp { .. }));
        } else {
            panic!("expected ConditionalAssignment");
        }
    }

    #[test]
    fn test_require_node() {
        let stmt = Statement::Require {
            condition: Expression::Identifier("payload".into()),
            error_code: 400,
            error_message: "Cart is empty".into(),
            line: 5,
            column: 3,
        };
        if let Statement::Require {
            error_code,
            error_message,
            ..
        } = stmt
        {
            assert_eq!(error_code, 400);
            assert_eq!(error_message, "Cart is empty");
        } else {
            panic!("expected Require");
        }
    }

    #[test]
    fn test_reject_node() {
        let stmt = Statement::Reject {
            condition: Expression::PropertyAccess {
                object: Box::new(Expression::Identifier("client".into())),
                property: "delinquent".into(),
            },
            error_code: 402,
            error_message: "Payment pending".into(),
            line: 8,
            column: 3,
        };
        if let Statement::Reject { error_code, .. } = stmt {
            assert_eq!(error_code, 402);
        } else {
            panic!("expected Reject");
        }
    }

    #[test]
    fn test_task_def_inline() {
        let stmt = Statement::TaskDef {
            name: "double".into(),
            params: vec![ParamDef {
                name: "n".into(),
                schema: None,
            }],
            body: TaskBody::Inline(Expression::BinaryOp {
                left: Box::new(Expression::Identifier("n".into())),
                operator: BinaryOperator::Multiply,
                right: Box::new(Expression::Integer(2)),
            }),
            line: 1,
            column: 1,
        };
        if let Statement::TaskDef {
            name, params, body, ..
        } = stmt
        {
            assert_eq!(name, "double");
            assert_eq!(params[0].name, "n");
            assert_eq!(params[0].schema, None);
            assert!(matches!(body, TaskBody::Inline(_)));
        } else {
            panic!("expected TaskDef");
        }
    }

    #[test]
    fn test_task_def_block() {
        let body = TaskBody::Block(
            vec![Statement::Assignment {
                target: "base".into(),
                value: Expression::Float(1.15),
                line: 2,
                column: 5,
            }],
            Expression::Identifier("base".into()),
        );
        assert!(matches!(body, TaskBody::Block(_, _)));
    }

    #[test]
    fn test_match_expression() {
        let expr = Expression::Match {
            subject: Box::new(Expression::Identifier("tipo".into())),
            arms: vec![
                MatchArm {
                    pattern: MatchPattern::Literal(Expression::StringLiteral("VIP".into())),
                    value: Expression::Float(0.0),
                },
                MatchArm {
                    pattern: MatchPattern::Fallback,
                    value: Expression::Float(15.0),
                },
            ],
        };
        if let Expression::Match { arms, .. } = expr {
            assert_eq!(arms.len(), 2);
            assert!(matches!(arms[1].pattern, MatchPattern::Fallback));
        } else {
            panic!("expected Match");
        }
    }

    #[test]
    fn test_pipeline_expression() {
        let expr = Expression::Pipeline {
            input: Box::new(Expression::Identifier("items".into())),
            stages: vec![
                PipelineStage::Expression(Expression::Identifier("save".into())),
                PipelineStage::Map {
                    variable: "item".into(),
                    body: vec![MapStatement::Keep {
                        value: Expression::Identifier("item".into()),
                        condition: None,
                    }],
                },
            ],
        };
        if let Expression::Pipeline { stages, .. } = expr {
            assert_eq!(stages.len(), 2);
            assert!(matches!(stages[0], PipelineStage::Expression(_)));
            assert!(matches!(stages[1], PipelineStage::Map { .. }));
        } else {
            panic!("expected Pipeline");
        }
    }

    #[test]
    fn test_broadcast_expression() {
        let expr = Expression::Broadcast {
            input: Box::new(Expression::Identifier("orders".into())),
            targets: vec![
                Expression::Identifier("target_a".into()),
                Expression::Identifier("target_b".into()),
            ],
        };
        if let Expression::Broadcast { targets, .. } = expr {
            assert_eq!(targets.len(), 2);
        } else {
            panic!("expected Broadcast");
        }
    }

    #[test]
    fn test_argument_variants() {
        let pos = Argument::Positional(Expression::Integer(10));
        let named = Argument::Named {
            name: "limit".into(),
            value: Expression::Integer(10),
        };
        assert!(matches!(pos, Argument::Positional(_)));
        assert!(matches!(named, Argument::Named { .. }));
    }

    #[test]
    fn test_binary_operator_display() {
        assert_eq!(format!("{}", BinaryOperator::Add), "+");
        assert_eq!(format!("{}", BinaryOperator::Subtract), "-");
        assert_eq!(format!("{}", BinaryOperator::Multiply), "*");
        assert_eq!(format!("{}", BinaryOperator::Divide), "/");
        assert_eq!(format!("{}", BinaryOperator::Modulo), "%");
        assert_eq!(format!("{}", BinaryOperator::Equal), "==");
        assert_eq!(format!("{}", BinaryOperator::NotEqual), "!=");
        assert_eq!(format!("{}", BinaryOperator::Greater), ">");
        assert_eq!(format!("{}", BinaryOperator::Less), "<");
        assert_eq!(format!("{}", BinaryOperator::GreaterEqual), ">=");
        assert_eq!(format!("{}", BinaryOperator::LessEqual), "<=");
        assert_eq!(format!("{}", BinaryOperator::In), "in");
        assert_eq!(format!("{}", BinaryOperator::And), "and");
        assert_eq!(format!("{}", BinaryOperator::Or), "or");
    }

    #[test]
    fn test_unary_operator_display() {
        assert_eq!(format!("{}", UnaryOperator::Negate), "-");
        assert_eq!(format!("{}", UnaryOperator::Not), "not");
    }

    #[test]
    fn test_nested_binary_ops() {
        // (1 + 2) * 3
        let expr = Expression::BinaryOp {
            left: Box::new(Expression::BinaryOp {
                left: Box::new(Expression::Integer(1)),
                operator: BinaryOperator::Add,
                right: Box::new(Expression::Integer(2)),
            }),
            operator: BinaryOperator::Multiply,
            right: Box::new(Expression::Integer(3)),
        };
        assert!(matches!(expr, Expression::BinaryOp { .. }));
    }

    #[test]
    fn test_method_call_node() {
        let expr = Expression::MethodCall {
            object: Box::new(Expression::Identifier("name".into())),
            method: "upper".into(),
            arguments: vec![],
        };
        if let Expression::MethodCall {
            method, arguments, ..
        } = expr
        {
            assert_eq!(method, "upper");
            assert!(arguments.is_empty());
        } else {
            panic!("expected MethodCall");
        }
    }

    #[test]
    fn test_map_literal() {
        let expr = Expression::MapLiteral(vec![
            ("name".into(), Expression::StringLiteral("Ana".into())),
            ("age".into(), Expression::Integer(30)),
        ]);
        if let Expression::MapLiteral(pairs) = expr {
            assert_eq!(pairs.len(), 2);
            assert_eq!(pairs[0].0, "name");
        } else {
            panic!("expected MapLiteral");
        }
    }

    // --- Schema node tests ---

    #[test]
    fn test_schema_field_required() {
        let field = SchemaField {
            name: "name".into(),
            field_type: SchemaType::StringType,
            optional: false,
        };
        assert_eq!(field.name, "name");
        assert_eq!(field.field_type, SchemaType::StringType);
        assert!(!field.optional);
    }

    #[test]
    fn test_schema_field_optional() {
        let field = SchemaField {
            name: "email".into(),
            field_type: SchemaType::StringType,
            optional: true,
        };
        assert!(field.optional);
    }

    #[test]
    fn test_schema_type_display() {
        assert_eq!(SchemaType::StringType.to_string(), "string");
        assert_eq!(SchemaType::IntegerType.to_string(), "integer");
        assert_eq!(SchemaType::FloatType.to_string(), "float");
        assert_eq!(SchemaType::DecimalType.to_string(), "decimal");
        assert_eq!(SchemaType::BooleanType.to_string(), "boolean");
        assert_eq!(SchemaType::InstantType.to_string(), "instant");
        assert_eq!(SchemaType::DateType.to_string(), "date");
        assert_eq!(SchemaType::TimeType.to_string(), "time");
        assert_eq!(SchemaType::DurationType.to_string(), "duration");
        assert_eq!(SchemaType::IntervalType.to_string(), "interval");
        assert_eq!(SchemaType::ListType.to_string(), "list");
        assert_eq!(SchemaType::MapType.to_string(), "map");
        assert_eq!(
            SchemaType::Reference("address".into()).to_string(),
            "address"
        );
        assert_eq!(
            SchemaType::TypedList(Box::new(SchemaType::StringType)).to_string(),
            "list of string"
        );
        assert_eq!(
            SchemaType::TypedList(Box::new(SchemaType::Reference("order_item".into()))).to_string(),
            "list of order_item"
        );
        assert_eq!(
            SchemaType::EnumType(vec!["pending".into(), "paid".into()]).to_string(),
            "enum [\"pending\", \"paid\"]"
        );
    }

    #[test]
    fn test_raise_statement_node() {
        let stmt = Statement::Raise {
            message: Expression::StringLiteral("domain error".into()),
            condition: Some(Expression::Boolean(true)),
            line: 3,
            column: 1,
        };
        if let Statement::Raise {
            message, condition, ..
        } = stmt
        {
            assert_eq!(message, Expression::StringLiteral("domain error".into()));
            assert!(condition.is_some());
        } else {
            panic!("expected Raise");
        }
    }

    #[test]
    fn test_raise_statement_no_condition() {
        let stmt = Statement::Raise {
            message: Expression::StringLiteral("always raised".into()),
            condition: None,
            line: 1,
            column: 1,
        };
        if let Statement::Raise { condition, .. } = stmt {
            assert!(condition.is_none());
        }
    }

    #[test]
    fn test_transaction_statement_node() {
        let stmt = Statement::Transaction {
            body: vec![Statement::Assignment {
                target: "x".into(),
                value: Expression::Integer(1),
                line: 2,
                column: 3,
            }],
            line: 1,
            column: 1,
        };
        if let Statement::Transaction { body, .. } = stmt {
            assert_eq!(body.len(), 1);
        } else {
            panic!("expected Transaction");
        }
    }

    #[test]
    fn test_export_statement_node() {
        let inner = Statement::Assignment {
            target: "api_name".into(),
            value: Expression::StringLiteral("MyAPI".into()),
            line: 1,
            column: 1,
        };
        let stmt = Statement::Export(Box::new(inner));
        if let Statement::Export(inner) = stmt {
            assert!(matches!(*inner, Statement::Assignment { .. }));
        } else {
            panic!("expected Export");
        }
    }

    #[test]
    fn test_pipeline_rescue_stage() {
        let stage = PipelineStage::Rescue {
            handler: RescueHandler::Inline(Expression::Null),
        };
        assert!(matches!(stage, PipelineStage::Rescue { .. }));
    }

    #[test]
    fn test_rescue_handler_block() {
        let handler = RescueHandler::Block(vec![Statement::Assignment {
            target: "err".into(),
            value: Expression::Null,
            line: 1,
            column: 1,
        }]);
        if let RescueHandler::Block(stmts) = handler {
            assert_eq!(stmts.len(), 1);
        } else {
            panic!("expected Block");
        }
    }

    #[test]
    fn test_map_statement_skip() {
        let s = MapStatement::Skip {
            condition: Expression::Boolean(true),
        };
        if let MapStatement::Skip { condition } = s {
            assert_eq!(condition, Expression::Boolean(true));
        } else {
            panic!("expected Skip");
        }
    }

    #[test]
    fn test_param_def_with_schema() {
        let p = ParamDef {
            name: "order".into(),
            schema: Some("order_payload".into()),
        };
        assert_eq!(p.schema, Some("order_payload".to_string()));
    }

    #[test]
    fn test_rescue_expression_node() {
        let expr = Expression::Rescue {
            expr: Box::new(Expression::Integer(1)),
            handler: Box::new(Expression::Null),
        };
        assert!(matches!(expr, Expression::Rescue { .. }));
    }

    #[test]
    fn test_schema_statement_node() {
        let stmt = Statement::Schema {
            name: "UserPayload".into(),
            db_table: None,
            fields: vec![
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
                SchemaField {
                    name: "email".into(),
                    field_type: SchemaType::StringType,
                    optional: true,
                },
            ],
            line: 1,
            column: 1,
        };
        if let Statement::Schema { name, fields, .. } = stmt {
            assert_eq!(name, "UserPayload");
            assert_eq!(fields.len(), 3);
            assert!(!fields[0].optional);
            assert!(fields[2].optional);
        } else {
            panic!("expected Schema");
        }
    }

    #[test]
    fn test_route_with_schema_binding() {
        let stmt = Statement::Route {
            verb: HttpVerb::Post,
            path: "/users".into(),
            auth: None,
            allow: vec![],
            take: vec![TakeBinding {
                kind: TakeKind::Payload,
                name: "payload".into(),
                schema: Some("UserPayload".into()),
            }],
            body: vec![],
            line: 1,
            column: 1,
        };
        if let Statement::Route { take, .. } = stmt {
            assert_eq!(payload_schema(&take), Some("UserPayload"));
        } else {
            panic!("expected Route");
        }
    }

    // --- HTTP node tests ---

    #[test]
    fn test_http_verb_display() {
        assert_eq!(HttpVerb::Get.to_string(), "GET");
        assert_eq!(HttpVerb::Post.to_string(), "POST");
        assert_eq!(HttpVerb::Put.to_string(), "PUT");
        assert_eq!(HttpVerb::Patch.to_string(), "PATCH");
        assert_eq!(HttpVerb::Delete.to_string(), "DELETE");
    }

    #[test]
    fn test_http_verb_equality() {
        assert_eq!(HttpVerb::Get, HttpVerb::Get);
        assert_ne!(HttpVerb::Get, HttpVerb::Post);
    }

    #[test]
    fn test_take_binding_variants() {
        let p = TakeBinding {
            kind: TakeKind::Payload,
            name: "payload".into(),
            schema: None,
        };
        let q = TakeBinding {
            kind: TakeKind::Query,
            name: "query".into(),
            schema: None,
        };
        let h = TakeBinding {
            kind: TakeKind::Headers,
            name: "headers".into(),
            schema: None,
        };
        let f = TakeBinding {
            kind: TakeKind::Form,
            name: "form".into(),
            schema: None,
        };
        let r = TakeBinding {
            kind: TakeKind::Raw,
            name: "raw".into(),
            schema: None,
        };
        assert!(matches!(
            p,
            TakeBinding {
                kind: TakeKind::Payload,
                name: _,
                schema: None
            }
        ));
        assert!(matches!(
            q,
            TakeBinding {
                kind: TakeKind::Query,
                name: _,
                schema: None
            }
        ));
        assert!(matches!(
            h,
            TakeBinding {
                kind: TakeKind::Headers,
                name: _,
                schema: None
            }
        ));
        assert!(matches!(
            f,
            TakeBinding {
                kind: TakeKind::Form,
                name: _,
                schema: None
            }
        ));
        assert!(matches!(
            r,
            TakeBinding {
                kind: TakeKind::Raw,
                name: _,
                schema: None
            }
        ));
    }

    #[test]
    fn test_route_take_is_vec() {
        let stmt = Statement::Route {
            verb: HttpVerb::Post,
            path: "/checkout".into(),
            auth: None,
            allow: vec![],
            take: vec![
                TakeBinding {
                    kind: TakeKind::Payload,
                    name: "payload".into(),
                    schema: None,
                },
                TakeBinding {
                    kind: TakeKind::Headers,
                    name: "headers".into(),
                    schema: None,
                },
            ],
            body: vec![],
            line: 1,
            column: 1,
        };
        if let Statement::Route { take, .. } = stmt {
            assert_eq!(take.len(), 2);
            assert!(matches!(
                take[0],
                TakeBinding {
                    kind: TakeKind::Payload,
                    name: _,
                    schema: None
                }
            ));
            assert!(matches!(
                take[1],
                TakeBinding {
                    kind: TakeKind::Headers,
                    name: _,
                    schema: None
                }
            ));
        }
    }

    #[test]
    fn test_reply_content_type_variants() {
        assert_eq!(ReplyContentType::Json, ReplyContentType::Json);
        assert_ne!(ReplyContentType::Html, ReplyContentType::Text);
        assert_ne!(ReplyContentType::Json, ReplyContentType::Html);
    }

    #[test]
    fn test_reply_with_html_content_type() {
        let stmt = Statement::Reply {
            status_code: Expression::Integer(200),
            content_type: ReplyContentType::Html,
            body: Expression::StringLiteral("<h1>Hello</h1>".into()),
            response_schema: None,
            extra_headers: None,
            line: 1,
            column: 1,
        };
        if let Statement::Reply { content_type, .. } = stmt {
            assert_eq!(content_type, ReplyContentType::Html);
        }
    }

    #[test]
    fn test_reply_with_extra_headers() {
        let stmt = Statement::Reply {
            status_code: Expression::Integer(302),
            content_type: ReplyContentType::Json,
            body: Expression::Null,
            response_schema: None,
            extra_headers: Some(Expression::MapLiteral(vec![(
                "Location".into(),
                Expression::StringLiteral("https://example.com".into()),
            )])),
            line: 1,
            column: 1,
        };
        if let Statement::Reply { extra_headers, .. } = stmt {
            assert!(extra_headers.is_some());
        }
    }

    #[test]
    fn test_route_statement_node() {
        let stmt = Statement::Route {
            verb: HttpVerb::Get,
            path: "/users/:id".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            body: vec![],
            line: 1,
            column: 1,
        };
        if let Statement::Route {
            verb, path, take, ..
        } = stmt
        {
            assert_eq!(verb, HttpVerb::Get);
            assert_eq!(path, "/users/:id");
            assert!(take.is_empty());
        } else {
            panic!("expected Route");
        }
    }

    #[test]
    fn test_route_with_take_payload() {
        let stmt = Statement::Route {
            verb: HttpVerb::Post,
            path: "/users".into(),
            auth: None,
            allow: vec![],
            take: vec![TakeBinding {
                kind: TakeKind::Payload,
                name: "payload".into(),
                schema: None,
            }],
            body: vec![],
            line: 1,
            column: 1,
        };
        if let Statement::Route { verb, take, .. } = stmt {
            assert_eq!(verb, HttpVerb::Post);
            assert!(matches!(
                take.first(),
                Some(TakeBinding {
                    kind: TakeKind::Payload,
                    name: _,
                    schema: None
                })
            ));
        } else {
            panic!("expected Route");
        }
    }

    #[test]
    fn test_reply_statement_node() {
        let stmt = Statement::Reply {
            status_code: Expression::Integer(200),
            content_type: ReplyContentType::Json,
            body: Expression::Integer(42),
            response_schema: None,
            extra_headers: None,
            line: 5,
            column: 3,
        };
        if let Statement::Reply {
            status_code,
            body,
            line,
            ..
        } = stmt
        {
            assert_eq!(status_code, Expression::Integer(200));
            assert_eq!(body, Expression::Integer(42));
            assert_eq!(line, 5);
        } else {
            panic!("expected Reply");
        }
    }

    #[test]
    fn test_fail_statement_node() {
        let stmt = Statement::Fail {
            status_code: 404,
            message: Expression::StringLiteral("Not found".into()),
            line: 7,
            column: 1,
        };
        if let Statement::Fail {
            status_code,
            message,
            ..
        } = stmt
        {
            assert_eq!(status_code, 404);
            assert_eq!(message, Expression::StringLiteral("Not found".into()));
        } else {
            panic!("expected Fail");
        }
    }

    #[test]
    fn test_route_body_contains_statements() {
        let reply = Statement::Reply {
            status_code: Expression::Integer(200),
            content_type: ReplyContentType::Json,
            body: Expression::Null,
            response_schema: None,
            extra_headers: None,
            line: 2,
            column: 5,
        };
        let stmt = Statement::Route {
            verb: HttpVerb::Get,
            path: "/health".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            body: vec![reply],
            line: 1,
            column: 1,
        };
        if let Statement::Route { body, .. } = stmt {
            assert_eq!(body.len(), 1);
        } else {
            panic!("expected Route");
        }
    }
}
