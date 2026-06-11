/// Multi-file project loader for MarretaLang.
///
/// Each `.marreta` file becomes a runtime module:
/// - `app.marreta` is implicitly global/public
/// - other files keep private top-level declarations for same-file use
/// - `export` publishes symbols into the shared public runtime
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::{AuthProvider, Expression, SchemaType, Statement};
use crate::auth::build_auth_registry;
use crate::environment::Environment;
use crate::error::MarretaError;
use crate::feature_flags::FeatureFlags;
use crate::interpreter::Interpreter;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::persistent_schema::{collect_persistent_schemas, validate_persistent_schema_references};
use crate::route_loader::{
    ConsumerDefinition, RouteDefinition, RouteRegistry, SchemaDefinition, validate_auth_contract,
};
use crate::value::Value;

pub type ModuleId = String;

#[derive(Debug, Clone)]
pub struct ModuleDefinition {
    pub id: ModuleId,
    pub is_entrypoint: bool,
    pub startup_stmts: Vec<Statement>,
    pub public_symbols: Vec<String>,
    pub routes: Vec<RouteDefinition>,
    pub consumers: Vec<ConsumerDefinition>,
    pub auth_providers: HashMap<String, AuthProvider>,
    pub private_schemas: HashMap<String, SchemaDefinition>,
    pub public_schemas: HashMap<String, SchemaDefinition>,
}

#[derive(Debug, Clone)]
pub struct ModuleRuntime {
    pub id: ModuleId,
    pub env: Environment,
    pub visible_schemas: HashMap<String, SchemaDefinition>,
}

#[derive(Debug, Clone)]
pub struct ProjectRuntime {
    pub global_env: Environment,
    pub modules: HashMap<ModuleId, ModuleRuntime>,
    pub public_schemas: HashMap<String, SchemaDefinition>,
    pub persistent_schemas: HashMap<String, SchemaDefinition>,
    pub feature_flags: FeatureFlags,
    /// File-name namespaces (Spec 061): `file stem -> { exported task name -> Task }`.
    /// An exported task is reached cross-file only as `stem.task()`; bare calls
    /// resolve only within the declaring file and from `app.marreta`.
    pub task_namespaces: HashMap<String, HashMap<String, Value>>,
}

impl ProjectRuntime {
    pub fn single(
        global_env: Environment,
        public_schemas: HashMap<String, SchemaDefinition>,
    ) -> Self {
        Self {
            global_env,
            modules: HashMap::new(),
            public_schemas,
            persistent_schemas: HashMap::new(),
            feature_flags: FeatureFlags::default(),
            task_namespaces: HashMap::new(),
        }
    }

    /// The exported task `task` under file-namespace `namespace`, if any.
    pub fn module_task(&self, namespace: &str, task: &str) -> Option<Value> {
        self.task_namespaces
            .get(namespace)
            .and_then(|tasks| tasks.get(task))
            .cloned()
    }

    /// Whether `namespace` names a known file-namespace (a file that exports tasks).
    pub fn is_module_namespace(&self, namespace: &str) -> bool {
        self.task_namespaces.contains_key(namespace)
    }

    pub fn env_for_module(&self, module_id: Option<&str>) -> Environment {
        module_id
            .and_then(|id| self.modules.get(id).map(|module| module.env.clone()))
            .unwrap_or_else(|| self.global_env.clone())
    }

    /// Freezes the global and per-module environments into shared, read-only
    /// bases so that `env_for_module` (and per task call / per broadcast branch
    /// clones) only bump an `Arc` instead of deep-copying every definition. Call
    /// once after project load and after all startup injection, before serving.
    pub fn freeze_envs(&mut self) {
        self.global_env.freeze();
        for module in self.modules.values_mut() {
            module.env.freeze();
        }
    }

    pub fn visible_schemas_for(
        &self,
        module_id: Option<&str>,
    ) -> HashMap<String, SchemaDefinition> {
        module_id
            .and_then(|id| {
                self.modules
                    .get(id)
                    .map(|module| module.visible_schemas.clone())
            })
            .unwrap_or_else(|| self.public_schemas.clone())
    }

    pub fn resolve_schema(
        &self,
        module_id: Option<&str>,
        schema_name: &str,
    ) -> Option<SchemaDefinition> {
        if let Some(id) = module_id
            && let Some(module) = self.modules.get(id)
            && let Some(schema) = module.visible_schemas.get(schema_name)
        {
            return Some(schema.clone());
        }

        self.public_schemas.get(schema_name).cloned()
    }

    pub fn resolve_persistent_schema_for_table(
        &self,
        table: &str,
    ) -> Option<(String, SchemaDefinition)> {
        self.persistent_schemas
            .iter()
            .find_map(|(schema_name, schema)| {
                schema
                    .db_table
                    .as_deref()
                    .filter(|db_table| *db_table == table)
                    .map(|_| (schema_name.clone(), schema.clone()))
            })
    }

    pub fn inject_global(&mut self, name: String, value: Value) {
        self.global_env.set(name.clone(), value.clone());
        for module in self.modules.values_mut() {
            module.env.set(name.clone(), value.clone());
        }
    }

    pub fn set_feature_flags(&mut self, feature_flags: FeatureFlags) {
        self.feature_flags = feature_flags;
    }
}

