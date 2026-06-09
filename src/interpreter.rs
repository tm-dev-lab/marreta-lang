use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::ptr::NonNull;
use std::sync::{Arc, RwLock};

use base64::Engine as _;
use base64::engine::general_purpose::{
    STANDARD as BASE64_STANDARD, STANDARD_NO_PAD as BASE64_STANDARD_NO_PAD,
    URL_SAFE as BASE64_URL_SAFE, URL_SAFE_NO_PAD as BASE64_URL_SAFE_NO_PAD,
};
use chrono::{
    Datelike, Duration as ChronoDuration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime,
    SecondsFormat, TimeZone, Timelike, Utc,
};
use chrono_tz::Tz;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::ast::*;
use crate::cache::CacheConfig;
use crate::cache::driver::CacheDriver;
use crate::db::DbEngine;
use crate::db::driver::{DbRow, DbTx, FilterClause, FilterOp, QueryState};
use crate::doc::DocEngine;
use crate::environment::Environment;
use crate::error::MarretaError;
use crate::feature_flags::{FEATURE_NAME_HELP, FeatureFlags, is_valid_feature_name};
use crate::file_loader::ProjectRuntime;
use crate::http_client::driver::{
    HttpClient, HttpMethod, HttpRequest, HttpResponse as HttpClientResponse,
};
use crate::queue::driver::{QueueDriver, QueueMessage};
use crate::route_loader::SchemaDefinition;
use crate::runtime_profile::{self, ProfilePhase, RouteProfile};
use crate::trace_context::TraceContext;
use crate::validator::coerce_payload;
use crate::value::{
    RelationCardinality, RelationHandle, TemporalInterval, TemporalValue, Value, ValueMap,
    json_to_value, value_to_json_strict,
};

