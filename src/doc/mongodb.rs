use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;

use crate::doc::query::DocQueryState;
use crate::error::MarretaError;
use crate::value::{Value, ValueMap};

// ─── Type aliases ─────────────────────────────────────────────────────────────

pub type DocResult<T> = Result<T, MarretaError>;
pub type DocRow = HashMap<String, Value>;

// ─── DocDriver trait ──────────────────────────────────────────────────────────

/// MongoDB-native driver trait for `doc.*` operations.
///
/// **This is NOT a mirror of `DbDriver`.**
/// `DbDriver` is SQL-centric (joins, select_cols, `begin()` returning `DbTx`).
/// `DocDriver` is MongoDB-native: query methods receive `DocQueryState`,
/// there is no `begin()` (MongoDB transactions require replica sets — out of scope),
/// and there are no joins.
#[async_trait]
pub trait DocDriver: Send + Sync {
    // ─── Direct CRUD ──────────────────────────────────────────────────────

    /// Insert a new document into the collection.
    /// Returns the persisted document with `_id` as `Value::String`.
    async fn save(&self, collection: &str, data: DocRow) -> DocResult<DocRow>;

    /// Find a document by `_id`.
    /// Returns `None` if not found (caller converts to `Value::Null`).
    async fn find(&self, collection: &str, id: &Value) -> DocResult<Option<DocRow>>;

    /// Return all documents in a collection (no filter).
    async fn find_all(&self, collection: &str) -> DocResult<Vec<DocRow>>;

    /// Partial update by `_id` — `$set` semantics.
    /// Uses `find_one_and_update` with `ReturnDocument::After` to return the
    /// updated document in a single atomic round-trip.
    async fn update_by_id(&self, collection: &str, id: &Value, data: DocRow) -> DocResult<DocRow>;

    /// Delete a document by `_id`.
    /// Returns `true` if a document was deleted, `false` if not found.
    async fn delete_by_id(&self, collection: &str, id: &Value) -> DocResult<bool>;

    // ─── Pipeline terminals ───────────────────────────────────────────────

    /// Execute the query and return all matching documents.
    async fn query_fetch(&self, q: &DocQueryState) -> DocResult<Vec<DocRow>>;

    /// Execute the query and return the first matching document, or `None`.
    async fn query_fetch_one(&self, q: &DocQueryState) -> DocResult<Option<DocRow>>;

    /// Count matching documents.
    async fn query_count(&self, q: &DocQueryState) -> DocResult<i64>;

    /// Check if at least one matching document exists.
    async fn query_exists(&self, q: &DocQueryState) -> DocResult<bool>;

    /// Update all matching documents with `$set` semantics.
    /// Returns the number of modified documents.
    async fn query_update(&self, q: &DocQueryState, data: DocRow) -> DocResult<i64>;

    /// Upsert — update matching documents or insert if none match.
    /// Returns the number of upserted/modified documents.
    async fn query_upsert(&self, q: &DocQueryState, data: DocRow) -> DocResult<i64>;

    /// Delete all matching documents.
    /// Returns the number of deleted documents.
    async fn query_delete(&self, q: &DocQueryState) -> DocResult<i64>;

    /// Execute an aggregation pipeline built from `DocQueryState` aggregation fields.
    /// Called when `mode == Aggregate`. Returns grouped/accumulated result rows.
    async fn query_aggregate(&self, q: &DocQueryState) -> DocResult<Vec<DocRow>>;

    /// Execute a raw MQL pipeline expressed as a list of Marreta `Value::Map` stage descriptors.
    /// Each stage map must have exactly one key (the stage name, without `$`).
    /// Called by `doc.pipeline(collection, list)`.
    async fn raw_pipeline(&self, collection: &str, stages: &[Value]) -> DocResult<Vec<DocRow>>;
}

// ─── DocEngine ────────────────────────────────────────────────────────────────

use crate::config::MarretaConfig;

/// Runtime doc engine: holds the active driver behind an `Arc`.
#[derive(Clone)]
pub struct DocEngine {
    pub driver: Arc<dyn DocDriver>,
}