#[derive(Debug)]
pub struct LoadedProject {
    pub registry: RouteRegistry,
    pub runtime: ProjectRuntime,
    /// Document indexes inferred from the query surface (Spec 067), ensured at serve startup.
    pub doc_index_plan: Vec<crate::doc::index_inference::InferredIndex>,
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

/// Rejects a disallowed schema-reference cycle at project load (Spec 062), restoring
/// the Spec 006 rule on the live `serve`/`test`/`doctor` path. A cycle is disallowed
/// only when it lies entirely within value schemas: the validator lets a reference to a
/// persistent (`db:`) schema pass as a relation (it does not recurse), so any cycle
/// through a persistent schema is broken at that edge and is safe. An all-value cycle is
/// the genuine infinite-embed defect and fails as a config error. Shares the rule with
/// `marreta lint` via `schema_cycle`.
fn detect_schema_cycle(schemas: &HashMap<String, SchemaDefinition>) -> Result<(), MarretaError> {
    let mut refs: HashMap<String, Vec<String>> = HashMap::new();
    let mut persistent: HashSet<String> = HashSet::new();
    for (name, def) in schemas {
        let mut schema_refs = Vec::new();
        for field in &def.fields {
            collect_schema_type_refs(&field.field_type, &mut schema_refs);
        }
        refs.insert(name.clone(), schema_refs);
        if def.db_table.is_some() {
            persistent.insert(name.clone());
        }
    }
    if let Some(cycle) = crate::schema_cycle::find_disallowed_cycle(&refs, &persistent) {
        return Err(MarretaError::CircularSchemaReference {
            cycle: cycle.join(" → "),
        });
    }
    Ok(())
}

/// Collects the schema names a field type references (`Reference`, `list of Reference`).
fn collect_schema_type_refs(field_type: &SchemaType, out: &mut Vec<String>) {
    match field_type {
        SchemaType::Reference(name) => out.push(name.clone()),
        SchemaType::TypedList(inner) => collect_schema_type_refs(inner, out),
        _ => {}
    }
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

/// Loads and bootstraps a multi-file MarretaLang project rooted at `entrypoint`.
pub fn load_project(entrypoint: &Path) -> Result<LoadedProject, MarretaError> {
    load_project_with_feature_flags(entrypoint, FeatureFlags::default())
}

/// Loads and bootstraps a project with the provided feature flag snapshot.
pub fn load_project_with_feature_flags(
    entrypoint: &Path,
    feature_flags: FeatureFlags,
) -> Result<LoadedProject, MarretaError> {
    let root_dir_buf;
    let root_dir = match entrypoint.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => {
            root_dir_buf = Path::new(".").to_path_buf();
            root_dir_buf.as_path()
        }
    };

    let mut module_defs = Vec::new();
    let mut merged_routes: Vec<RouteDefinition> = Vec::new();
    let mut merged_schemas: HashMap<String, SchemaDefinition> = HashMap::new();
    let mut all_project_schemas: HashMap<String, SchemaDefinition> = HashMap::new();
    let mut merged_startup: Vec<Statement> = Vec::new();
    // Every parsed statement, retained only to infer document indexes from the query surface
    // (Spec 067). Inference needs the route/task/consumer bodies, not just the startup block.
    let mut all_statements: Vec<Statement> = Vec::new();
    let mut merged_consumers: Vec<ConsumerDefinition> = Vec::new();
    let mut merged_auth_providers: HashMap<String, AuthProvider> = HashMap::new();
    let mut exported_names: HashMap<String, String> = HashMap::new();

    let mut file_paths = collect_marreta_files(root_dir, entrypoint);
    file_paths.push(entrypoint.to_path_buf());

    for file_path in &file_paths {
        let is_entrypoint = path_eq(file_path, entrypoint);
        let source = read_file(file_path)?;
        let program = parse_source(&source, file_path)?;
        if is_entrypoint {
            validate_entrypoint_metadata(file_path, &program)?;
        }
        all_statements.extend(program.iter().cloned());
        let module_id = module_id(root_dir, file_path);
        let source_tag = file_stem(file_path);
        let module = build_module_definition(
            program,
            &module_id,
            &source_tag,
            is_entrypoint,
            &mut exported_names,
        )?;

        for route in &module.routes {
            check_route_conflict(
                &merged_routes,
                &route.verb,
                &route.path,
                route.line,
                route.column,
            )?;
        }
        merged_routes.extend(module.routes.iter().cloned());
        merged_consumers.extend(module.consumers.iter().cloned());
        for (name, provider) in &module.auth_providers {
            if merged_auth_providers
                .insert(name.clone(), provider.clone())
                .is_some()
            {
                return Err(MarretaError::RuntimeError {
                    message: format!("duplicate auth provider '{}'", name),
                    line: 0,
                    column: 0,
                });
            }
        }

        if is_entrypoint {
            merged_startup.extend(module.startup_stmts.iter().cloned());
            for (name, def) in &module.public_schemas {
                merged_schemas.insert(name.clone(), def.clone());
            }
        } else {
            merged_startup.extend(
                module
                    .startup_stmts
                    .iter()
                    .filter(|stmt| matches!(stmt, Statement::Export(_)))
                    .cloned(),
            );
            for (name, def) in &module.public_schemas {
                merged_schemas.insert(name.clone(), def.clone());
            }
        }

        for (name, def) in &module.public_schemas {
            all_project_schemas.insert(name.clone(), def.clone());
        }
        for (name, def) in &module.private_schemas {
            all_project_schemas.insert(name.clone(), def.clone());
        }

        module_defs.push(module);
    }

    validate_schema_naming(&all_project_schemas)?;
    detect_schema_cycle(&all_project_schemas)?;
    validate_persistent_schema_references(&all_project_schemas)?;
    validate_auth_contract(&merged_routes, &merged_auth_providers)?;
    build_auth_registry(&merged_auth_providers)?;
    let persistent_schemas = collect_persistent_schemas(&all_project_schemas);
    let runtime = build_project_runtime(
        module_defs,
        &merged_schemas,
        &persistent_schemas,
        feature_flags,
    )?;

    let doc_index_plan = crate::doc::index_inference::infer_indexes(&all_statements);

    Ok(LoadedProject {
        registry: RouteRegistry {
            routes: merged_routes,
            schemas: merged_schemas,
            persistent_schemas,
            startup_stmts: merged_startup,
            consumers: merged_consumers,
            auth_providers: merged_auth_providers,
        },
        runtime,
        doc_index_plan,
    })
}

fn validate_entrypoint_metadata(path: &Path, program: &[Statement]) -> Result<(), MarretaError> {
    let mut project_name = None;
    let mut project_version = None;
    let mut requires_marreta = None;
    let mut requires_marreta_non_string = false;

    for stmt in program {
        if let Statement::Assignment { target, value, .. } = stmt {
            match (target.as_str(), value) {
                ("project_name", Expression::StringLiteral(value)) => project_name = Some(value),
                ("project_version", Expression::StringLiteral(value)) => {
                    project_version = Some(value)
                }
                ("requires_marreta", Expression::StringLiteral(value)) => {
                    requires_marreta = Some(value)
                }
                // Present but not a string (e.g. `requires_marreta = 123`): the field is
                // optional, but if declared it must be a string — don't silently skip it.
                ("requires_marreta", _) => requires_marreta_non_string = true,
                _ => {}
            }
        }
    }

    if project_name.is_none() {
        return Err(MarretaError::IoError {
            message: format!(
                "invalid project entrypoint '{}': missing required string assignment 'project_name'",
                path.display()
            ),
        });
    }

    if project_version.is_none() {
        return Err(MarretaError::IoError {
            message: format!(
                "invalid project entrypoint '{}': missing required string assignment 'project_version'",
                path.display()
            ),
        });
    }

    if requires_marreta_non_string {
        return Err(MarretaError::IoError {
            message: format!(
                "invalid project entrypoint '{}': 'requires_marreta' must be a string like \">=0.2.0\"",
                path.display()
            ),
        });
    }

    // Spec 063: optional `requires_marreta = ">=X.Y.Z"` — the minimum runtime the project
    // needs. Absent → no check (backward compatible). Present → must parse and be
    // satisfied by the running runtime, else fail load like any config error.
    if let Some(requirement) = requires_marreta {
        check_runtime_compatibility(requirement)?;
    }

    Ok(())
}

/// Validates `requires_marreta` (Spec 063): the value must be a well-formed
/// `>=MAJOR.MINOR.PATCH`, and the running runtime version must satisfy it.
fn check_runtime_compatibility(requirement: &str) -> Result<(), MarretaError> {
    let Some(minimum) = crate::version::parse_requires_marreta(requirement) else {
        return Err(MarretaError::IoError {
            message: format!(
                "invalid 'requires_marreta' value '{requirement}': expected a minimum like \">=0.2.0\""
            ),
        });
    };
    let actual = crate::version::parse_version(crate::version::MARRETA_VERSION)
        .expect("runtime version is valid semver");
    if actual < minimum {
        return Err(MarretaError::IncompatibleRuntime {
            required: requirement.trim().to_string(),
            actual: crate::version::MARRETA_VERSION.to_string(),
        });
    }
    Ok(())
}

fn build_module_definition(
    program: Vec<Statement>,
    module_id: &str,
    source_tag: &str,
    is_entrypoint: bool,
    exported_names: &mut HashMap<String, String>,
) -> Result<ModuleDefinition, MarretaError> {
    let mut routes = Vec::new();
    let mut consumers = Vec::new();
    let mut auth_providers = HashMap::new();
    let mut startup_stmts = Vec::new();
    let mut public_symbols = Vec::new();
    let mut private_schemas = HashMap::new();
    let mut public_schemas = HashMap::new();
    // Spec 061: exported tasks dedup per file (a file-namespace), not globally, so the
    // same task name in different files is allowed. Schemas/variables keep global dedup.
    let mut local_exported_tasks: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for stmt in program {
        match stmt {
            Statement::Export(ref inner) => {
                if let Some(name) = exported_symbol_name(inner) {
                    if matches!(inner.as_ref(), Statement::TaskDef { .. }) {
                        if !local_exported_tasks.insert(name.clone()) {
                            return Err(MarretaError::RuntimeError {
                                message: format!(
                                    "duplicate exported task '{name}' in '{module_id}.marreta'"
                                ),
                                line: 0,
                                column: 0,
                            });
                        }
                    } else if let Some(prev) = exported_names.get(&name) {
                        return Err(MarretaError::ExportConflict {
                            name,
                            file_a: prev.clone(),
                            file_b: module_id.to_string(),
                        });
                    } else {
                        exported_names.insert(name.clone(), module_id.to_string());
                    }
                    public_symbols.push(name);
                }

                if let Statement::Schema {
                    name,
                    db_table,
                    fields,
                    ..
                } = inner.as_ref()
                {
                    public_schemas.insert(
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
                ref name,
                ref db_table,
                ref fields,
                ..
            } => {
                if is_entrypoint {
                    public_schemas.insert(
                        name.clone(),
                        SchemaDefinition {
                            db_table: db_table.clone(),
                            fields: fields.clone(),
                        },
                    );
                    public_symbols.push(name.clone());
                } else {
                    private_schemas.insert(
                        name.clone(),
                        SchemaDefinition {
                            db_table: db_table.clone(),
                            fields: fields.clone(),
                        },
                    );
                }
                startup_stmts.push(stmt);
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
                    source_file: Some(source_tag.to_string()),
                    module_id: Some(module_id.to_string()),
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
                use crate::route_loader::{ConsumerDefinition, ConsumerKind};
                consumers.push(ConsumerDefinition {
                    kind: ConsumerKind::Queue,
                    target: queue_name,
                    binding,
                    schema,
                    body,
                    line,
                    column,
                    source_file: Some(source_tag.to_string()),
                    module_id: Some(module_id.to_string()),
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
                use crate::route_loader::{ConsumerDefinition, ConsumerKind};
                consumers.push(ConsumerDefinition {
                    kind: ConsumerKind::Topic,
                    target: pattern,
                    binding,
                    schema,
                    body,
                    line,
                    column,
                    source_file: Some(source_tag.to_string()),
                    module_id: Some(module_id.to_string()),
                });
            }
            other => {
                if is_entrypoint && let Some(name) = declared_symbol_name(&other) {
                    public_symbols.push(name);
                }
                startup_stmts.push(other);
            }
        }
    }

    dedupe(&mut public_symbols);

    Ok(ModuleDefinition {
        id: module_id.to_string(),
        is_entrypoint,
        startup_stmts,
        public_symbols,
        routes,
        consumers,
        auth_providers,
        private_schemas,
        public_schemas,
    })
}

fn build_project_runtime(
    module_defs: Vec<ModuleDefinition>,
    merged_public_schemas: &HashMap<String, SchemaDefinition>,
    persistent_schemas: &HashMap<String, SchemaDefinition>,
    feature_flags: FeatureFlags,
) -> Result<ProjectRuntime, MarretaError> {
    let mut global_env = Environment::new();
    let mut modules = HashMap::new();
    // Spec 061 file-namespaces: exported tasks from non-entrypoint files go here,
    // keyed by file stem, instead of into every module's bare env.
    let mut task_namespaces: HashMap<String, HashMap<String, Value>> = HashMap::new();
    let mut namespace_owner: HashMap<String, String> = HashMap::new();

    for module in module_defs {
        let mut interp = Interpreter::from_environment(global_env.clone())
            .with_feature_flags(feature_flags.clone())
            .with_current_module(Some(module.id.clone()));
        interp.execute(&module.startup_stmts)?;
        let module_env = interp.into_environment();

        // Partition exported symbols: a non-entrypoint file's exported tasks become a
        // file-namespace (`stem.task()`); everything else (entrypoint tasks, exported
        // vars) stays global and bare.
        let mut exported_tasks: HashMap<String, Value> = HashMap::new();
        for symbol in &module.public_symbols {
            let Some(value) = module_env.get(symbol) else {
                continue;
            };
            if !module.is_entrypoint && matches!(value, Value::Task { .. }) {
                exported_tasks.insert(symbol.clone(), value);
            } else {
                global_env.update(symbol.clone(), value);
            }
        }
        if !exported_tasks.is_empty() {
            let stem = namespace_from_module_id(&module.id);
            validate_namespace_stem(stem, &module.id)?;
            if let Some(prev) = namespace_owner.get(stem) {
                return Err(MarretaError::RuntimeError {
                    message: format!(
                        "namespace '{stem}' is exported by two files ('{prev}' and '{}') — rename one",
                        module.id
                    ),
                    line: 0,
                    column: 0,
                });
            }
            namespace_owner.insert(stem.to_string(), module.id.clone());
            task_namespaces.insert(stem.to_string(), exported_tasks);
        }

        let mut visible_schemas = merged_public_schemas.clone();
        for (name, schema) in &module.private_schemas {
            visible_schemas.insert(name.clone(), schema.clone());
        }
        for (name, schema) in &module.public_schemas {
            visible_schemas.insert(name.clone(), schema.clone());
        }

        modules.insert(
            module.id.clone(),
            ModuleRuntime {
                id: module.id,
                env: module_env,
                visible_schemas,
            },
        );
    }

    let global_bindings = global_env.all_variables();
    for module in modules.values_mut() {
        for (name, value) in &global_bindings {
            if !module.env.has(name) {
                module.env.set(name.clone(), value.clone());
            }
        }
    }

    Ok(ProjectRuntime {
        global_env,
        modules,
        public_schemas: merged_public_schemas.clone(),
        persistent_schemas: persistent_schemas.clone(),
        feature_flags,
        task_namespaces,
    })
}

/// Whether `stem` is a reserved file-namespace (Spec 061 §4.2/§4.4): the built-in
/// namespaces — derived from the catalog so the set stays in sync as native
/// namespaces are added — plus `app` (the implicitly-global entrypoint, never a
/// namespace). A non-entrypoint file exporting tasks may not take one of these as its
/// stem.
fn is_reserved_namespace(stem: &str) -> bool {
    use crate::tooling::catalog::{CatalogKind, catalog};
    stem == "app"
        || catalog()
            .iter()
            .any(|entry| matches!(entry.kind, CatalogKind::Namespace) && entry.name == stem)
}

/// The file-namespace for a module id is its stem (last path component), e.g.
/// `tasks/billing` -> `billing`.
fn namespace_from_module_id(module_id: &str) -> &str {
    module_id.rsplit('/').next().unwrap_or(module_id)
}

/// Validates a file stem used as a task namespace (Spec 061 §4.2/§4.3/§4.4).
fn validate_namespace_stem(stem: &str, module_id: &str) -> Result<(), MarretaError> {
    if is_reserved_namespace(stem) {
        return Err(MarretaError::RuntimeError {
            message: format!(
                "file '{module_id}.marreta' exports tasks but its name '{stem}' is a reserved namespace; rename the file"
            ),
            line: 0,
            column: 0,
        });
    }
    let is_identifier = {
        let mut chars = stem.chars();
        match chars.next() {
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
            }
            _ => false,
        }
    };
    if !is_identifier {
        let hint: String = stem
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        return Err(MarretaError::RuntimeError {
            message: format!(
                "file '{module_id}.marreta' exports tasks but its name '{stem}' is not a valid namespace identifier; rename it (e.g. '{hint}')"
            ),
            line: 0,
            column: 0,
        });
    }
    Ok(())
}

/// Recursively collects all `.marreta` files under `root_dir`, excluding `entrypoint`.
fn collect_marreta_files(root_dir: &Path, entrypoint: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    collect_recursive(root_dir, entrypoint, &mut result);
    result.sort();
    result
}

fn collect_recursive(dir: &Path, entrypoint: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_recursive(&path, entrypoint, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("marreta")
            && !path_eq(&path, entrypoint)
        {
            out.push(path);
        }
    }
}

fn path_eq(a: &Path, b: &Path) -> bool {
    a.canonicalize().ok() == b.canonicalize().ok()
}

fn read_file(path: &Path) -> Result<String, MarretaError> {
    std::fs::read_to_string(path).map_err(|e| MarretaError::IoError {
        message: format!("cannot read '{}': {}", path.display(), e),
    })
}

fn parse_source(source: &str, _path: &Path) -> Result<Vec<Statement>, MarretaError> {
    let tokens = Lexer::new(source).tokenize()?;
    Parser::new(tokens).parse()
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

fn module_id(root_dir: &Path, path: &Path) -> String {
    path.strip_prefix(root_dir)
        .unwrap_or(path)
        .with_extension("")
        .to_string_lossy()
        .replace('\\', "/")
}

fn declared_symbol_name(stmt: &Statement) -> Option<String> {
    match stmt {
        Statement::TaskDef { name, .. } => Some(name.clone()),
        Statement::Schema { name, .. } => Some(name.clone()),
        Statement::Assignment { target, .. } => Some(target.clone()),
        Statement::ConditionalAssignment { target, .. } => Some(target.clone()),
        _ => None,
    }
}

fn exported_symbol_name(stmt: &Statement) -> Option<String> {
    declared_symbol_name(stmt)
}

fn dedupe(items: &mut Vec<String>) {
    let mut seen = HashMap::new();
    items.retain(|item| seen.insert(item.clone(), ()).is_none());
}

fn check_route_conflict(
    routes: &[RouteDefinition],
    verb: &crate::ast::HttpVerb,
    path: &str,
    line: usize,
    column: usize,
) -> Result<(), MarretaError> {
    let new_pattern = crate::route_loader::path_pattern(path);
    for existing in routes {
        if &existing.verb == verb
            && crate::route_loader::path_pattern(&existing.path) == new_pattern
        {
            return Err(MarretaError::RouteConflict {
                verb: verb.to_string(),
                path_a: existing.path.clone(),
                path_b: path.to_string(),
                line,
                column,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn test_single_file_project() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            r#"
project_name = "test-project"
project_version = "1.0.0"
route GET "/health"
    reply 200, { ok: true }
"#,
        );
        let loaded = load_project(&dir.path().join("app.marreta")).unwrap();
        assert_eq!(loaded.registry.routes.len(), 1);
        assert_eq!(loaded.registry.routes[0].path, "/health");
        assert!(loaded.runtime.modules.contains_key("app"));
    }

    // ---- Spec 063: requires_marreta runtime compatibility ----

    fn entrypoint_with(extra: &str) -> String {
        format!(
            "project_name = \"app\"\nproject_version = \"1.0.0\"\n{extra}route GET \"/health\"\n    reply 200, {{ ok: true }}\n"
        )
    }

    #[test]
    fn test_requires_marreta_satisfied_loads() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            &entrypoint_with("requires_marreta = \">=0.0.1\"\n"),
        );
        assert!(load_project(&dir.path().join("app.marreta")).is_ok());
    }

    #[test]
    fn test_requires_marreta_too_high_fails_with_incompatible_runtime() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            &entrypoint_with("requires_marreta = \">=99.0.0\"\n"),
        );
        let result = load_project(&dir.path().join("app.marreta"));
        assert!(
            matches!(result, Err(MarretaError::IncompatibleRuntime { ref required, .. }) if required == ">=99.0.0"),
            "expected IncompatibleRuntime, got {result:?}"
        );
    }

    #[test]
    fn test_requires_marreta_malformed_fails_at_load() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            &entrypoint_with("requires_marreta = \">=not-a-version\"\n"),
        );
        let result = load_project(&dir.path().join("app.marreta"));
        assert!(
            matches!(result, Err(MarretaError::IoError { ref message }) if message.contains("requires_marreta")),
            "expected a malformed-requires_marreta load error, got {result:?}"
        );
    }

    #[test]
    fn test_requires_marreta_non_string_fails_at_load() {
        // Declared but not a string — must fail, not be silently skipped.
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            &entrypoint_with("requires_marreta = 123\n"),
        );
        let result = load_project(&dir.path().join("app.marreta"));
        assert!(
            matches!(result, Err(MarretaError::IoError { ref message }) if message.contains("requires_marreta") && message.contains("must be a string")),
            "expected a non-string requires_marreta load error, got {result:?}"
        );
    }