#[derive(Debug, Clone, PartialEq)]
pub enum FrameLabel {
    Route { verb: HttpVerb, path: Arc<str> },
    Task(Arc<str>),
    Consumer { kind: Arc<str>, target: Arc<str> },
    Op(Arc<str>),
    Raw(Arc<str>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum FrameKind {
    Route,
    Task,
    Consumer,
    Op,
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

impl FrameLabel {
    fn raw(label: impl Into<Arc<str>>) -> Self {
        Self::Raw(label.into())
    }

    fn kind(&self) -> FrameKind {
        match self {
            Self::Route { .. } => FrameKind::Route,
            Self::Task(_) => FrameKind::Task,
            Self::Consumer { .. } => FrameKind::Consumer,
            Self::Op(_) => FrameKind::Op,
            Self::Raw(_) => FrameKind::Raw,
        }
    }

    fn render(&self) -> String {
        match self {
            Self::Route { verb, path } => format!("route {} {}", verb, path),
            Self::Task(name) => format!("task {}", name),
            Self::Consumer { kind, target } => format!("consumer {} {}", kind, target),
            Self::Op(op) | Self::Raw(op) => op.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarretaFrame {
    pub label: FrameLabel,
    pub source_module: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl MarretaFrame {
    pub fn new(
        label: impl Into<String>,
        source_module: Option<String>,
        line: Option<usize>,
        column: Option<usize>,
    ) -> Self {
        Self::with_label(FrameLabel::raw(label.into()), source_module, line, column)
    }

    pub fn route(
        verb: HttpVerb,
        path: impl Into<Arc<str>>,
        source_module: Option<String>,
        line: Option<usize>,
        column: Option<usize>,
    ) -> Self {
        Self::with_label(
            FrameLabel::Route {
                verb,
                path: path.into(),
            },
            source_module,
            line,
            column,
        )
    }

    pub fn task(
        name: impl Into<Arc<str>>,
        source_module: Option<String>,
        line: Option<usize>,
        column: Option<usize>,
    ) -> Self {
        Self::with_label(FrameLabel::Task(name.into()), source_module, line, column)
    }

    pub fn consumer(
        kind: impl Into<Arc<str>>,
        target: impl Into<Arc<str>>,
        source_module: Option<String>,
        line: Option<usize>,
        column: Option<usize>,
    ) -> Self {
        Self::with_label(
            FrameLabel::Consumer {
                kind: kind.into(),
                target: target.into(),
            },
            source_module,
            line,
            column,
        )
    }

    pub fn operation(
        op: impl Into<Arc<str>>,
        source_module: Option<String>,
        line: Option<usize>,
        column: Option<usize>,
    ) -> Self {
        Self::with_label(FrameLabel::Op(op.into()), source_module, line, column)
    }

    fn with_label(
        label: FrameLabel,
        source_module: Option<String>,
        line: Option<usize>,
        column: Option<usize>,
    ) -> Self {
        Self {
            label,
            source_module,
            line,
            column,
        }
    }

    pub fn render(&self) -> String {
        let label = self.label.render();
        match (&self.source_module, self.line) {
            (Some(module), Some(line)) if line > 0 => {
                format!("at {} ({}.marreta:{})", label, module, line)
            }
            _ => format!("at {}", label),
        }
    }
}

#[must_use = "trace frame guards must be kept alive for the full traced operation"]
/// Crate-internal only: the guard holds a raw pointer into an `Interpreter`'s
/// trace stack and is sound only while that interpreter outlives it. It is never
/// exposed across the crate boundary, and all construction sites (the `enter_*`
/// methods) keep the interpreter alive for the guard's scope.
pub(crate) struct TraceFrameGuard {
    frames: NonNull<Vec<MarretaFrame>>,
    depth: usize,
    expected_kind: FrameKind,
    active: bool,
}

impl TraceFrameGuard {
    fn new(frames: &mut Vec<MarretaFrame>, depth: usize, expected_kind: FrameKind) -> Self {
        Self {
            frames: NonNull::from(frames),
            depth,
            expected_kind,
            active: true,
        }
    }

    pub(crate) fn preserve(mut self) {
        self.active = false;
    }
}

impl Drop for TraceFrameGuard {
    fn drop(&mut self) {
        if self.active {
            // SAFETY: the guard is only ever created from an Interpreter's own
            // trace stack, and call sites keep that interpreter (and thus the
            // `frames` Vec) alive until the guard is dropped. The pointer is
            // therefore valid and uniquely owned here — no other reference to
            // the Vec exists for the duration of this `&mut`.
            unsafe {
                let frames = self.frames.as_mut();
                debug_assert_eq!(
                    frames.get(self.depth).map(|frame| frame.label.kind()),
                    Some(self.expected_kind),
                    "trace frame guard popped a different frame kind than it pushed"
                );
                frames.truncate(self.depth);
            }
        }
    }
}

/// Tree-walking interpreter for MarretaLang.
///
/// Recursively traverses the AST and executes each node,
/// maintaining variable scopes via `Environment`.
///
/// `Clone` is implemented to support parallel `*>>` execution: each branch
/// receives an independent fork of the interpreter at the point of broadcast,
/// so branches cannot observe each other's side effects.
/// Wrapper around an optional in-flight `DbTx` that is `Clone`-safe.
///
/// When an `Interpreter` is cloned for `*>>` broadcast, the clone gets an
/// empty holder (`None`) so the spawned branch never sees the parent's
/// transaction — which is intentional: `*>>` inside `transaction` is
/// already rejected at runtime.
struct TxHolder(Arc<std::sync::Mutex<Option<Box<dyn DbTx + Send>>>>);

#[derive(Debug, Clone, Copy)]
struct MathNumericValue {
    value: f64,
    is_float: bool,
}

impl Clone for TxHolder {
    fn clone(&self) -> Self {
        TxHolder(Arc::new(std::sync::Mutex::new(None)))
    }
}

impl Default for TxHolder {
    fn default() -> Self {
        TxHolder(Arc::new(std::sync::Mutex::new(None)))
    }
}

#[derive(Clone)]
pub struct Interpreter {
    env: Environment,
    current_line: usize,
    current_column: usize,
    call_depth: usize,
    max_recursion_depth: usize,
    current_module: Option<String>,
    project_runtime: Option<Arc<ProjectRuntime>>,
    /// Schema registry injected at serve time, used by `reply … as schema_name`.
    schemas: Option<Arc<HashMap<String, SchemaDefinition>>>,
    /// DB engine injected at serve time. `None` when no DB is configured.
    db_engine: Option<DbEngine>,
    /// Doc engine injected at serve time. `None` when no Doc DB is configured.
    doc_engine: Option<DocEngine>,
    /// Queue driver injected at serve time. `None` when no queue is configured.
    queue_driver: Option<Arc<dyn QueueDriver>>,
    /// Cache driver injected at serve time. `None` when no cache is configured.
    cache_driver: Option<Arc<dyn CacheDriver>>,
    /// Cache config (prefix, default_ttl) for the interpreter to apply at eval time.
    cache_config: Option<CacheConfig>,
    /// HTTP client driver injected at serve time. Always available (no external dependency).
    http_client_driver: Option<Arc<dyn HttpClient>>,
    /// Active W3C Trace Context for the current HTTP request, if any.
    trace_context: Option<TraceContext>,
    /// Feature flag snapshot used when no project runtime is attached.
    feature_flags: FeatureFlags,
    /// Active route profiling bucket, if hot-path profiling is enabled.
    runtime_profile_route: Option<Arc<RouteProfile>>,
    /// Guards against `*>>` (broadcast) inside a `transaction` block at runtime.
    inside_transaction: bool,
    /// Active database transaction, if any. Held across DB calls within a `transaction` block.
    active_tx: TxHolder,
    trace_frames: Vec<MarretaFrame>,
}

/// Conservative purity check for a broadcast branch body: returns `true` if the
/// expression may have side effects or cannot be proven pure. Used to decide
/// whether broadcast branches can run sequentially (no OS-thread spawn). Calls,
/// queue ops, nested broadcast, schema constructors, rescue, pipelines, string
/// interpolation, and access to the db/doc/cache/queue/http_client namespaces
/// all count as "not provably pure".
fn expression_has_side_effects(expr: &Expression) -> bool {
    match expr {
        Expression::Integer(_)
        | Expression::Float(_)
        | Expression::Boolean(_)
        | Expression::Null => false,
        Expression::StringLiteral(s) => s.contains("#{"),
        Expression::Identifier(name) => {
            matches!(
                name.as_str(),
                "db" | "doc" | "cache" | "queue" | "http_client"
            )
        }
        Expression::List(items) => items.iter().any(expression_has_side_effects),
        Expression::MapLiteral(entries) => {
            entries.iter().any(|(_, v)| expression_has_side_effects(v))
        }
        Expression::BinaryOp { left, right, .. } => {
            expression_has_side_effects(left) || expression_has_side_effects(right)
        }
        Expression::UnaryOp { operand, .. } => expression_has_side_effects(operand),
        Expression::PropertyAccess { object, .. } => expression_has_side_effects(object),
        Expression::Subscript { object, key } => {
            expression_has_side_effects(object) || expression_has_side_effects(key)
        }
        Expression::MethodCall {
            object, arguments, ..
        } => expression_has_side_effects(object) || arguments.iter().any(argument_has_side_effects),
        // Anything else (function/task calls, control flow, queue ops, nested
        // broadcast, schema constructors, rescue, pipelines, ...) is treated as
        // not provably pure.
        _ => true,
    }
}

fn argument_has_side_effects(arg: &Argument) -> bool {
    match arg {
        Argument::Positional(expr) | Argument::Named { value: expr, .. } => {
            expression_has_side_effects(expr)
        }
    }
}

/// A task body is provably pure when it is a pure inline expression, or a block
/// with no statements whose return expression is pure. Block bodies with
/// statements are conservatively treated as not provably pure.
fn task_body_is_pure(body: &TaskBody) -> bool {
    match body {
        TaskBody::Inline(expr) => !expression_has_side_effects(expr),
        TaskBody::Block(stmts, ret) => stmts.is_empty() && !expression_has_side_effects(ret),
    }
}

/// Awaits a list of `JoinHandle`s in order, returning results.
/// Declared as a standalone async fn so the compiler can infer the Future type
/// without pulling in the `futures` crate.
async fn collect_join_handles<T>(
    handles: Vec<tokio::task::JoinHandle<T>>,
) -> Vec<Result<T, tokio::task::JoinError>> {
    let mut out = Vec::with_capacity(handles.len());
    for h in handles {
        out.push(h.await);
    }
    out
}

/// Drives an async future from synchronous code, compatible with both tokio
/// worker threads (uses `block_in_place`) and non-tokio threads (creates a
/// temporary runtime). Unlike `Interpreter::block_db`, this is a free function
/// that does not borrow `self`, enabling the take-and-restore tx pattern.
fn run_async<F, T>(fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => tokio::runtime::Runtime::new()
            .expect("failed to create tokio runtime")
            .block_on(fut),
    }
}

mod access;
mod infra;
mod namespaces;
mod operators;
mod pipeline;
impl Interpreter {
    fn default_max_recursion_depth() -> usize {
        std::env::var("MARRETA_MAX_RECURSION_DEPTH")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(500)
    }

    /// Creates a new interpreter with an empty global scope.
    pub fn new() -> Self {
        Self {
            env: Environment::new(),
            current_line: 0,
            current_column: 0,
            call_depth: 0,
            max_recursion_depth: Self::default_max_recursion_depth(),
            current_module: None,
            project_runtime: None,
            schemas: None,
            db_engine: None,
            doc_engine: None,
            queue_driver: None,
            cache_driver: None,
            cache_config: None,
            http_client_driver: None,
            trace_context: None,
            feature_flags: FeatureFlags::default(),
            runtime_profile_route: None,
            inside_transaction: false,
            active_tx: TxHolder::default(),
            trace_frames: Vec::new(),
        }
    }

    /// Injects the schema registry so `reply … as schema_name` can apply response serialization.
    pub fn with_schemas(mut self, schemas: Arc<HashMap<String, SchemaDefinition>>) -> Self {
        self.schemas = Some(schemas);
        self
    }

    pub fn with_project_runtime(mut self, runtime: Arc<ProjectRuntime>) -> Self {
        self.project_runtime = Some(runtime);
        self
    }

    pub fn with_current_module(mut self, module_id: Option<String>) -> Self {
        self.current_module = module_id;
        self
    }

    /// Injects the DB engine so `db.*` calls can dispatch to the active driver.
    pub fn with_db(mut self, engine: DbEngine) -> Self {
        self.db_engine = Some(engine);
        self
    }

    /// Injects the Doc engine so `doc.*` calls can dispatch to the active driver.
    pub fn with_doc(mut self, engine: DocEngine) -> Self {
        self.doc_engine = Some(engine);
        self
    }

    /// Injects the queue driver so `queue.push` / `topic.publish` can dispatch to the broker.
    pub fn with_queue(mut self, driver: Arc<dyn QueueDriver>) -> Self {
        self.queue_driver = Some(driver);
        self
    }

    /// Injects the cache driver and config so `cache.*` calls can dispatch.
    pub fn with_cache(mut self, driver: Arc<dyn CacheDriver>, config: CacheConfig) -> Self {
        self.cache_driver = Some(driver);
        self.cache_config = Some(config);
        self
    }

    /// Injects the HTTP client driver so `http_client.*` calls can dispatch.
    pub fn with_http_client(mut self, driver: Arc<dyn HttpClient>) -> Self {
        self.http_client_driver = Some(driver);
        self
    }

    /// Injects the active runtime trace context for request-scoped logs and HTTP calls.
    pub fn with_trace_context(mut self, trace_context: TraceContext) -> Self {
        self.trace_context = Some(trace_context);
        self
    }

    pub fn with_feature_flags(mut self, feature_flags: FeatureFlags) -> Self {
        self.feature_flags = feature_flags;
        self
    }

    pub fn with_runtime_profile(mut self, route_profile: Option<Arc<RouteProfile>>) -> Self {
        self.runtime_profile_route = route_profile;
        self
    }

    pub fn trace_context(&self) -> Option<&TraceContext> {
        self.trace_context.as_ref()
    }

    /// Returns a reference to the environment (for REPL introspection).
    pub fn environment(&self) -> &Environment {
        &self.env
    }

    /// Creates an interpreter pre-loaded with an existing environment (for HTTP request isolation).
    pub fn from_environment(env: Environment) -> Self {
        Self {
            env,
            current_line: 0,
            current_column: 0,
            call_depth: 0,
            max_recursion_depth: Self::default_max_recursion_depth(),
            current_module: None,
            project_runtime: None,
            schemas: None,
            db_engine: None,
            doc_engine: None,
            queue_driver: None,
            cache_driver: None,
            cache_config: None,
            http_client_driver: None,
            trace_context: None,
            feature_flags: FeatureFlags::default(),
            runtime_profile_route: None,
            inside_transaction: false,
            active_tx: TxHolder::default(),
            trace_frames: Vec::new(),
        }
    }

    /// Consumes the interpreter and returns its environment (for sharing across requests).
    pub fn into_environment(self) -> Environment {
        self.env
    }

    /// Sets a variable directly in the current scope (used to inject request context).
    pub fn env_set(&mut self, name: String, value: Value) {
        self.env.set(name, value);
    }

    fn schema_registry_for(&self, module_id: Option<&str>) -> HashMap<String, SchemaDefinition> {
        if let Some(runtime) = &self.project_runtime {
            return runtime.visible_schemas_for(module_id);
        }

        self.schemas.as_deref().cloned().unwrap_or_default()
    }

    fn resolve_schema(&self, schema_name: &str) -> Option<SchemaDefinition> {
        if let Some(runtime) = &self.project_runtime {
            if let Some(schema) =
                runtime.resolve_schema(self.current_module.as_deref(), schema_name)
            {
                return Some(schema);
            }
            return runtime.persistent_schemas.get(schema_name).cloned();
        }

        self.schemas
            .as_deref()
            .and_then(|schemas| schemas.get(schema_name).cloned())
    }

    fn constructor_schema_registry(
        &self,
        schema_name: &str,
    ) -> (HashMap<String, SchemaDefinition>, Option<SchemaDefinition>) {
        let mut schemas = self.schema_registry_for(self.current_module.as_deref());
        if !schemas.contains_key(schema_name)
            && let Some(schema) = self.resolve_schema(schema_name)
        {
            schemas.insert(schema_name.to_string(), schema);
        }

        for schema in schemas.values_mut() {
            if schema.db_table.is_some() {
                for field in &mut schema.fields {
                    if field.name == "id" {
                        field.optional = true;
                    }
                }
            }
        }

        let schema = schemas.get(schema_name).cloned();
        (schemas, schema)
    }

    fn validation_error_detail(err: MarretaError) -> String {
        match err {
            MarretaError::HttpResponse { body, .. } => {
                serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|json| json["error"].as_str().map(ToString::to_string))
                    .unwrap_or(body)
            }
            other => other.to_string(),
        }
    }

    fn fork_with_env(&self, env: Environment, current_module: Option<String>) -> Self {
        Self {
            env,
            current_line: self.current_line,
            current_column: self.current_column,
            call_depth: self.call_depth,
            max_recursion_depth: self.max_recursion_depth,
            current_module,
            project_runtime: self.project_runtime.clone(),
            schemas: self.schemas.clone(),
            db_engine: self.db_engine.clone(),
            doc_engine: self.doc_engine.clone(),
            queue_driver: self.queue_driver.clone(),
            cache_driver: self.cache_driver.clone(),
            cache_config: self.cache_config.clone(),
            http_client_driver: self.http_client_driver.clone(),
            trace_context: self.trace_context.clone(),
            feature_flags: self.feature_flags.clone(),
            runtime_profile_route: self.runtime_profile_route.clone(),
            inside_transaction: self.inside_transaction,
            active_tx: TxHolder(Arc::clone(&self.active_tx.0)),
            trace_frames: self.trace_frames.clone(),
        }
    }

    fn push_trace_frame(&mut self, frame: MarretaFrame) -> TraceFrameGuard {
        let depth = self.trace_frames.len();
        let expected_kind = frame.label.kind();
        self.trace_frames.push(frame);
        TraceFrameGuard::new(&mut self.trace_frames, depth, expected_kind)
    }

    pub(crate) fn enter_route(
        &mut self,
        verb: &HttpVerb,
        path: &str,
        module_id: Option<String>,
        line: usize,
        column: usize,
    ) -> TraceFrameGuard {
        self.push_trace_frame(MarretaFrame::route(
            verb.clone(),
            path,
            module_id,
            Some(line),
            Some(column),
        ))
    }

    pub(crate) fn enter_consumer(
        &mut self,
        kind: &str,
        target: &str,
        module_id: Option<String>,
        line: usize,
        column: usize,
    ) -> TraceFrameGuard {
        self.push_trace_frame(MarretaFrame::consumer(
            kind,
            target,
            module_id,
            Some(line),
            Some(column),
        ))
    }

    pub(crate) fn enter_task(
        &mut self,
        name: &str,
        source_module: Option<String>,
        line: usize,
        column: usize,
    ) -> TraceFrameGuard {
        self.push_trace_frame(MarretaFrame::task(
            name,
            source_module,
            Some(line),
            Some(column),
        ))
    }

    pub fn uncaught_trace_lines(&self, err: &MarretaError) -> Vec<String> {
        let mut frames = self.trace_frames.clone();
        if let Some(operation_label) = err.trace_operation_label() {
            frames.push(MarretaFrame::operation(
                operation_label,
                self.current_module.clone(),
                Some(self.current_line),
                Some(self.current_column),
            ));
        }
        frames.into_iter().map(|f| f.render()).collect()
    }

    /// Executes a full program (list of statements).
    /// Returns the value of the last expression statement, or Null.
    pub fn execute(&mut self, program: &Program) -> Result<Value, MarretaError> {
        let mut last = Value::Null;
        for stmt in program {
            last = self.execute_statement(stmt)?;
        }
        Ok(last)
    }

    /// Evaluates an expression — public wrapper used by the queue consumer runner.
    pub fn evaluate_pub(&mut self, expr: &Expression) -> Result<Value, MarretaError> {
        self.evaluate(expr)
    }

    /// Executes a slice of statements — public wrapper used by the queue consumer runner.
    pub fn execute_stmts_pub(&mut self, stmts: &[Statement]) -> Result<Value, MarretaError> {
        let mut last = Value::Null;
        for stmt in stmts {
            last = self.execute_statement(stmt)?;
        }
        Ok(last)
    }

    fn evaluate_branch_body(&mut self, body: &TaskBody) -> Result<Value, MarretaError> {
        let saved_env = self.env.clone();
        let result = (|| match body {
            TaskBody::Inline(expr) => self.evaluate(expr),
            TaskBody::Block(stmts, return_expr) => {
                for stmt in stmts {
                    self.execute_statement(stmt)?;
                }
                self.evaluate(return_expr)
            }
        })();
        self.env = saved_env;
        result
    }

    // =========================================================================
    // Statements
    // =========================================================================

    fn execute_statement(&mut self, stmt: &Statement) -> Result<Value, MarretaError> {
        // Update position tracking from statement line/column
        match stmt {
            Statement::Assignment { line, column, .. }
            | Statement::ConditionalAssignment { line, column, .. }
            | Statement::Require { line, column, .. }
            | Statement::Reject { line, column, .. }
            | Statement::While { line, column, .. }
            | Statement::TaskDef { line, column, .. }
            | Statement::Route { line, column, .. }
            | Statement::Reply { line, column, .. }
            | Statement::Fail { line, column, .. }
            | Statement::Schema { line, column, .. }
            | Statement::AuthProvider { line, column, .. }
            | Statement::Transaction { line, column, .. }
            | Statement::Raise { line, column, .. }
            | Statement::OnQueue { line, column, .. }
            | Statement::OnTopic { line, column, .. }
            | Statement::ExpressionStatement { line, column, .. }
            | Statement::Nack { line, column, .. }
            | Statement::Scenario { line, column, .. } => {
                self.current_line = *line;
                self.current_column = *column;
            }
            Statement::Export(inner) => {
                // Position tracking delegates to the inner statement
                match inner.as_ref() {
                    Statement::Assignment { line, column, .. }
                    | Statement::TaskDef { line, column, .. }
                    | Statement::Schema { line, column, .. }
                    | Statement::ExpressionStatement { line, column, .. } => {
                        self.current_line = *line;
                        self.current_column = *column;
                    }
                    _ => {}
                }
            }
        }

        match stmt {
            Statement::Assignment { target, value, .. } => {
                let val = self.evaluate(value)?;
                self.env.update(target.clone(), val);
                Ok(Value::Null)
            }

            Statement::ConditionalAssignment {
                target,
                value,
                condition,
                ..
            } => {
                let cond = self.evaluate(condition)?;
                if cond.is_truthy() {
                    let val = self.evaluate(value)?;
                    self.env.update(target.clone(), val);
                }
                Ok(Value::Null)
            }

            Statement::Require {
                condition,
                error_code,
                error_message,
                ..
            } => {
                let val = self.evaluate(condition)?;
                if !val.is_truthy() {
                    return Err(MarretaError::HttpError {
                        status_code: *error_code,
                        message: error_message.clone(),
                    });
                }
                Ok(Value::Null)
            }

            Statement::Reject {
                condition,
                error_code,
                error_message,
                ..
            } => {
                let val = self.evaluate(condition)?;
                if val.is_truthy() {
                    return Err(MarretaError::HttpError {
                        status_code: *error_code,
                        message: error_message.clone(),
                    });
                }
                Ok(Value::Null)
            }

            Statement::While {
                condition, body, ..
            } => {
                let mut iterations = 0usize;
                while self.evaluate(condition)?.is_truthy() {
                    if iterations >= 10_000 {
                        return Err(MarretaError::RuntimeError {
                            message: "while loop exceeded the maximum of 10000 iterations"
                                .to_string(),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                    for stmt in body {
                        self.execute_statement(stmt)?;
                    }
                    iterations += 1;
                }
                Ok(Value::Null)
            }

            Statement::TaskDef {
                name,
                params,
                body,
                line,
                column,
            } => {
                let task = Value::Task {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    owner_module: self.current_module.clone(),
                    source_module: self.current_module.clone(),
                    line: *line,
                    column: *column,
                };
                self.env.set(name.clone(), task);
                Ok(Value::Null)
            }

            Statement::ExpressionStatement { expression, .. } => self.evaluate(expression),

            // Route and Schema declarations are collected by the route loader, not executed directly.
            Statement::Route { .. } => Ok(Value::Null),
            Statement::Schema { .. } => Ok(Value::Null),
            Statement::AuthProvider { .. } => Ok(Value::Null),
            Statement::Scenario { .. } => Ok(Value::Null),

            // Export wraps a task, schema, or assignment. At startup the inner statement is
            // executed directly — the export semantics (global visibility) are handled by the
            // multi-file loader, not by the interpreter.
            Statement::Export(inner) => self.execute_statement(inner),

            Statement::Transaction { body, .. } => {
                if self.inside_transaction {
                    return Err(MarretaError::TypeError {
                        message: "nested transaction blocks are not allowed".to_string(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
                let engine = self.require_db_engine()?;

                // Acquire a dedicated connection for the transaction. All DB
                // operations inside the block will use this same connection via
                // `active_tx`, ensuring BEGIN/DML/COMMIT all share one session.
                let tx =
                    self.block_db(engine.driver.begin())
                        .map_err(|e| MarretaError::TypeError {
                            message: format!("transaction BEGIN failed: {e}"),
                            line: self.current_line,
                            column: self.current_column,
                        })?;
                self.restore_tx(tx);
                self.inside_transaction = true;

                let result = (|| {
                    for stmt in body {
                        self.execute_statement(stmt)?;
                    }
                    Ok::<(), MarretaError>(())
                })();

                self.inside_transaction = false;

                if let Err(e) = result {
                    // Rollback — take the tx so no other code can use it
                    let tx = self.take_tx();
                    let _ = run_async(async move { tx.rollback().await });
                    return Err(e);
                }

                let tx = self.take_tx();
                run_async(async move { tx.commit().await }).map_err(|e| {
                    MarretaError::TypeError {
                        message: format!("transaction COMMIT failed: {e}"),
                        line: self.current_line,
                        column: self.current_column,
                    }
                })?;
                Ok(Value::Null)
            }

            Statement::Reply {
                status_code,
                content_type,
                body,
                response_schema,
                extra_headers,
                ..
            } => {
                let status_val = self.evaluate(status_code)?;
                let status_int = match status_val {
                    Value::Integer(n) => n,
                    other => {
                        return Err(MarretaError::TypeError {
                            message: format!(
                                "reply status must be Integer, got {}",
                                other.type_name()
                            ),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                };
                let val = self.evaluate(body)?;
                // Apply response schema serialization if bound via `reply … as schema_name`
                let val = if let Some(schema_name) = response_schema {
                    if let Some(schema_def) = self.resolve_schema(schema_name) {
                        crate::response_serializer::serialize(val, &schema_def)
                    } else {
                        val
                    }
                } else {
                    val
                };
                let (body_str, mime) = match content_type {
                    ReplyContentType::Json => {
                        let _timer = runtime_profile::timer(
                            self.runtime_profile_route.as_ref(),
                            ProfilePhase::JsonSerialize,
                        );
                        let json = crate::value::value_to_json_string(&val);
                        (json, "application/json".to_string())
                    }
                    ReplyContentType::Html => {
                        (val.to_string(), "text/html; charset=utf-8".to_string())
                    }
                    ReplyContentType::Text => {
                        (val.to_string(), "text/plain; charset=utf-8".to_string())
                    }
                };
                // Evaluate optional extra headers map
                let hdrs = if let Some(hdr_expr) = extra_headers {
                    let hdr_val = self.evaluate(hdr_expr)?;
                    if let Value::Map(m) = hdr_val {
                        m.read()
                            .unwrap()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.to_string()))
                            .collect()
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                };
                Err(MarretaError::HttpResponse {
                    status_code: status_int as u16,
                    body: body_str,
                    content_type: mime,
                    extra_headers: hdrs,
                    is_error: false,
                })
            }

            Statement::Fail {
                status_code,
                message,
                ..
            } => {
                let msg = self.evaluate(message)?;
                // If the message is a Map or List, serialize it directly as JSON body.
                // Otherwise wrap it in {"error": "..."}.
                let body = match &msg {
                    Value::Map(_) | Value::List(_) => {
                        let _timer = runtime_profile::timer(
                            self.runtime_profile_route.as_ref(),
                            ProfilePhase::JsonSerialize,
                        );
                        crate::value::value_to_json_string(&msg)
                    }
                    _ => serde_json::json!({ "error": msg.to_string() }).to_string(),
                };
                Err(MarretaError::HttpResponse {
                    status_code: *status_code as u16,
                    body,
                    content_type: "application/json".to_string(),
                    extra_headers: vec![],
                    is_error: true,
                })
            }

            Statement::Raise {
                message, condition, ..
            } => {
                if let Some(cond_expr) = condition {
                    let cond_val = self.evaluate(cond_expr)?;
                    if !cond_val.is_truthy() {
                        return Ok(Value::Null);
                    }
                }
                let msg = self.evaluate(message)?;
                Err(MarretaError::RaiseError {
                    message: msg.to_string(),
                })
            }

            // ── Queue (v0.8) — execution stubs; full implementation in Phase 4 ──
            Statement::OnQueue { .. } | Statement::OnTopic { .. } => {
                // Consumers are registered at startup by the queue runtime, not
                // executed inline. Reaching this branch means the statement was
                // used in a non-startup context — treat as no-op for now.
                Ok(Value::Null)
            }

            Statement::Nack {
                requeue, condition, ..
            } => {
                if let Some(cond_expr) = condition {
                    let cond_val = self.evaluate(cond_expr)?;
                    if !cond_val.is_truthy() {
                        return Ok(Value::Null);
                    }
                }
                Err(MarretaError::NackSignal { requeue: *requeue })
            }
        }
    }

    // =========================================================================
    // Expressions
    // =========================================================================

    fn evaluate_schema_constructor(
        &mut self,
        schema_name: &str,
        fields: &[(String, Expression)],
    ) -> Result<Value, MarretaError> {
        let (schemas, Some(schema)) = self.constructor_schema_registry(schema_name) else {
            return Err(MarretaError::TypeError {
                message: format!("unknown schema '{}'", schema_name),
                line: self.current_line,
                column: self.current_column,
            });
        };

        let mut map = ValueMap::new();
        for (key, expr) in fields {
            map.insert(key.clone(), self.evaluate(expr)?);
        }
        let value = Value::Map(Arc::new(RwLock::new(map)));

        if let Err(message) = Self::reject_undeclared_schema_fields(&value, &schema, &schemas, "") {
            return Err(MarretaError::TypeError {
                message: format!("schema constructor {} {}", schema_name, message),
                line: self.current_line,
                column: self.current_column,
            });
        }

        coerce_payload(&value, &schema, &schemas).map_err(|err| MarretaError::TypeError {
            message: format!(
                "schema constructor {}: {}",
                schema_name,
                Self::validation_error_detail(err)
            ),
            line: self.current_line,
            column: self.current_column,
        })
    }

    fn reject_undeclared_schema_fields(
        value: &Value,
        schema: &SchemaDefinition,
        schemas: &HashMap<String, SchemaDefinition>,
        path_prefix: &str,
    ) -> Result<(), String> {
        let Value::Map(map) = value else {
            return Ok(());
        };
        let guard = map.read().unwrap();
        let declared: HashSet<&str> = schema
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect();

        for key in guard.keys() {
            if !declared.contains(key.as_str()) {
                let path = if path_prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", path_prefix, key)
                };
                return Err(format!("received undeclared field '{}'", path));
            }
        }

        for field in &schema.fields {
            let Some(field_value) = guard.get(&field.name) else {
                continue;
            };
            let field_path = if path_prefix.is_empty() {
                field.name.clone()
            } else {
                format!("{}.{}", path_prefix, field.name)
            };
            match &field.field_type {
                SchemaType::Reference(schema_name) => {
                    if let Some(nested_schema) = schemas.get(schema_name) {
                        Self::reject_undeclared_schema_fields(
                            field_value,
                            nested_schema,
                            schemas,
                            &field_path,
                        )?;
                    }
                }
                SchemaType::TypedList(inner) => {
                    let SchemaType::Reference(schema_name) = inner.as_ref() else {
                        continue;
                    };
                    let Some(nested_schema) = schemas.get(schema_name) else {
                        continue;
                    };
                    let Value::List(items) = field_value else {
                        continue;
                    };
                    for (index, item) in items.iter().enumerate() {
                        Self::reject_undeclared_schema_fields(
                            item,
                            nested_schema,
                            schemas,
                            &format!("{}[{}]", field_path, index),
                        )?;
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn evaluate_http_client_response_schema(
        &mut self,
        call: &Expression,
        schema_name: &str,
    ) -> Result<Value, MarretaError> {
        let op = match call {
            Expression::MethodCall { object, method, .. } if matches!(object.as_ref(), Expression::Identifier(name) if name == "http_client") =>
            {
                format!("http_client.{}", method)
            }
            _ => "http_client".to_string(),
        };

        let response = self.evaluate(call)?;
        let Value::Map(response_map) = response else {
            return Err(Self::http_client_error(
                "http_client schema annotation expected a response map".into(),
                &op,
            ));
        };

        let schema = self.resolve_schema(schema_name).ok_or_else(|| {
            Self::http_client_error(format!("unknown schema '{}'", schema_name), &op)
        })?;
        let visible_schemas = self.schema_registry_for(self.current_module.as_deref());
        let body = response_map
            .read()
            .unwrap()
            .get("body")
            .cloned()
            .unwrap_or(Value::Null);
        let coerced = coerce_payload(&body, &schema, &visible_schemas).map_err(|err| {
            Self::http_client_error(
                format!(
                    "response body does not match schema '{}': {}",
                    schema_name,
                    Self::validation_error_detail(err)
                ),
                &op,
            )
        })?;

        response_map
            .write()
            .unwrap()
            .insert("body".to_string(), coerced);
        Ok(Value::Map(response_map))
    }

    fn evaluate(&mut self, expr: &Expression) -> Result<Value, MarretaError> {
        match expr {
            // --- Literals ---
            Expression::Integer(n) => Ok(Value::Integer(*n)),
            Expression::Float(n) => Ok(Value::Float(*n)),
            Expression::StringLiteral(s) => self.interpolate_string(s),
            Expression::Boolean(b) => Ok(Value::Boolean(*b)),
            Expression::Null => Ok(Value::Null),

            Expression::List(items) => {
                let vals: Result<Vec<Value>, _> = items.iter().map(|e| self.evaluate(e)).collect();
                Ok(Value::List(vals?))
            }

            Expression::MapLiteral(pairs) => {
                let mut map = ValueMap::new();
                for (key, expr) in pairs {
                    map.insert(key.clone(), self.evaluate(expr)?);
                }
                Ok(Value::Map(Arc::new(RwLock::new(map))))
            }

            Expression::SchemaConstructor {
                schema_name,
                fields,
            } => self.evaluate_schema_constructor(schema_name, fields),

            // --- Identifier ---
            Expression::Identifier(name) => {
                // `db` is the namespace entry point for the relational DB module.
                if name == "db" {
                    return Ok(Value::DbNamespace);
                }
                if name == "doc" {
                    return Ok(Value::DocNamespace);
                }
                if name == "cache" {
                    return Ok(Value::CacheNamespace);
                }
                if name == "fs" {
                    return Ok(Value::FsNamespace);
                }
                if name == "json" {
                    return Ok(Value::JsonNamespace);
                }
                if name == "base64" {
                    return Ok(Value::Base64Namespace);
                }
                if name == "uuid" {
                    return Ok(Value::UuidNamespace);
                }
                if name == "feature" {
                    return Ok(Value::FeatureNamespace);
                }
                if name == "log" {
                    return Ok(Value::LogNamespace);
                }
                if name == "time" {
                    return Ok(Value::TimeNamespace);
                }
                if name == "math" {
                    return Ok(Value::MathNamespace);
                }
                if name == "http_client" {
                    return Ok(Value::HttpClientNamespace);
                }
                self.env
                    .get(name)
                    .ok_or_else(|| MarretaError::UndefinedVariable {
                        name: name.clone(),
                        line: self.current_line,
                        column: self.current_column,
                    })
            }

            // --- Binary ---
            Expression::BinaryOp {
                left,
                operator,
                right,
            } => {
                // Short-circuit for `and`/`or`
                if *operator == BinaryOperator::And {
                    let lv = self.evaluate(left)?;
                    if !lv.is_truthy() {
                        return Ok(lv);
                    }
                    return self.evaluate(right);
                }
                if *operator == BinaryOperator::Or {
                    let lv = self.evaluate(left)?;
                    if lv.is_truthy() {
                        return Ok(lv);
                    }
                    return self.evaluate(right);
                }
                let lv = self.evaluate(left)?;
                let rv = self.evaluate(right)?;
                self.apply_binary_op(operator, &lv, &rv)
            }

            // --- Unary ---
            Expression::UnaryOp { operator, operand } => {
                let val = self.evaluate(operand)?;
                self.apply_unary_op(operator, &val)
            }

            // --- Property access: obj.field ---
            // --- Subscript access: expr[key] ---
            Expression::Subscript { object, key } => {
                let obj = self.evaluate(object)?;
                let k = self.evaluate(key)?;
                match (&obj, &k) {
                    (Value::Map(m), Value::String(s)) => {
                        Ok(m.read().unwrap().get(s).cloned().unwrap_or(Value::Null))
                    }
                    (Value::List(l), Value::Integer(idx)) => {
                        let i = if *idx < 0 {
                            let adjusted = l.len() as i64 + idx;
                            if adjusted < 0 {
                                return Ok(Value::Null);
                            }
                            adjusted as usize
                        } else {
                            *idx as usize
                        };
                        Ok(l.get(i).cloned().unwrap_or(Value::Null))
                    }
                    _ => Err(MarretaError::TypeError {
                        message: format!(
                            "subscript requires Map[String] or List[Integer], got {}[{}]",
                            obj.type_name(),
                            k.type_name()
                        ),
                        line: self.current_line,
                        column: self.current_column,
                    }),
                }
            }

            Expression::PropertyAccess { object, property } => {
                let obj = self.evaluate(object)?;
                // `db.TABLE` → DbTable intermediate value
                if matches!(obj, Value::DbNamespace) {
                    return Ok(Value::DbTable(property.clone()));
                }
                if matches!(obj, Value::DocNamespace) {
                    return Ok(Value::DocCollection(property.clone()));
                }
                // `db.TABLE` evaluates to QueryBuilder when used in pipeline (Phase 3)
                self.access_property(&obj, property)
            }

            // --- Method call: obj.method(args) ---
            Expression::MethodCall {
                object,
                method,
                arguments,
            } => {
                // Spec 061 file-namespace: `file.task(args)` where `file` names a known
                // file-namespace and is not a bound variable in scope. Must run before
                // evaluating `object`, which would turn `Identifier("file")` into an
                // UndefinedVariable. Built-in namespaces are reserved (never registered as
                // a file-namespace) so they still fall through to their sentinels below; a
                // bound variable named `file` shadows the namespace (the has() check).
                if let Expression::Identifier(ns) = object.as_ref() {
                    if !self.env.has(ns) {
                        if let Some(rt) = self.project_runtime.as_ref() {
                            if let Some(task) = rt.module_task(ns, method) {
                                let args = self.evaluate_args(arguments)?;
                                return self.call_task_value(&task, &args, method);
                            }
                            // `ns` is a known file-namespace but has no exported task
                            // `method` (it may exist privately, or not at all). Report it
                            // as an undefined task under that namespace rather than letting
                            // `ns` fall through to a misleading "variable not defined".
                            if rt.is_module_namespace(ns) {
                                return Err(MarretaError::UndefinedTask {
                                    name: format!("{ns}.{method}"),
                                    line: self.current_line,
                                    column: self.current_column,
                                });
                            }
                        }
                    }
                }
                let obj = self.evaluate(object)?;
                // `db.native_query(sql, arg1, arg2, …)` — raw SQL with #{} interpolation
                if matches!(obj, Value::DbNamespace) && method == "native_query" {
                    return self.dispatch_native_query(arguments);
                }
                // `doc.pipeline(collection, list)` — Layer 4 power pipeline
                if matches!(obj, Value::DocNamespace) && method == "pipeline" {
                    let args = self.evaluate_args(arguments)?;
                    return self.dispatch_doc_pipeline(&args);
                }
                // `db.TABLE.operation(args)` → DB direct call dispatch
                // Pass raw arguments so named arg keys are preserved (needed for find_all filters).
                if let Value::DbTable(table) = &obj {
                    return self.dispatch_db_direct(table.clone(), method, arguments);
                }
                // `cache.method(args)` → cache dispatch
                if matches!(obj, Value::CacheNamespace) {
                    return self.dispatch_cache(method, arguments);
                }
                if matches!(obj, Value::FsNamespace) {
                    return self.dispatch_fs(method, arguments);
                }
                if matches!(obj, Value::JsonNamespace) {
                    return self.dispatch_json(method, arguments);
                }
                if matches!(obj, Value::Base64Namespace) {
                    return self.dispatch_base64(method, arguments);
                }
                if matches!(obj, Value::UuidNamespace) {
                    return self.dispatch_uuid(method, arguments);
                }
                if matches!(obj, Value::FeatureNamespace) {
                    return self.dispatch_feature(method, arguments);
                }
                if matches!(obj, Value::LogNamespace) {
                    return self.dispatch_log(method, arguments);
                }
                if matches!(obj, Value::TimeNamespace) {
                    return self.dispatch_time(method, arguments);
                }
                if matches!(obj, Value::MathNamespace) {
                    return self.dispatch_math(method, arguments);
                }
                if matches!(obj, Value::HttpClientNamespace) {
                    return self.dispatch_http_client(method, arguments);
                }
                if let Value::DocCollection(collection) = &obj {
                    return self.dispatch_doc_direct(collection.clone(), method, arguments);
                }
                let args = self.evaluate_args(arguments)?;
                obj.call_method_at(method, &args, self.current_line, self.current_column)
            }

            Expression::HttpClientResponseSchema { call, schema_name } => {
                self.evaluate_http_client_response_schema(call, schema_name)
            }

            // --- Function call: name(args) ---
            Expression::FunctionCall { name, arguments } => {
                let args = self.evaluate_args(arguments)?;
                self.call_function(name, &args)
            }

            // --- TaskCall in pipeline context ---
            Expression::TaskCall { name } => {
                // Returns the task value itself for pipeline use
                self.env
                    .get(name)
                    .ok_or_else(|| MarretaError::UndefinedTask {
                        name: name.clone(),
                        line: self.current_line,
                        column: self.current_column,
                    })
            }

            // --- Match ---
            Expression::Match { subject, arms } => {
                let subj = self.evaluate(subject)?;
                self.evaluate_match(&subj, arms)
            }

            // --- If ---
            Expression::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond = self.evaluate(condition)?;
                if cond.is_truthy() {
                    self.evaluate_branch_body(then_branch)
                } else if let Some(else_branch) = else_branch {
                    self.evaluate_branch_body(else_branch)
                } else {
                    Ok(Value::Null)
                }
            }

            // --- Pipeline ---
            Expression::Pipeline { input, stages } => {
                // Find if there is a rescue stage — if so, use error-deferring evaluation.
                let has_rescue = stages
                    .iter()
                    .any(|s| matches!(s, PipelineStage::Rescue { .. }));

                if has_rescue {
                    let depth_before = self.trace_frames.len();
                    // Railway-oriented: thread a Result through the pre-rescue stages.
                    let mut current: Result<Value, MarretaError> = self.evaluate(input);
                    for stage in stages {
                        match stage {
                            PipelineStage::Rescue { handler } => {
                                match current {
                                    Ok(val) => {
                                        // No error — skip rescue, pass value through
                                        current = Ok(val);
                                    }
                                    Err(ref e) => {
                                        // fail/reply are not caught by rescue
                                        if matches!(e, MarretaError::HttpResponse { .. }) {
                                            return current;
                                        }
                                        // Drop frames accumulated inside the failed branch.
                                        self.trace_frames.truncate(depth_before);
                                        let error_map = Self::build_error_map(e);
                                        self.env.push_scope();
                                        self.env.set("error".to_string(), error_map);
                                        current = self.execute_rescue_handler(handler);
                                        self.env.pop_scope();
                                    }
                                }
                            }
                            _ => {
                                if let Ok(ref val) = current {
                                    let val = val.clone();
                                    current = self.evaluate_pipeline_stage(&val, stage);
                                }
                                // If current is Err, keep deferring to the rescue stage
                            }
                        }
                    }
                    current
                } else {
                    let mut current = self.evaluate(input)?;
                    for stage in stages {
                        current = self.evaluate_pipeline_stage(&current, stage)?;
                    }
                    Ok(current)
                }
            }

            Expression::Rescue { expr, handler } => {
                let depth_before = self.trace_frames.len();
                match self.evaluate(expr) {
                    Ok(val) => Ok(val),
                    Err(e) => {
                        // fail/reply propagate without being caught
                        if matches!(e, MarretaError::HttpResponse { .. }) {
                            return Err(e);
                        }
                        // Drop any trace frames accumulated inside the failed branch —
                        // a handled error must not leave ghost frames on the stack.
                        self.trace_frames.truncate(depth_before);
                        let error_map = Self::build_error_map(&e);
                        self.env.push_scope();
                        self.env.set("error".to_string(), error_map);
                        let result = self.evaluate(handler);
                        self.env.pop_scope();
                        result
                    }
                }
            }

            // ── Queue producers (v0.8) ──────────────────────────────────────────
            Expression::QueuePush {
                queue_name,
                schema,
                payload,
            } => self.eval_queue_push(queue_name, schema, payload, None),

            Expression::TopicPublish {
                topic,
                schema,
                payload,
            } => self.eval_topic_publish(topic, schema, payload, None),

            // --- Broadcast ---
            // Each target receives an independent fork of the interpreter so branches
            // cannot observe each other's side effects. Branches execute concurrently via
            // OS threads (the interpreter is sync/CPU-bound — threads are the right primitive).
            // Results are collected in declaration order regardless of completion order.
            Expression::Broadcast { input, targets } => {
                if self.inside_transaction {
                    return Err(MarretaError::TypeError {
                        message: "*>> (broadcast) is not allowed inside a transaction block"
                            .to_string(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
                let val = self.evaluate(input)?;

                // Fast path: when every branch is provably side-effect-free (a
                // task with an inline body that touches no db/doc/cache/queue/
                // http_client, makes no calls, and runs no nested broadcast),
                // parallel execution is pure overhead — the OS-thread spawn costs
                // far more than the work. Such branches produce identical results
                // in declaration order run sequentially. Anything not provably
                // pure keeps the parallel path below, preserving its
                // parallelism and error/side-effect semantics.
                if self.broadcast_branches_are_pure(targets) {
                    let mut results = Vec::with_capacity(targets.len());
                    for target in targets {
                        let mut branch = self.clone();
                        results.push(branch.apply_broadcast_value(&val, target)?);
                    }
                    return Ok(Value::List(results));
                }

                // Broadcast parallel execution strategy:
                //
                // When inside a tokio runtime (HTTP handler path):
                //   Each branch is spawned as a `tokio::spawn` task (worker thread with
                //   `allow_block_in_place: true`). The current thread calls
                //   `block_in_place(|| handle.block_on(join_all))` to exit the async
                //   context before waiting, allowing a backup worker to run the tasks.
                //   DB calls inside branches use `block_in_place + handle.block_on`
                //   which works correctly on worker threads.
                //
                // When outside a runtime (nested *>> from a non-runtime thread):
                //   Plain OS threads with mpsc channels. DB calls won't work in this path.
                match tokio::runtime::Handle::try_current() {
                    Ok(handle) => {
                        // Spawn all branches as tokio tasks so they run on worker threads
                        // with `allow_block_in_place: true`. Tasks are spawned first
                        // (start running immediately) then awaited — true parallelism.
                        let join_handles: Vec<
                            tokio::task::JoinHandle<Result<Value, MarretaError>>,
                        > = targets
                            .iter()
                            .map(|target| {
                                let mut branch = self.clone();
                                let val = val.clone();
                                let target = target.clone();
                                handle.spawn(async move {
                                    tokio::task::block_in_place(move || {
                                        branch.apply_broadcast_value(&val, &target)
                                    })
                                })
                            })
                            .collect();

                        // Must exit the async context before calling `handle.block_on`.
                        // `block_in_place` signals tokio to spawn a backup worker so
                        // the spawned tasks can run while this thread waits.
                        let err_line = self.current_line;
                        let err_col = self.current_column;
                        let outcomes = tokio::task::block_in_place(|| {
                            handle.block_on(async { collect_join_handles(join_handles).await })
                        });

                        let mut results = Vec::with_capacity(targets.len());
                        for outcome in outcomes {
                            let val = outcome.map_err(|_| MarretaError::TypeError {
                                message: "broadcast branch panicked during parallel execution"
                                    .to_string(),
                                line: err_line,
                                column: err_col,
                            })??;
                            results.push(val);
                        }
                        Ok(Value::List(results))
                    }
                    Err(_) => {
                        let (senders, receivers): (Vec<_>, Vec<_>) = targets
                            .iter()
                            .map(|_| std::sync::mpsc::channel::<Result<Value, MarretaError>>())
                            .unzip();

                        for (target, sender) in targets.iter().zip(senders) {
                            let mut branch = self.clone();
                            let val = val.clone();
                            let target = target.clone();
                            std::thread::spawn(move || {
                                let result = branch.apply_broadcast_value(&val, &target);
                                let _ = sender.send(result);
                            });
                        }

                        let mut results = Vec::with_capacity(targets.len());
                        for rx in receivers {
                            let result = rx.recv().map_err(|_| MarretaError::TypeError {
                                message: "broadcast branch panicked during parallel execution"
                                    .to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            })??;
                            results.push(result);
                        }
                        Ok(Value::List(results))
                    }
                }
            }
        }
    }

    // =========================================================================
    // Binary operations
    // =========================================================================

    // =========================================================================
    // Property access
    // =========================================================================

    fn resolve_schema_name_for_table(&self, table: &str) -> Option<String> {
        if let Some(runtime) = &self.project_runtime
            && let Some((schema_name, _)) = runtime.resolve_persistent_schema_for_table(table)
        {
            return Some(schema_name);
        }

        self.schema_registry_for(self.current_module.as_deref())
            .into_iter()
            .find_map(|(schema_name, schema)| {
                schema
                    .db_table
                    .as_deref()
                    .filter(|db_table| *db_table == table)
                    .map(|_| schema_name)
            })
    }

    fn infer_inverse_reference_field(
        &self,
        owner_schema_name: &str,
        target_schema_name: &str,
    ) -> Option<String> {
        let target_schema = self.resolve_schema(target_schema_name)?;
        let inverse_fields: Vec<_> = target_schema
            .fields
            .iter()
            .filter_map(|field| match &field.field_type {
                SchemaType::Reference(inverse_target) if inverse_target == owner_schema_name => {
                    Some(field.name.clone())
                }
                _ => None,
            })
            .collect();

        match inverse_fields.as_slice() {
            [field_name] => Some(field_name.clone()),
            _ => None,
        }
    }

    fn relation_handle_for_property(
        &self,
        schema_name: &str,
        fields: &Arc<RwLock<ValueMap>>,
        property: &str,
    ) -> Option<Value> {
        let schema = self.resolve_schema(schema_name)?;
        let field = schema.fields.iter().find(|field| field.name == property)?;

        match &field.field_type {
            SchemaType::Reference(target_schema_name) => {
                let target_schema = self.resolve_schema(target_schema_name)?;
                let target_table = target_schema.db_table?;
                let fk_column = format!("{}_id", property);
                let fk_value = fields
                    .read()
                    .unwrap()
                    .get(&fk_column)
                    .cloned()
                    .unwrap_or(Value::Null);
                let mut query = QueryState::new(target_table);
                query.filters.push(FilterClause {
                    column: "id".to_string(),
                    op: FilterOp::Eq,
                    value: fk_value.clone(),
                });
                Some(Value::RelationHandle(Box::new(RelationHandle {
                    query,
                    cardinality: RelationCardinality::One,
                    null_short_circuit: matches!(fk_value, Value::Null),
                })))
            }
            SchemaType::TypedList(inner) => {
                let SchemaType::Reference(target_schema_name) = inner.as_ref() else {
                    return None;
                };
                let target_schema = self.resolve_schema(target_schema_name)?;
                let target_table = target_schema.db_table?;
                let inverse_field =
                    self.infer_inverse_reference_field(schema_name, target_schema_name)?;
                let owner_id = fields
                    .read()
                    .unwrap()
                    .get("id")
                    .cloned()
                    .unwrap_or(Value::Null);
                let mut query = QueryState::new(target_table);
                query.filters.push(FilterClause {
                    column: format!("{}_id", inverse_field),
                    op: FilterOp::Eq,
                    value: owner_id,
                });
                Some(Value::RelationHandle(Box::new(RelationHandle {
                    query,
                    cardinality: RelationCardinality::Many,
                    null_short_circuit: false,
                })))
            }
            _ => None,
        }
    }

    fn db_row_to_runtime_value(&self, table: &str, row: DbRow) -> Value {
        match self.resolve_schema_name_for_table(table) {
            Some(schema_name) => {
                let coerced_row = self
                    .resolve_schema(&schema_name)
                    .and_then(|schema| {
                        let visible_schemas =
                            self.schema_registry_for(self.current_module.as_deref());
                        let row_value =
                            Value::Map(Arc::new(RwLock::new(row.clone().into_iter().collect())));
                        match coerce_payload(&row_value, &schema, &visible_schemas) {
                            Ok(Value::Map(map)) => Some(map.read().unwrap().clone()),
                            _ => None,
                        }
                    })
                    .unwrap_or_else(|| row.into_iter().collect());

                Value::RelationalRecord {
                    schema_name,
                    fields: Arc::new(RwLock::new(coerced_row)),
                }
            }
            None => db_row_to_value(row),
        }
    }

    // =========================================================================
    // Function/task calls
    // =========================================================================

    fn call_function(&mut self, name: &str, args: &[Value]) -> Result<Value, MarretaError> {
        // Built-in functions
        if let Some(result) = self.builtin_function(name, args) {
            return result;
        }

        let task = self
            .env
            .get(name)
            .ok_or_else(|| MarretaError::UndefinedTask {
                name: name.into(),
                line: self.current_line,
                column: self.current_column,
            })?;

        self.call_task_value(&task, args, name)
    }

    fn call_task_value(
        &mut self,
        task: &Value,
        args: &[Value],
        call_name: &str,
    ) -> Result<Value, MarretaError> {
        match task {
            Value::Task {
                name,
                params,
                body,
                owner_module,
                source_module,
                line,
                column,
            } => {
                if params.len() != args.len() {
                    return Err(MarretaError::WrongArity {
                        task_name: name.clone(),
                        expected: params.len(),
                        got: args.len(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }

                // Task contract enforcement — validate arguments against bound schemas
                // before the task body runs. A violation yields TypeError → HTTP 500.
                let schema_lookup_module =
                    owner_module.as_deref().or(self.current_module.as_deref());
                let visible_schemas = self.schema_registry_for(schema_lookup_module);

                let mut coerced_args = args.to_vec();

                for (index, (param, arg)) in params.iter().zip(args.iter()).enumerate() {
                    if let Some(schema_name) = &param.schema {
                        match visible_schemas.get(schema_name) {
                            Some(schema_def) => {
                                match coerce_payload(arg, schema_def, &visible_schemas) {
                                    Ok(coerced) => coerced_args[index] = coerced,
                                    Err(validation_err) => {
                                        // Extract the validation message and re-wrap as TypeError
                                        let detail =
                                            if let MarretaError::HttpResponse { ref body, .. } =
                                                validation_err
                                            {
                                                serde_json::from_str::<serde_json::Value>(body)
                                                    .ok()
                                                    .and_then(|j| {
                                                        j["error"].as_str().map(|s| s.to_string())
                                                    })
                                                    .unwrap_or_else(|| body.clone())
                                            } else {
                                                validation_err.to_string()
                                            };
                                        return Err(MarretaError::TypeError {
                                            message: format!(
                                                "task '{}' argument '{}' does not match schema '{}': {}",
                                                name, param.name, schema_name, detail
                                            ),
                                            line: self.current_line,
                                            column: self.current_column,
                                        });
                                    }
                                };
                            }
                            None => {
                                return Err(MarretaError::TypeError {
                                    message: format!(
                                        "task '{}' argument '{}' references unknown schema '{}'",
                                        name, param.name, schema_name
                                    ),
                                    line: self.current_line,
                                    column: self.current_column,
                                });
                            }
                        }
                    }
                }

                if self.call_depth >= self.max_recursion_depth {
                    return Err(MarretaError::RuntimeError {
                        message: "maximum recursion depth exceeded".to_string(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
                // Only cross-module calls need a fork (to switch to the owner
                // module's environment base and `current_module`). A same-module
                // call — including recursion and route-local composition — already
                // runs against the right base in `self`, so it takes the cheap
                // path below: no per-call clone of the interpreter (which would
                // copy the growing trace-frame stack, making recursion quadratic).
                if let Some(owner_module_id) = owner_module
                    && self.current_module.as_deref() != Some(owner_module_id.as_str())
                {
                    let base_env = self
                        .project_runtime
                        .as_ref()
                        .map(|runtime| runtime.env_for_module(Some(owner_module_id.as_str())))
                        .unwrap_or_else(|| self.env.clone());
                    // Propagate only the caller's LOCAL definitions (e.g.
                    // route-local tasks, which must remain callable down a task
                    // chain). Global/module definitions are NOT re-injected here:
                    // base_env already provides them via the shared Arc, and
                    // cloning every global per call was the expensive part.
                    let caller_locals = self.env.local_variables();
                    let mut task_interp =
                        self.fork_with_env(base_env, Some(owner_module_id.clone()));
                    task_interp.call_depth = self.call_depth + 1;
                    let task_trace =
                        task_interp.enter_task(name, source_module.clone(), *line, *column);
                    task_interp.env.push_scope();
                    for (name, value) in caller_locals {
                        task_interp.env.set(name, value);
                    }
                    task_interp.env.push_scope();
                    for (param, arg) in params.iter().zip(coerced_args.iter()) {
                        task_interp.env.set(param.name.clone(), arg.clone());
                    }

                    let result = (|| match body {
                        TaskBody::Inline(expr) => task_interp.evaluate(expr),
                        TaskBody::Block(stmts, return_expr) => {
                            for stmt in stmts {
                                task_interp.execute_statement(stmt)?;
                            }
                            task_interp.evaluate(return_expr)
                        }
                    })();

                    match result {
                        Ok(value) => Ok(value),
                        Err(err) => {
                            task_trace.preserve();
                            self.trace_frames = task_interp.trace_frames.clone();
                            Err(err)
                        }
                    }
                } else {
                    self.call_depth += 1;
                    let task_trace = self.enter_task(name, source_module.clone(), *line, *column);

                    self.env.push_scope();
                    for (param, arg) in params.iter().zip(coerced_args.iter()) {
                        self.env.set(param.name.clone(), arg.clone());
                    }

                    let result = (|| match body {
                        TaskBody::Inline(expr) => self.evaluate(expr),
                        TaskBody::Block(stmts, return_expr) => {
                            for stmt in stmts {
                                self.execute_statement(stmt)?;
                            }
                            self.evaluate(return_expr)
                        }
                    })();

                    self.env.pop_scope();
                    self.call_depth = self.call_depth.saturating_sub(1);
                    if result.is_err() {
                        task_trace.preserve();
                    }
                    result
                }
            }
            _ => Err(MarretaError::NotCallable {
                name: call_name.into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    /// Built-in functions available without definition.
    fn builtin_function(&self, name: &str, args: &[Value]) -> Option<Result<Value, MarretaError>> {
        match name {
            // `__fail__` is the internal name for `fail CODE, MSG` used in expression position.
            "__fail__" => {
                let status_code = match args.first() {
                    Some(Value::Integer(n)) => *n as u16,
                    _ => 500,
                };
                let msg = args.get(1).cloned().unwrap_or(Value::Null);
                let body = match &msg {
                    Value::Map(_) | Value::List(_) => {
                        let _timer = runtime_profile::timer(
                            self.runtime_profile_route.as_ref(),
                            ProfilePhase::JsonSerialize,
                        );
                        crate::value::value_to_json_string(&msg)
                    }
                    _ => serde_json::json!({ "error": msg.to_string() }).to_string(),
                };
                Some(Err(MarretaError::HttpResponse {
                    status_code,
                    body,
                    content_type: "application/json".to_string(),
                    extra_headers: vec![],
                    is_error: true,
                }))
            }
            "print" => {
                let output: Vec<String> = args.iter().map(|a| format!("{}", a)).collect();
                println!("{}", output.join(" "));
                Some(Ok(Value::Null))
            }
            "type" => {
                if args.len() != 1 {
                    return Some(Err(MarretaError::WrongArity {
                        task_name: "type".into(),
                        expected: 1,
                        got: args.len(),
                        line: self.current_line,
                        column: self.current_column,
                    }));
                }
                Some(Ok(Value::String(args[0].type_name().into())))
            }
            "decimal" => Some(self.builtin_decimal(args)),
            "len" => {
                if args.len() != 1 {
                    return Some(Err(MarretaError::WrongArity {
                        task_name: "len".into(),
                        expected: 1,
                        got: args.len(),
                        line: self.current_line,
                        column: self.current_column,
                    }));
                }
                let length = match &args[0] {
                    Value::String(s) => s.len() as i64,
                    Value::List(l) => l.len() as i64,
                    Value::Map(m) => m.read().unwrap().len() as i64,
                    other => {
                        return Some(Err(MarretaError::TypeError {
                            message: format!("len() not supported for {}", other.type_name()),
                            line: self.current_line,
                            column: self.current_column,
                        }));
                    }
                };
                Some(Ok(Value::Integer(length)))
            }
            "range" => {
                let (start, end) = match args {
                    [Value::Integer(end)] => (1, *end),
                    [Value::Integer(start), Value::Integer(end)] => (*start, *end),
                    [single] => {
                        return Some(Err(MarretaError::TypeError {
                            message: format!(
                                "range() expects Integer arguments, got {}",
                                single.type_name()
                            ),
                            line: self.current_line,
                            column: self.current_column,
                        }));
                    }
                    [first, second] => {
                        return Some(Err(MarretaError::TypeError {
                            message: format!(
                                "range() expects Integer arguments, got {} and {}",
                                first.type_name(),
                                second.type_name()
                            ),
                            line: self.current_line,
                            column: self.current_column,
                        }));
                    }
                    _ => {
                        return Some(Err(MarretaError::WrongArity {
                            task_name: "range".into(),
                            expected: 1,
                            got: args.len(),
                            line: self.current_line,
                            column: self.current_column,
                        }));
                    }
                };

                if start > end {
                    return Some(Ok(Value::List(vec![])));
                }

                let values = (start..=end).map(Value::Integer).collect();
                Some(Ok(Value::List(values)))
            }
            _ => None,
        }
    }

    fn builtin_decimal(&self, args: &[Value]) -> Result<Value, MarretaError> {
        if args.len() != 1 {
            return Err(MarretaError::WrongArity {
                task_name: "decimal".into(),
                expected: 1,
                got: args.len(),
                line: self.current_line,
                column: self.current_column,
            });
        }

        match &args[0] {
            Value::Decimal(d) => Ok(Value::Decimal(*d)),
            Value::Integer(n) => Ok(Value::Decimal(Decimal::from(*n))),
            Value::String(s) if !s.contains('e') && !s.contains('E') => s
                .parse::<Decimal>()
                .map(Value::Decimal)
                .map_err(|_| MarretaError::TypeError {
                    message: "decimal() requires a plain decimal string or Integer".into(),
                    line: self.current_line,
                    column: self.current_column,
                }),
            Value::String(_) => Err(MarretaError::TypeError {
                message: "decimal() rejects scientific notation; use a plain decimal string".into(),
                line: self.current_line,
                column: self.current_column,
            }),
            other => Err(MarretaError::TypeError {
                message: format!(
                    "decimal() requires String or Integer, got {}",
                    other.type_name()
                ),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    // =========================================================================
    // Match expression
    // =========================================================================

    fn evaluate_match(
        &mut self,
        subject: &Value,
        arms: &[MatchArm],
    ) -> Result<Value, MarretaError> {
        for arm in arms {
            match &arm.pattern {
                MatchPattern::Literal(expr) => {
                    let pattern_val = self.evaluate(expr)?;
                    if subject == &pattern_val {
                        return self.evaluate(&arm.value);
                    }
                }
                MatchPattern::Fallback => {
                    return self.evaluate(&arm.value);
                }
            }
        }
        Ok(Value::Null)
    }

    /// Returns the DB engine or an error if none is configured.
    fn require_db_engine(&self) -> Result<DbEngine, MarretaError> {
        self.db_engine.clone().ok_or_else(|| MarretaError::TypeError {
            message: "db.* called but no DB is configured (set MARRETA_DB_PROVIDER, MARRETA_DB_HOST, MARRETA_DB_PORT, MARRETA_DB_NAME, MARRETA_DB_USER, and optional MARRETA_DB_PASSWORD)".to_string(),
            line: self.current_line,
            column: self.current_column,
        })
    }

    /// Returns the Doc engine or an error if none is configured.
    fn require_doc_engine(&self) -> Result<DocEngine, MarretaError> {
        self.doc_engine.clone().ok_or_else(|| MarretaError::TypeError {
            message: "doc.* called but no document DB is configured (set MARRETA_DOC_PROVIDER, MARRETA_DOC_HOST, MARRETA_DOC_PORT, MARRETA_DOC_NAME, optional MARRETA_DOC_USER, and optional MARRETA_DOC_PASSWORD)".to_string(),
            line: self.current_line,
            column: self.current_column,
        })
    }

    /// Runs an async DB future from the synchronous interpreter.
    ///
    /// Two cases:
    /// - Inside a tokio runtime (HTTP handler): uses `block_in_place` to yield the
    ///   worker thread, preventing "Cannot start a runtime from within a runtime".
    /// - Outside a runtime (spawned thread from `*>>`): creates a temporary single-
    ///   threaded runtime for the duration of the call.
    fn block_db<F, T>(&self, fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
            Err(_) => tokio::runtime::Runtime::new()
                .expect("failed to create tokio runtime for db call")
                .block_on(fut),
        }
    }

    fn has_active_tx(&self) -> bool {
        self.active_tx.0.lock().unwrap().is_some()
    }

    fn take_tx(&self) -> Box<dyn DbTx + Send> {
        self.active_tx
            .0
            .lock()
            .unwrap()
            .take()
            .expect("expected active transaction but found none")
    }

    fn restore_tx(&self, tx: Box<dyn DbTx + Send>) {
        *self.active_tx.0.lock().unwrap() = Some(tx);
    }

    /// Parses `where(...)` argument list into `FilterClause` list.
    ///
    /// Accepts:
    /// - Named args: `status: "active"` → `FilterClause { col: "status", op: Eq, val: "active" }`
    /// - Binary expressions: `total > 1000` → `FilterClause { col: "total", op: Gt, val: 1000 }`
    ///
    /// `like` and `in` are handled as `BinaryOp` with `BinaryOperator::Like` / `In`
    /// (added to lexer/parser as context operators in Phase 3).
    /// For now they fall through to the expression evaluator path if present.
    fn parse_where_args(
        &mut self,
        arguments: &[Argument],
    ) -> Result<Vec<crate::db::driver::FilterClause>, MarretaError> {
        use crate::db::driver::{FilterClause, FilterOp};

        let mut filters = Vec::new();

        for arg in arguments {
            match arg {
                // Named arg: `status: "active"` → equality filter
                Argument::Named { name, value } => {
                    let val = self.evaluate(value)?;
                    filters.push(FilterClause {
                        column: name.clone(),
                        op: FilterOp::Eq,
                        value: val,
                    });
                }

                // Positional arg: must be a binary comparison expression
                Argument::Positional(expr) => {
                    let clause = self.extract_filter_from_expr(expr)?;
                    filters.push(clause);
                }
            }
        }

        Ok(filters)
    }

    /// Walks a binary expression AST node and extracts a `FilterClause`.
    /// Supports: `col > val`, `col >= val`, `col < val`, `col <= val`, `col != val`, `col == val`
    /// Returns an error for unsupported expression shapes.
    /// Wraps expressions back to DocFilter
    fn extract_filter_from_expr(
        &mut self,
        expr: &Expression,
    ) -> Result<crate::db::driver::FilterClause, MarretaError> {
        use crate::db::driver::{FilterClause, FilterOp};

        match expr {
            Expression::BinaryOp {
                left,
                operator,
                right,
            } => {
                let op = match operator {
                    BinaryOperator::Greater => FilterOp::Gt,
                    BinaryOperator::GreaterEqual => FilterOp::Gte,
                    BinaryOperator::Less => FilterOp::Lt,
                    BinaryOperator::LessEqual => FilterOp::Lte,
                    BinaryOperator::NotEqual => FilterOp::Ne,
                    BinaryOperator::Equal => FilterOp::Eq,
                    other => {
                        return Err(MarretaError::TypeError {
                            message: format!(
                                "unsupported operator {:?} in where() — supported: >, >=, <, <=, !=, ==",
                                other
                            ),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                };

                let column = match left.as_ref() {
                    Expression::Identifier(name) => name.clone(),
                    _ => return Err(MarretaError::TypeError {
                        message: "left side of where() filter must be a column identifier (e.g. total > 1000)".to_string(),
                        line: self.current_line,
                        column: self.current_column,
                    }),
                };

                let value = self.evaluate(right)?;
                Ok(FilterClause { column, op, value })
            }
            other => Err(MarretaError::TypeError {
                message: format!(
                    "unsupported expression in where() — use 'col > val' or 'col: val' (got {:?})",
                    other
                ),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn parse_doc_where_args(
        &mut self,
        arguments: &[Argument],
    ) -> Result<Vec<crate::doc::query::DocFilter>, MarretaError> {
        let mut filters = Vec::new();
        for arg in arguments {
            match arg {
                Argument::Named { .. } => {
                    return Err(MarretaError::TypeError {
                        message: "doc.* where() requires string field names — use where(\"field\" == value) instead of named arguments".to_string(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
                Argument::Positional(expr) => {
                    let clause = self.extract_doc_filter_from_expr(expr)?;
                    filters.push(clause);
                }
            }
        }
        Ok(filters)
    }

    fn extract_doc_filter_from_expr(
        &mut self,
        expr: &Expression,
    ) -> Result<crate::doc::query::DocFilter, MarretaError> {
        match expr {
            Expression::BinaryOp {
                left,
                operator,
                right,
            } => {
                let column = match left.as_ref() {
                    Expression::StringLiteral(s) => s.clone(),
                    Expression::Identifier(_) => return Err(MarretaError::TypeError {
                        message: "doc.* where() requires string field names — use where(\"field\" == value) not where(field == value). String field names support dot-notation paths like \"address.city\".".to_string(),
                        line: self.current_line,
                        column: self.current_column,
                    }),
                    _ => return Err(MarretaError::TypeError {
                        message: "left side of doc where() filter must be a string field name, e.g. where(\"status\" == \"pending\")".to_string(),
                        line: self.current_line,
                        column: self.current_column,
                    }),
                };
                let value = self.evaluate(right)?;
                match operator {
                    BinaryOperator::Greater => Ok(crate::doc::query::DocFilter::Gt(column, value)),
                    BinaryOperator::GreaterEqual => {
                        Ok(crate::doc::query::DocFilter::Gte(column, value))
                    }
                    BinaryOperator::Less => Ok(crate::doc::query::DocFilter::Lt(column, value)),
                    BinaryOperator::LessEqual => {
                        Ok(crate::doc::query::DocFilter::Lte(column, value))
                    }
                    BinaryOperator::NotEqual => Ok(crate::doc::query::DocFilter::Ne(column, value)),
                    BinaryOperator::Equal => Ok(crate::doc::query::DocFilter::Eq(column, value)),
                    other => Err(MarretaError::TypeError {
                        message: format!("unsupported operator {:?} in where()", other),
                        line: self.current_line,
                        column: self.current_column,
                    }),
                }
            }
            _other => Err(MarretaError::TypeError {
                message: "unsupported expression in where()".to_string(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    /// Extracts the value of a specific named argument from an argument list.
    /// Returns `None` if the named argument is not present.
    fn extract_named_string_arg(
        &mut self,
        arguments: &[Argument],
        key: &str,
    ) -> Result<Option<String>, MarretaError> {
        for arg in arguments {
            if let Argument::Named { name, value } = arg
                && name == key
            {
                let val = self.evaluate(value)?;
                return match val {
                    Value::String(s) => Ok(Some(s)),
                    other => Err(MarretaError::TypeError {
                        message: format!(
                            "'{}:' argument must be a string, got {}",
                            key,
                            other.type_name()
                        ),
                        line: self.current_line,
                        column: self.current_column,
                    }),
                };
            }
        }
        Ok(None)
    }

    /// Applies a broadcast branch to input.
    /// Unlike `apply_pipeline_value`, this does NOT implicitly iterate over Lists —
    /// the input value is passed as-is to the task. This is the correct semantics for
    /// `*>>`: each branch receives the same input, whole, regardless of its type.
    /// True only when every broadcast target is provably side-effect-free, so the
    /// branches can run sequentially instead of spawning an OS thread each. Used
    /// by the fast path in the `Broadcast` arm. Conservative: anything not proven
    /// pure returns false and keeps the parallel path.
    fn broadcast_branches_are_pure(&self, targets: &[Expression]) -> bool {
        targets
            .iter()
            .all(|target| self.broadcast_branch_is_pure(target))
    }

    fn broadcast_branch_is_pure(&self, target: &Expression) -> bool {
        match target {
            Expression::Identifier(name) | Expression::TaskCall { name } => {
                if matches!(name.as_str(), "fetch" | "fetch_one") {
                    return false;
                }
                match self.env.get(name) {
                    Some(Value::Task { body, .. }) => task_body_is_pure(&body),
                    _ => false,
                }
            }
            // Spec 061: `-> file.task` branch (PropertyAccess) resolved from the registry.
            Expression::PropertyAccess { object, property } => {
                match self.namespace_stage_task(object, property) {
                    Some(Value::Task { body, .. }) => task_body_is_pure(&body),
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn apply_broadcast_value(
        &mut self,
        input: &Value,
        expr: &Expression,
    ) -> Result<Value, MarretaError> {
        match expr {
            Expression::TaskCall { name } | Expression::Identifier(name) => {
                if matches!(name.as_str(), "fetch" | "fetch_one") {
                    return Err(MarretaError::TypeError {
                        message: format!(
                            "cannot apply >> {} to {}; this value is not a relation or query",
                            name,
                            input.type_name()
                        ),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
                if let Some(task) = self.env.get(name)
                    && matches!(task, Value::Task { .. })
                {
                    // No implicit iteration — pass the full input value to the task
                    return self.call_task_value(&task, std::slice::from_ref(input), name);
                }
                self.evaluate(expr)
            }
            Expression::FunctionCall { name, arguments } => {
                let mut args = vec![input.clone()];
                args.extend(self.evaluate_args(arguments)?);
                self.call_function(name, &args)
            }
            Expression::MethodCall {
                object,
                method,
                arguments,
            } if matches!(object.as_ref(), Expression::Identifier(n) if n == "fs") => {
                self.dispatch_fs_with_input(method, arguments, Some(input))
            }
            Expression::MethodCall {
                object,
                method,
                arguments,
            } if matches!(object.as_ref(), Expression::Identifier(n) if n == "json") => {
                self.dispatch_json_with_input(method, arguments, Some(input))
            }
            Expression::MethodCall {
                object,
                method,
                arguments,
            } if matches!(object.as_ref(), Expression::Identifier(n) if n == "base64") => {
                self.dispatch_base64_with_input(method, arguments, Some(input))
            }
            Expression::MethodCall {
                object,
                method,
                arguments,
            } if matches!(object.as_ref(), Expression::Identifier(n) if n == "log") => {
                self.dispatch_log_with_input(method, arguments, Some(input))
            }
            Expression::MethodCall {
                object,
                method,
                arguments,
            } if matches!(object.as_ref(), Expression::Identifier(n) if n == "math") => {
                self.dispatch_math_with_input(method, arguments, Some(input))
            }
            // Spec 061: `-> file.task` broadcast branch (no implicit iteration: the whole
            // input value is passed to the task, matching the bare-`Identifier` branch).
            Expression::PropertyAccess { object, property }
                if self.namespace_stage_task(object, property).is_some() =>
            {
                let task = self.namespace_stage_task(object, property).unwrap();
                self.call_task_value(&task, std::slice::from_ref(input), property)
            }
            // Spec 061: `-> file.task(extra)` — input is prepended as the first argument.
            Expression::MethodCall {
                object,
                method,
                arguments,
            } if self.namespace_stage_task(object, method).is_some() => {
                let task = self.namespace_stage_task(object, method).unwrap();
                let mut args = vec![input.clone()];
                args.extend(self.evaluate_args(arguments)?);
                self.call_task_value(&task, &args, method)
            }
            _ => self.evaluate(expr),
        }
    }

    // ── Queue producer helpers ─────────────────────────────────────────────
    // Shared between direct evaluation (payload present) and pipeline
    // injection (payload absent — `pipeline_input` supplies the value).

    fn eval_queue_push(
        &mut self,
        queue_name: &Expression,
        schema: &Option<String>,
        payload: &Option<Box<Expression>>,
        pipeline_input: Option<&Value>,
    ) -> Result<Value, MarretaError> {
        let driver = self.queue_driver.clone().ok_or_else(|| MarretaError::QueueError {
            message: "Queue provider not configured — set MARRETA_QUEUE_PROVIDER, MARRETA_QUEUE_HOST, MARRETA_QUEUE_PORT, MARRETA_QUEUE_USER, and optional MARRETA_QUEUE_PASSWORD".to_string(),
            operation: "queue.push".to_string(),
        })?;
        let queue = match self.evaluate(queue_name)? {
            Value::String(s) => s,
            other => {
                return Err(MarretaError::TypeError {
                    message: format!("queue.push: queue name must be a string, got {:?}", other),
                    line: self.current_line,
                    column: self.current_column,
                });
            }
        };
        let mut value = match payload {
            Some(expr) => self.evaluate(expr)?,
            None => pipeline_input
                .cloned()
                .ok_or_else(|| MarretaError::TypeError {
                    message: "queue.push: no payload provided and not inside a pipeline"
                        .to_string(),
                    line: self.current_line,
                    column: self.current_column,
                })?,
        };
        if let Some(schema_name) = schema
            && let Some(schemas) = &self.schemas
            && let Some(schema_def) = schemas.get(schema_name)
        {
            value = crate::response_serializer::serialize(value, schema_def);
        }
        let result = value.clone();
        let message = self.queue_message(value);
        run_async(async move { driver.push(&queue, &message).await }).map_err(|e| {
            MarretaError::QueueError {
                message: e.to_string(),
                operation: "queue.push".to_string(),
            }
        })?;
        Ok(result)
    }

    fn eval_topic_publish(
        &mut self,
        topic: &Expression,
        schema: &Option<String>,
        payload: &Option<Box<Expression>>,
        pipeline_input: Option<&Value>,
    ) -> Result<Value, MarretaError> {
        let driver = self.queue_driver.clone().ok_or_else(|| MarretaError::QueueError {
            message: "Queue provider not configured — set MARRETA_QUEUE_PROVIDER, MARRETA_QUEUE_HOST, MARRETA_QUEUE_PORT, MARRETA_QUEUE_USER, and optional MARRETA_QUEUE_PASSWORD".to_string(),
            operation: "topic.publish".to_string(),
        })?;
        let t = match self.evaluate(topic)? {
            Value::String(s) => s,
            other => {
                return Err(MarretaError::TypeError {
                    message: format!("topic.publish: topic must be a string, got {:?}", other),
                    line: self.current_line,
                    column: self.current_column,
                });
            }
        };
        let mut value = match payload {
            Some(expr) => self.evaluate(expr)?,
            None => pipeline_input
                .cloned()
                .ok_or_else(|| MarretaError::TypeError {
                    message: "topic.publish: no payload provided and not inside a pipeline"
                        .to_string(),
                    line: self.current_line,
                    column: self.current_column,
                })?,
        };
        if let Some(schema_name) = schema
            && let Some(schemas) = &self.schemas
            && let Some(schema_def) = schemas.get(schema_name)
        {
            value = crate::response_serializer::serialize(value, schema_def);
        }
        let result = value.clone();
        let message = self.queue_message(value);
        run_async(async move { driver.publish(&t, &message).await }).map_err(|e| {
            MarretaError::QueueError {
                message: e.to_string(),
                operation: "topic.publish".to_string(),
            }
        })?;
        Ok(result)
    }

    fn queue_message(&self, payload: Value) -> QueueMessage {
        let mut message = QueueMessage::new(payload);
        if let Some(trace_context) = &self.trace_context {
            let child = trace_context.outbound_child();
            message
                .metadata
                .insert("traceparent".to_string(), child.traceparent());
            if let Some(tracestate) = child.tracestate {
                message
                    .metadata
                    .insert("tracestate".to_string(), tracestate);
            }
        }
        message
    }

    fn apply_pipeline_value(
        &mut self,
        input: &Value,
        expr: &Expression,
    ) -> Result<Value, MarretaError> {
        match expr {
            Expression::TaskCall { name } | Expression::Identifier(name) => {
                if matches!(name.as_str(), "fetch" | "fetch_one") {
                    return Err(MarretaError::TypeError {
                        message: format!(
                            "cannot apply >> {} to {}; this value is not a relation or query",
                            name,
                            input.type_name()
                        ),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
                if let Some(task) = self.env.get(name)
                    && matches!(task, Value::Task { .. })
                {
                    // Implicit iteration: if input is a List, apply task to each element
                    if let Value::List(items) = input {
                        let mut results = Vec::new();
                        for item in items {
                            results.push(self.call_task_value(
                                &task,
                                std::slice::from_ref(item),
                                name,
                            )?);
                        }
                        return Ok(Value::List(results));
                    }
                    return self.call_task_value(&task, std::slice::from_ref(input), name);
                }
                // If not a task, just evaluate
                self.evaluate(expr)
            }
            Expression::FunctionCall { name, arguments } => {
                // Prepend input as first argument
                let mut args = vec![input.clone()];
                args.extend(self.evaluate_args(arguments)?);
                self.call_function(name, &args)
            }
            // Pipeline injection for queue producers: `value >> queue.push("q")`
            Expression::QueuePush {
                queue_name,
                schema,
                payload,
            } => self.eval_queue_push(queue_name, schema, payload, Some(input)),
            Expression::TopicPublish {
                topic,
                schema,
                payload,
            } => self.eval_topic_publish(topic, schema, payload, Some(input)),
            // Pipeline injection for cache: `value >> cache.set("key")`
            // Prepends the pipeline input as the value argument (second positional).
            Expression::MethodCall {
                object,
                method,
                arguments,
            } if matches!(object.as_ref(), Expression::Identifier(n) if n == "cache")
                && method == "set" =>
            {
                let op = format!("cache.{}", method);
                let driver = self.require_cache_driver(&op)?;
                // First arg is the key
                let key = match arguments.first() {
                    Some(Argument::Positional(expr)) => self.evaluate(expr)?.to_string(),
                    _ => {
                        return Err(Self::cache_error(
                            "cache.set in pipeline requires a key argument".into(),
                            &op,
                        ));
                    }
                };
                // Pipeline input is the value
                let value = input.clone();
                // Collect named args
                let mut named: Vec<(String, Value)> = Vec::new();
                for arg in arguments.iter().skip(1) {
                    if let Argument::Named { name, value } = arg {
                        named.push((name.clone(), self.evaluate(value)?));
                    }
                }
                let ttl = self.resolve_ttl(&[], &named);
                let only_if_absent = Self::resolve_named_bool(&named, "only_if_absent");
                // Apply schema filtering if `as` is used (future)
                let result = value.clone();
                run_async(async move { driver.set(&key, &value, ttl, only_if_absent).await })
                    .map_err(|e| Self::cache_error(e.to_string(), &op))?;
                Ok(result)
            }
            // Pipeline injection for http_client: `payload >> http_client.post(url)`
            // Injects pipeline input as request body (POST/PUT/PATCH) or query params (GET/DELETE).
            Expression::MethodCall {
                object,
                method,
                arguments,
            } if matches!(object.as_ref(), Expression::Identifier(n) if n == "http_client") => {
                self.dispatch_http_client_with_pipeline(method, arguments, Some(input))
            }
            Expression::MethodCall {
                object,
                method,
                arguments,
            } if matches!(object.as_ref(), Expression::Identifier(n) if n == "fs") => {
                self.dispatch_fs_with_input(method, arguments, Some(input))
            }
            Expression::MethodCall {
                object,
                method,
                arguments,
            } if matches!(object.as_ref(), Expression::Identifier(n) if n == "json") => {
                self.dispatch_json_with_input(method, arguments, Some(input))
            }
            Expression::MethodCall {
                object,
                method,
                arguments,
            } if matches!(object.as_ref(), Expression::Identifier(n) if n == "base64") => {
                self.dispatch_base64_with_input(method, arguments, Some(input))
            }
            Expression::MethodCall {
                object,
                method,
                arguments,
            } if matches!(object.as_ref(), Expression::Identifier(n) if n == "log") => {
                self.dispatch_log_with_input(method, arguments, Some(input))
            }
            Expression::MethodCall {
                object,
                method,
                arguments,
            } if matches!(object.as_ref(), Expression::Identifier(n) if n == "math") => {
                self.dispatch_math_with_input(method, arguments, Some(input))
            }
            // Spec 061: `value >> file.task` — a file-namespace task in stage position
            // (no parens parses as PropertyAccess). Mirrors the bare-`Identifier` stage,
            // including implicit per-element iteration over a list input.
            Expression::PropertyAccess { object, property }
                if self.namespace_stage_task(object, property).is_some() =>
            {
                let task = self.namespace_stage_task(object, property).unwrap();
                if let Value::List(items) = input {
                    let mut results = Vec::new();
                    for item in items {
                        results.push(self.call_task_value(
                            &task,
                            std::slice::from_ref(item),
                            property,
                        )?);
                    }
                    return Ok(Value::List(results));
                }
                self.call_task_value(&task, std::slice::from_ref(input), property)
            }
            // Spec 061: `value >> file.task(extra)` — file-namespace task with extra args;
            // the pipeline input is prepended as the first argument (like FunctionCall).
            Expression::MethodCall {
                object,
                method,
                arguments,
            } if self.namespace_stage_task(object, method).is_some() => {
                let task = self.namespace_stage_task(object, method).unwrap();
                let mut args = vec![input.clone()];
                args.extend(self.evaluate_args(arguments)?);
                self.call_task_value(&task, &args, method)
            }
            _ => self.evaluate(expr),
        }
    }

    /// Resolves a file-namespace task referenced as `ns.task` in a pipeline or
    /// broadcast stage (Spec 061), where `ns` is a known file-namespace and not a
    /// value bound in scope. Returns the `Value::Task`, or `None` to fall through to
    /// ordinary evaluation.
    fn namespace_stage_task(&self, object: &Expression, name: &str) -> Option<Value> {
        if let Expression::Identifier(ns) = object {
            if !self.env.has(ns) {
                return self
                    .project_runtime
                    .as_ref()
                    .and_then(|rt| rt.module_task(ns, name));
            }
        }
        None
    }

    // =========================================================================
    // Rescue helpers
    // =========================================================================

    /// Builds the `error` Map injected into rescue handler scope.
    fn build_error_map(err: &MarretaError) -> Value {
        let mut m = ValueMap::new();
        m.insert("message".to_string(), Value::String(err.display_message()));
        m.insert("op".to_string(), Value::String(err.operation_name()));
        m.insert("code".to_string(), Value::String(err.semantic_code()));
        Value::Map(Arc::new(RwLock::new(m)))
    }

    /// Executes the rescue handler body and returns the result.
    fn execute_rescue_handler(&mut self, handler: &RescueHandler) -> Result<Value, MarretaError> {
        match handler {
            RescueHandler::Inline(expr) => self.evaluate(expr),
            RescueHandler::Block(stmts) => self.execute(stmts),
        }
    }

    // =========================================================================
    // Cache dispatch
    // =========================================================================

    fn cache_error(msg: String, op: &str) -> MarretaError {
        MarretaError::CacheError {
            message: msg,
            operation: op.to_string(),
        }
    }

    fn require_cache_driver(&self, op: &str) -> Result<Arc<dyn CacheDriver>, MarretaError> {
        self.cache_driver.clone().ok_or_else(|| MarretaError::CacheError {
            message: "cache.* called but no cache is configured (set MARRETA_CACHE_PROVIDER, MARRETA_CACHE_HOST, MARRETA_CACHE_PORT, and optional MARRETA_CACHE_PASSWORD)".into(),
            operation: op.to_string(),
        })
    }

    /// Resolve the effective TTL: explicit `ttl:` arg > default_ttl config > None.
    fn resolve_ttl(
        &self,
        args: &[Value],
        named_args: &[(String, Value)],
    ) -> Option<std::time::Duration> {
        // Check named arg `ttl:`
        for (name, val) in named_args {
            if name == "ttl"
                && let Some(secs) = val.as_integer()
                && secs > 0
            {
                return Some(std::time::Duration::from_secs(secs as u64));
            }
        }
        // Fall back to config default_ttl (but not for incr/decr — handled by caller)
        let _ = args;
        self.cache_config.as_ref().and_then(|c| c.default_ttl)
    }

    /// Resolve named arg `ttl:` only (no default fallback — used by incr/decr).
    fn resolve_explicit_ttl(&self, named_args: &[(String, Value)]) -> Option<std::time::Duration> {
        for (name, val) in named_args {
            if name == "ttl"
                && let Some(secs) = val.as_integer()
                && secs > 0
            {
                return Some(std::time::Duration::from_secs(secs as u64));
            }
        }
        None
    }

    fn resolve_named_bool(named_args: &[(String, Value)], key: &str) -> bool {
        named_args.iter().any(|(n, v)| n == key && v.is_truthy())
    }

    fn expect_string_value(
        args: &[Value],
        index: usize,
        method: &str,
        line: usize,
        column: usize,
    ) -> Result<String, MarretaError> {
        match args.get(index) {
            Some(Value::String(s)) => Ok(s.clone()),
            Some(other) => Err(MarretaError::TypeError {
                message: format!(
                    "{}() argument {} must be String, got {}",
                    method,
                    index + 1,
                    other.type_name()
                ),
                line,
                column,
            }),
            None => Err(MarretaError::TypeError {
                message: format!("{}() missing required argument {}", method, index + 1),
                line,
                column,
            }),
        }
    }

    fn expect_numeric_i64_value(
        args: &[Value],
        index: usize,
        method: &str,
        line: usize,
        column: usize,
    ) -> Result<i64, MarretaError> {
        match args.get(index) {
            Some(value) => value.as_integer().ok_or_else(|| MarretaError::TypeError {
                message: format!(
                    "{}() argument {} must be Integer, got {}",
                    method,
                    index + 1,
                    value.type_name()
                ),
                line,
                column,
            }),
            None => Err(MarretaError::TypeError {
                message: format!("{}() missing required argument {}", method, index + 1),
                line,
                column,
            }),
        }
    }

    // =========================================================================
    // String interpolation
    // =========================================================================

    fn interpolate_string(&mut self, s: &str) -> Result<Value, MarretaError> {
        if !s.contains("#{") {
            return Ok(Value::String(s.to_string()));
        }

        let _timer = runtime_profile::timer(
            self.runtime_profile_route.as_ref(),
            ProfilePhase::Interpolation,
        );
        let mut result = String::new();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if i + 1 < chars.len() && chars[i] == '#' && chars[i + 1] == '{' {
                i += 2; // skip `#{`
                let start = i;
                let mut depth = 1;
                while i < chars.len() && depth > 0 {
                    if chars[i] == '{' {
                        depth += 1;
                    }
                    if chars[i] == '}' {
                        depth -= 1;
                    }
                    if depth > 0 {
                        i += 1;
                    }
                }
                let inner: String = chars[start..i].iter().collect();
                i += 1; // skip `}`

                // Parse and evaluate the inner expression; undefined vars coerce to null
                let val = self
                    .evaluate_source_expression(inner.trim())
                    .unwrap_or(Value::Null);
                result.push_str(&format!("{}", val));
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }

        Ok(Value::String(result))
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    fn evaluate_args(&mut self, arguments: &[Argument]) -> Result<Vec<Value>, MarretaError> {
        let mut vals = Vec::new();
        for arg in arguments {
            match arg {
                Argument::Positional(expr) => vals.push(self.evaluate(expr)?),
                Argument::Named { value, .. } => vals.push(self.evaluate(value)?),
            }
        }
        Ok(vals)
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// DB conversion helpers (free functions — no interpreter state needed)
// =============================================================================

/// Converts a `Value::Map` to a `DbRow` (HashMap<String, Value>).
/// Returns a TypeError if the value is not a Map.
fn value_to_db_row(val: &Value, line: usize, column: usize) -> Result<DbRow, MarretaError> {
    match val {
        Value::Map(m) => Ok(m.read().unwrap().clone().into_iter().collect()),
        other => Err(MarretaError::TypeError {
            message: format!("expected a map, got {}", other.type_name()),
            line,
            column,
        }),
    }
}

/// Converts a `DbRow` (HashMap<String, Value>) to a `Value::Map`.
fn db_row_to_value(row: DbRow) -> Value {
    Value::Map(Arc::new(RwLock::new(row.into_iter().collect())))
}

#[cfg(test)]
mod mock_db;
#[cfg(test)]
mod mock_doc;
#[cfg(test)]
#[allow(clippy::approx_constant)] // sample float literals (3.14…), not the PI constant
mod tests;
#[cfg(test)]
mod tests_base64_namespace;
#[cfg(test)]
#[allow(clippy::approx_constant)] // sample float literals (3.14…), not the PI constant
mod tests_coverage;
#[cfg(test)]
mod tests_db_mock;
#[cfg(test)]
mod tests_doc_mock;
#[cfg(test)]
mod tests_feature_namespace;
#[cfg(test)]
mod tests_http_client_trace_context;
#[cfg(test)]
mod tests_json_namespace;
#[cfg(test)]
mod tests_log_namespace;
#[cfg(test)]
mod tests_queue;
#[cfg(test)]
mod tests_rescue_raise;
#[cfg(test)]
mod tests_uuid_namespace;
