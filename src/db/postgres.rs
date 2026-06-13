use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{Column, PgPool, Row, TypeInfo};

use crate::db::driver::{DbDriver, DbResult, DbRow, DbTx, FilterClause, QueryState};
use crate::db::query_builder::{build_delete, build_select, build_update};
use crate::error::MarretaError;
use crate::migrations::{
    AppliedMigration, DatabaseColumn, DatabaseForeignKey, DatabaseSchema, DatabaseTable,
    LocalMigration,
};
use crate::value::Value;

pub struct PostgresDriver {
    pool: PgPool,
}

pub struct PoolConfig {
    pub max_connections: Option<u32>,
    pub min_connections: Option<u32>,
    pub acquire_timeout_secs: Option<u64>,
    pub idle_timeout_secs: Option<u64>,
    pub max_lifetime_secs: Option<u64>,
    pub test_before_acquire: Option<bool>,
}

impl PostgresDriver {
    pub async fn connect(url: &str, cfg: PoolConfig) -> Result<Self, MarretaError> {
        let pool = PgPoolOptions::new()
            .max_connections(cfg.max_connections.unwrap_or(10))
            .min_connections(cfg.min_connections.unwrap_or(0))
            .acquire_timeout(Duration::from_secs(cfg.acquire_timeout_secs.unwrap_or(30)))
            .idle_timeout(Duration::from_secs(cfg.idle_timeout_secs.unwrap_or(600)))
            .max_lifetime(Duration::from_secs(cfg.max_lifetime_secs.unwrap_or(1800)))
            .test_before_acquire(cfg.test_before_acquire.unwrap_or(true))
            .connect(url)
            .await
            .map_err(|_| MarretaError::DbError {
                message: "could not connect to database".to_string(),
                operation: "db.connect".to_string(),
            })?;
        Ok(Self { pool })
    }

    pub async fn introspect_schema(&self) -> Result<DatabaseSchema, MarretaError> {
        let column_rows = sqlx::query(
            r#"
            SELECT
                table_name,
                column_name
            FROM information_schema.columns
            WHERE table_schema = 'public'
            ORDER BY table_name, ordinal_position
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|err| translate_pg_error(err, "_schema", "introspect_columns"))?;

        let fk_rows = sqlx::query(
            r#"
            SELECT
                tc.table_name,
                tc.constraint_name,
                kcu.column_name,
                ccu.table_name AS foreign_table_name,
                ccu.column_name AS foreign_column_name
            FROM information_schema.table_constraints tc
            JOIN information_schema.key_column_usage kcu
              ON tc.constraint_name = kcu.constraint_name
             AND tc.table_schema = kcu.table_schema
            JOIN information_schema.constraint_column_usage ccu
              ON ccu.constraint_name = tc.constraint_name
             AND ccu.table_schema = tc.table_schema
            WHERE tc.constraint_type = 'FOREIGN KEY'
              AND tc.table_schema = 'public'
            ORDER BY tc.table_name, tc.constraint_name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|err| translate_pg_error(err, "_schema", "introspect_foreign_keys"))?;

        let columns = column_rows
            .iter()
            .map(|row| PgColumnInfo {
                table_name: row.get::<String, _>("table_name"),
                column_name: row.get::<String, _>("column_name"),
            })
            .collect();
        let foreign_keys = fk_rows
            .iter()
            .map(|row| PgForeignKeyInfo {
                table_name: row.get::<String, _>("table_name"),
                constraint_name: row.get::<String, _>("constraint_name"),
                column_name: row.get::<String, _>("column_name"),
                foreign_table_name: row.get::<String, _>("foreign_table_name"),
                foreign_column_name: row.get::<String, _>("foreign_column_name"),
            })
            .collect();

        Ok(build_database_schema(columns, foreign_keys))
    }