    #[test]
    fn test_requires_marreta_absent_loads_as_before() {
        // No requires_marreta — backward compatible, no check.
        let dir = TempDir::new().unwrap();
        write(dir.path(), "app.marreta", &entrypoint_with(""));
        assert!(load_project(&dir.path().join("app.marreta")).is_ok());
    }

    #[test]
    fn test_multifile_routes_merged() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/health"
    reply 200, { ok: true }
"#,
        );
        write(
            dir.path(),
            "routes/products.marreta",
            r#"
route GET "/products"
    reply 200, { items: [] }
"#,
        );
        let loaded = load_project(&dir.path().join("app.marreta")).unwrap();
        assert_eq!(loaded.registry.routes.len(), 2);
        let paths: Vec<&str> = loaded
            .registry
            .routes
            .iter()
            .map(|r| r.path.as_str())
            .collect();
        assert!(paths.contains(&"/health"));
        assert!(paths.contains(&"/products"));
    }

    #[test]
    fn test_export_task_available_in_startup() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/health"
    reply 200, { ok: true }
"#,
        );
        write(
            dir.path(),
            "tasks/pricing.marreta",
            r#"
export task double(x) => x * 2
private_rate = 0.05
"#,
        );
        let loaded = load_project(&dir.path().join("app.marreta")).unwrap();
        let has_export = loaded
            .registry
            .startup_stmts
            .iter()
            .any(|s| matches!(s, Statement::Export(_)));
        assert!(has_export, "exported task must be in startup_stmts");
    }

    #[test]
    fn test_export_schema_in_registry() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/health"
    reply 200, { ok: true }
