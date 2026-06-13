use std::collections::HashMap;

use async_trait::async_trait;

use crate::error::MarretaError;
use crate::value::Value;

// ─── Filter types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum FilterOp {
    Eq,
    Gt,
    Gte,
    Lt,
    Lte,
    Ne,
    Like,
    In,
}

impl FilterOp {
    pub fn to_sql(&self) -> &'static str {
        match self {
            FilterOp::Eq => "=",
            FilterOp::Gt => ">",
            FilterOp::Gte => ">=",
            FilterOp::Lt => "<",
            FilterOp::Lte => "<=",
            FilterOp::Ne => "!=",
            FilterOp::Like => "LIKE",
            FilterOp::In => "IN",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FilterClause {
    pub column: String,
    pub op: FilterOp,
    pub value: Value,
}

// ─── Join types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum JoinKind {
    Inner,
    Left,
}

#[derive(Debug, Clone)]
pub struct JoinClause {
    pub kind: JoinKind,
    /// Target table to join
    pub table: String,
    /// Foreign key column on the left (source) table
    pub on: String,
}

// ─── QueryState ───────────────────────────────────────────────────────────────

/// Accumulated pipeline state for a lazy query.
/// Grows as pipeline steps are added; executed only when a terminal is reached.
#[derive(Debug, Clone)]
pub struct QueryState {
    pub table: String,
    pub filters: Vec<FilterClause>,
    pub joins: Vec<JoinClause>,
    pub select_cols: Vec<String>,
    pub order_by: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    /// Spec 076: a `COUNT(*)` query. Rendered by the builder as trusted SQL, so user `select_cols`
    /// are not consulted (and not stuffed with a raw `COUNT(*)` expression that would trip the
    /// identifier guard).
    pub count: bool,
    /// Spec 076: the table's known column names, when a `db:` schema declares it. The identifier
    /// guard's schema layer uses it to reject an unknown column. `None` for a schema-less table
    /// (the syntactic floor still guards it).
    pub known_columns: Option<std::collections::HashSet<String>>,
}

impl QueryState {
    pub fn new(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            filters: Vec::new(),
            joins: Vec::new(),
            select_cols: Vec::new(),
            order_by: None,
            limit: None,
            offset: None,
            count: false,
            known_columns: None,
        }
    }
}

// ─── DbDriver trait ───────────────────────────────────────────────────────────

pub type DbResult<T> = Result<T, MarretaError>;
pub type DbRow = HashMap<String, Value>;

#[async_trait]
pub trait DbDriver: Send + Sync {
    // Direct operations
    async fn save(&self, table: &str, data: DbRow) -> DbResult<DbRow>;
    async fn find(&self, table: &str, id: &Value) -> DbResult<Option<DbRow>>;
    async fn find_all(&self, table: &str, filters: Vec<FilterClause>) -> DbResult<Vec<DbRow>>;
    async fn update_by_id(&self, table: &str, id: &Value, data: DbRow) -> DbResult<Option<DbRow>>;
    async fn delete_by_id(&self, table: &str, id: &Value) -> DbResult<bool>;

    // Pipeline terminals
    async fn query_fetch(&self, q: &QueryState) -> DbResult<Vec<DbRow>>;
    async fn query_fetch_one(&self, q: &QueryState) -> DbResult<Option<DbRow>>;
    async fn query_count(&self, q: &QueryState) -> DbResult<i64>;
    async fn query_exists(&self, q: &QueryState) -> DbResult<bool>;
    async fn query_update(&self, q: &QueryState, data: DbRow) -> DbResult<u64>;
    async fn query_delete(&self, q: &QueryState) -> DbResult<u64>;

    // Native query
    async fn native_query(&self, sql: &str, params: Vec<Value>) -> DbResult<Vec<DbRow>>;

    // Transaction
    async fn begin(&self) -> DbResult<Box<dyn DbTx>>;
}

// ─── DbTx trait ───────────────────────────────────────────────────────────────

/// A single in-flight database transaction.
/// All methods take `&mut self` so every call reuses the same connection.
/// Call `commit` or `rollback` to finalize (both consume the box).
#[async_trait]
pub trait DbTx: Send {
    async fn save(&mut self, table: &str, data: DbRow) -> DbResult<DbRow>;
    async fn find(&mut self, table: &str, id: &Value) -> DbResult<Option<DbRow>>;
    async fn find_all(&mut self, table: &str, filters: Vec<FilterClause>) -> DbResult<Vec<DbRow>>;
    async fn update_by_id(
        &mut self,
        table: &str,
        id: &Value,
        data: DbRow,
    ) -> DbResult<Option<DbRow>>;
    async fn delete_by_id(&mut self, table: &str, id: &Value) -> DbResult<bool>;
    async fn query_fetch(&mut self, q: &QueryState) -> DbResult<Vec<DbRow>>;
    async fn query_fetch_one(&mut self, q: &QueryState) -> DbResult<Option<DbRow>>;
    async fn query_count(&mut self, q: &QueryState) -> DbResult<i64>;
    async fn query_exists(&mut self, q: &QueryState) -> DbResult<bool>;
    async fn query_update(&mut self, q: &QueryState, data: DbRow) -> DbResult<u64>;
    async fn query_delete(&mut self, q: &QueryState) -> DbResult<u64>;
    async fn native_query(&mut self, sql: &str, params: Vec<Value>) -> DbResult<Vec<DbRow>>;
    async fn commit(self: Box<Self>) -> DbResult<()>;
    async fn rollback(self: Box<Self>) -> DbResult<()>;
}