impl DocEngine {
    /// Initializes the doc engine from config.
    /// Reads structured `MARRETA_DOC_*` config — independent of `MARRETA_DB_*`.
    /// Returns `None` if `MARRETA_DOC_PROVIDER` is not set.
    pub async fn from_config(config: &MarretaConfig) -> Result<Option<Self>, MarretaError> {
        if let Some(message) = config.first_config_error() {
            return Err(MarretaError::DbError {
                message: message.to_string(),
                operation: "doc.connect".to_string(),
            });
        }
        let doc = match &config.doc {
            Some(doc) => doc,
            None => return Ok(None),
        };
        let provider_str = doc.provider_name();

        if provider_str.to_lowercase().as_str() != "mongodb" {
            return Err(MarretaError::DbError {
                message: format!(
                    "Unsupported MARRETA_DOC_PROVIDER '{}'. Supported: mongodb",
                    provider_str
                ),
                operation: "doc.connect".to_string(),
            });
        }

        let url = doc
            .connection_url()
            .map_err(|message| MarretaError::DbError {
                message,
                operation: "doc.connect".to_string(),
            })?;

        let pool_cfg = DocPoolConfig {
            max_connections: doc.pool_max_connections,
            min_connections: doc.pool_min_connections,
            connect_timeout_ms: doc.pool_connect_timeout_ms,
            server_selection_timeout_ms: doc.pool_server_selection_timeout_ms,
        };
        let driver = MongoDbDriver::connect(url.as_str(), pool_cfg).await?;

        Ok(Some(DocEngine {
            driver: Arc::new(driver),
        }))
    }
}

// ─── Value ↔ DocRow conversion helpers ────────────────────────────────────────

/// Convert a `Value::Map` into a flat `DocRow` (HashMap).
/// Returns `Err` if the value is not a Map.
pub fn value_to_doc_row(val: &Value, line: usize, column: usize) -> DocResult<DocRow> {
    match val {
        Value::Map(m) => {
            let guard = m.read().unwrap();
            Ok(guard.clone().into_iter().collect())
        }
        other => Err(MarretaError::TypeError {
            message: format!("expected Map for document data, got {}", other.type_name()),
            line,
            column,
        }),
    }
}

/// Convert a `DocRow` (HashMap) into a `Value::Map`.
pub fn doc_row_to_value(row: DocRow) -> Value {
    Value::Map(Arc::new(RwLock::new(row.into_iter().collect::<ValueMap>())))
}

/// Convert a list of `DocRow` into a `Value::List` of `Value::Map`.
pub fn doc_rows_to_value(rows: Vec<DocRow>) -> Value {
    Value::List(rows.into_iter().map(doc_row_to_value).collect())
}

// ─── MongoDbDriver Implementation ─────────────────────────────────────────────

use crate::doc::bson::{bson_to_value, value_to_bson};
use futures_util::StreamExt;
use mongodb::bson;
use mongodb::error::ErrorKind;
use mongodb::{Client, options::ClientOptions};

/// Connection pool configuration for the MongoDB driver.
/// All fields are optional — unset values use the MongoDB driver defaults.
pub struct DocPoolConfig {
    /// Maximum number of connections in the pool (default: 10).
    pub max_connections: Option<u32>,
    /// Minimum number of idle connections to maintain (default: 0).
    pub min_connections: Option<u32>,
    /// Timeout (ms) waiting for a socket connect (default: 10 000 ms).
    pub connect_timeout_ms: Option<u64>,
    /// Timeout (ms) to select a suitable server (default: 30 000 ms).
    pub server_selection_timeout_ms: Option<u64>,
}

pub struct MongoDbDriver {
    pub client: Client,
    pub db_name: String,
}

impl MongoDbDriver {
    pub async fn connect(url: &str, pool_cfg: DocPoolConfig) -> DocResult<Self> {
        let mut options = ClientOptions::parse(url)
            .await
            .map_err(translate_mongo_error)?;

        if let Some(max) = pool_cfg.max_connections {
            options.max_pool_size = Some(max);
        }
        if let Some(min) = pool_cfg.min_connections {
            options.min_pool_size = Some(min);
        }
        if let Some(ms) = pool_cfg.connect_timeout_ms {
            options.connect_timeout = Some(std::time::Duration::from_millis(ms));
        }
        if let Some(ms) = pool_cfg.server_selection_timeout_ms {
            options.server_selection_timeout = Some(std::time::Duration::from_millis(ms));
        }

        let db_name = options
            .default_database
            .clone()
            .unwrap_or_else(|| "test".to_string());

        let client = Client::with_options(options).map_err(translate_mongo_error)?;

        // Ping to verify connection
        client
            .database("admin")
            .run_command(bson::doc! { "ping": 1 })
            .await
            .map_err(translate_mongo_error)?;

        Ok(Self { client, db_name })
    }
}

pub fn translate_mongo_error_op(err: mongodb::error::Error, operation: &str) -> MarretaError {
    let msg = err.to_string();
    let code = match *err.kind {
        ErrorKind::Authentication { .. } => "auth_error",
        ErrorKind::ServerSelection { .. } => "connection_error",
        ErrorKind::Write(_) => "write_error",
        ErrorKind::Command(_) => "command_error",
        _ => "db_error",
    };

    MarretaError::DbError {
        message: format!("mongodb error [{}]: {}", code, msg),
        operation: operation.to_string(),
    }
}