"#,
        );
        write(
            dir.path(),
            "schemas/payloads.marreta",
            r#"
export schema OrderPayload
    total: float
    paid: boolean
"#,
        );
        let loaded = load_project(&dir.path().join("app.marreta")).unwrap();
        assert!(loaded.registry.schemas.contains_key("OrderPayload"));
        assert!(
            !loaded
                .registry
                .persistent_schemas
                .contains_key("OrderPayload")
        );
    }

    #[test]
    fn test_persistent_schema_extracted_in_project_load() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/health"
    reply 200, { ok: true }
"#,
        );
        write(
            dir.path(),
            "schemas/users.marreta",
            r#"
schema User
    db: users
    id: integer
    name: string
"#,
        );
        let loaded = load_project(&dir.path().join("app.marreta")).unwrap();
        assert!(!loaded.registry.schemas.contains_key("User"));
        assert!(loaded.registry.persistent_schemas.contains_key("User"));
        assert_eq!(
            loaded.registry.persistent_schemas["User"]
                .db_table
                .as_deref(),
            Some("users")
        );
    }

    #[test]
    fn test_file_private_symbol_not_in_public_startup() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/health"
    reply 200, { ok: true }
"#,
        );
        write(
            dir.path(),
            "tasks/pricing.marreta",
            r#"
export task double(x) => x * 2
private_rate = 0.05
"#,
        );
        let loaded = load_project(&dir.path().join("app.marreta")).unwrap();
        let has_private =
            loaded.registry.startup_stmts.iter().any(
                |s| matches!(s, Statement::Assignment { target, .. } if target == "private_rate"),
            );
        assert!(
            !has_private,
            "file-private symbol must not leak into public startup"
        );
        let pricing = loaded.runtime.modules.get("tasks/pricing").unwrap();
        assert_eq!(pricing.env.get("private_rate"), Some(Value::Float(0.05)));
    }

    #[test]
    fn test_private_task_survives_in_same_module_runtime() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            "project_name = \"shop\"\nproject_version = \"1.0.0\"\nroute GET \"/health\"\n    reply 200, { ok: true }\n",
        );
        write(
            dir.path(),
            "routes/math.marreta",
            r#"
task factorial(n) => n
route GET "/factorial/:n"
    reply 200, { ok: true }
"#,
        );
        let loaded = load_project(&dir.path().join("app.marreta")).unwrap();
        let runtime = loaded.runtime.modules.get("routes/math").unwrap();
        assert!(matches!(
            runtime.env.get("factorial"),
            Some(Value::Task { .. })
        ));
    }

    #[test]
    fn test_export_conflict_detected() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/health"
    reply 200, { ok: true }