    pub async fn ensure_migration_table(&self) -> Result<(), MarretaError> {
        sqlx::raw_sql(
            r#"
            CREATE TABLE IF NOT EXISTS _marreta_migrations (
              version TEXT PRIMARY KEY,
              name TEXT NOT NULL,
              checksum TEXT NOT NULL,
              applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|err| translate_pg_error(err, "_marreta_migrations", "ensure"))
    }

    pub async fn list_applied_migrations(&self) -> Result<Vec<AppliedMigration>, MarretaError> {
        let rows = sqlx::query(
            r#"
            SELECT version, name, checksum, applied_at::text AS applied_at
            FROM _marreta_migrations
            ORDER BY version
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .or_else(|err| {
            if is_undefined_table_error(&err, "_marreta_migrations") {
                Ok(Vec::new())
            } else {
                Err(translate_pg_error(err, "_marreta_migrations", "list"))
            }
        })?;

        Ok(rows
            .iter()
            .map(|row| AppliedMigration {
                version: row.get::<String, _>("version"),
                name: row.get::<String, _>("name"),
                checksum: row.get::<String, _>("checksum"),
                applied_at: row.get::<String, _>("applied_at"),
            })
            .collect())
    }

    pub async fn apply_migration(&self, migration: &LocalMigration) -> Result<(), MarretaError> {
        self.ensure_migration_table().await?;

        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::raw_sql(&migration.up_sql)
            .execute(&mut *tx)
            .await
            .map_err(|err| translate_pg_error(err, "_marreta_migrations", "apply_sql"))?;
        sqlx::query(
            r#"
            INSERT INTO _marreta_migrations (version, name, checksum)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(&migration.version)
        .bind(&migration.name)
        .bind(&migration.checksum)
        .execute(&mut *tx)
        .await
        .map_err(|err| translate_pg_error(err, "_marreta_migrations", "record"))?;
        tx.commit().await.map_err(db_err)
    }

    pub async fn rollback_migration(&self, migration: &LocalMigration) -> Result<(), MarretaError> {
        self.ensure_migration_table().await?;
        let down_sql = migration
            .down_sql
            .as_deref()
            .ok_or_else(|| MarretaError::IoError {
                message: format!(
                    "migration '{}_{}' has no down.sql file",
                    migration.version, migration.name
                ),
            })?;

        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::raw_sql(down_sql)
            .execute(&mut *tx)
            .await
            .map_err(|err| translate_pg_error(err, "_marreta_migrations", "rollback_sql"))?;
        sqlx::query("DELETE FROM _marreta_migrations WHERE version = $1")
            .bind(&migration.version)
            .execute(&mut *tx)
            .await
            .map_err(|err| translate_pg_error(err, "_marreta_migrations", "delete"))?;
        tx.commit().await.map_err(db_err)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PgColumnInfo {
    table_name: String,
    column_name: String,
}

#[derive(Debug, Clone, PartialEq)]
struct PgForeignKeyInfo {
    table_name: String,
    constraint_name: String,
    column_name: String,
    foreign_table_name: String,
    foreign_column_name: String,
}

fn build_database_schema(
    columns: Vec<PgColumnInfo>,
    foreign_keys: Vec<PgForeignKeyInfo>,
) -> DatabaseSchema {
    let mut tables: HashMap<String, DatabaseTable> = HashMap::new();

    for column in columns {
        let table = tables
            .entry(column.table_name.clone())
            .or_insert_with(|| DatabaseTable {
                name: column.table_name.clone(),
                columns: HashMap::new(),
                foreign_keys: HashMap::new(),
            });
        table.columns.insert(
            column.column_name.clone(),
            DatabaseColumn {
                name: column.column_name,
                // Live introspection does not query type/nullability; the drift report (Spec 073)
                // derives those from the migration files, not from this live-DB path.
                rendered_type: None,
                nullable: None,
            },
        );
    }

    for fk in foreign_keys {
        let table = tables
            .entry(fk.table_name.clone())
            .or_insert_with(|| DatabaseTable {
                name: fk.table_name.clone(),
                columns: HashMap::new(),
                foreign_keys: HashMap::new(),
            });
        table.foreign_keys.insert(
            fk.constraint_name.clone(),
            DatabaseForeignKey {
                name: fk.constraint_name,
                column_name: fk.column_name,
                references_table: fk.foreign_table_name,
                references_column: fk.foreign_column_name,
            },
        );
    }

    DatabaseSchema { tables }
}

// ─── PG row → DbRow ──────────────────────────────────────────────────────────

fn pg_row_to_map(row: &PgRow) -> DbRow {
    let mut map = HashMap::new();
    for col in row.columns() {
        let name = col.name().to_string();
        let type_name = col.type_info().name();
        let val = pg_col_to_value(row, col.ordinal(), type_name);
        map.insert(name, val);
    }
    map
}

fn pg_col_to_value(row: &PgRow, idx: usize, type_name: &str) -> Value {
    match type_name {
        "BOOL" => row
            .try_get::<bool, _>(idx)
            .map(Value::Boolean)
            .unwrap_or(Value::Null),
        "INT2" | "INT4" => row
            .try_get::<i32, _>(idx)
            .map(|v| Value::Integer(v as i64))
            .unwrap_or(Value::Null),
        "INT8" => row
            .try_get::<i64, _>(idx)
            .map(Value::Integer)
            .unwrap_or(Value::Null),
        "FLOAT4" => row
            .try_get::<f32, _>(idx)
            .map(|v| Value::Float(v as f64))
            .unwrap_or(Value::Null),
        "FLOAT8" => row
            .try_get::<f64, _>(idx)
            .map(Value::Float)
            .unwrap_or(Value::Null),
        "NUMERIC" => row
            .try_get::<rust_decimal::Decimal, _>(idx)
            .map(Value::Decimal)
            .unwrap_or(Value::Null),
        "TIMESTAMPTZ" | "TIMESTAMP" => row
            .try_get::<chrono::DateTime<chrono::Utc>, _>(idx)
            .map(Value::Instant)
            .unwrap_or(Value::Null),
        "DATE" => row
            .try_get::<chrono::NaiveDate, _>(idx)
            .map(Value::Date)
            .unwrap_or(Value::Null),
        "TIME" => row
            .try_get::<chrono::NaiveTime, _>(idx)
            .map(Value::Time)
            .unwrap_or(Value::Null),
        "JSON" | "JSONB" => row
            .try_get::<serde_json::Value, _>(idx)
            .map(|json| crate::value::json_to_value(&json))
            .unwrap_or(Value::Null),
        _ => row
            .try_get::<String, _>(idx)
            .map(Value::String)
            .unwrap_or(Value::Null),
    }
}

// ─── Value → sqlx query binding ──────────────────────────────────────────────

/// Binds a `Vec<Value>` as query parameters and executes, returning rows.
/// sqlx requires compile-time checked queries OR dynamic binding via `Query`.
macro_rules! bind_and_fetch {
    ($query:expr, $params:expr, $pool:expr) => {{
        let mut q = sqlx::query($query);
        for p in $params {
            q = match p {
                Value::Integer(n) => q.bind(n),
                Value::Float(f) => q.bind(f),
                Value::Decimal(d) => q.bind(d),
                Value::Boolean(b) => q.bind(b),
                Value::String(s) => q.bind(s),
                Value::Instant(dt) => q.bind(dt),
                Value::Date(date) => q.bind(date),
                Value::Time(time) => q.bind(time),
                Value::Duration(duration) => q.bind(duration.num_milliseconds()),
                Value::Interval(interval) => q.bind(sqlx::types::Json(
                    crate::value::value_to_json(&Value::Interval(interval.clone())),
                )),
                Value::Null => q.bind(Option::<String>::None),
                _ => q.bind(format!("{}", p)),
            };
        }
        q.fetch_all($pool).await
    }};
}

macro_rules! bind_and_execute {
    ($query:expr, $params:expr, $pool:expr) => {{
        let mut q = sqlx::query($query);
        for p in $params {
            q = match p {
                Value::Integer(n) => q.bind(n),
                Value::Float(f) => q.bind(f),
                Value::Decimal(d) => q.bind(d),
                Value::Boolean(b) => q.bind(b),
                Value::String(s) => q.bind(s),
                Value::Instant(dt) => q.bind(dt),
                Value::Date(date) => q.bind(date),
                Value::Time(time) => q.bind(time),
                Value::Duration(duration) => q.bind(duration.num_milliseconds()),
                Value::Interval(interval) => q.bind(sqlx::types::Json(
                    crate::value::value_to_json(&Value::Interval(interval.clone())),
                )),
                Value::Null => q.bind(Option::<String>::None),
                _ => q.bind(format!("{}", p)),
            };
        }
        q.execute($pool).await
    }};
}

fn db_err(e: sqlx::Error) -> MarretaError {
    translate_pg_error_without_table(e, "query")
}

/// Translates a sqlx error to a `MarretaError`.
/// Uses the driver's own error message — no static text beyond what sqlx provides.
/// No `sqlx::Error` propagates beyond this module boundary.
fn translate_pg_error(err: sqlx::Error, table: &str, op: &str) -> MarretaError {
    let operation = format!("db.{}.{}", table, op);
    translate_pg_error_with_operation(err, table, operation)
}

fn translate_pg_error_without_table(err: sqlx::Error, op: &str) -> MarretaError {
    translate_pg_error_with_operation(err, "query", format!("db.{}", op))
}

fn translate_pg_error_with_operation(
    err: sqlx::Error,
    table: &str,
    operation: String,
) -> MarretaError {
    // Postgres unique_violation (SQLSTATE 23505) -> dedicated error, surfaced as 409 (Spec 067).
    if let sqlx::Error::Database(db_error) = &err {
        if let Some(violation) =
            pg_unique_violation(db_error.code().as_deref(), db_error.message(), &operation)
        {
            return violation;
        }
    }
    let message = match &err {
        sqlx::Error::Database(db_err) => db_err.message().to_string(),
        sqlx::Error::RowNotFound => format!("record not found in '{}'", table),
        sqlx::Error::PoolTimedOut => "database connection pool timed out".to_string(),
        sqlx::Error::PoolClosed => "database connection pool is closed".to_string(),
        _ => err.to_string(),
    };
    MarretaError::DbError { message, operation }
}

/// Classify a Postgres SQLSTATE as a unique-violation error if it is one (23505). Pure and
/// testable without a live connection.
fn pg_unique_violation(code: Option<&str>, message: &str, operation: &str) -> Option<MarretaError> {
    if code == Some("23505") {
        Some(MarretaError::UniqueConstraintViolation {
            message: message.to_string(),
            operation: operation.to_string(),
        })
    } else {
        None
    }
}

fn is_undefined_table_error(err: &sqlx::Error, table: &str) -> bool {
    match err {
        sqlx::Error::Database(db_err) => {
            db_err.code().as_deref() == Some("42P01")
                || db_err
                    .message()
                    .contains(&format!("relation \"{}\" does not exist", table))
        }
        _ => false,
    }
}

// ─── DbDriver impl ───────────────────────────────────────────────────────────

#[async_trait]
impl DbDriver for PostgresDriver {
    async fn save(&self, table: &str, data: DbRow) -> DbResult<DbRow> {
        let mut keys: Vec<String> = data.keys().cloned().collect();
        keys.sort(); // deterministic column order
        let values: Vec<Value> = keys.iter().map(|k| data[k].clone()).collect();

        let cols = keys.join(", ");
        let placeholders: Vec<String> = (1..=keys.len()).map(|i| format!("${}", i)).collect();
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({}) RETURNING *",
            table,
            cols,
            placeholders.join(", ")
        );

        let rows = bind_and_fetch!(&sql, values, &self.pool).map_err(db_err)?;
        match rows.first() {
            Some(row) => Ok(pg_row_to_map(row)),
            None => Err(MarretaError::DbError {
                message: format!("INSERT into '{}' returned no rows", table),
                operation: format!("db.{}.save", table),
            }),
        }
    }

    async fn find(&self, table: &str, id: &Value) -> DbResult<Option<DbRow>> {
        let sql = format!("SELECT * FROM {} WHERE id = $1", table);
        let rows = bind_and_fetch!(&sql, [id.clone()], &self.pool).map_err(db_err)?;
        Ok(rows.first().map(pg_row_to_map))
    }

    async fn find_all(&self, table: &str, filters: Vec<FilterClause>) -> DbResult<Vec<DbRow>> {
        let q = {
            let mut qs = QueryState::new(table);
            qs.filters = filters;
            qs
        };
        self.query_fetch(&q).await
    }

    async fn update_by_id(&self, table: &str, id: &Value, data: DbRow) -> DbResult<Option<DbRow>> {
        let mut keys: Vec<String> = data.keys().cloned().collect();
        keys.sort();
        let mut values: Vec<Value> = keys.iter().map(|k| data[k].clone()).collect();
        let id_param = keys.len() + 1;

        let set_clauses: Vec<String> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| format!("{} = ${}", k, i + 1))
            .collect();

        let sql = format!(
            "UPDATE {} SET {} WHERE id = ${} RETURNING *",
            table,
            set_clauses.join(", "),
            id_param
        );
        values.push(id.clone());

        let rows = bind_and_fetch!(&sql, values, &self.pool).map_err(db_err)?;
        Ok(rows.first().map(pg_row_to_map))
    }

    async fn delete_by_id(&self, table: &str, id: &Value) -> DbResult<bool> {
        let sql = format!("DELETE FROM {} WHERE id = $1", table);
        let result = bind_and_execute!(&sql, [id.clone()], &self.pool).map_err(db_err)?;
        Ok(result.rows_affected() > 0)
    }

    async fn query_fetch(&self, q: &QueryState) -> DbResult<Vec<DbRow>> {
        let (sql, params) = build_select(q)?;
        let rows = bind_and_fetch!(&sql, params, &self.pool).map_err(db_err)?;
        Ok(rows.iter().map(pg_row_to_map).collect())
    }

    async fn query_fetch_one(&self, q: &QueryState) -> DbResult<Option<DbRow>> {
        let (mut sql, params) = build_select(q)?;
        // Force LIMIT 1 if not already set
        if q.limit.is_none() {
            sql.push_str(" LIMIT 1");
        }
        let rows = bind_and_fetch!(&sql, params, &self.pool).map_err(db_err)?;
        Ok(rows.first().map(pg_row_to_map))
    }

    async fn query_count(&self, q: &QueryState) -> DbResult<i64> {
        let mut count_q = q.clone();
        count_q.count = true;
        count_q.order_by = None;
        count_q.limit = None;
        count_q.offset = None;

        let (sql, params) = build_select(&count_q)?;
        let rows = bind_and_fetch!(&sql, params, &self.pool).map_err(db_err)?;
        match rows.first() {
            Some(row) => Ok(row.try_get::<i64, _>(0).unwrap_or(0)),
            None => Ok(0),
        }
    }

    async fn query_exists(&self, q: &QueryState) -> DbResult<bool> {
        Ok(self.query_count(q).await? > 0)
    }

    async fn query_update(&self, q: &QueryState, data: DbRow) -> DbResult<u64> {
        let mut keys: Vec<String> = data.keys().cloned().collect();
        keys.sort();
        let mut params: Vec<Value> = keys.iter().map(|k| data[k].clone()).collect();
        params.extend(q.filters.iter().map(|f| f.value.clone()));

        let (sql, _) = build_update(&q.table, &keys, &q.filters, &q.known_columns)?;
        let result = bind_and_execute!(&sql, params, &self.pool).map_err(db_err)?;
        Ok(result.rows_affected())
    }

    async fn query_delete(&self, q: &QueryState) -> DbResult<u64> {
        let params: Vec<Value> = q.filters.iter().map(|f| f.value.clone()).collect();
        let (sql, _) = build_delete(&q.table, &q.filters, &q.known_columns)?;
        let result = bind_and_execute!(&sql, params, &self.pool).map_err(db_err)?;
        Ok(result.rows_affected())
    }

    async fn native_query(&self, sql: &str, params: Vec<Value>) -> DbResult<Vec<DbRow>> {
        let rows = bind_and_fetch!(sql, params, &self.pool).map_err(db_err)?;
        Ok(rows.iter().map(pg_row_to_map).collect())
    }

    async fn begin(&self) -> DbResult<Box<dyn DbTx>> {
        let tx = self.pool.begin().await.map_err(db_err)?;
        Ok(Box::new(PgTransaction { inner: tx }))
    }
}

// ─── PgTransaction ───────────────────────────────────────────────────────────

pub struct PgTransaction {
    inner: sqlx::Transaction<'static, sqlx::Postgres>,
}

#[async_trait]
impl DbTx for PgTransaction {
    async fn save(&mut self, table: &str, data: DbRow) -> DbResult<DbRow> {
        let mut keys: Vec<String> = data.keys().cloned().collect();
        keys.sort();
        let values: Vec<Value> = keys.iter().map(|k| data[k].clone()).collect();
        let cols = keys.join(", ");
        let placeholders: Vec<String> = (1..=keys.len()).map(|i| format!("${}", i)).collect();
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({}) RETURNING *",
            table,
            cols,
            placeholders.join(", ")
        );
        let rows = bind_and_fetch!(&sql, values, &mut *self.inner).map_err(db_err)?;
        match rows.first() {
            Some(row) => Ok(pg_row_to_map(row)),
            None => Err(MarretaError::DbError {
                message: format!("INSERT into '{}' returned no rows", table),
                operation: format!("db.{}.save", table),
            }),
        }
    }

    async fn find(&mut self, table: &str, id: &Value) -> DbResult<Option<DbRow>> {
        let sql = format!("SELECT * FROM {} WHERE id = $1", table);
        let rows = bind_and_fetch!(&sql, [id.clone()], &mut *self.inner).map_err(db_err)?;
        Ok(rows.first().map(pg_row_to_map))
    }

    async fn find_all(&mut self, table: &str, filters: Vec<FilterClause>) -> DbResult<Vec<DbRow>> {
        let mut qs = QueryState::new(table);
        qs.filters = filters;
        self.query_fetch(&qs).await
    }

    async fn update_by_id(
        &mut self,
        table: &str,
        id: &Value,
        data: DbRow,
    ) -> DbResult<Option<DbRow>> {
        let mut keys: Vec<String> = data.keys().cloned().collect();
        keys.sort();
        let mut values: Vec<Value> = keys.iter().map(|k| data[k].clone()).collect();
        let id_param = keys.len() + 1;
        let set_clauses: Vec<String> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| format!("{} = ${}", k, i + 1))
            .collect();
        let sql = format!(
            "UPDATE {} SET {} WHERE id = ${} RETURNING *",
            table,
            set_clauses.join(", "),
            id_param
        );
        values.push(id.clone());
        let rows = bind_and_fetch!(&sql, values, &mut *self.inner).map_err(db_err)?;
        Ok(rows.first().map(pg_row_to_map))
    }

    async fn delete_by_id(&mut self, table: &str, id: &Value) -> DbResult<bool> {
        let sql = format!("DELETE FROM {} WHERE id = $1", table);
        let result = bind_and_execute!(&sql, [id.clone()], &mut *self.inner).map_err(db_err)?;
        Ok(result.rows_affected() > 0)
    }

    async fn query_fetch(&mut self, q: &QueryState) -> DbResult<Vec<DbRow>> {
        let (sql, params) = build_select(q)?;
        let rows = bind_and_fetch!(&sql, params, &mut *self.inner).map_err(db_err)?;
        Ok(rows.iter().map(pg_row_to_map).collect())
    }

    async fn query_fetch_one(&mut self, q: &QueryState) -> DbResult<Option<DbRow>> {
        let (mut sql, params) = build_select(q)?;
        if q.limit.is_none() {
            sql.push_str(" LIMIT 1");
        }
        let rows = bind_and_fetch!(&sql, params, &mut *self.inner).map_err(db_err)?;
        Ok(rows.first().map(pg_row_to_map))
    }

    async fn query_count(&mut self, q: &QueryState) -> DbResult<i64> {
        let mut count_q = q.clone();
        count_q.count = true;
        count_q.order_by = None;
        count_q.limit = None;
        count_q.offset = None;
        let (sql, params) = build_select(&count_q)?;
        let rows = bind_and_fetch!(&sql, params, &mut *self.inner).map_err(db_err)?;
        match rows.first() {
            Some(row) => Ok(row.try_get::<i64, _>(0).unwrap_or(0)),
            None => Ok(0),
        }
    }

    async fn query_exists(&mut self, q: &QueryState) -> DbResult<bool> {
        Ok(self.query_count(q).await? > 0)
    }

    async fn query_update(&mut self, q: &QueryState, data: DbRow) -> DbResult<u64> {
        let mut keys: Vec<String> = data.keys().cloned().collect();
        keys.sort();
        let mut params: Vec<Value> = keys.iter().map(|k| data[k].clone()).collect();
        params.extend(q.filters.iter().map(|f| f.value.clone()));
        let (sql, _) = build_update(&q.table, &keys, &q.filters, &q.known_columns)?;
        let result = bind_and_execute!(&sql, params, &mut *self.inner).map_err(db_err)?;
        Ok(result.rows_affected())
    }

    async fn query_delete(&mut self, q: &QueryState) -> DbResult<u64> {
        let params: Vec<Value> = q.filters.iter().map(|f| f.value.clone()).collect();
        let (sql, _) = build_delete(&q.table, &q.filters, &q.known_columns)?;
        let result = bind_and_execute!(&sql, params, &mut *self.inner).map_err(db_err)?;
        Ok(result.rows_affected())
    }

    async fn native_query(&mut self, sql: &str, params: Vec<Value>) -> DbResult<Vec<DbRow>> {
        let rows = bind_and_fetch!(sql, params, &mut *self.inner).map_err(db_err)?;
        Ok(rows.iter().map(pg_row_to_map).collect())
    }

    async fn commit(self: Box<Self>) -> DbResult<()> {
        self.inner.commit().await.map_err(db_err)
    }

    async fn rollback(self: Box<Self>) -> DbResult<()> {
        self.inner.rollback().await.map_err(db_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pg_unique_violation_classifies_sqlstate_23505() {
        assert!(matches!(
            pg_unique_violation(Some("23505"), "dup", "db.users.save"),
            Some(MarretaError::UniqueConstraintViolation { .. })
        ));
        assert!(pg_unique_violation(Some("23503"), "fk", "db.users.save").is_none());
        assert!(pg_unique_violation(None, "x", "db.users.save").is_none());
    }

    // ─── PoolConfig ─────────────────────────────────────────────────────────────

    #[test]
    fn test_pool_config_all_none_uses_defaults() {
        let cfg = PoolConfig {
            max_connections: None,
            min_connections: None,
            acquire_timeout_secs: None,
            idle_timeout_secs: None,
            max_lifetime_secs: None,
            test_before_acquire: None,
        };
        assert_eq!(cfg.max_connections.unwrap_or(10), 10);
        assert_eq!(cfg.min_connections.unwrap_or(0), 0);
        assert_eq!(cfg.acquire_timeout_secs.unwrap_or(30), 30);
        assert_eq!(cfg.idle_timeout_secs.unwrap_or(600), 600);
        assert_eq!(cfg.max_lifetime_secs.unwrap_or(1800), 1800);
        assert!(cfg.test_before_acquire.unwrap_or(true));
    }

    #[test]
    fn test_pool_config_explicit_values_preserved() {
        let cfg = PoolConfig {
            max_connections: Some(50),
            min_connections: Some(5),
            acquire_timeout_secs: Some(15),
            idle_timeout_secs: Some(300),
            max_lifetime_secs: Some(900),
            test_before_acquire: Some(false),
        };
        assert_eq!(cfg.max_connections.unwrap_or(10), 50);
        assert_eq!(cfg.min_connections.unwrap_or(0), 5);
        assert_eq!(cfg.acquire_timeout_secs.unwrap_or(30), 15);
        assert_eq!(cfg.idle_timeout_secs.unwrap_or(600), 300);
        assert_eq!(cfg.max_lifetime_secs.unwrap_or(1800), 900);
        assert!(!cfg.test_before_acquire.unwrap_or(true));
    }

    #[test]
    fn test_pool_config_zero_min_connections_is_valid() {
        let cfg = PoolConfig {
            min_connections: Some(0),
            max_connections: Some(1),
            acquire_timeout_secs: None,
            idle_timeout_secs: None,
            max_lifetime_secs: None,
            test_before_acquire: None,
        };
        assert_eq!(cfg.min_connections.unwrap(), 0);
    }

    // ─── translate_pg_error ─────────────────────────────────────────────────────

    #[test]
    fn test_translate_pool_timed_out() {
        let err = translate_pg_error(sqlx::Error::PoolTimedOut, "users", "find");
        if let MarretaError::DbError { message, operation } = err {
            assert_eq!(message, "database connection pool timed out");
            assert_eq!(operation, "db.users.find");
        } else {
            panic!("expected DbError");
        }
    }

    #[test]
    fn test_translate_pool_closed() {
        let err = translate_pg_error(sqlx::Error::PoolClosed, "orders", "save");
        if let MarretaError::DbError { message, operation } = err {
            assert_eq!(message, "database connection pool is closed");
            assert_eq!(operation, "db.orders.save");
        } else {
            panic!("expected DbError");
        }
    }

    #[test]
    fn test_translate_row_not_found() {
        let err = translate_pg_error(sqlx::Error::RowNotFound, "products", "find");
        if let MarretaError::DbError { message, operation } = err {
            assert_eq!(message, "record not found in 'products'");
            assert_eq!(operation, "db.products.find");
        } else {
            panic!("expected DbError");
        }
    }

    #[test]
    fn test_translate_generic_error_uses_to_string() {
        let io_err =
            std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "connection refused");
        let err = translate_pg_error(sqlx::Error::Io(io_err), "logs", "query");
        if let MarretaError::DbError { message, operation } = err {
            assert!(!message.is_empty());
            assert_eq!(operation, "db.logs.query");
        } else {
            panic!("expected DbError");
        }
    }

    #[test]
    fn test_translate_operation_format() {
        let err = translate_pg_error(sqlx::Error::PoolTimedOut, "my_table", "my_op");
        if let MarretaError::DbError { operation, .. } = err {
            assert_eq!(operation, "db.my_table.my_op");
        }
    }

    #[test]
    fn test_translate_error_code_is_db_error() {
        let err = translate_pg_error(sqlx::Error::RowNotFound, "t", "op");
        assert_eq!(err.semantic_code(), "db_error");
    }

    #[test]
    fn test_db_err_helper_uses_unknown_table() {
        let err = db_err(sqlx::Error::PoolTimedOut);
        if let MarretaError::DbError { operation, .. } = err {
            assert_eq!(operation, "db.query");
        } else {
            panic!("expected DbError");
        }
    }

    #[test]
    fn test_build_database_schema_groups_columns_by_table() {
        let schema = build_database_schema(
            vec![
                PgColumnInfo {
                    table_name: "users".into(),
                    column_name: "id".into(),
                },
                PgColumnInfo {
                    table_name: "users".into(),
                    column_name: "email".into(),
                },
                PgColumnInfo {
                    table_name: "orders".into(),
                    column_name: "id".into(),
                },
            ],
            vec![],
        );

        assert_eq!(schema.tables.len(), 2);
        assert!(schema.tables["users"].columns.contains_key("id"));
        assert!(schema.tables["users"].columns.contains_key("email"));
        assert!(schema.tables["orders"].columns.contains_key("id"));
    }

    #[test]
    fn test_build_database_schema_groups_foreign_keys() {
        let schema = build_database_schema(
            vec![PgColumnInfo {
                table_name: "orders".into(),
                column_name: "user_id".into(),
            }],
            vec![PgForeignKeyInfo {
                table_name: "orders".into(),
                constraint_name: "fk_orders_user_id".into(),
                column_name: "user_id".into(),
                foreign_table_name: "users".into(),
                foreign_column_name: "id".into(),
            }],
        );

        let orders = &schema.tables["orders"];
        assert!(orders.foreign_keys.contains_key("fk_orders_user_id"));
        assert_eq!(
            orders.foreign_keys["fk_orders_user_id"].references_table,
            "users"
        );
        assert_eq!(
            orders.foreign_keys["fk_orders_user_id"].references_column,
            "id"
        );
    }
}