/// Convenience wrapper used by connect/parse where no collection context exists.
pub fn translate_mongo_error(err: mongodb::error::Error) -> MarretaError {
    translate_mongo_error_op(err, "doc.connect")
}

#[async_trait]
impl DocDriver for MongoDbDriver {
    async fn save(&self, collection: &str, data: DocRow) -> DocResult<DocRow> {
        let db = self.client.database(&self.db_name);
        let coll = db.collection::<bson::Document>(collection);

        let val = doc_row_to_value(data);
        let bson_val = value_to_bson(&val);

        let mut insert_doc = match bson_val {
            bson::Bson::Document(d) => d,
            _ => {
                return Err(MarretaError::DbError {
                    message: "Root element must be a map/document".to_string(),
                    operation: format!("doc.{}.save", collection),
                });
            }
        };

        let op = format!("doc.{}.save", collection);
        let result = coll
            .insert_one(insert_doc.clone())
            .await
            .map_err(|e| translate_mongo_error_op(e, &op))?;

        insert_doc.insert("_id", result.inserted_id);

        let row = value_to_doc_row(&bson_to_value(&bson::Bson::Document(insert_doc)), 0, 0)?;
        Ok(row)
    }

    async fn find(&self, collection: &str, id: &Value) -> DocResult<Option<DocRow>> {
        let db = self.client.database(&self.db_name);
        let coll = db.collection::<bson::Document>(collection);

        let id_bson = value_to_bson(id);

        // Smart cast string ID to ObjectId if possible
        let filter_id = if let bson::Bson::String(ref s) = id_bson {
            if let Ok(oid) = bson::oid::ObjectId::parse_str(s) {
                bson::Bson::ObjectId(oid)
            } else {
                id_bson
            }
        } else {
            id_bson
        };

        let filter = bson::doc! { "_id": filter_id };
        let op = format!("doc.{}.find", collection);
        let result = coll
            .find_one(filter)
            .await
            .map_err(|e| translate_mongo_error_op(e, &op))?;

        if let Some(doc) = result {
            Ok(Some(value_to_doc_row(
                &bson_to_value(&bson::Bson::Document(doc)),
                0,
                0,
            )?))
        } else {
            Ok(None)
        }
    }

    async fn find_all(&self, collection: &str) -> DocResult<Vec<DocRow>> {
        let db = self.client.database(&self.db_name);
        let coll = db.collection::<bson::Document>(collection);
        let op = format!("doc.{}.find_all", collection);

        let mut cursor = coll
            .find(bson::doc! {})
            .await
            .map_err(|e| translate_mongo_error_op(e, &op))?;
        let mut rows = Vec::new();

        while let Some(result) = cursor.next().await {
            let doc = result.map_err(|e| translate_mongo_error_op(e, &op))?;
            rows.push(value_to_doc_row(
                &bson_to_value(&bson::Bson::Document(doc)),
                0,
                0,
            )?);
        }

        Ok(rows)
    }

    async fn update_by_id(&self, collection: &str, id: &Value, data: DocRow) -> DocResult<DocRow> {
        let db = self.client.database(&self.db_name);
        let coll = db.collection::<bson::Document>(collection);

        let id_bson = value_to_bson(id);
        let filter_id = if let bson::Bson::String(ref s) = id_bson {
            if let Ok(oid) = bson::oid::ObjectId::parse_str(s) {
                bson::Bson::ObjectId(oid)
            } else {
                id_bson
            }
        } else {
            id_bson
        };

        let filter = bson::doc! { "_id": filter_id };

        let val = doc_row_to_value(data);
        let bson_val = value_to_bson(&val);

        let update_doc = match bson_val {
            bson::Bson::Document(d) => d,
            _ => {
                return Err(MarretaError::DbError {
                    message: "Root element must be a map/document".to_string(),
                    operation: format!("doc.{}.update", collection),
                });
            }
        };

        let update = bson::doc! { "$set": update_doc };

        let options = mongodb::options::FindOneAndUpdateOptions::builder()
            .return_document(mongodb::options::ReturnDocument::After)
            .build();

        let op = format!("doc.{}.update", collection);
        let result = coll
            .find_one_and_update(filter, update)
            .with_options(options)
            .await
            .map_err(|e| translate_mongo_error_op(e, &op))?;

        if let Some(doc) = result {
            Ok(value_to_doc_row(
                &bson_to_value(&bson::Bson::Document(doc)),
                0,
                0,
            )?)
        } else {
            Err(MarretaError::DbError {
                message: "Document not found for update".to_string(),
                operation: format!("doc.{}.update", collection),
            })
        }
    }