"#,
        );
        write(dir.path(), "tasks/a.marreta", "export tax_rate = 0.1\n");
        write(dir.path(), "tasks/b.marreta", "export tax_rate = 0.2\n");
        let result = load_project(&dir.path().join("app.marreta"));
        assert!(
            matches!(result, Err(MarretaError::ExportConflict { ref name, .. }) if name == "tax_rate"),
            "expected ExportConflict for tax_rate, got {:?}",
            result
        );
    }

    // ---- Spec 061: file-name namespaces for exported tasks ----

    fn app_only(dir: &Path) {
        write(
            dir,
            "app.marreta",
            "project_name = \"shop\"\nproject_version = \"1.0.0\"\nroute GET \"/health\"\n    reply 200, { ok: true }\n",
        );
    }

    // ---- Spec 062: schema-reference cycle enforcement at load ----

    #[test]
    fn test_value_schema_cycle_fails_at_load() {
        let dir = TempDir::new().unwrap();
        app_only(dir.path());
        write(
            dir.path(),
            "schemas/core.marreta",
            "export schema A\n    b: B\n\nexport schema B\n    a: A\n",
        );
        let result = load_project(&dir.path().join("app.marreta"));
        assert!(
            matches!(result, Err(MarretaError::CircularSchemaReference { ref cycle }) if cycle.contains("A") && cycle.contains("B")),
            "a value-schema cycle must fail at load, got {result:?}"
        );
    }

    #[test]
    fn test_value_schema_referencing_persistent_cycle_loads() {
        // A value schema (Profile) that references into a relational cycle
        // (DbUser <-> DbOrder) is safe: validating Profile lets the DbUser relation pass
        // without entering the cycle (Spec 062, relation-aware validator), so it loads.
        let dir = TempDir::new().unwrap();
        app_only(dir.path());
        write(
            dir.path(),
            "schemas/core.marreta",
            "export schema Profile\n    user: DbUser\n\nexport schema DbUser\n    db: users\n    id: integer\n    orders: list of DbOrder\n\nexport schema DbOrder\n    db: orders\n    id: integer\n    user: DbUser\n",
        );
        let result = load_project(&dir.path().join("app.marreta"));
        assert!(
            result.is_ok(),
            "a value schema referencing a relational cycle is safe and must load, got {result:?}"
        );
    }

    #[test]
    fn test_persistent_bidirectional_relation_loads() {
        // The canonical allowed bidirectional relation (Spec 025): a foreign key
        // (`Order.user: User`) and its inverse collection (`User.orders: list of
        // Order`), both persistent. It forms a reference cycle but is a relational
        // graph, not a value-embed cycle, so it must load.
        let dir = TempDir::new().unwrap();
        app_only(dir.path());
        write(
            dir.path(),
            "schemas/core.marreta",
            "export schema User\n    db: users\n    id: integer\n    orders: list of Order\n\nexport schema Order\n    db: orders\n    id: integer\n    user: User\n",
        );
        let result = load_project(&dir.path().join("app.marreta"));
        assert!(
            result.is_ok(),
            "an all-persistent bidirectional relation must load, got {result:?}"
        );
    }

    #[test]
    fn test_exported_task_registered_under_file_namespace() {
        let dir = TempDir::new().unwrap();
        app_only(dir.path());
        write(
            dir.path(),
            "tasks/billing.marreta",
            "export task charge(x) => x * 2\ntask helper(x) => x\n",
        );
        let loaded = load_project(&dir.path().join("app.marreta")).unwrap();
        // The exported task lives in the namespace registry, not the global env.
        assert!(matches!(
            loaded.runtime.module_task("billing", "charge"),
            Some(Value::Task { .. })
        ));
        assert!(loaded.runtime.is_module_namespace("billing"));
        // It is NOT bare-global (cross-file bare must not resolve).
        assert!(loaded.runtime.global_env.get("charge").is_none());
        // A non-exported task is never in the namespace.
        assert!(loaded.runtime.module_task("billing", "helper").is_none());
    }

    #[test]
    fn test_same_task_name_in_two_namespaces_allowed() {
        let dir = TempDir::new().unwrap();
        app_only(dir.path());
        write(
            dir.path(),
            "tasks/billing.marreta",
            "export task charge(x) => x\n",
        );
        write(
            dir.path(),
            "tasks/payments.marreta",
            "export task charge(x) => x\n",
        );
        let loaded = load_project(&dir.path().join("app.marreta")).unwrap();
        assert!(loaded.runtime.module_task("billing", "charge").is_some());
        assert!(loaded.runtime.module_task("payments", "charge").is_some());
    }

    #[test]
    fn test_duplicate_exported_task_same_file_is_error() {
        let dir = TempDir::new().unwrap();
        app_only(dir.path());
        write(
            dir.path(),
            "tasks/billing.marreta",
            "export task charge(x) => x\nexport task charge(x) => x + 1\n",
        );
        let result = load_project(&dir.path().join("app.marreta"));
        assert!(
            matches!(result, Err(MarretaError::RuntimeError { ref message, .. }) if message.contains("duplicate exported task 'charge'")),
            "expected duplicate-task error, got {result:?}"
        );
    }

    #[test]
    fn test_two_exporting_files_same_stem_is_error() {
        let dir = TempDir::new().unwrap();
        app_only(dir.path());
        write(dir.path(), "a/util.marreta", "export task one(x) => x\n");
        write(dir.path(), "b/util.marreta", "export task two(x) => x\n");
        let result = load_project(&dir.path().join("app.marreta"));
        assert!(
            matches!(result, Err(MarretaError::RuntimeError { ref message, .. }) if message.contains("namespace 'util' is exported by two files")),
            "expected stem-collision error, got {result:?}"
        );
    }

    #[test]
    fn test_stem_matching_builtin_namespace_is_error() {
        let dir = TempDir::new().unwrap();
        app_only(dir.path());
        write(dir.path(), "tasks/db.marreta", "export task find(x) => x\n");
        let result = load_project(&dir.path().join("app.marreta"));
        assert!(
            matches!(result, Err(MarretaError::RuntimeError { ref message, .. }) if message.contains("reserved namespace")),
            "expected reserved-namespace error, got {result:?}"
        );
    }

    #[test]
    fn test_exporting_file_named_app_is_error() {
        let dir = TempDir::new().unwrap();
        app_only(dir.path());
        write(
            dir.path(),
            "routes/app.marreta",
            "export task glue(x) => x\n",
        );
        let result = load_project(&dir.path().join("app.marreta"));
        assert!(
            matches!(result, Err(MarretaError::RuntimeError { ref message, .. }) if message.contains("reserved namespace")),
            "expected reserved 'app' error, got {result:?}"
        );
    }

    #[test]
    fn test_non_identifier_stem_with_exports_is_error() {
        let dir = TempDir::new().unwrap();
        app_only(dir.path());
        write(
            dir.path(),
            "tasks/my-tasks.marreta",
            "export task greet(x) => x\n",
        );
        let result = load_project(&dir.path().join("app.marreta"));
        assert!(
            matches!(result, Err(MarretaError::RuntimeError { ref message, .. }) if message.contains("not a valid namespace identifier") && message.contains("my_tasks")),
            "expected non-identifier-stem error with rename hint, got {result:?}"
        );
    }

    #[test]
    fn test_non_exporting_file_with_reserved_stem_is_allowed() {
        // A file that exports nothing never registers a namespace, so a reserved or
        // non-identifier stem is harmless.
        let dir = TempDir::new().unwrap();
        app_only(dir.path());
        write(dir.path(), "tasks/db.marreta", "task helper(x) => x\n");
        let loaded = load_project(&dir.path().join("app.marreta")).unwrap();
        assert!(!loaded.runtime.is_module_namespace("db"));
    }

    #[test]
    fn test_reserved_namespaces_derive_from_catalog() {
        // Guardrail (review finding): the reserved set must cover every built-in
        // namespace, derived from the catalog so it cannot drift when a new native
        // namespace is added — plus `app`.
        use crate::tooling::catalog::{CatalogKind, catalog};
        for entry in catalog() {
            if matches!(entry.kind, CatalogKind::Namespace) {
                assert!(
                    is_reserved_namespace(entry.name),
                    "built-in namespace '{}' must be reserved for file-namespaces",
                    entry.name
                );
            }
        }
        assert!(is_reserved_namespace("app"));
        assert!(!is_reserved_namespace("billing"));
    }

    #[test]
    fn test_source_file_tag_on_route() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/health"
    reply 200, { ok: true }
