use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::db::driver::{DbDriver, DbResult, DbRow, DbTx, FilterClause, QueryState};
use crate::db::{DbEngine, DbProvider};
use crate::value::Value;

// ── MockTx ────────────────────────────────────────────────────────────────

pub struct MockTx {
    pub committed: Arc<Mutex<bool>>,
    pub rolled_back: Arc<Mutex<bool>>,
    pub save_result: DbRow,
    pub find_result: Option<DbRow>,
    pub find_all_result: Vec<DbRow>,
    pub fetch_result: Vec<DbRow>,
    pub fetch_one_result: Option<DbRow>,
    pub count_result: i64,
    pub exists_result: bool,
    pub query_update_result: u64,
    pub query_delete_result: u64,
    pub native_result: Vec<DbRow>,
}

impl MockTx {
    pub fn new(seed: DbRow) -> Self {
        Self {
            committed: Arc::new(Mutex::new(false)),
            rolled_back: Arc::new(Mutex::new(false)),
            save_result: seed.clone(),
            find_result: Some(seed.clone()),
            find_all_result: vec![seed.clone()],
            fetch_result: vec![seed.clone()],
            fetch_one_result: Some(seed),
            count_result: 1,
            exists_result: true,
            query_update_result: 1,
            query_delete_result: 1,
            native_result: vec![],
        }
    }
}

#[async_trait]
impl DbTx for MockTx {
    async fn save(&mut self, _t: &str, _d: DbRow) -> DbResult<DbRow> {
        Ok(self.save_result.clone())
    }
    async fn find(&mut self, _t: &str, _id: &Value) -> DbResult<Option<DbRow>> {
        Ok(self.find_result.clone())
    }
    async fn find_all(&mut self, _t: &str, _f: Vec<FilterClause>) -> DbResult<Vec<DbRow>> {
        Ok(self.find_all_result.clone())
    }
    async fn update_by_id(&mut self, _t: &str, _id: &Value, _d: DbRow) -> DbResult<Option<DbRow>> {
        Ok(Some(self.save_result.clone()))
    }
    async fn delete_by_id(&mut self, _t: &str, _id: &Value) -> DbResult<bool> {
        Ok(true)
    }
    async fn query_fetch(&mut self, _q: &QueryState) -> DbResult<Vec<DbRow>> {
        Ok(self.fetch_result.clone())
    }
    async fn query_fetch_one(&mut self, _q: &QueryState) -> DbResult<Option<DbRow>> {
        Ok(self.fetch_one_result.clone())
    }
    async fn query_count(&mut self, _q: &QueryState) -> DbResult<i64> {
        Ok(self.count_result)
    }
    async fn query_exists(&mut self, _q: &QueryState) -> DbResult<bool> {
        Ok(self.exists_result)
    }
    async fn query_update(&mut self, _q: &QueryState, _d: DbRow) -> DbResult<u64> {
        Ok(self.query_update_result)
    }
    async fn query_delete(&mut self, _q: &QueryState) -> DbResult<u64> {
        Ok(self.query_delete_result)
    }
    async fn native_query(&mut self, _sql: &str, _p: Vec<Value>) -> DbResult<Vec<DbRow>> {
        Ok(self.native_result.clone())
    }
    async fn commit(self: Box<Self>) -> DbResult<()> {
        *self.committed.lock().unwrap() = true;
        Ok(())
    }
    async fn rollback(self: Box<Self>) -> DbResult<()> {
        *self.rolled_back.lock().unwrap() = true;
        Ok(())
    }
}

// ── MockDriver ────────────────────────────────────────────────────────────

pub struct MockDriver {
    pub seed: DbRow,
}

impl MockDriver {
    pub fn new(seed: DbRow) -> Self {
        Self { seed }
    }
}

#[async_trait]
impl DbDriver for MockDriver {
    async fn save(&self, _t: &str, _d: DbRow) -> DbResult<DbRow> {
        Ok(self.seed.clone())
    }
    async fn find(&self, _t: &str, _id: &Value) -> DbResult<Option<DbRow>> {
        Ok(Some(self.seed.clone()))
    }
    async fn find_all(&self, _t: &str, _f: Vec<FilterClause>) -> DbResult<Vec<DbRow>> {
        Ok(vec![self.seed.clone()])
    }
    async fn update_by_id(&self, _t: &str, _id: &Value, _d: DbRow) -> DbResult<Option<DbRow>> {
        Ok(Some(self.seed.clone()))
    }
    async fn delete_by_id(&self, _t: &str, _id: &Value) -> DbResult<bool> {
        Ok(true)
    }
    async fn query_fetch(&self, _q: &QueryState) -> DbResult<Vec<DbRow>> {
        Ok(vec![self.seed.clone()])
    }
    async fn query_fetch_one(&self, _q: &QueryState) -> DbResult<Option<DbRow>> {
        Ok(Some(self.seed.clone()))
    }
    async fn query_count(&self, _q: &QueryState) -> DbResult<i64> {
        Ok(42)
    }
    async fn query_exists(&self, _q: &QueryState) -> DbResult<bool> {
        Ok(true)
    }
    async fn query_update(&self, _q: &QueryState, _d: DbRow) -> DbResult<u64> {
        Ok(3)
    }
    async fn query_delete(&self, _q: &QueryState) -> DbResult<u64> {
        Ok(2)
    }
    async fn native_query(&self, _sql: &str, _p: Vec<Value>) -> DbResult<Vec<DbRow>> {
        Ok(vec![self.seed.clone()])
    }
    async fn begin(&self) -> DbResult<Box<dyn DbTx>> {
        Ok(Box::new(MockTx::new(self.seed.clone())))
    }
}

/// Creates an `Interpreter` pre-wired with a `MockDriver` seeded with `row`.
pub fn interp_with_mock(seed: DbRow) -> super::Interpreter {
    let driver = Arc::new(MockDriver::new(seed));
    let engine = DbEngine {
        driver,
        provider: DbProvider::Postgres,
    };
    super::Interpreter::new().with_db(engine)
}

/// Seed row helper: `{ "id": 1, "name": "Alice" }`
pub fn seed_row() -> DbRow {
    let mut m = HashMap::new();
    m.insert("id".into(), Value::Integer(1));
    m.insert("name".into(), Value::String("Alice".into()));
    m
}