    async fn delete_by_id(&self, collection: &str, id: &Value) -> DocResult<bool> {
        let db = self.client.database(&self.db_name);
        let coll = db.collection::<bson::Document>(collection);

        let id_bson = value_to_bson(id);
        let filter_id = if let bson::Bson::String(ref s) = id_bson {
            if let Ok(oid) = bson::oid::ObjectId::parse_str(s) {
                bson::Bson::ObjectId(oid)
            } else {
                id_bson
            }
        } else {
            id_bson
        };

        let filter = bson::doc! { "_id": filter_id };
        let op = format!("doc.{}.delete", collection);
        let result = coll
            .delete_one(filter)
            .await
            .map_err(|e| translate_mongo_error_op(e, &op))?;

        Ok(result.deleted_count > 0)
    }

    async fn query_fetch(&self, q: &DocQueryState) -> DocResult<Vec<DocRow>> {
        let db = self.client.database(&self.db_name);
        let coll = db.collection::<bson::Document>(&q.collection);
        let filter = build_query_filter(q);
        let options = build_query_options(q);

        let op = format!("doc.{}.fetch_all", q.collection);
        let mut cursor = coll
            .find(filter)
            .with_options(Some(options))
            .await
            .map_err(|e| translate_mongo_error_op(e, &op))?;
        let mut rows = Vec::new();
        while let Some(result) = cursor.next().await {
            let doc = result.map_err(|e| translate_mongo_error_op(e, &op))?;
            rows.push(value_to_doc_row(
                &bson_to_value(&bson::Bson::Document(doc)),
                0,
                0,
            )?);
        }
        Ok(rows)
    }

    async fn query_fetch_one(&self, q: &DocQueryState) -> DocResult<Option<DocRow>> {
        let db = self.client.database(&self.db_name);
        let coll = db.collection::<bson::Document>(&q.collection);
        let filter = build_query_filter(q);

        let options = build_query_options(q);
        let mut f_opts = mongodb::options::FindOneOptions::default();
        if let Some(sort) = options.sort {
            f_opts.sort = Some(sort);
        }
        if let Some(proj) = options.projection {
            f_opts.projection = Some(proj);
        }
        if let Some(skip) = options.skip {
            f_opts.skip = Some(skip);
        }

        let op = format!("doc.{}.fetch_one", q.collection);
        let result = coll
            .find_one(filter)
            .with_options(Some(f_opts))
            .await
            .map_err(|e| translate_mongo_error_op(e, &op))?;
        if let Some(doc) = result {
            Ok(Some(value_to_doc_row(
                &bson_to_value(&bson::Bson::Document(doc)),
                0,
                0,
            )?))
        } else {
            Ok(None)
        }
    }

    async fn query_count(&self, q: &DocQueryState) -> DocResult<i64> {
        let db = self.client.database(&self.db_name);
        let coll = db.collection::<bson::Document>(&q.collection);
        let filter = build_query_filter(q);
        let op = format!("doc.{}.count", q.collection);
        let count = coll
            .count_documents(filter)
            .await
            .map_err(|e| translate_mongo_error_op(e, &op))?;
        Ok(count as i64)
    }

    async fn query_exists(&self, q: &DocQueryState) -> DocResult<bool> {
        let db = self.client.database(&self.db_name);
        let coll = db.collection::<bson::Document>(&q.collection);
        let filter = build_query_filter(q);
        let op = format!("doc.{}.exists", q.collection);

        let mut f_opts = mongodb::options::FindOneOptions::default();
        let options = build_query_options(q);
        if let Some(skip) = options.skip {
            f_opts.skip = Some(skip);
        }

        let result = coll
            .find_one(filter)
            .with_options(Some(f_opts))
            .await
            .map_err(|e| translate_mongo_error_op(e, &op))?;
        Ok(result.is_some())
    }

    async fn query_update(&self, q: &DocQueryState, data: DocRow) -> DocResult<i64> {
        let db = self.client.database(&self.db_name);
        let coll = db.collection::<bson::Document>(&q.collection);
        let filter = build_query_filter(q);
        let update =
            bson::doc! { "$set": crate::doc::bson::value_to_bson(&doc_row_to_value(data.clone())) };
        let op = format!("doc.{}.update", q.collection);

        let result = coll
            .update_many(filter, update)
            .await
            .map_err(|e| translate_mongo_error_op(e, &op))?;
        Ok(result.modified_count as i64)
    }