"#,
        );
        write(
            dir.path(),
            "routes/orders.marreta",
            r#"
route POST "/orders"
    reply 201, { id: 1 }
"#,
        );
        let loaded = load_project(&dir.path().join("app.marreta")).unwrap();
        let orders_route = loaded
            .registry
            .routes
            .iter()
            .find(|r| r.path == "/orders")
            .unwrap();
        assert_eq!(orders_route.source_file.as_deref(), Some("orders"));
        assert_eq!(orders_route.module_id.as_deref(), Some("routes/orders"));
    }

    #[test]
    fn test_route_conflict_across_files() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/health"
    reply 200, { ok: true }
"#,
        );
        write(
            dir.path(),
            "routes/a.marreta",
            "route GET \"/health\"\n    reply 200, { ok: true }\n",
        );
        let result = load_project(&dir.path().join("app.marreta"));
        assert!(
            matches!(result, Err(MarretaError::RouteConflict { .. })),
            "expected RouteConflict, got {:?}",
            result
        );
    }

    #[test]
    fn test_auth_provider_is_project_wide_across_files() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
auth jwt customer_auth {
    issuer: "https://issuer.example.test"
    audience: "shop-api"
}
"#,
        );
        write(
            dir.path(),
            "routes/orders.marreta",
            r#"
route GET "/orders"
    require auth customer_auth
    allow "admin" in auth.user.roles
    reply 200, { ok: true }
"#,
        );

        let loaded = load_project(&dir.path().join("app.marreta")).unwrap();
        assert!(loaded.registry.auth_providers.contains_key("customer_auth"));
        let orders = loaded
            .registry
            .routes
            .iter()
            .find(|r| r.path == "/orders")
            .unwrap();
        assert_eq!(
            orders.auth.as_ref().map(|auth| auth.provider.as_str()),
            Some("customer_auth")
        );
        assert_eq!(orders.allow.len(), 1);
    }

    #[test]
    fn test_project_load_rejects_unknown_auth_provider() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/orders"
    require auth missing_auth
    reply 200, { ok: true }
