use std::sync::Arc;

use async_trait::async_trait;

use crate::doc::mongodb::DocDriver;
use crate::doc::mongodb::{DocEngine, DocResult, DocRow};
use crate::doc::query::DocQueryState;
use crate::value::Value;

pub struct MockDocDriver;

#[async_trait]
impl DocDriver for MockDocDriver {
    async fn save(&self, _col: &str, data: DocRow) -> DocResult<DocRow> {
        Ok(data)
    }
    async fn find(&self, _col: &str, _id: &Value) -> DocResult<Option<DocRow>> {
        Ok(None)
    }
    async fn find_all(&self, _col: &str) -> DocResult<Vec<DocRow>> {
        Ok(vec![])
    }
    async fn update_by_id(&self, _col: &str, _id: &Value, data: DocRow) -> DocResult<DocRow> {
        Ok(data)
    }
    async fn delete_by_id(&self, _col: &str, _id: &Value) -> DocResult<bool> {
        Ok(false)
    }
    async fn query_fetch(&self, _q: &DocQueryState) -> DocResult<Vec<DocRow>> {
        Ok(vec![])
    }
    async fn query_fetch_one(&self, _q: &DocQueryState) -> DocResult<Option<DocRow>> {
        Ok(None)
    }
    async fn query_count(&self, _q: &DocQueryState) -> DocResult<i64> {
        Ok(0)
    }
    async fn query_exists(&self, _q: &DocQueryState) -> DocResult<bool> {
        Ok(false)
    }
    async fn query_update(&self, _q: &DocQueryState, _data: DocRow) -> DocResult<i64> {
        Ok(0)
    }
    async fn query_upsert(&self, _q: &DocQueryState, _data: DocRow) -> DocResult<i64> {
        Ok(0)
    }
    async fn query_delete(&self, _q: &DocQueryState) -> DocResult<i64> {
        Ok(0)
    }
    async fn query_aggregate(&self, _q: &DocQueryState) -> DocResult<Vec<DocRow>> {
        // Return a single mock group row for testing
        let mut row = DocRow::new();
        row.insert("_id".into(), Value::String("mock_group".into()));
        row.insert("total".into(), Value::Integer(100));
        row.insert("n".into(), Value::Integer(2));
        Ok(vec![row])
    }
    async fn raw_pipeline(&self, _collection: &str, _stages: &[Value]) -> DocResult<Vec<DocRow>> {
        let mut row = DocRow::new();
        row.insert("_id".into(), Value::String("mock_pipeline".into()));
        row.insert("total".into(), Value::Integer(42));
        Ok(vec![row])
    }
}

pub fn interp_with_doc() -> super::Interpreter {
    let engine = DocEngine {
        driver: Arc::new(MockDocDriver),
    };
    super::Interpreter::new().with_doc(engine)
}