    async fn query_upsert(&self, q: &DocQueryState, data: DocRow) -> DocResult<i64> {
        let db = self.client.database(&self.db_name);
        let coll = db.collection::<bson::Document>(&q.collection);
        let filter = build_query_filter(q);

        let update = if data.contains_key("$set") || data.contains_key("$inc") {
            crate::doc::bson::value_to_bson(&doc_row_to_value(data.clone()))
        } else {
            bson::doc! { "$set": crate::doc::bson::value_to_bson(&doc_row_to_value(data.clone())) }
                .into()
        };

        let update_doc = update
            .as_document()
            .ok_or_else(|| MarretaError::DbError {
                message: "upsert data must be a map/document".to_string(),
                operation: format!("doc.{}.upsert", q.collection),
            })?
            .clone();
        let mut opts = mongodb::options::UpdateOptions::default();
        opts.upsert = Some(true);
        let op = format!("doc.{}.upsert", q.collection);
        let result = coll
            .update_many(filter, update_doc)
            .with_options(Some(opts))
            .await
            .map_err(|e| translate_mongo_error_op(e, &op))?;
        Ok((result.modified_count + result.upserted_id.map(|_| 1).unwrap_or(0)) as i64)
    }

    async fn query_delete(&self, q: &DocQueryState) -> DocResult<i64> {
        let db = self.client.database(&self.db_name);
        let coll = db.collection::<bson::Document>(&q.collection);
        let filter = build_query_filter(q);
        let op = format!("doc.{}.delete", q.collection);
        let result = coll
            .delete_many(filter)
            .await
            .map_err(|e| translate_mongo_error_op(e, &op))?;
        Ok(result.deleted_count as i64)
    }

    async fn query_aggregate(&self, q: &DocQueryState) -> DocResult<Vec<DocRow>> {
        use crate::doc::query::{Accumulator, SortDirection};

        let db = self.client.database(&self.db_name);
        let coll = db.collection::<bson::Document>(&q.collection);
        let op = format!("doc.{}.aggregate", q.collection);

        let mut pipeline: Vec<bson::Document> = Vec::new();

        // Stage 1 — $match (pre-group filters)
        let filter = build_query_filter(q);
        if !filter.is_empty() {
            pipeline.push(bson::doc! { "$match": filter });
        }

        // Stage 2 — $group
        let mut group_doc = bson::Document::new();
        // _id: "$field" for grouped, null for global aggregation
        match &q.group_by {
            Some(field) => group_doc.insert("_id", format!("${}", field)),
            None => group_doc.insert("_id", bson::Bson::Null),
        };
        for acc in &q.accumulators {
            match acc {
                Accumulator::Sum { field, alias } => {
                    group_doc.insert(alias, bson::doc! { "$sum": format!("${}", field) });
                }
                Accumulator::Avg { field, alias } => {
                    group_doc.insert(alias, bson::doc! { "$avg": format!("${}", field) });
                }
                Accumulator::Min { field, alias } => {
                    group_doc.insert(alias, bson::doc! { "$min": format!("${}", field) });
                }
                Accumulator::Max { field, alias } => {
                    group_doc.insert(alias, bson::doc! { "$max": format!("${}", field) });
                }
                Accumulator::Count { alias } => {
                    group_doc.insert(alias, bson::doc! { "$sum": 1_i32 });
                }
            }
        }
        pipeline.push(bson::doc! { "$group": group_doc });

        // Stage 3 — $sort (post-group)
        if let Some((field, dir)) = &q.post_sort {
            let order = if *dir == SortDirection::Asc {
                1_i32
            } else {
                -1_i32
            };
            pipeline.push(bson::doc! { "$sort": { field: order } });
        }

        // Stage 4 — $limit (post-group)
        if let Some(n) = q.post_limit {
            pipeline.push(bson::doc! { "$limit": n });
        }

        let mut cursor = coll
            .aggregate(pipeline)
            .await
            .map_err(|e| translate_mongo_error_op(e, &op))?;
        let mut rows = Vec::new();
        while let Some(doc) = cursor.next().await {
            let doc = doc.map_err(|e| translate_mongo_error_op(e, &op))?;
            let val = bson_to_value(&bson::Bson::Document(doc));
            rows.push(crate::doc::mongodb::value_to_doc_row(&val, 0, 0)?);
        }
        Ok(rows)
    }