"#,
        );

        let result = load_project(&dir.path().join("app.marreta"));
        assert!(
            matches!(result, Err(MarretaError::RuntimeError { ref message, .. }) if message.contains("unknown auth provider")),
            "expected unknown auth provider error, got {:?}",
            result
        );
    }

    #[test]
    fn test_consumer_in_non_entrypoint_file_collected() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/health"
    reply 200, { ok: true }
"#,
        );
        write(
            dir.path(),
            "consumers/orders.marreta",
            r#"
on queue "orders" take msg
    reply 200, { ok: true }
"#,
        );
        let loaded = load_project(&dir.path().join("app.marreta")).unwrap();
        assert_eq!(loaded.registry.consumers.len(), 1);
        assert_eq!(loaded.registry.consumers[0].binding, "msg");
        assert_eq!(
            loaded.registry.consumers[0].source_file.as_deref(),
            Some("orders")
        );
        assert_eq!(
            loaded.registry.consumers[0].module_id.as_deref(),
            Some("consumers/orders")
        );
    }

    #[test]
    fn test_project_load_rejects_persistent_reference_to_contract_schema() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            r#"
project_name = "shop"
project_version = "1.0.0"
route GET "/health"
    reply 200, { ok: true }
"#,
        );
        write(
            dir.path(),
            "schemas/address.marreta",
            r#"
schema AddressPayload
    city: string
"#,
        );
        write(
            dir.path(),
            "schemas/users.marreta",
            r#"
schema User
    db: users
    id: integer
    address: AddressPayload
"#,
        );

        let result = load_project(&dir.path().join("app.marreta"));
        assert!(matches!(
            result,
            Err(MarretaError::InvalidPersistentSchemaReference { .. })
        ));
    }
}
