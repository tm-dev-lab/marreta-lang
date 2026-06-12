use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{Expression, SchemaField, SchemaType};
use crate::error::MarretaError;
use crate::persistent_schema::validate_persistent_schema_references;
use crate::route_loader::SchemaDefinition;
use chrono::Utc;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq)]
pub struct PersistentTable {
    pub schema_name: String,
    pub table_name: String,
    pub columns: Vec<PersistentColumn>,
    pub foreign_keys: Vec<PersistentForeignKey>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PersistentColumn {
    pub field_name: String,
    pub column_name: String,
    pub field_type: SchemaType,
    pub nullable: bool,
    pub primary: bool,
    pub generated: bool,
    pub unique: bool,
    pub default: Option<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PersistentForeignKey {
    pub field_name: String,
    pub column_name: String,
    pub references_schema: String,
    pub references_table: String,
    pub references_column: String,
    pub nullable: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DatabaseSchema {
    pub tables: HashMap<String, DatabaseTable>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DatabaseTable {
    pub name: String,
    pub columns: HashMap<String, DatabaseColumn>,
    pub foreign_keys: HashMap<String, DatabaseForeignKey>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DatabaseColumn {
    pub name: String,
    /// Spec 073 (2.2): the column's rendered Postgres type as captured from the migration DDL
    /// (raw, for the drift message), or `None` when the replay could not resolve it (pin (d):
    /// precision over recall, the type-drift check is skipped for such a column).
    pub rendered_type: Option<String>,
    /// Whether the column is nullable, captured from the migration DDL (`None` when unresolved).
    pub nullable: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DatabaseForeignKey {
    pub name: String,
    pub column_name: String,
    pub references_table: String,
    pub references_column: String,
}

/// Spec 073 (2.2): a schema change the additive-only planner deliberately does not support.
/// These are reported, never acted on.
#[derive(Debug, Clone, PartialEq)]
pub enum SchemaDrift {
    ColumnTypeDiffers {
        table: String,
        column: String,
        history: String,
        schema: String,
    },
    ColumnNullabilityDiffers {
        table: String,
        column: String,
        history_nullable: bool,
        schema_nullable: bool,
    },
    ColumnRemoved {
        table: String,
        column: String,
    },
    TableRemoved {
        table: String,
    },
}

impl SchemaDrift {
    /// One report line in the doctor style, for `migrate diff` / `migrate generate`.
    pub fn report_line(&self) -> String {
        match self {
            SchemaDrift::ColumnTypeDiffers {
                table,
                column,
                history,
                schema,
            } => format!(
                "  {}.{}: type differs (history {}, schema {})",
                table, column, history, schema
            ),
            SchemaDrift::ColumnNullabilityDiffers {
                table,
                column,
                history_nullable,
                schema_nullable,
            } => format!(
                "  {}.{}: nullability differs (history {}, schema {})",
                table,
                column,
                if *history_nullable {
                    "nullable"
                } else {
                    "NOT NULL"
                },
                if *schema_nullable {
                    "nullable"
                } else {
                    "NOT NULL"
                }
            ),
            SchemaDrift::ColumnRemoved { table, column } => format!(
                "  {}.{}: present in history, no longer in any schema",
                table, column
            ),
            SchemaDrift::TableRemoved { table } => format!(
                "  {}: present in history, no schema declares it anymore",
                table
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalMigration {
    pub version: String,
    pub name: String,
    pub up_path: PathBuf,
    pub down_path: Option<PathBuf>,
    pub up_sql: String,
    pub down_sql: Option<String>,
    pub checksum: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppliedMigration {
    pub version: String,
    pub name: String,
    pub checksum: String,
    pub applied_at: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MigrationStatusReport {
    pub applied: Vec<String>,
    pub pending: Vec<String>,
    pub changed: Vec<String>,
    pub missing_local: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationState {
    Applied,
    Pending,
    Changed,
    MissingLocal,
}

impl MigrationState {
    pub fn as_str(&self) -> &'static str {
        match self {
            MigrationState::Applied => "applied",
            MigrationState::Pending => "pending",
            MigrationState::Changed => "changed",
            MigrationState::MissingLocal => "missing_local",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationListEntry {
    pub version: String,
    pub name: String,
    pub state: MigrationState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MigrationOp {
    CreateTable {
        table_name: String,
        columns: Vec<PersistentColumn>,
    },
    AddColumn {
        table_name: String,
        column: PersistentColumn,
    },
    AddForeignKey {
        table_name: String,
        foreign_key: PersistentForeignKey,
    },
}

pub fn build_persistent_tables(
    schemas: &HashMap<String, SchemaDefinition>,
) -> Result<HashMap<String, PersistentTable>, MarretaError> {
    validate_persistent_schema_references(schemas)?;
    let mut tables = HashMap::new();

    for (schema_name, schema) in schemas {
        let Some(table_name) = &schema.db_table else {
            continue;
        };

        let mut columns = Vec::new();
        let mut foreign_keys = Vec::new();

        for field in &schema.fields {
            match &field.field_type {
                SchemaType::StringType
                | SchemaType::IntegerType
                | SchemaType::FloatType
                | SchemaType::DecimalType
                | SchemaType::BooleanType
                | SchemaType::InstantType
                | SchemaType::DateType
                | SchemaType::TimeType
                | SchemaType::DurationType
                | SchemaType::IntervalType
                | SchemaType::EnumType(_) => {
                    columns.push(build_primitive_column(field)?);
                }
                SchemaType::Reference(target_schema) => {
                    let target = schemas.get(target_schema).ok_or_else(|| {
                        MarretaError::InvalidPersistentSchemaDefinition {
                            schema_name: schema_name.clone(),
                            message: format!(
                                "field '{}' references unknown schema '{}'",
                                field.name, target_schema
                            ),
                        }
                    })?;

                    let Some(target_table) = &target.db_table else {
                        return Err(MarretaError::InvalidPersistentSchemaReference {
                            schema_name: schema_name.clone(),
                            field_name: field.name.clone(),
                            target_schema: target_schema.clone(),
                        });
                    };

                    let column_name = format!("{}_id", field.name);

                    columns.push(PersistentColumn {
                        field_name: field.name.clone(),
                        column_name: column_name.clone(),
                        field_type: SchemaType::IntegerType,
                        nullable: field.optional,
                        primary: false,
                        generated: false,
                        unique: false,
                        default: None,
                    });
                    foreign_keys.push(PersistentForeignKey {
                        field_name: field.name.clone(),
                        column_name,
                        references_schema: target_schema.clone(),
                        references_table: target_table.clone(),
                        references_column: "id".to_string(),
                        nullable: field.optional,
                    });
                }
                SchemaType::TypedList(_) => continue,
                other => {
                    return Err(MarretaError::InvalidPersistentSchemaDefinition {
                        schema_name: schema_name.clone(),
                        message: format!(
                            "field '{}' uses unsupported persistent type '{}'",
                            field.name, other
                        ),
                    });
                }
            }
        }

        tables.insert(
            schema_name.clone(),
            PersistentTable {
                schema_name: schema_name.clone(),
                table_name: table_name.clone(),
                columns,
                foreign_keys,
            },
        );
    }

    Ok(tables)
}

pub fn plan_migration(
    current: &DatabaseSchema,
    desired: &HashMap<String, PersistentTable>,
) -> Vec<MigrationOp> {
    let mut desired_tables: Vec<&PersistentTable> = desired.values().collect();
    desired_tables.sort_by(|a, b| a.table_name.cmp(&b.table_name));

    let mut create_ops = Vec::new();
    let mut add_column_ops = Vec::new();
    let mut add_fk_ops = Vec::new();

    for table in desired_tables {
        match current.tables.get(&table.table_name) {
            None => {
                create_ops.push(MigrationOp::CreateTable {
                    table_name: table.table_name.clone(),
                    columns: table.columns.clone(),
                });
                for fk in &table.foreign_keys {
                    add_fk_ops.push(MigrationOp::AddForeignKey {
                        table_name: table.table_name.clone(),
                        foreign_key: fk.clone(),
                    });
                }
            }
            Some(existing) => {
                for column in &table.columns {
                    if !existing.columns.contains_key(&column.column_name) {
                        add_column_ops.push(MigrationOp::AddColumn {
                            table_name: table.table_name.clone(),
                            column: column.clone(),
                        });
                    }
                }

                for fk in &table.foreign_keys {
                    let constraint_name =
                        foreign_key_constraint_name(&table.table_name, &fk.column_name);
                    if !existing.foreign_keys.contains_key(&constraint_name) {
                        add_fk_ops.push(MigrationOp::AddForeignKey {
                            table_name: table.table_name.clone(),
                            foreign_key: fk.clone(),
                        });
                    }
                }
            }
        }
    }

    let mut ops = create_ops;
    ops.extend(add_column_ops);
    ops.extend(add_fk_ops);
    ops
}

/// Spec 073 (2.2): report-only comparison of changes the additive-only planner deliberately does
/// not support. Never produces operations. Entries are ordered by table then column for stable
/// output. The desired side mirrors pin (c): a primary-key column is treated as non-null.
pub fn detect_schema_drift(
    current: &DatabaseSchema,
    desired: &HashMap<String, PersistentTable>,
) -> Vec<SchemaDrift> {
    let desired_by_table: HashMap<&str, &PersistentTable> = desired
        .values()
        .map(|table| (table.table_name.as_str(), table))
        .collect();

    let mut drift = Vec::new();
    let mut table_names: Vec<&String> = current.tables.keys().collect();
    table_names.sort();

    for table_name in table_names {
        let current_table = &current.tables[table_name];
        let Some(desired_table) = desired_by_table.get(table_name.as_str()) else {
            drift.push(SchemaDrift::TableRemoved {
                table: table_name.clone(),
            });
            continue;
        };
        let desired_cols: HashMap<&str, &PersistentColumn> = desired_table
            .columns
            .iter()
            .map(|column| (column.column_name.as_str(), column))
            .collect();

        let mut col_names: Vec<&String> = current_table.columns.keys().collect();
        col_names.sort();
        for col_name in col_names {
            let history_col = &current_table.columns[col_name];
            let Some(desired_col) = desired_cols.get(col_name.as_str()) else {
                drift.push(SchemaDrift::ColumnRemoved {
                    table: table_name.clone(),
                    column: col_name.clone(),
                });
                continue;
            };

            if let Some(history_type) = &history_col.rendered_type {
                if let Ok(desired_type) = postgres_type(&desired_col.field_type) {
                    if normalize_pg_type(history_type) != normalize_pg_type(desired_type) {
                        drift.push(SchemaDrift::ColumnTypeDiffers {
                            table: table_name.clone(),
                            column: col_name.clone(),
                            history: history_type.clone(),
                            schema: desired_type.to_string(),
                        });
                    }
                }
            }

            if let Some(history_nullable) = history_col.nullable {
                let schema_nullable = desired_col.nullable && !desired_col.primary;
                if history_nullable != schema_nullable {
                    drift.push(SchemaDrift::ColumnNullabilityDiffers {
                        table: table_name.clone(),
                        column: col_name.clone(),
                        history_nullable,
                        schema_nullable,
                    });
                }
            }
        }
    }
    drift
}

pub fn render_postgres_up_sql(ops: &[MigrationOp]) -> Result<Vec<String>, MarretaError> {
    ops.iter().map(render_postgres_op).collect()
}

pub fn render_postgres_down_sql(ops: &[MigrationOp]) -> Result<Vec<String>, MarretaError> {
    let mut sql = Vec::new();
    for op in ops.iter().rev() {
        sql.push(render_postgres_down_op(op)?);
    }
    Ok(sql)
}

pub fn default_migration_name(ops: &[MigrationOp]) -> String {
    match ops.first() {
        Some(MigrationOp::CreateTable { table_name, .. }) if ops.len() == 1 => {
            format!("create_{}", table_name)
        }
        Some(MigrationOp::AddColumn { table_name, .. }) if ops.len() == 1 => {
            format!("alter_{}", table_name)
        }
        Some(MigrationOp::AddForeignKey { table_name, .. }) if ops.len() == 1 => {
            format!("alter_{}", table_name)
        }
        Some(MigrationOp::CreateTable { table_name, .. }) => format!("update_{}", table_name),
        Some(MigrationOp::AddColumn { table_name, .. }) => format!("update_{}", table_name),
        Some(MigrationOp::AddForeignKey { table_name, .. }) => format!("update_{}", table_name),
        None => "empty_migration".to_string(),
    }
}

pub fn write_migration_files(
    migrations_dir: &Path,
    name: &str,
    up_sql: &[String],
    down_sql: &[String],
) -> Result<LocalMigration, MarretaError> {
    fs::create_dir_all(migrations_dir).map_err(|err| MarretaError::IoError {
        message: format!(
            "could not create migrations directory '{}': {}",
            migrations_dir.display(),
            err
        ),
    })?;

    let version = Utc::now().format("%Y%m%d_%H%M%S").to_string();

    // Spec 073 (2.3): the version is second-resolution, so two `generate` runs within one second
    // would silently collide (clobbering a file, or producing two migrations that sort
    // ambiguously). Refuse rather than overwrite; the developer retries once the second advances.
    if migration_version_exists(migrations_dir, &version)? {
        return Err(MarretaError::IoError {
            message: format!(
                "a migration with version '{}' already exists (two 'generate' runs in the same \
                 second); retry in a moment so the timestamp advances",
                version
            ),
        });
    }

    let sanitized_name = sanitize_migration_name(name);
    let base_name = format!("{}_{}", version, sanitized_name);
    let up_path = migrations_dir.join(format!("{}.up.sql", base_name));
    let down_path = migrations_dir.join(format!("{}.down.sql", base_name));
    let up_content = join_sql(up_sql);
    let down_content = join_sql(down_sql);

    fs::write(&up_path, &up_content).map_err(|err| MarretaError::IoError {
        message: format!("could not write migration '{}': {}", up_path.display(), err),
    })?;
    fs::write(&down_path, &down_content).map_err(|err| MarretaError::IoError {
        message: format!(
            "could not write migration '{}': {}",
            down_path.display(),
            err
        ),
    })?;

    Ok(LocalMigration {
        version,
        name: sanitized_name,
        up_path,
        down_path: Some(down_path),
        checksum: migration_checksum(&up_content, Some(&down_content)),
        up_sql: up_content,
        down_sql: Some(down_content),
    })
}

/// Whether any migration file in the directory already uses `version` as its timestamp prefix.
fn migration_version_exists(migrations_dir: &Path, version: &str) -> Result<bool, MarretaError> {
    let prefix = format!("{}_", version);
    for entry in fs::read_dir(migrations_dir).map_err(|err| MarretaError::IoError {
        message: format!(
            "could not read migrations directory '{}': {}",
            migrations_dir.display(),
            err
        ),
    })? {
        let entry = entry.map_err(|err| MarretaError::IoError {
            message: format!("could not read migrations directory entry: {}", err),
        })?;
        if let Some(name) = entry.file_name().to_str() {
            if name.starts_with(&prefix) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn discover_local_migrations(
    migrations_dir: &Path,
) -> Result<Vec<LocalMigration>, MarretaError> {
    if !migrations_dir.exists() {
        return Ok(Vec::new());
    }

    let mut up_entries = Vec::new();
    for entry in fs::read_dir(migrations_dir).map_err(|err| MarretaError::IoError {
        message: format!(
            "could not read migrations directory '{}': {}",
            migrations_dir.display(),
            err
        ),
    })? {
        let entry = entry.map_err(|err| MarretaError::IoError {
            message: format!(
                "could not read migrations directory entry in '{}': {}",
                migrations_dir.display(),
                err
            ),
        })?;
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".up.sql"))
        {
            up_entries.push(path);
        }
    }

    up_entries.sort();
    let mut migrations = Vec::new();

    for up_path in up_entries {
        let filename = up_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| MarretaError::IoError {
                message: format!("invalid migration filename '{}'", up_path.display()),
            })?;
        let (version, name) = parse_migration_filename(filename, ".up.sql")?;
        let down_path = migrations_dir.join(format!("{}_{}.down.sql", version, name));
        let up_sql = fs::read_to_string(&up_path).map_err(|err| MarretaError::IoError {
            message: format!("could not read migration '{}': {}", up_path.display(), err),
        })?;
        let down_sql = if down_path.exists() {
            Some(
                fs::read_to_string(&down_path).map_err(|err| MarretaError::IoError {
                    message: format!(
                        "could not read migration '{}': {}",
                        down_path.display(),
                        err
                    ),
                })?,
            )
        } else {
            None
        };

        migrations.push(LocalMigration {
            version,
            name,
            checksum: migration_checksum(&up_sql, down_sql.as_deref()),
            up_path,
            down_path: if down_path.exists() {
                Some(down_path)
            } else {
                None
            },
            up_sql,
            down_sql,
        });
    }

    Ok(migrations)
}

pub fn build_schema_from_local_migrations(
    local: &[LocalMigration],
) -> Result<DatabaseSchema, MarretaError> {
    let mut schema = DatabaseSchema::default();
    for migration in local {
        apply_local_migration_to_schema(&mut schema, migration)?;
    }
    Ok(schema)
}

pub fn compare_migration_state(
    local: &[LocalMigration],
    applied: &[AppliedMigration],
) -> MigrationStatusReport {
    let mut report = MigrationStatusReport::default();
    for entry in build_migration_inventory(local, applied) {
        let formatted = format!("{}_{}", entry.version, entry.name);
        match entry.state {
            MigrationState::Applied => report.applied.push(formatted),
            MigrationState::Pending => report.pending.push(formatted),
            MigrationState::Changed => report.changed.push(formatted),
            MigrationState::MissingLocal => report.missing_local.push(formatted),
        }
    }

    report.applied.sort();
    report.pending.sort();
    report.changed.sort();
    report.missing_local.sort();
    report
}

pub fn build_migration_inventory(
    local: &[LocalMigration],
    applied: &[AppliedMigration],
) -> Vec<MigrationListEntry> {
    let local_by_version: HashMap<&str, &LocalMigration> = local
        .iter()
        .map(|migration| (migration.version.as_str(), migration))
        .collect();
    let applied_by_version: HashMap<&str, &AppliedMigration> = applied
        .iter()
        .map(|migration| (migration.version.as_str(), migration))
        .collect();
    let mut versions: Vec<String> = local
        .iter()
        .map(|migration| migration.version.clone())
        .chain(applied.iter().map(|migration| migration.version.clone()))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    versions.sort();

    versions
        .into_iter()
        .filter_map(|version| {
            let local_migration = local_by_version.get(version.as_str()).copied();
            let applied_migration = applied_by_version.get(version.as_str()).copied();

            match (local_migration, applied_migration) {
                (Some(local), Some(applied)) if local.checksum == applied.checksum => {
                    Some(MigrationListEntry {
                        version,
                        name: local.name.clone(),
                        state: MigrationState::Applied,
                    })
                }
                (Some(local), Some(_)) => Some(MigrationListEntry {
                    version,
                    name: local.name.clone(),
                    state: MigrationState::Changed,
                }),
                (Some(local), None) => Some(MigrationListEntry {
                    version,
                    name: local.name.clone(),
                    state: MigrationState::Pending,
                }),
                (None, Some(applied)) => Some(MigrationListEntry {
                    version,
                    name: applied.name.clone(),
                    state: MigrationState::MissingLocal,
                }),
                (None, None) => None,
            }
        })
        .collect()
}

pub fn discard_pending_migration(
    version: &str,
    local: &[LocalMigration],
    applied: &[AppliedMigration],
) -> Result<String, MarretaError> {
    let report = compare_migration_state(local, applied);
    if report
        .applied
        .iter()
        .any(|item| item.starts_with(&format!("{}_", version)))
    {
        return Err(MarretaError::IoError {
            message: format!(
                "cannot discard migration {} because it is already applied",
                version
            ),
        });
    }
    if report
        .changed
        .iter()
        .any(|item| item.starts_with(&format!("{}_", version)))
    {
        return Err(MarretaError::IoError {
            message: format!(
                "cannot discard migration {} because it is in changed state",
                version
            ),
        });
    }
    if report
        .missing_local
        .iter()
        .any(|item| item.starts_with(&format!("{}_", version)))
    {
        return Err(MarretaError::IoError {
            message: format!(
                "cannot discard migration {} because it is in missing_local state",
                version
            ),
        });
    }

    let migration = local
        .iter()
        .find(|migration| migration.version == version)
        .ok_or_else(|| MarretaError::IoError {
            message: format!("pending migration {} was not found locally", version),
        })?;

    let down_path = migration
        .down_path
        .as_ref()
        .ok_or_else(|| MarretaError::IoError {
            message: format!(
                "cannot discard migration {}_{} because the down migration file is missing",
                migration.version, migration.name
            ),
        })?;

    if !migration.up_path.exists() || !down_path.exists() {
        return Err(MarretaError::IoError {
            message: format!(
                "cannot discard migration {}_{} because the local migration pair is incomplete",
                migration.version, migration.name
            ),
        });
    }

    fs::remove_file(&migration.up_path).map_err(|err| MarretaError::IoError {
        message: format!(
            "could not remove migration '{}': {}",
            migration.up_path.display(),
            err
        ),
    })?;
    fs::remove_file(down_path).map_err(|err| MarretaError::IoError {
        message: format!(
            "could not remove migration '{}': {}",
            down_path.display(),
            err
        ),
    })?;
    Ok(format!("{}_{}", migration.version, migration.name))
}

fn build_primitive_column(field: &SchemaField) -> Result<PersistentColumn, MarretaError> {
    Ok(PersistentColumn {
        field_name: field.name.clone(),
        column_name: field.name.clone(),
        field_type: field.field_type.clone(),
        nullable: field.optional,
        primary: field.name == "id",
        generated: field.name == "id" && matches!(field.field_type, SchemaType::IntegerType),
        unique: false,
        default: None,
    })
}

fn render_postgres_op(op: &MigrationOp) -> Result<String, MarretaError> {
    match op {
        MigrationOp::CreateTable {
            table_name,
            columns,
        } => {
            let mut defs = Vec::new();
            for column in columns {
                defs.push(render_postgres_column(column)?);
            }
            Ok(format!(
                "CREATE TABLE {} (\n  {}\n);",
                table_name,
                defs.join(",\n  ")
            ))
        }
        MigrationOp::AddColumn { table_name, column } => Ok(format!(
            "ALTER TABLE {} ADD COLUMN {};",
            table_name,
            render_postgres_column(column)?
        )),
        MigrationOp::AddForeignKey {
            table_name,
            foreign_key,
        } => Ok(format!(
            "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {}({});",
            table_name,
            foreign_key_constraint_name(table_name, &foreign_key.column_name),
            foreign_key.column_name,
            foreign_key.references_table,
            foreign_key.references_column
        )),
    }
}

fn render_postgres_down_op(op: &MigrationOp) -> Result<String, MarretaError> {
    match op {
        MigrationOp::CreateTable { table_name, .. } => Ok(format!("DROP TABLE {};", table_name)),
        MigrationOp::AddColumn { table_name, column } => Ok(format!(
            "ALTER TABLE {} DROP COLUMN {};",
            table_name, column.column_name
        )),
        MigrationOp::AddForeignKey {
            table_name,
            foreign_key,
        } => Ok(format!(
            "ALTER TABLE {} DROP CONSTRAINT {};",
            table_name,
            foreign_key_constraint_name(table_name, &foreign_key.column_name)
        )),
    }
}

fn render_postgres_column(column: &PersistentColumn) -> Result<String, MarretaError> {
    let mut parts = vec![
        column.column_name.clone(),
        postgres_type(&column.field_type)?.to_string(),
    ];

    if column.primary {
        parts.push("PRIMARY KEY".to_string());
    }
    if column.generated {
        parts.push("GENERATED BY DEFAULT AS IDENTITY".to_string());
    }
    if !column.nullable {
        parts.push("NOT NULL".to_string());
    }
    if column.unique {
        parts.push("UNIQUE".to_string());
    }
    if let Some(default) = &column.default {
        parts.push(format!("DEFAULT {}", postgres_default_expr(default)?));
    }

    Ok(parts.join(" "))
}

fn postgres_type(field_type: &SchemaType) -> Result<&'static str, MarretaError> {
    match field_type {
        SchemaType::StringType => Ok("TEXT"),
        SchemaType::IntegerType => Ok("BIGINT"),
        SchemaType::FloatType => Ok("DOUBLE PRECISION"),
        SchemaType::DecimalType => Ok("NUMERIC"),
        SchemaType::BooleanType => Ok("BOOLEAN"),
        SchemaType::InstantType => Ok("TIMESTAMPTZ"),
        SchemaType::DateType => Ok("DATE"),
        SchemaType::TimeType => Ok("TIME"),
        SchemaType::DurationType => Ok("BIGINT"),
        SchemaType::IntervalType => Ok("JSONB"),
        SchemaType::EnumType(_) => Ok("TEXT"),
        other => Err(MarretaError::RuntimeError {
            message: format!("unsupported postgres type mapping for '{}'", other),
            line: 0,
            column: 0,
        }),
    }
}

fn postgres_default_expr(expr: &Expression) -> Result<String, MarretaError> {
    match expr {
        Expression::Integer(n) => Ok(n.to_string()),
        Expression::Float(n) => Ok(n.to_string()),
        Expression::Boolean(v) => Ok(if *v { "true".into() } else { "false".into() }),
        Expression::StringLiteral(s) => Ok(format!("'{}'", s.replace('\'', "''"))),
        Expression::Identifier(name) if name == "now" => Ok("now()".to_string()),
        _ => Err(MarretaError::RuntimeError {
            message: format!("unsupported default expression '{:?}'", expr),
            line: 0,
            column: 0,
        }),
    }
}

fn foreign_key_constraint_name(table_name: &str, column_name: &str) -> String {
    format!("fk_{}_{}", table_name, column_name)
}

fn join_sql(statements: &[String]) -> String {
    let mut joined = statements.join("\n\n");
    if !joined.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

fn sanitize_migration_name(name: &str) -> String {
    let mut result = String::new();
    let mut last_was_sep = false;
    for ch in name.chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '_'
        };
        if normalized == '_' {
            if !last_was_sep && !result.is_empty() {
                result.push('_');
            }
            last_was_sep = true;
        } else {
            result.push(normalized);
            last_was_sep = false;
        }
    }
    result.trim_matches('_').to_string()
}

fn parse_migration_filename(
    filename: &str,
    suffix: &str,
) -> Result<(String, String), MarretaError> {
    let stem = filename
        .strip_suffix(suffix)
        .ok_or_else(|| MarretaError::IoError {
            message: format!("invalid migration filename '{}'", filename),
        })?;
    let parts: Vec<&str> = stem.splitn(3, '_').collect();
    if parts.len() != 3 || parts[0].len() != 8 || parts[1].len() != 6 || parts[2].is_empty() {
        return Err(MarretaError::IoError {
            message: format!("invalid migration filename '{}'", filename),
        });
    }
    Ok((format!("{}_{}", parts[0], parts[1]), parts[2].to_string()))
}

fn migration_checksum(up_sql: &str, down_sql: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(up_sql.as_bytes());
    hasher.update(b"\n-- marreta:down --\n");
    if let Some(down_sql) = down_sql {
        hasher.update(down_sql.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

// Spec 073 (2.1): statement classes the replay tolerates because they cannot affect the
// table/column model it derives. Matched case-insensitively against the trimmed statement.
const AUTO_TOLERATED_PREFIXES: &[&str] = &[
    "CREATE INDEX",
    "CREATE UNIQUE INDEX",
    "DROP INDEX",
    "INSERT",
    "UPDATE",
    "DELETE",
    "WITH",
];

const REPLAY_SKIP_MARKER: &str = "-- marreta: skip-replay";

/// Split a SQL script into statements on top-level `;`, ignoring semicolons inside single-quoted
/// string literals (`'...'`, with `''` as an escaped quote), dollar-quoted strings
/// (`$tag$ ... $tag$`, including `$$ ... $$`), and comments (`-- ...` to end of line, `/* ... */`).
/// It does not parse SQL, only finds statement boundaries safely. Spec 073 (2.1): hand-written
/// migrations are now first-class, so the naive `split(';')` is unsafe; a `;` (or an apostrophe
/// that would otherwise open a string) inside a comment must not break the split.
fn split_sql_statements(sql: &str) -> Vec<String> {
    let bytes = sql.as_bytes();
    let n = bytes.len();
    let mut statements = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < n {
        match bytes[i] {
            b'\'' => i = skip_single_quoted(bytes, i),
            b'-' if i + 1 < n && bytes[i + 1] == b'-' => i = skip_line_comment(bytes, i),
            b'/' if i + 1 < n && bytes[i + 1] == b'*' => i = skip_block_comment(bytes, i),
            b'$' => match dollar_tag_len(bytes, i) {
                Some(tag_len) => i = skip_dollar_quoted(sql, i, tag_len),
                None => i += 1,
            },
            b';' => {
                statements.push(sql[start..i].to_string());
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < n {
        statements.push(sql[start..].to_string());
    }
    statements
}

/// Index of the newline ending a `--` line comment starting at `pos` (or EOF). The newline is left
/// in place so it still delimits lines.
fn skip_line_comment(bytes: &[u8], pos: usize) -> usize {
    let n = bytes.len();
    let mut i = pos + 2;
    while i < n && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

/// Index just past a `/* ... */` block comment starting at `pos` (or EOF if unterminated).
fn skip_block_comment(bytes: &[u8], pos: usize) -> usize {
    let n = bytes.len();
    let mut i = pos + 2;
    while i + 1 < n {
        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            return i + 2;
        }
        i += 1;
    }
    n
}

/// Index just past a single-quoted string starting at `open` (a `'`). Handles the `''` escape.
fn skip_single_quoted(bytes: &[u8], open: usize) -> usize {
    let n = bytes.len();
    let mut i = open + 1;
    while i < n {
        if bytes[i] == b'\'' {
            if i + 1 < n && bytes[i + 1] == b'\'' {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    n
}

/// Length of a dollar-quote opening tag at `pos` (`$$` or `$ident$`), or `None` if `pos` does not
/// open a dollar quote. A tag's inner text is a valid identifier (no leading digit), which avoids
/// treating things like `$1` as a tag.
fn dollar_tag_len(bytes: &[u8], pos: usize) -> Option<usize> {
    let n = bytes.len();
    let mut i = pos + 1;
    if i < n && bytes[i] == b'$' {
        return Some(2);
    }
    if i < n && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
        i += 1;
        while i < n && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        if i < n && bytes[i] == b'$' {
            return Some(i + 1 - pos);
        }
    }
    None
}

/// Index just past a dollar-quoted string opened at `open` with a tag of `tag_len` bytes.
fn skip_dollar_quoted(sql: &str, open: usize, tag_len: usize) -> usize {
    let tag = &sql[open..open + tag_len];
    let body_start = open + tag_len;
    match sql[body_start..].find(tag) {
        Some(rel) => body_start + rel + tag_len,
        None => sql.len(),
    }
}

/// Split a `CREATE TABLE` body (the text inside the outer parentheses) into column and constraint
/// definitions on top-level commas, ignoring commas nested in parentheses (for example
/// `NUMERIC(10, 2)`) or inside single-quoted strings. Spec 073 (2.1, pin (a)): with single-line
/// hand-written `CREATE TABLE` now first-class, splitting on lines or naive commas is unsafe.
fn split_column_defs(body: &str) -> Vec<String> {
    let bytes = body.as_bytes();
    let n = bytes.len();
    let mut defs = Vec::new();
    let mut start = 0;
    let mut depth: i32 = 0;
    let mut i = 0;
    while i < n {
        match bytes[i] {
            b'\'' => i = skip_single_quoted(bytes, i),
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
            }
            b',' if depth == 0 => {
                defs.push(body[start..i].to_string());
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < n {
        defs.push(body[start..].to_string());
    }
    defs
}

/// Whether the statement chunk carries the `-- marreta: skip-replay` marker on a line of its own.
fn chunk_has_skip_marker(chunk: &str) -> bool {
    chunk.lines().any(|line| line.trim() == REPLAY_SKIP_MARKER)
}

/// Remove SQL comments (`-- ...` to end of line, `/* ... */`) from a statement, string-safely: a
/// `--` or `/*` inside a single-quoted or dollar-quoted literal is content, not a comment, and is
/// preserved. Spec 073 (2.1): comments (full-line or trailing) must be stripped before the
/// statement is classified or its columns parsed, or a trailing comment corrupts the parse.
fn strip_sql_comments(sql: &str) -> String {
    let bytes = sql.as_bytes();
    let n = bytes.len();
    let mut out = String::with_capacity(sql.len());
    let mut keep_start = 0;
    let mut i = 0;
    while i < n {
        match bytes[i] {
            b'\'' => i = skip_single_quoted(bytes, i),
            b'$' if dollar_tag_len(bytes, i).is_some() => {
                let tag_len = dollar_tag_len(bytes, i).unwrap();
                i = skip_dollar_quoted(sql, i, tag_len);
            }
            b'-' if i + 1 < n && bytes[i + 1] == b'-' => {
                out.push_str(&sql[keep_start..i]);
                i = skip_line_comment(bytes, i);
                keep_start = i;
            }
            b'/' if i + 1 < n && bytes[i + 1] == b'*' => {
                out.push_str(&sql[keep_start..i]);
                i = skip_block_comment(bytes, i);
                keep_start = i;
            }
            _ => i += 1,
        }
    }
    out.push_str(&sql[keep_start..]);
    out
}

/// Replace single-quoted string literals in a column-definition fragment with a single space, so a
/// scan for modifier keywords cannot be fooled by a `DEFAULT 'NOT NULL'` literal. Spec 073 (2.2).
fn blank_string_literals(fragment: &str) -> String {
    let bytes = fragment.as_bytes();
    let n = bytes.len();
    let mut out = String::with_capacity(n);
    let mut keep_start = 0;
    let mut i = 0;
    while i < n {
        if bytes[i] == b'\'' {
            out.push_str(&fragment[keep_start..i]);
            out.push(' ');
            i = skip_single_quoted(bytes, i);
            keep_start = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&fragment[keep_start..]);
    out
}

/// Case-insensitive keyword strip: if `stmt` begins with `keyword`, return the remainder.
fn strip_keyword_ci<'a>(stmt: &'a str, keyword: &str) -> Option<&'a str> {
    stmt.get(..keyword.len())
        .filter(|head| head.eq_ignore_ascii_case(keyword))
        .map(|_| &stmt[keyword.len()..])
}

/// Whether the trimmed, uppercased statement begins with an auto-tolerated class.
fn is_auto_tolerated(upper_stmt: &str) -> bool {
    AUTO_TOLERATED_PREFIXES.iter().any(|prefix| {
        upper_stmt == *prefix
            || upper_stmt
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with(|c: char| c.is_whitespace() || c == '('))
    })
}

/// The improved rejection for column-mutating DDL the replay cannot derive (Spec 073, 2.1):
/// names the file, the statement, the rule, and the two recovery options.
fn rejected_ddl_error(migration: &LocalMigration, statement: &str) -> MarretaError {
    local_migration_parse_error(
        migration,
        format!(
            "unsupported statement '{}'. The replay derives schema state from generated DDL, and \
             this statement changes columns in a way it cannot derive. Either rewrite the change \
             as a schema edit plus a generated migration, or, if the statement is actually \
             schema-neutral, put a line '{}' before it to skip it during derivation.",
            collapse_whitespace(statement),
            REPLAY_SKIP_MARKER
        ),
    )
}

/// Collapse runs of whitespace (including newlines) to single spaces, for one-line error text.
fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Modifier keywords that terminate the type span in a column definition (Spec 073, 2.2).
const TYPE_TERMINATORS: &[&str] = &[
    "PRIMARY",
    "GENERATED",
    "NOT",
    "NULL",
    "DEFAULT",
    "UNIQUE",
    "REFERENCES",
    "CHECK",
    "COLLATE",
];

/// Capture a column's rendered type and nullability from the definition text after the column name
/// (for example `BIGINT PRIMARY KEY NOT NULL`). The type is the leading run of tokens up to the
/// first modifier keyword, with parenthesised groups kept whole so `NUMERIC(10, 2)` survives;
/// returns `None` when no type can be resolved (Spec 073 pin (d): precision over recall, the
/// type-drift check is then skipped for the column). Nullability is `false` when `NOT NULL` or
/// `PRIMARY KEY` is present (pin (c): a primary key is non-null in Postgres), else `true`.
fn capture_type_and_nullability(after_name: &str) -> (Option<String>, Option<bool>) {
    let mut type_tokens = Vec::new();
    for token in tokenize_paren_aware(after_name) {
        if TYPE_TERMINATORS
            .iter()
            .any(|keyword| token.eq_ignore_ascii_case(keyword))
        {
            break;
        }
        type_tokens.push(token);
    }
    if type_tokens.is_empty() {
        return (None, None);
    }
    // Scan for modifiers with string literals blanked, so a `DEFAULT 'NOT NULL'` literal cannot be
    // mistaken for a NOT NULL constraint (pin (d): a false drift is worse than a missed one).
    let modifiers = blank_string_literals(after_name).to_ascii_uppercase();
    let nullable = !(modifiers.contains("NOT NULL") || modifiers.contains("PRIMARY KEY"));
    (Some(type_tokens.join(" ")), Some(nullable))
}

/// Tokenize on whitespace at parenthesis depth 0, keeping parenthesised groups (and their inner
/// commas and spaces) as a single token. Spec 073 (2.2, pin (a)).
fn tokenize_paren_aware(s: &str) -> Vec<String> {
    let bytes = s.as_bytes();
    let n = bytes.len();
    let mut tokens = Vec::new();
    let mut depth: i32 = 0;
    let mut start: Option<usize> = None;
    let mut i = 0;
    while i < n {
        let c = bytes[i];
        if c == b'(' {
            depth += 1;
            if start.is_none() {
                start = Some(i);
            }
        } else if c == b')' {
            depth -= 1;
        } else if depth == 0 && c.is_ascii_whitespace() {
            if let Some(begin) = start.take() {
                tokens.push(s[begin..i].to_string());
            }
        } else if start.is_none() {
            start = Some(i);
        }
        i += 1;
    }
    if let Some(begin) = start {
        tokens.push(s[begin..].to_string());
    }
    tokens
}

/// Normalize a Postgres type for drift comparison, mapping the aliases of the types
/// `postgres_type()` emits to a single canonical form (Spec 073 pin (b)). A type outside that set
/// compares textually (case-insensitive, whitespace-collapsed), best-effort.
fn normalize_pg_type(rendered: &str) -> String {
    let collapsed = collapse_whitespace(rendered).to_ascii_lowercase();
    match collapsed.as_str() {
        "int8" | "bigint" => "bigint".to_string(),
        "float8" | "double precision" => "double precision".to_string(),
        "bool" | "boolean" => "boolean".to_string(),
        "timestamptz" | "timestamp with time zone" => "timestamptz".to_string(),
        "decimal" | "numeric" => "numeric".to_string(),
        _ => collapsed,
    }
}

fn apply_local_migration_to_schema(
    schema: &mut DatabaseSchema,
    migration: &LocalMigration,
) -> Result<(), MarretaError> {
    for raw_stmt in split_sql_statements(&migration.up_sql) {
        let skip_marked = chunk_has_skip_marker(&raw_stmt);
        // Strip comments (full-line and trailing) before classifying or parsing the statement, so a
        // trailing comment never leaks into the column-def parse or the type capture.
        let sql = strip_sql_comments(&raw_stmt);
        let stmt = sql.trim();
        if stmt.is_empty() {
            continue;
        }
        // Tier 2: an explicit per-statement escape valve, regardless of the statement's shape.
        if skip_marked {
            continue;
        }
        // Schema-deriving DDL is parsed first (CREATE TABLE / ALTER TABLE ... ADD).
        if let Some(rest) = strip_keyword_ci(stmt, "CREATE TABLE ") {
            apply_create_table_stmt(schema, rest, &migration.version)?;
            continue;
        }
        if let Some(rest) = strip_keyword_ci(stmt, "ALTER TABLE ") {
            apply_alter_table_stmt(schema, migration, stmt, rest)?;
            continue;
        }
        // Tier 1: statement classes that cannot affect the table/column model.
        if is_auto_tolerated(&stmt.to_ascii_uppercase()) {
            continue;
        }
        // Tier 3: still rejected, with the improved error.
        return Err(rejected_ddl_error(migration, stmt));
    }
    Ok(())
}

fn apply_create_table_stmt(
    schema: &mut DatabaseSchema,
    rest: &str,
    migration_version: &str,
) -> Result<(), MarretaError> {
    let open_paren = rest.find('(').ok_or_else(|| MarretaError::IoError {
        message: format!(
            "could not parse local migration {}: CREATE TABLE missing '('",
            migration_version
        ),
    })?;
    let table_name = rest[..open_paren].trim();
    let body = rest[open_paren + 1..].trim();
    let body = body
        .strip_suffix(')')
        .ok_or_else(|| MarretaError::IoError {
            message: format!(
                "could not parse local migration {}: CREATE TABLE missing closing ')'",
                migration_version
            ),
        })?;

    let mut table = DatabaseTable {
        name: table_name.to_string(),
        columns: HashMap::new(),
        foreign_keys: HashMap::new(),
    };

    for raw_def in split_column_defs(body) {
        let def = raw_def.trim().trim_end_matches(',').trim();
        if def.is_empty() {
            continue;
        }
        if is_table_level_constraint(def) {
            continue;
        }
        let (column_name, after_name) = match def.split_once(char::is_whitespace) {
            Some((name, rest)) => (name, rest),
            None => {
                return Err(MarretaError::IoError {
                    message: format!(
                        "could not parse local migration {}: invalid column definition '{}'",
                        migration_version, def
                    ),
                });
            }
        };
        let (rendered_type, nullable) = capture_type_and_nullability(after_name);
        table.columns.insert(
            column_name.to_string(),
            DatabaseColumn {
                name: column_name.to_string(),
                rendered_type,
                nullable,
            },
        );
    }

    schema.tables.insert(table_name.to_string(), table);
    Ok(())
}

/// Whether a `CREATE TABLE` body entry is a table-level constraint rather than a column.
fn is_table_level_constraint(def: &str) -> bool {
    const CONSTRAINT_LEADS: &[&str] = &[
        "CONSTRAINT",
        "PRIMARY KEY",
        "FOREIGN KEY",
        "UNIQUE",
        "CHECK",
        "EXCLUDE",
    ];
    let upper = def.to_ascii_uppercase();
    CONSTRAINT_LEADS.iter().any(|lead| {
        upper == *lead
            || upper.starts_with(&format!("{} ", lead))
            || upper.starts_with(&format!("{}(", lead))
    })
}

fn apply_alter_table_stmt(
    schema: &mut DatabaseSchema,
    migration: &LocalMigration,
    stmt: &str,
    rest: &str,
) -> Result<(), MarretaError> {
    let migration_version = &migration.version;
    let (table_name, remainder) = rest.split_once(' ').ok_or_else(|| MarretaError::IoError {
        message: format!(
            "could not parse local migration {}: invalid ALTER TABLE '{}'",
            migration_version, rest
        ),
    })?;

    // Only ADD COLUMN / ADD CONSTRAINT are derivable; column-mutating DDL is rejected (tier 3)
    // before mutating the model, so a rejected statement leaves no half-built table behind.
    if let Some(column_def) = strip_keyword_ci(remainder, "ADD COLUMN ") {
        let (column_name, after_name) = match column_def.split_once(char::is_whitespace) {
            Some((name, after)) => (name, after),
            None => {
                return Err(MarretaError::IoError {
                    message: format!(
                        "could not parse local migration {}: invalid ADD COLUMN '{}'",
                        migration_version, column_def
                    ),
                });
            }
        };
        let (rendered_type, nullable) = capture_type_and_nullability(after_name);
        let table = alter_table_entry(schema, table_name);
        table.columns.insert(
            column_name.to_string(),
            DatabaseColumn {
                name: column_name.to_string(),
                rendered_type,
                nullable,
            },
        );
        return Ok(());
    }

    if let Some(fk_def) = strip_keyword_ci(remainder, "ADD CONSTRAINT ") {
        let table = alter_table_entry(schema, table_name);
        let (constraint_name, fk_rest) =
            fk_def
                .split_once(" FOREIGN KEY ")
                .ok_or_else(|| MarretaError::IoError {
                    message: format!(
                        "could not parse local migration {}: invalid ADD CONSTRAINT '{}'",
                        migration_version, fk_def
                    ),
                })?;
        let open = fk_rest.find('(').ok_or_else(|| MarretaError::IoError {
            message: format!(
                "could not parse local migration {}: foreign key missing '('",
                migration_version
            ),
        })?;
        let close = fk_rest.find(')').ok_or_else(|| MarretaError::IoError {
            message: format!(
                "could not parse local migration {}: foreign key missing ')'",
                migration_version
            ),
        })?;
        let column_name = fk_rest[open + 1..close].trim();
        let refs = fk_rest[close + 1..].trim();
        let refs = refs
            .strip_prefix("REFERENCES ")
            .ok_or_else(|| MarretaError::IoError {
                message: format!(
                    "could not parse local migration {}: foreign key missing REFERENCES",
                    migration_version
                ),
            })?;
        let refs_open = refs.find('(').ok_or_else(|| MarretaError::IoError {
            message: format!(
                "could not parse local migration {}: references missing '('",
                migration_version
            ),
        })?;
        let refs_close = refs.find(')').ok_or_else(|| MarretaError::IoError {
            message: format!(
                "could not parse local migration {}: references missing ')'",
                migration_version
            ),
        })?;
        let references_table = refs[..refs_open].trim();
        let references_column = refs[refs_open + 1..refs_close].trim();
        table.foreign_keys.insert(
            constraint_name.trim().to_string(),
            DatabaseForeignKey {
                name: constraint_name.trim().to_string(),
                column_name: column_name.to_string(),
                references_table: references_table.to_string(),
                references_column: references_column.to_string(),
            },
        );
        return Ok(());
    }

    // Tier 3: column-mutating ALTER TABLE (DROP COLUMN, ALTER COLUMN ... TYPE, RENAME) the replay
    // cannot derive. Reject with the improved, actionable error.
    Err(rejected_ddl_error(migration, stmt))
}

/// Get or create the table entry the replay mutates for an ALTER TABLE.
fn alter_table_entry<'a>(
    schema: &'a mut DatabaseSchema,
    table_name: &str,
) -> &'a mut DatabaseTable {
    schema
        .tables
        .entry(table_name.to_string())
        .or_insert_with(|| DatabaseTable {
            name: table_name.to_string(),
            columns: HashMap::new(),
            foreign_keys: HashMap::new(),
        })
}

fn local_migration_parse_error(migration: &LocalMigration, detail: String) -> MarretaError {
    MarretaError::IoError {
        message: format!(
            "could not derive schema from local migration {}_{}: {}",
            migration.version, migration.name, detail
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn field(name: &str, field_type: SchemaType, optional: bool) -> SchemaField {
        SchemaField {
            name: name.into(),
            field_type,
            optional,
        }
    }

    fn schema(db_table: Option<&str>, fields: Vec<SchemaField>) -> SchemaDefinition {
        SchemaDefinition {
            db_table: db_table.map(str::to_string),
            fields,
        }
    }

    #[test]
    fn test_build_persistent_tables_maps_primitive_columns() {
        let mut schemas = HashMap::new();
        schemas.insert(
            "User".into(),
            schema(
                Some("users"),
                vec![
                    field("id", SchemaType::IntegerType, false),
                    field("email", SchemaType::StringType, false),
                    field("created_at", SchemaType::InstantType, false),
                ],
            ),
        );

        let tables = build_persistent_tables(&schemas).unwrap();
        let user = &tables["User"];
        assert_eq!(user.table_name, "users");
        assert_eq!(user.columns.len(), 3);
        assert!(user.columns[0].primary);
        assert!(user.columns[0].generated);
        assert!(!user.columns[1].unique);
        assert_eq!(user.columns[2].field_type, SchemaType::InstantType);
    }

    #[test]
    fn test_build_persistent_tables_infers_foreign_key_column() {
        let mut schemas = HashMap::new();
        schemas.insert(
            "Address".into(),
            schema(
                Some("addresses"),
                vec![field("id", SchemaType::IntegerType, false)],
            ),
        );
        schemas.insert(
            "User".into(),
            schema(
                Some("users"),
                vec![
                    field("id", SchemaType::IntegerType, false),
                    field("address", SchemaType::Reference("Address".into()), true),
                ],
            ),
        );

        let tables = build_persistent_tables(&schemas).unwrap();
        let user = &tables["User"];
        let address_column = user
            .columns
            .iter()
            .find(|column| column.column_name == "address_id")
            .expect("address_id column should be generated");
        assert!(address_column.nullable);
        assert_eq!(user.foreign_keys.len(), 1);
        assert_eq!(user.foreign_keys[0].references_table, "addresses");
        assert_eq!(user.foreign_keys[0].references_column, "id");
    }

    #[test]
    fn test_schema_type_to_sql_maps_temporal_types() {
        assert_eq!(
            postgres_type(&SchemaType::InstantType).unwrap(),
            "TIMESTAMPTZ"
        );
        assert_eq!(postgres_type(&SchemaType::DateType).unwrap(), "DATE");
        assert_eq!(postgres_type(&SchemaType::TimeType).unwrap(), "TIME");
        assert_eq!(postgres_type(&SchemaType::DurationType).unwrap(), "BIGINT");
        assert_eq!(postgres_type(&SchemaType::IntervalType).unwrap(), "JSONB");
    }

    #[test]
    fn test_schema_type_to_sql_maps_enum_and_decimal_contract_types() {
        assert_eq!(
            postgres_type(&SchemaType::EnumType(vec!["pending".into(), "paid".into()])).unwrap(),
            "TEXT"
        );
        assert_eq!(postgres_type(&SchemaType::DecimalType).unwrap(), "NUMERIC");
    }

    #[test]
    fn test_render_postgres_up_sql_for_persistent_enum_uses_text_without_constraint() {
        let ops = vec![MigrationOp::CreateTable {
            table_name: "orders".into(),
            columns: vec![PersistentColumn {
                field_name: "status".into(),
                column_name: "status".into(),
                field_type: SchemaType::EnumType(vec!["pending".into(), "paid".into()]),
                nullable: false,
                primary: false,
                generated: false,
                unique: false,
                default: None,
            }],
        }];

        let sql = render_postgres_up_sql(&ops).unwrap().join("\n");
        assert!(sql.contains("status TEXT NOT NULL"));
        assert!(!sql.contains("CREATE TYPE"));
        assert!(!sql.contains("CHECK"));
    }

    #[test]
    fn test_build_persistent_tables_rejects_string_id() {
        let mut schemas = HashMap::new();
        schemas.insert(
            "User".into(),
            schema(
                Some("users"),
                vec![field("id", SchemaType::StringType, false)],
            ),
        );

        let err = build_persistent_tables(&schemas).unwrap_err();
        assert!(matches!(
            err,
            MarretaError::InvalidPersistentSchemaDefinition { .. }
        ));
    }

    #[test]
    fn test_build_persistent_tables_ignores_list_of_schema_for_storage() {
        let mut schemas = HashMap::new();
        schemas.insert(
            "Item".into(),
            schema(
                Some("items"),
                vec![
                    field("id", SchemaType::IntegerType, false),
                    field("owner", SchemaType::Reference("Order".into()), false),
                ],
            ),
        );
        schemas.insert(
            "Order".into(),
            schema(
                Some("orders"),
                vec![
                    field("id", SchemaType::IntegerType, false),
                    field(
                        "items",
                        SchemaType::TypedList(Box::new(SchemaType::Reference("Item".into()))),
                        false,
                    ),
                ],
            ),
        );

        let tables = build_persistent_tables(&schemas).unwrap();
        let order = &tables["Order"];
        assert_eq!(order.columns.len(), 1);
        assert_eq!(order.columns[0].column_name, "id");
        assert!(order.foreign_keys.is_empty());
    }

    #[test]
    fn test_plan_migration_creates_missing_tables_and_fk() {
        let mut desired = HashMap::new();
        desired.insert(
            "Address".into(),
            PersistentTable {
                schema_name: "Address".into(),
                table_name: "addresses".into(),
                columns: vec![PersistentColumn {
                    field_name: "id".into(),
                    column_name: "id".into(),
                    field_type: SchemaType::IntegerType,
                    nullable: false,
                    primary: true,
                    generated: true,
                    unique: false,
                    default: None,
                }],
                foreign_keys: vec![],
            },
        );
        desired.insert(
            "User".into(),
            PersistentTable {
                schema_name: "User".into(),
                table_name: "users".into(),
                columns: vec![PersistentColumn {
                    field_name: "address".into(),
                    column_name: "address_id".into(),
                    field_type: SchemaType::IntegerType,
                    nullable: false,
                    primary: false,
                    generated: false,
                    unique: false,
                    default: None,
                }],
                foreign_keys: vec![PersistentForeignKey {
                    field_name: "address".into(),
                    column_name: "address_id".into(),
                    references_schema: "Address".into(),
                    references_table: "addresses".into(),
                    references_column: "id".into(),
                    nullable: false,
                }],
            },
        );

        let ops = plan_migration(&DatabaseSchema::default(), &desired);
        assert!(ops.iter().any(|op| matches!(op, MigrationOp::CreateTable { table_name, .. } if table_name == "addresses")));
        assert!(ops.iter().any(
            |op| matches!(op, MigrationOp::CreateTable { table_name, .. } if table_name == "users")
        ));
        assert!(ops.iter().any(|op| matches!(op, MigrationOp::AddForeignKey { table_name, .. } if table_name == "users")));
        let last_create_index = ops
            .iter()
            .rposition(|op| matches!(op, MigrationOp::CreateTable { .. }))
            .expect("expected create table ops");
        let first_fk_index = ops
            .iter()
            .position(|op| matches!(op, MigrationOp::AddForeignKey { .. }))
            .expect("expected foreign key ops");
        assert!(last_create_index < first_fk_index);
    }

    #[test]
    fn test_plan_migration_adds_missing_column_to_existing_table() {
        let current = DatabaseSchema {
            tables: HashMap::from([(
                "users".into(),
                DatabaseTable {
                    name: "users".into(),
                    columns: HashMap::new(),
                    foreign_keys: HashMap::new(),
                },
            )]),
        };
        let desired = HashMap::from([(
            "User".into(),
            PersistentTable {
                schema_name: "User".into(),
                table_name: "users".into(),
                columns: vec![PersistentColumn {
                    field_name: "email".into(),
                    column_name: "email".into(),
                    field_type: SchemaType::StringType,
                    nullable: false,
                    primary: false,
                    generated: false,
                    unique: true,
                    default: None,
                }],
                foreign_keys: vec![],
            },
        )]);

        let ops = plan_migration(&current, &desired);
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], MigrationOp::AddColumn { .. }));
    }

    #[test]
    fn test_render_postgres_up_sql_for_create_table_and_fk() {
        let ops = vec![
            MigrationOp::CreateTable {
                table_name: "users".into(),
                columns: vec![
                    PersistentColumn {
                        field_name: "id".into(),
                        column_name: "id".into(),
                        field_type: SchemaType::IntegerType,
                        nullable: false,
                        primary: true,
                        generated: true,
                        unique: false,
                        default: None,
                    },
                    PersistentColumn {
                        field_name: "active".into(),
                        column_name: "active".into(),
                        field_type: SchemaType::BooleanType,
                        nullable: false,
                        primary: false,
                        generated: false,
                        unique: false,
                        default: Some(Expression::Boolean(true)),
                    },
                ],
            },
            MigrationOp::AddForeignKey {
                table_name: "users".into(),
                foreign_key: PersistentForeignKey {
                    field_name: "address".into(),
                    column_name: "address_id".into(),
                    references_schema: "Address".into(),
                    references_table: "addresses".into(),
                    references_column: "id".into(),
                    nullable: false,
                },
            },
        ];

        let sql = render_postgres_up_sql(&ops).unwrap();
        assert!(sql[0].contains("CREATE TABLE users"));
        assert!(sql[0].contains("GENERATED BY DEFAULT AS IDENTITY"));
        assert!(sql[0].contains("DEFAULT true"));
        assert!(sql[1].contains("ALTER TABLE users ADD CONSTRAINT fk_users_address_id"));
    }

    #[test]
    fn test_render_postgres_down_sql_reverses_ops() {
        let ops = vec![
            MigrationOp::AddForeignKey {
                table_name: "users".into(),
                foreign_key: PersistentForeignKey {
                    field_name: "address".into(),
                    column_name: "address_id".into(),
                    references_schema: "Address".into(),
                    references_table: "addresses".into(),
                    references_column: "id".into(),
                    nullable: false,
                },
            },
            MigrationOp::AddColumn {
                table_name: "users".into(),
                column: PersistentColumn {
                    field_name: "email".into(),
                    column_name: "email".into(),
                    field_type: SchemaType::StringType,
                    nullable: false,
                    primary: false,
                    generated: false,
                    unique: false,
                    default: None,
                },
            },
        ];

        let sql = render_postgres_down_sql(&ops).unwrap();
        assert_eq!(sql[0], "ALTER TABLE users DROP COLUMN email;");
        assert_eq!(
            sql[1],
            "ALTER TABLE users DROP CONSTRAINT fk_users_address_id;"
        );
    }

    #[test]
    fn test_write_and_discover_local_migrations() {
        let dir = tempdir().unwrap();
        let migration = write_migration_files(
            dir.path(),
            "Create Users",
            &[String::from("CREATE TABLE users (id BIGINT PRIMARY KEY);")],
            &[String::from("DROP TABLE users;")],
        )
        .unwrap();

        let discovered = discover_local_migrations(dir.path()).unwrap();
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].version, migration.version);
        assert_eq!(discovered[0].name, "create_users");
        assert_eq!(discovered[0].checksum, migration.checksum);
        assert!(discovered[0].up_sql.contains("CREATE TABLE users"));
        assert!(
            discovered[0]
                .down_sql
                .as_deref()
                .unwrap()
                .contains("DROP TABLE users")
        );
    }

    #[test]
    fn test_build_schema_from_local_migrations_reconstructs_tables_columns_and_fks() {
        let dir = tempdir().unwrap();
        write_migration_files(
            dir.path(),
            "Create Customers",
            &[
                String::from(
                    "CREATE TABLE customers (\n  id BIGINT PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY NOT NULL,\n  name TEXT NOT NULL\n);",
                ),
                String::from(
                    "CREATE TABLE orders (\n  id BIGINT PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY NOT NULL,\n  customer_id BIGINT NOT NULL,\n  status TEXT NOT NULL\n);",
                ),
                String::from(
                    "ALTER TABLE orders ADD CONSTRAINT fk_orders_customer_id FOREIGN KEY (customer_id) REFERENCES customers(id);",
                ),
            ],
            &[
                String::from("ALTER TABLE orders DROP CONSTRAINT fk_orders_customer_id;"),
                String::from("DROP TABLE orders;"),
                String::from("DROP TABLE customers;"),
            ],
        )
        .unwrap();

        let local = discover_local_migrations(dir.path()).unwrap();
        let schema = build_schema_from_local_migrations(&local).unwrap();
        let customers = schema.tables.get("customers").expect("customers table");
        let orders = schema.tables.get("orders").expect("orders table");

        assert!(customers.columns.contains_key("id"));
        assert!(customers.columns.contains_key("name"));
        assert!(orders.columns.contains_key("id"));
        assert!(orders.columns.contains_key("customer_id"));
        assert!(orders.columns.contains_key("status"));
        assert!(orders.foreign_keys.contains_key("fk_orders_customer_id"));
        assert_eq!(
            orders.foreign_keys["fk_orders_customer_id"].references_table,
            "customers"
        );
        assert_eq!(
            orders.foreign_keys["fk_orders_customer_id"].references_column,
            "id"
        );
    }

    #[test]
    fn test_compare_migration_state_classifies_versions() {
        let local = vec![
            LocalMigration {
                version: "20260410_120000".into(),
                name: "create_users".into(),
                up_path: PathBuf::from("migrations/20260410_120000_create_users.up.sql"),
                down_path: Some(PathBuf::from(
                    "migrations/20260410_120000_create_users.down.sql",
                )),
                up_sql: "CREATE TABLE users ();".into(),
                down_sql: Some("DROP TABLE users;".into()),
                checksum: "same".into(),
            },
            LocalMigration {
                version: "20260410_130000".into(),
                name: "alter_users".into(),
                up_path: PathBuf::from("migrations/20260410_130000_alter_users.up.sql"),
                down_path: Some(PathBuf::from(
                    "migrations/20260410_130000_alter_users.down.sql",
                )),
                up_sql: "ALTER TABLE users ADD COLUMN email TEXT;".into(),
                down_sql: Some("ALTER TABLE users DROP COLUMN email;".into()),
                checksum: "local_changed".into(),
            },
            LocalMigration {
                version: "20260410_140000".into(),
                name: "create_orders".into(),
                up_path: PathBuf::from("migrations/20260410_140000_create_orders.up.sql"),
                down_path: Some(PathBuf::from(
                    "migrations/20260410_140000_create_orders.down.sql",
                )),
                up_sql: "CREATE TABLE orders ();".into(),
                down_sql: Some("DROP TABLE orders;".into()),
                checksum: "pending".into(),
            },
        ];
        let applied = vec![
            AppliedMigration {
                version: "20260410_120000".into(),
                name: "create_users".into(),
                checksum: "same".into(),
                applied_at: "2026-04-10T12:00:00Z".into(),
            },
            AppliedMigration {
                version: "20260410_130000".into(),
                name: "alter_users".into(),
                checksum: "db_changed".into(),
                applied_at: "2026-04-10T13:00:00Z".into(),
            },
            AppliedMigration {
                version: "20260410_110000".into(),
                name: "bootstrap".into(),
                checksum: "missing".into(),
                applied_at: "2026-04-10T11:00:00Z".into(),
            },
        ];

        let report = compare_migration_state(&local, &applied);
        assert_eq!(report.applied, vec!["20260410_120000_create_users"]);
        assert_eq!(report.changed, vec!["20260410_130000_alter_users"]);
        assert_eq!(report.pending, vec!["20260410_140000_create_orders"]);
        assert_eq!(report.missing_local, vec!["20260410_110000_bootstrap"]);
    }

    #[test]
    fn test_build_migration_inventory_orders_and_classifies_states() {
        let local = vec![
            LocalMigration {
                version: "20260410_120000".into(),
                name: "create_users".into(),
                up_path: PathBuf::from("migrations/20260410_120000_create_users.up.sql"),
                down_path: Some(PathBuf::from(
                    "migrations/20260410_120000_create_users.down.sql",
                )),
                up_sql: "CREATE TABLE users ();".into(),
                down_sql: Some("DROP TABLE users;".into()),
                checksum: "same".into(),
            },
            LocalMigration {
                version: "20260410_130000".into(),
                name: "alter_users".into(),
                up_path: PathBuf::from("migrations/20260410_130000_alter_users.up.sql"),
                down_path: Some(PathBuf::from(
                    "migrations/20260410_130000_alter_users.down.sql",
                )),
                up_sql: "ALTER TABLE users ADD COLUMN email TEXT;".into(),
                down_sql: Some("ALTER TABLE users DROP COLUMN email;".into()),
                checksum: "local_changed".into(),
            },
            LocalMigration {
                version: "20260410_140000".into(),
                name: "create_orders".into(),
                up_path: PathBuf::from("migrations/20260410_140000_create_orders.up.sql"),
                down_path: Some(PathBuf::from(
                    "migrations/20260410_140000_create_orders.down.sql",
                )),
                up_sql: "CREATE TABLE orders ();".into(),
                down_sql: Some("DROP TABLE orders;".into()),
                checksum: "pending".into(),
            },
        ];
        let applied = vec![
            AppliedMigration {
                version: "20260410_120000".into(),
                name: "create_users".into(),
                checksum: "same".into(),
                applied_at: "2026-04-10T12:00:00Z".into(),
            },
            AppliedMigration {
                version: "20260410_130000".into(),
                name: "alter_users".into(),
                checksum: "db_changed".into(),
                applied_at: "2026-04-10T13:00:00Z".into(),
            },
            AppliedMigration {
                version: "20260410_110000".into(),
                name: "bootstrap".into(),
                checksum: "missing".into(),
                applied_at: "2026-04-10T11:00:00Z".into(),
            },
        ];

        let inventory = build_migration_inventory(&local, &applied);
        assert_eq!(inventory.len(), 4);
        assert_eq!(inventory[0].version, "20260410_110000");
        assert_eq!(inventory[0].name, "bootstrap");
        assert_eq!(inventory[0].state, MigrationState::MissingLocal);
        assert_eq!(inventory[1].state, MigrationState::Applied);
        assert_eq!(inventory[2].state, MigrationState::Changed);
        assert_eq!(inventory[3].state, MigrationState::Pending);
    }

    #[test]
    fn test_discard_pending_migration_removes_pair() {
        let dir = tempdir().unwrap();
        let migration = write_migration_files(
            dir.path(),
            "Alter Users",
            &[String::from("ALTER TABLE users ADD COLUMN email TEXT;")],
            &[String::from("ALTER TABLE users DROP COLUMN email;")],
        )
        .unwrap();
        let local = discover_local_migrations(dir.path()).unwrap();
        let discarded = discard_pending_migration(&migration.version, &local, &[]).unwrap();

        assert_eq!(
            discarded,
            format!("{}_{}", migration.version, migration.name)
        );
        assert!(!migration.up_path.exists());
        assert!(!migration.down_path.unwrap().exists());
    }

    #[test]
    fn test_discard_pending_migration_fails_for_applied() {
        let local = vec![LocalMigration {
            version: "20260410_120000".into(),
            name: "create_users".into(),
            up_path: PathBuf::from("migrations/20260410_120000_create_users.up.sql"),
            down_path: Some(PathBuf::from(
                "migrations/20260410_120000_create_users.down.sql",
            )),
            up_sql: "CREATE TABLE users ();".into(),
            down_sql: Some("DROP TABLE users;".into()),
            checksum: "same".into(),
        }];
        let applied = vec![AppliedMigration {
            version: "20260410_120000".into(),
            name: "create_users".into(),
            checksum: "same".into(),
            applied_at: "2026-04-10T12:00:00Z".into(),
        }];

        let err = discard_pending_migration("20260410_120000", &local, &applied).unwrap_err();
        assert!(err.to_string().contains("already applied"));
    }

    #[test]
    fn test_discard_pending_migration_fails_when_pair_incomplete() {
        let dir = tempdir().unwrap();
        let up_path = dir.path().join("20260410_120000_create_users.up.sql");
        std::fs::write(&up_path, "CREATE TABLE users ();").unwrap();
        let local = vec![LocalMigration {
            version: "20260410_120000".into(),
            name: "create_users".into(),
            up_path,
            down_path: Some(dir.path().join("20260410_120000_create_users.down.sql")),
            up_sql: "CREATE TABLE users ();".into(),
            down_sql: None,
            checksum: "pending".into(),
        }];

        let err = discard_pending_migration("20260410_120000", &local, &[]).unwrap_err();
        assert!(
            err.to_string()
                .contains("local migration pair is incomplete")
        );
    }

    // --- Spec 073: migrate roundness pass ---

    fn mig(version: &str, up_sql: &str) -> LocalMigration {
        LocalMigration {
            version: version.into(),
            name: "test".into(),
            up_path: PathBuf::from(format!("migrations/{}_test.up.sql", version)),
            down_path: None,
            up_sql: up_sql.into(),
            down_sql: None,
            checksum: "x".into(),
        }
    }

    fn desired_accounts(balance: SchemaType) -> HashMap<String, PersistentTable> {
        let mut schemas = HashMap::new();
        schemas.insert(
            "Account".into(),
            schema(
                Some("accounts"),
                vec![
                    field("id", SchemaType::IntegerType, false),
                    field("owner", SchemaType::StringType, false),
                    field("balance", balance, false),
                ],
            ),
        );
        build_persistent_tables(&schemas).unwrap()
    }

    const ACCOUNTS_DDL: &str = "CREATE TABLE accounts (id BIGINT PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY NOT NULL, owner TEXT NOT NULL, balance BIGINT NOT NULL);";

    // 2.1: the statement splitter is string-safe and dollar-quote-safe.
    #[test]
    fn test_split_sql_statements_ignores_semicolons_in_strings_and_dollar_quotes() {
        let stmts = split_sql_statements("INSERT INTO t VALUES ('a;b'); CREATE INDEX i ON t (c);");
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].contains("'a;b'"));

        let stmts = split_sql_statements("DO $$ BEGIN; END $$; SELECT 1;");
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].contains("BEGIN; END"));
    }

    // 2.1 Finding 1: comments must not break the split. A `;` or an apostrophe inside a `--` or
    // `/* */` comment is comment text, not a delimiter or a string start; a `--` inside a string
    // literal is content, not a comment.
    #[test]
    fn test_split_sql_statements_is_comment_aware() {
        // a `;` inside a line comment does not split
        let stmts =
            split_sql_statements("CREATE INDEX i ON t (c); -- drop it later; really\nSELECT 1;");
        assert_eq!(stmts.len(), 2);

        // an apostrophe inside a comment does not open a string and swallow the next `;`
        let stmts = split_sql_statements(
            "CREATE TABLE accounts ( -- the customer's accounts\n  id BIGINT PRIMARY KEY\n);\nSELECT 1;",
        );
        assert_eq!(stmts.len(), 2);

        // a `;` inside a block comment does not split
        let stmts = split_sql_statements("CREATE INDEX i ON t (c) /* a;b */;\nSELECT 1;");
        assert_eq!(stmts.len(), 2);

        // a `--` inside a string literal is preserved (not treated as a comment)
        let stmts = split_sql_statements("INSERT INTO t VALUES ('a--b'); SELECT 1;");
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].contains("'a--b'"));
    }

    // 2.1 Finding 1: a trailing comment (with an apostrophe) is stripped before the column-def
    // parse, so the table derives correctly and the type capture is not corrupted.
    #[test]
    fn test_replay_handles_trailing_comment_with_apostrophe() {
        let mut schema = DatabaseSchema::default();
        apply_local_migration_to_schema(
            &mut schema,
            &mig(
                "20260101_120000",
                "CREATE TABLE accounts ( -- the customer's accounts\n  id BIGINT PRIMARY KEY NOT NULL,\n  owner TEXT NOT NULL, -- who owns it\n  balance BIGINT NOT NULL\n);",
            ),
        )
        .unwrap();
        let accounts = &schema.tables["accounts"];
        assert_eq!(accounts.columns.len(), 3);
        assert_eq!(
            accounts.columns["balance"].rendered_type.as_deref(),
            Some("BIGINT")
        );
        assert_eq!(accounts.columns["owner"].nullable, Some(false));
    }

    // 2.2 minor note: a DEFAULT 'NOT NULL' string literal must not be read as a NOT NULL constraint.
    #[test]
    fn test_default_literal_does_not_fake_not_null() {
        let (rendered, nullable) = capture_type_and_nullability("TEXT DEFAULT 'NOT NULL'");
        assert_eq!(rendered.as_deref(), Some("TEXT"));
        assert_eq!(nullable, Some(true));
    }

    // 2.1 pin (a): the column-def split is parenthesis aware.
    #[test]
    fn test_split_column_defs_is_paren_aware() {
        let defs = split_column_defs("id BIGINT, amount NUMERIC(10, 2) NOT NULL, name TEXT");
        assert_eq!(defs.len(), 3);
        assert_eq!(defs[1].trim(), "amount NUMERIC(10, 2) NOT NULL");
    }

    // 2.1 tier 1: auto-tolerated statement classes are skipped, derivation is unchanged.
    #[test]
    fn test_replay_auto_tolerates_schema_neutral_statements() {
        let mut schema = DatabaseSchema::default();
        apply_local_migration_to_schema(&mut schema, &mig("20260101_120000", ACCOUNTS_DDL))
            .unwrap();
        apply_local_migration_to_schema(
            &mut schema,
            &mig(
                "20260101_120001",
                "CREATE INDEX idx ON accounts (owner);\n\
                 CREATE UNIQUE INDEX uidx ON accounts (owner);\n\
                 DROP INDEX idx;\n\
                 INSERT INTO accounts (owner) VALUES ('x');\n\
                 UPDATE accounts SET owner = 'y';\n\
                 DELETE FROM accounts WHERE id = 1;\n\
                 WITH cte AS (SELECT 1) SELECT * FROM cte;",
            ),
        )
        .unwrap();
        let accounts = &schema.tables["accounts"];
        assert_eq!(accounts.columns.len(), 3);
        assert!(accounts.columns.contains_key("owner"));
    }

    // 2.1 tier 2: the skip-replay marker skips an otherwise-rejected statement.
    #[test]
    fn test_replay_skip_marker_skips_arbitrary_statement() {
        let mut schema = DatabaseSchema::default();
        apply_local_migration_to_schema(
            &mut schema,
            &mig(
                "20260101_120000",
                "-- marreta: skip-replay\nCREATE EXTENSION pgcrypto;",
            ),
        )
        .unwrap();
        assert!(schema.tables.is_empty());
    }

    // 2.1 tier 3: column-mutating DDL is still rejected, with the improved error.
    #[test]
    fn test_replay_rejects_column_mutating_ddl_with_actionable_error() {
        for stmt in [
            "ALTER TABLE accounts DROP COLUMN owner;",
            "ALTER TABLE accounts ALTER COLUMN balance TYPE TEXT;",
            "DROP TABLE accounts;",
        ] {
            let mut schema = DatabaseSchema::default();
            apply_local_migration_to_schema(&mut schema, &mig("20260101_120000", ACCOUNTS_DDL))
                .unwrap();
            let err = apply_local_migration_to_schema(&mut schema, &mig("20260101_120001", stmt))
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("20260101_120001_test"),
                "names the file: {err}"
            );
            assert!(
                err.contains(stmt.trim_end_matches(';')),
                "names the statement: {err}"
            );
            assert!(
                err.contains("skip-replay"),
                "offers the escape valve: {err}"
            );
        }
    }

    // 2.2: drift report cases.
    #[test]
    fn test_drift_reports_type_change_but_not_a_match() {
        let mut current = DatabaseSchema::default();
        apply_local_migration_to_schema(&mut current, &mig("20260101_120000", ACCOUNTS_DDL))
            .unwrap();

        let drift = detect_schema_drift(&current, &desired_accounts(SchemaType::StringType));
        assert!(drift.iter().any(|d| matches!(
            d,
            SchemaDrift::ColumnTypeDiffers { column, .. } if column == "balance"
        )));

        let clean = detect_schema_drift(&current, &desired_accounts(SchemaType::IntegerType));
        assert!(clean.is_empty(), "no drift when types match: {clean:?}");
    }

    #[test]
    fn test_drift_reports_nullability_change() {
        let mut current = DatabaseSchema::default();
        apply_local_migration_to_schema(&mut current, &mig("20260101_120000", ACCOUNTS_DDL))
            .unwrap();

        let mut schemas = HashMap::new();
        schemas.insert(
            "Account".into(),
            schema(
                Some("accounts"),
                vec![
                    field("id", SchemaType::IntegerType, false),
                    field("owner", SchemaType::StringType, true), // now nullable
                    field("balance", SchemaType::IntegerType, false),
                ],
            ),
        );
        let desired = build_persistent_tables(&schemas).unwrap();

        let drift = detect_schema_drift(&current, &desired);
        assert!(drift.iter().any(|d| matches!(
            d,
            SchemaDrift::ColumnNullabilityDiffers { column, .. } if column == "owner"
        )));
    }

    #[test]
    fn test_drift_reports_removed_field_and_removed_table() {
        let mut current = DatabaseSchema::default();
        apply_local_migration_to_schema(
            &mut current,
            &mig(
                "20260101_120000",
                "CREATE TABLE accounts (id BIGINT PRIMARY KEY NOT NULL, owner TEXT NOT NULL, balance BIGINT NOT NULL, legacy TEXT NOT NULL);",
            ),
        )
        .unwrap();
        apply_local_migration_to_schema(
            &mut current,
            &mig(
                "20260101_120001",
                "CREATE TABLE ghosts (id BIGINT NOT NULL);",
            ),
        )
        .unwrap();

        let drift = detect_schema_drift(&current, &desired_accounts(SchemaType::IntegerType));
        assert!(drift.iter().any(|d| matches!(
            d,
            SchemaDrift::ColumnRemoved { column, .. } if column == "legacy"
        )));
        assert!(drift.iter().any(|d| matches!(
            d,
            SchemaDrift::TableRemoved { table } if table == "ghosts"
        )));
    }

    // 2.2 pin (a): a single-line NUMERIC(10, 2) column captures its full type.
    #[test]
    fn test_capture_keeps_parenthesised_type() {
        let (rendered, nullable) = capture_type_and_nullability("NUMERIC(10, 2) NOT NULL");
        assert_eq!(rendered.as_deref(), Some("NUMERIC(10, 2)"));
        assert_eq!(nullable, Some(false));
    }

    // 2.2 pin (b): aliases of our rendered set normalize equal; genuine changes still differ.
    #[test]
    fn test_type_alias_normalization() {
        assert_eq!(normalize_pg_type("int8"), normalize_pg_type("BIGINT"));
        assert_eq!(
            normalize_pg_type("timestamptz"),
            normalize_pg_type("timestamp with time zone")
        );
        assert_eq!(
            normalize_pg_type("float8"),
            normalize_pg_type("DOUBLE PRECISION")
        );
        assert_ne!(normalize_pg_type("BIGINT"), normalize_pg_type("TEXT"));

        let mut current = DatabaseSchema::default();
        apply_local_migration_to_schema(
            &mut current,
            &mig(
                "20260101_120000",
                "CREATE TABLE accounts (id BIGINT PRIMARY KEY NOT NULL, owner TEXT NOT NULL, balance INT8 NOT NULL);",
            ),
        )
        .unwrap();
        let drift = detect_schema_drift(&current, &desired_accounts(SchemaType::IntegerType));
        assert!(
            drift.is_empty(),
            "INT8 must not drift against BIGINT: {drift:?}"
        );
    }

    // 2.2 pin (c): PRIMARY KEY implies NOT NULL.
    #[test]
    fn test_primary_key_implies_not_null() {
        let (_, nullable) = capture_type_and_nullability("BIGINT PRIMARY KEY");
        assert_eq!(nullable, Some(false));
    }

    // 2.2 pin (d): an unresolved type captures None and produces no drift entry.
    #[test]
    fn test_unresolved_type_is_skipped_silently() {
        let (rendered, nullable) = capture_type_and_nullability("NOT NULL");
        assert_eq!(rendered, None);
        assert_eq!(nullable, None);

        let mut current = DatabaseSchema::default();
        let mut columns = HashMap::new();
        columns.insert(
            "id".into(),
            DatabaseColumn {
                name: "id".into(),
                rendered_type: Some("BIGINT".into()),
                nullable: Some(false),
            },
        );
        columns.insert(
            "owner".into(),
            DatabaseColumn {
                name: "owner".into(),
                rendered_type: Some("TEXT".into()),
                nullable: Some(false),
            },
        );
        columns.insert(
            "balance".into(),
            DatabaseColumn {
                name: "balance".into(),
                rendered_type: None,
                nullable: None,
            },
        );
        current.tables.insert(
            "accounts".into(),
            DatabaseTable {
                name: "accounts".into(),
                columns,
                foreign_keys: HashMap::new(),
            },
        );

        let drift = detect_schema_drift(&current, &desired_accounts(SchemaType::StringType));
        assert!(
            drift.is_empty(),
            "unresolved type must not drift: {drift:?}"
        );
    }

    // 2.3: same-second generate collision is detected (no silent clobber).
    #[test]
    fn test_migration_version_collision_is_detected() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("20260101_120000_create_users.up.sql"),
            "CREATE TABLE users ();",
        )
        .unwrap();
        assert!(migration_version_exists(dir.path(), "20260101_120000").unwrap());
        assert!(!migration_version_exists(dir.path(), "20260101_120001").unwrap());
    }
}