    async fn raw_pipeline(&self, collection: &str, stages: &[Value]) -> DocResult<Vec<DocRow>> {
        let db = self.client.database(&self.db_name);
        let coll = db.collection::<bson::Document>(collection);
        let op = format!("doc.pipeline({})", collection);

        let mut pipeline: Vec<bson::Document> = Vec::new();
        for stage_val in stages {
            let stage_doc = translate_pipeline_stage(stage_val)?;
            pipeline.push(stage_doc);
        }

        let mut cursor = coll
            .aggregate(pipeline)
            .await
            .map_err(|e| translate_mongo_error_op(e, &op))?;
        let mut rows = Vec::new();
        while let Some(doc) = cursor.next().await {
            let doc = doc.map_err(|e| translate_mongo_error_op(e, &op))?;
            let val = bson_to_value(&bson::Bson::Document(doc));
            rows.push(crate::doc::mongodb::value_to_doc_row(&val, 0, 0)?);
        }
        Ok(rows)
    }
}

// ─── Layer 4: Pipeline Stage Translation ─────────────────────────────────────

fn db_err(msg: impl Into<String>) -> MarretaError {
    MarretaError::DbError {
        message: msg.into(),
        operation: "doc.pipeline".to_string(),
    }
}

/// Translates a single Marreta stage map (e.g. `{ match: { status: "paid" } }`)
/// to a BSON pipeline stage document (`{ "$match": { "status": "paid" } }`).
/// The outer key gains a `$` prefix. Inner values are translated recursively.
pub fn translate_pipeline_stage(stage_val: &Value) -> DocResult<bson::Document> {
    let arc = match stage_val {
        Value::Map(m) => m.clone(),
        _ => {
            return Err(db_err(
                "doc.pipeline stage must be a map (e.g. { match: { ... } })",
            ));
        }
    };
    let guard = arc.read().unwrap();

    if guard.len() != 1 {
        return Err(db_err(format!(
            "doc.pipeline stage map must have exactly one key, got {}",
            guard.len()
        )));
    }

    let (stage_key, stage_inner) = guard.iter().next().unwrap();
    let stage_key = stage_key.clone();
    let stage_inner = stage_inner.clone();
    drop(guard);

    let bson_stage = match stage_key.as_str() {
        "match" => {
            let doc = translate_stage_value_to_doc(&stage_inner)?;
            bson::doc! { "$match": doc }
        }
        "sort" => {
            let doc = translate_stage_value_to_doc(&stage_inner)?;
            bson::doc! { "$sort": doc }
        }
        "limit" => {
            let n = extract_i64(&stage_inner, "limit")?;
            bson::doc! { "$limit": n }
        }
        "skip" => {
            let n = extract_i64(&stage_inner, "skip")?;
            bson::doc! { "$skip": n }
        }
        "unwind" => {
            let field = extract_string(&stage_inner, "unwind")?;
            let field_ref = if field.starts_with('$') {
                field
            } else {
                format!("${}", field)
            };
            bson::doc! { "$unwind": field_ref }
        }
        "add_fields" => {
            let doc = translate_stage_value_to_doc(&stage_inner)?;
            bson::doc! { "$addFields": doc }
        }
        "project" => {
            let doc = translate_stage_value_to_doc(&stage_inner)?;
            bson::doc! { "$project": doc }
        }
        "count" => {
            let field = extract_string(&stage_inner, "count")?;
            bson::doc! { "$count": field }
        }
        "lookup" => {
            let inner = translate_lookup(&stage_inner)?;
            bson::doc! { "$lookup": inner }
        }
        "group" => {
            let inner = translate_group(&stage_inner)?;
            bson::doc! { "$group": inner }
        }
        "bucket" => {
            let inner = translate_bucket(&stage_inner)?;
            bson::doc! { "$bucket": inner }
        }
        other => {
            return Err(db_err(format!(
                "unknown doc.pipeline stage '{}' — supported: match, sort, limit, skip, unwind, add_fields, project, count, lookup, group, bucket",
                other
            )));
        }
    };

    Ok(bson_stage)
}

/// Converts a Marreta `Value` to a BSON `Document`. The value must be a map.
fn translate_stage_value_to_doc(val: &Value) -> DocResult<bson::Document> {
    match val {
        Value::Map(arc) => {
            let guard = arc.read().unwrap();
            let mut doc = bson::Document::new();
            for (k, v) in guard.iter() {
                doc.insert(k.clone(), translate_stage_value_to_bson(v));
            }
            Ok(doc)
        }
        _ => Err(db_err(format!(
            "expected a map for stage inner value, got {}",
            val.type_name()
        ))),
    }
}

/// Recursively translates a Marreta `Value` to BSON for use inside a pipeline stage.
/// String values starting with `$` are passed through as-is (field references).
fn translate_stage_value_to_bson(val: &Value) -> bson::Bson {
    match val {
        Value::Map(arc) => {
            let guard = arc.read().unwrap();
            let mut doc = bson::Document::new();
            for (k, v) in guard.iter() {
                // Sub-map keys that are accumulator names map to their MQL operator
                let bson_key = if let Some(mql_op) = accumulator_mql_key(k) {
                    mql_op.to_string()
                } else {
                    k.clone()
                };
                doc.insert(bson_key, translate_stage_value_to_bson(v));
            }
            bson::Bson::Document(doc)
        }
        Value::List(items) => {
            let bson_items: Vec<bson::Bson> =
                items.iter().map(translate_stage_value_to_bson).collect();
            bson::Bson::Array(bson_items)
        }
        other => crate::doc::bson::value_to_bson(other),
    }
}

/// Map a Marreta accumulator key to its MQL `$` operator.
/// `count` inside a `$group` accumulator map is an alias for `$sum` (use value `1`).
fn accumulator_mql_key(k: &str) -> Option<&'static str> {
    match k {
        "sum" => Some("$sum"),
        "avg" => Some("$avg"),
        "min" => Some("$min"),
        "max" => Some("$max"),
        "first" => Some("$first"),
        "last" => Some("$last"),
        "push" => Some("$push"),
        "addToSet" => Some("$addToSet"),
        // `count: N` in a group accumulator means `$sum: N` (e.g. `{ count: 1 }` → `{ $sum: 1 }`)
        "count" => Some("$sum"),
        _ => None,
    }
}

fn translate_lookup(val: &Value) -> DocResult<bson::Document> {
    match val {
        Value::Map(arc) => {
            let guard = arc.read().unwrap();
            let mut doc = bson::Document::new();
            for (k, v) in guard.iter() {
                let key = match k.as_str() {
                    "local" => "localField",
                    "foreign" => "foreignField",
                    other => other,
                };
                doc.insert(key, crate::doc::bson::value_to_bson(v));
            }
            Ok(doc)
        }
        _ => Err(db_err("lookup stage value must be a map")),
    }
}

fn translate_group(val: &Value) -> DocResult<bson::Document> {
    match val {
        Value::Map(arc) => {
            let guard = arc.read().unwrap();
            let mut doc = bson::Document::new();
            for (k, v) in guard.iter() {
                if k == "by" {
                    // by: "field" → _id: "$field"; by: null → _id: null
                    let id_val = match v {
                        Value::Null => bson::Bson::Null,
                        Value::String(s) => {
                            if s.starts_with('$') {
                                bson::Bson::String(s.clone())
                            } else {
                                bson::Bson::String(format!("${}", s))
                            }
                        }
                        other => crate::doc::bson::value_to_bson(other),
                    };
                    doc.insert("_id", id_val);
                } else {
                    // accumulator field: sub-map keys gain $ prefix
                    doc.insert(k.clone(), translate_stage_value_to_bson(v));
                }
            }
            Ok(doc)
        }
        _ => Err(db_err("group stage value must be a map")),
    }
}

fn translate_bucket(val: &Value) -> DocResult<bson::Document> {
    match val {
        Value::Map(arc) => {
            let guard = arc.read().unwrap();
            let mut doc = bson::Document::new();
            for (k, v) in guard.iter() {
                match k.as_str() {
                    "by" => {
                        doc.insert("groupBy", crate::doc::bson::value_to_bson(v));
                    }
                    "output" => {
                        // output sub-keys are accumulator maps
                        let out_doc = translate_stage_value_to_doc(v)?;
                        doc.insert("output", out_doc);
                    }
                    other => {
                        doc.insert(other, translate_stage_value_to_bson(v));
                    }
                }
            }
            Ok(doc)
        }
        _ => Err(db_err("bucket stage value must be a map")),
    }
}

fn extract_i64(val: &Value, stage: &str) -> DocResult<i64> {
    match val {
        Value::Integer(n) => Ok(*n),
        _ => Err(db_err(format!(
            "doc.pipeline '{}' stage expects an integer value",
            stage
        ))),
    }
}

fn extract_string(val: &Value, stage: &str) -> DocResult<String> {
    match val {
        Value::String(s) => Ok(s.clone()),
        _ => Err(db_err(format!(
            "doc.pipeline '{}' stage expects a string value",
            stage
        ))),
    }
}

// ─── Query Builder Helpers ──────────────────────────────────────────────────
fn build_query_filter(q: &DocQueryState) -> bson::Document {
    use crate::doc::query::DocFilter;
    let mut and_list = Vec::new();

    for f in &q.filters {
        match f {
            DocFilter::Eq(field, val) => {
                let mut b_val = crate::doc::bson::value_to_bson(val);
                if (field == "_id" || field == "id")
                    && let bson::Bson::String(s) = &b_val
                    && let Ok(oid) = bson::oid::ObjectId::parse_str(s)
                {
                    b_val = bson::Bson::ObjectId(oid);
                }
                and_list.push(bson::doc! { field: { "$eq": b_val } });
            }
            DocFilter::Ne(field, val) => {
                let mut b_val = crate::doc::bson::value_to_bson(val);
                if (field == "_id" || field == "id")
                    && let bson::Bson::String(s) = &b_val
                    && let Ok(oid) = bson::oid::ObjectId::parse_str(s)
                {
                    b_val = bson::Bson::ObjectId(oid);
                }
                and_list.push(bson::doc! { field: { "$ne": b_val } });
            }
            DocFilter::Gt(f, v) => {
                and_list.push(bson::doc! { f: { "$gt": crate::doc::bson::value_to_bson(v) } })
            }
            DocFilter::Gte(f, v) => {
                and_list.push(bson::doc! { f: { "$gte": crate::doc::bson::value_to_bson(v) } })
            }
            DocFilter::Lt(f, v) => {
                and_list.push(bson::doc! { f: { "$lt": crate::doc::bson::value_to_bson(v) } })
            }
            DocFilter::Lte(f, v) => {
                and_list.push(bson::doc! { f: { "$lte": crate::doc::bson::value_to_bson(v) } })
            }
            DocFilter::Like(f, v) => {
                let pattern = v.replace("%", ".*").replace("_", ".");
                and_list.push(bson::doc! { f: { "$regex": pattern, "$options": "i" } });
            }
            DocFilter::In(f, vals) => {
                let mut bsons = Vec::new();
                for v in vals {
                    let mut b_val = crate::doc::bson::value_to_bson(v);
                    if (f == "_id" || f == "id")
                        && let bson::Bson::String(s) = &b_val
                        && let Ok(oid) = bson::oid::ObjectId::parse_str(s)
                    {
                        b_val = bson::Bson::ObjectId(oid);
                    }
                    bsons.push(b_val);
                }
                and_list.push(bson::doc! { f: { "$in": bsons } });
            }
        }
    }

    if and_list.is_empty() {
        bson::Document::new()
    } else {
        bson::doc! { "$and": and_list }
    }
}

fn build_query_options(q: &DocQueryState) -> mongodb::options::FindOptions {
    let mut opts = mongodb::options::FindOptions::default();

    if let Some((field, dir)) = &q.sort {
        let sort_val = if *dir == crate::doc::query::SortDirection::Asc {
            1
        } else {
            -1
        };
        opts.sort = Some(bson::doc! { field: sort_val });
    }
    if let Some(limit) = q.limit {
        opts.limit = Some(limit);
    }
    if let Some(skip) = q.offset {
        opts.skip = Some(skip as u64);
    }
    if let Some(proj) = &q.projection {
        let mut proj_doc = bson::Document::new();
        for p in proj {
            proj_doc.insert(p, 1);
        }
        opts.projection = Some(proj_doc);
    }

    opts
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_to_doc_row_from_map() {
        let val = Value::map_from(vec![
            ("name".into(), Value::String("Ana".into())),
            ("age".into(), Value::Integer(30)),
        ]);
        let row = value_to_doc_row(&val, 0, 0).unwrap();
        assert_eq!(row.get("name").unwrap().type_name(), "String");
        assert_eq!(row.get("age").unwrap().type_name(), "Integer");
    }

    #[test]
    fn test_value_to_doc_row_from_non_map_errors() {
        let val = Value::Integer(42);
        let err = value_to_doc_row(&val, 0, 0).unwrap_err();
        assert!(err.to_string().contains("expected Map"));
    }

    #[test]
    fn test_doc_row_to_value() {
        let mut row = HashMap::new();
        row.insert("x".into(), Value::Integer(1));
        let val = doc_row_to_value(row);
        assert_eq!(val.type_name(), "Map");
    }

    #[test]
    fn test_doc_rows_to_value() {
        let rows = vec![
            {
                let mut r = HashMap::new();
                r.insert("id".into(), Value::Integer(1));
                r
            },
            {
                let mut r = HashMap::new();
                r.insert("id".into(), Value::Integer(2));
                r
            },
        ];
        let val = doc_rows_to_value(rows);
        if let Value::List(items) = val {
            assert_eq!(items.len(), 2);
        } else {
            panic!("expected List");
        }
    }
}
