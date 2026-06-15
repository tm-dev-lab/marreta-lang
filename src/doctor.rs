use std::collections::BTreeSet;
use std::path::Path;

use crate::ast::SchemaType;
use crate::ast::{
    Argument, AuthProvider, AuthProviderConfig, Expression, MapStatement, PipelineStage,
    RescueHandler, ScenarioStep, Statement, TaskBody,
};
use crate::auth::{AuthProviderRuntimeConfig, JwtValidationSource, build_auth_registry};
use crate::config::MarretaConfig;
use crate::coverage;
use crate::feature_flags::env_key_for_feature_name;
use crate::file_loader::LoadedProject;
use crate::migrations::{compare_migration_state, discover_local_migrations};
use crate::route_loader::RouteDefinition;
use crate::route_loader::RouteRegistry;
use crate::route_loader::SchemaDefinition;
use crate::scenario_tests::{
    discover_scenario_files, load_scenario_file, plan_scenario_route_presence,
};
use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorStatus {
    Ok,
    Error,
    Skip,
    /// An untagged informational line (rendered without a status label).
    Plain,
}

impl DoctorStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Error => "ERROR",
            Self::Skip => "SKIP",
            Self::Plain => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorEntry {
    pub status: DoctorStatus,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorSection {
    pub title: String,
    pub entries: Vec<DoctorEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DoctorIntent {
    pub db: bool,
    pub doc: bool,
    pub cache: bool,
    pub queue: bool,
    pub migrations: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    pub sections: Vec<DoctorSection>,
    pub has_errors: bool,
}

pub fn build_doctor_report(
    entrypoint: &Path,
    loaded: &LoadedProject,
    config: &MarretaConfig,
    connect: bool,
) -> DoctorReport {
    let mut sections = Vec::new();
    let mut has_errors = false;

    let mut project_entries = Vec::new();
    project_entries.push(ok("app.marreta found"));
    if let Some(name) = loaded.runtime.global_env.get("project_name") {
        project_entries.push(ok(format!("project_name = {}", display_value(&name))));
    }
    if let Some(version) = loaded.runtime.global_env.get("project_version") {
        project_entries.push(ok(format!("project_version = {}", display_value(&version))));
    }
    // Spec 063: the project loaded, so the runtime already satisfies `requires_marreta`
    // (incompatibility is a hard load error caught before this point). Show the declared
    // minimum, plus the running runtime version it was checked against.
    if let Some(requirement) = loaded.runtime.global_env.get("requires_marreta") {
        project_entries.push(ok(format!(
            "requires_marreta = {} (runtime {})",
            display_value(&requirement),
            crate::version::MARRETA_VERSION
        )));
    }
    project_entries.push(ok("project loads successfully"));
    sections.push(DoctorSection {
        title: "Project".to_string(),
        entries: project_entries,
    });

    let intent = discover_project_intent(&loaded.registry);
    let mut intent_entries = Vec::new();
    if intent.db {
        intent_entries.push(ok("db"));
    }
    if intent.doc {
        intent_entries.push(ok("doc"));
    }
    if intent.cache {
        intent_entries.push(ok("cache"));
    }
    if intent.queue {
        intent_entries.push(ok("queue"));
    }
    if intent.migrations {
        intent_entries.push(ok("migrations"));
    }
    if !intent_entries.is_empty() {
        sections.push(DoctorSection {
            title: "Intent".to_string(),
            entries: intent_entries,
        });
    }

    if !loaded.registry.persistent_schemas.is_empty() {
        sections.push(DoctorSection {
            title: "Persistence (db)".to_string(),
            entries: build_persistence_entries(&loaded.registry.persistent_schemas),
        });
    }

    if !loaded.registry.auth_providers.is_empty() {
        let auth_entries = match validate_auth_providers(&loaded.registry) {
            Ok(entries) => entries,
            Err(entries) => {
                has_errors = true;
                entries
            }
        };
        sections.push(DoctorSection {
            title: "Auth".to_string(),
            entries: auth_entries,
        });
    }

    let mut config_entries = Vec::new();
    let mut db_ready = false;
    let mut doc_ready = false;
    let mut cache_ready = false;
    let mut queue_ready = false;

    if intent.db {
        match validate_db_config(config) {
            Ok(lines) => {
                db_ready = true;
                config_entries.extend(lines);
            }
            Err(lines) => {
                has_errors = true;
                config_entries.extend(lines);
            }
        }
    }
    if intent.doc {
        match validate_doc_config(config) {
            Ok(lines) => {
                doc_ready = true;
                config_entries.extend(lines);
            }
            Err(lines) => {
                has_errors = true;
                config_entries.extend(lines);
            }
        }
    }
    if intent.cache {
        match validate_cache_config(config) {
            Ok(lines) => {
                cache_ready = true;
                config_entries.extend(lines);
            }
            Err(lines) => {
                has_errors = true;
                config_entries.extend(lines);
            }
        }
    }
    if intent.queue {
        match validate_queue_config(config) {
            Ok(lines) => {
                queue_ready = true;
                config_entries.extend(lines);
            }
            Err(lines) => {
                has_errors = true;
                config_entries.extend(lines);
            }
        }
    }
    if !config_entries.is_empty() {
        sections.push(DoctorSection {
            title: "Config".to_string(),
            entries: config_entries,
        });
    }

    let mut feature_flag_entries = Vec::new();
    let feature_flag_errors = config.feature_flag_config_errors();
    if !feature_flag_errors.is_empty() {
        has_errors = true;
        feature_flag_entries.extend(feature_flag_errors.into_iter().map(error));
    }
    for (name, enabled) in config.feature_flags.entries() {
        let state = if enabled { "enabled" } else { "disabled" };
        feature_flag_entries.push(ok(format!(
            "{} = {}",
            env_key_for_feature_name(name),
            state
        )));
    }
    if !feature_flag_entries.is_empty() {
        sections.push(DoctorSection {
            title: "Feature Flags".to_string(),
            entries: feature_flag_entries,
        });
    }

    if connect {
        let mut connectivity_entries = Vec::new();
        if intent.db {
            if db_ready {
                match connectivity_db(config) {
                    Ok(message) => connectivity_entries.push(ok(message)),
                    Err(message) => {
                        has_errors = true;
                        connectivity_entries.push(error(message));
                    }
                }
            } else {
                connectivity_entries
                    .push(skip("db connectivity skipped because config is invalid"));
            }
        }
        if intent.doc {
            if doc_ready {
                match connectivity_doc(config) {
                    Ok(message) => connectivity_entries.push(ok(message)),
                    Err(message) => {
                        has_errors = true;
                        connectivity_entries.push(error(message));
                    }
                }
            } else {
                connectivity_entries
                    .push(skip("doc connectivity skipped because config is invalid"));
            }
        }
        if intent.cache {
            if cache_ready {
                match connectivity_cache(config) {
                    Ok(message) => connectivity_entries.push(ok(message)),
                    Err(message) => {
                        has_errors = true;
                        connectivity_entries.push(error(message));
                    }
                }
            } else {
                connectivity_entries
                    .push(skip("cache connectivity skipped because config is invalid"));
            }
        }
        if intent.queue {
            if queue_ready {
                match connectivity_queue(config) {
                    Ok(message) => connectivity_entries.push(ok(message)),
                    Err(message) => {
                        has_errors = true;
                        connectivity_entries.push(error(message));
                    }
                }
            } else {
                connectivity_entries
                    .push(skip("queue connectivity skipped because config is invalid"));
            }
        }
        if !connectivity_entries.is_empty() {
            sections.push(DoctorSection {
                title: "Connectivity".to_string(),
                entries: connectivity_entries,
            });
        }
    }

    // Spec 067: report the inferred document indexes (present / absent / orphan).
    if let Some(section) = doc_index_section(&loaded.doc_index_plan, config, connect, doc_ready) {
        if section
            .entries
            .iter()
            .any(|e| matches!(e.status, DoctorStatus::Error))
        {
            has_errors = true;
        }
        sections.push(section);
    }

    if intent.migrations {
        let mut migration_entries = Vec::new();
        let migrations_dir = entrypoint
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("migrations");
        match discover_local_migrations(&migrations_dir) {
            Ok(local) => {
                migration_entries.push(ok(format!(
                    "{} local migration{}",
                    local.len(),
                    if local.len() == 1 { "" } else { "s" }
                )));

                if connect && db_ready {
                    let rt = tokio::runtime::Runtime::new();
                    match rt {
                        Ok(rt) => match rt.block_on(async {
                            crate::db::list_applied_migrations_from_config(config).await
                        }) {
                            Ok(applied) => {
                                let report = compare_migration_state(&local, &applied);
                                migration_entries
                                    .push(ok(format!("{} applied", report.applied.len())));
                                if report.pending.is_empty() {
                                    migration_entries.push(ok("no pending migrations"));
                                } else {
                                    migration_entries.push(error(format!(
                                        "{} pending migration{}",
                                        report.pending.len(),
                                        if report.pending.len() == 1 { "" } else { "s" }
                                    )));
                                    has_errors = true;
                                }
                                if !report.changed.is_empty() {
                                    migration_entries.push(error(format!(
                                        "{} changed migration{}",
                                        report.changed.len(),
                                        if report.changed.len() == 1 { "" } else { "s" }
                                    )));
                                    has_errors = true;
                                }
                                if !report.missing_local.is_empty() {
                                    migration_entries.push(error(format!(
                                        "{} missing_local migration{}",
                                        report.missing_local.len(),
                                        if report.missing_local.len() == 1 {
                                            ""
                                        } else {
                                            "s"
                                        }
                                    )));
                                    has_errors = true;
                                }
                            }
                            Err(err) => {
                                has_errors = true;
                                migration_entries
                                    .push(error(format!("migration state check failed: {}", err)));
                            }
                        },
                        Err(err) => {
                            has_errors = true;
                            migration_entries.push(error(format!(
                                "failed to create async runtime for migration checks: {}",
                                err
                            )));
                        }
                    }
                } else if connect && !db_ready {
                    migration_entries.push(skip(
                        "migration state check skipped because db config is invalid",
                    ));
                } else {
                    migration_entries
                        .push(ok("migration state summary requires --connect".to_string()));
                }
            }
            Err(err) => {
                has_errors = true;
                migration_entries.push(error(format!(
                    "could not inspect migrations directory: {}",
                    err
                )));
            }
        }
        sections.push(DoctorSection {
            title: "Migrations".to_string(),
            entries: migration_entries,
        });
    }

    if let Some(modules_section) = build_modules_section(loaded) {
        sections.push(modules_section);
    }

    sections.push(build_tests_section(entrypoint, &loaded.registry.routes));

    DoctorReport {
        sections,
        has_errors,
    }
}

/// Informational `Modules` section (Spec 061): each file-namespace and the exported
/// tasks reachable cross-file as `namespace.task`. Like the Tests coverage summary,
/// it never fails the command; the §4 load rules are already enforced at project load.
fn build_modules_section(loaded: &LoadedProject) -> Option<DoctorSection> {
    let namespaces = &loaded.runtime.task_namespaces;
    if namespaces.is_empty() {
        return None;
    }
    let mut names: Vec<&String> = namespaces.keys().collect();
    names.sort();
    let mut entries = Vec::new();
    for ns in names {
        let mut tasks: Vec<&String> = namespaces[ns].keys().collect();
        tasks.sort();
        let listed = tasks
            .iter()
            .map(|task| format!("{ns}.{task}"))
            .collect::<Vec<_>>()
            .join(", ");
        entries.push(ok(format!("{ns} ({}): {listed}", tasks.len())));
    }
    Some(DoctorSection {
        title: "Modules".to_string(),
        entries,
    })
}

/// Build the consolidated, static `Tests` section: how many declared routes have
/// at least one declared scenario, with no per-route listing. Doctor never runs
/// scenarios; the per-route, pass/fail view lives in `marreta test --coverage`.
///
/// Discovered scenario files are expected to parse, because project load already
/// parsed every visible `.marreta` (a malformed file fails the command at load,
/// like `serve` and `test`). Reading the `tests/` directory itself is *not* gated
/// by project load (`collect_recursive` ignores `read_dir` errors), so a discovery
/// I/O error is surfaced here as a soft `SKIP` note. The section is always
/// informational and never sets `has_errors`.
fn build_tests_section(entrypoint: &Path, routes: &[RouteDefinition]) -> DoctorSection {
    let project_root = entrypoint.parent().unwrap_or_else(|| Path::new("."));
    // A missing tests/ directory is Ok(empty); a real I/O failure reading an
    // existing tests/ is not caught by project load, so surface it as a note
    // rather than silently reporting "no tests".
    let (files, discovery_error) = match discover_scenario_files(project_root) {
        Ok(files) => (files, None),
        Err(_) => (
            Vec::new(),
            Some(
                "could not read the tests/ directory (check it exists and is a readable directory)"
                    .to_string(),
            ),
        ),
    };

    let mut scenarios_total = 0usize;
    let mut files_with_scenarios = 0usize;
    let mut unmatched = 0usize;
    let mut covered: BTreeSet<String> = BTreeSet::new();

    // The doctor report is built from an already-loaded project, and
    // `load_project` parses every `.marreta` file including `tests/`. So if a
    // scenario file were malformed the command would have already failed at load,
    // exactly like `serve` and `test`. Every file discovered here therefore
    // parses; an unexpected error is simply not counted.
    for path in &files {
        let Ok(file) = load_scenario_file(path) else {
            continue;
        };
        if file.scenarios.is_empty() {
            continue;
        }
        files_with_scenarios += 1;
        for scenario in &file.scenarios {
            scenarios_total += 1;
            match plan_scenario_route_presence(routes, scenario) {
                Some(key) => {
                    covered.insert(key);
                }
                None => unmatched += 1,
            }
        }
    }

    let all_routes: BTreeSet<String> = routes
        .iter()
        .map(|route| coverage::route_key(&route.verb, &route.path))
        .collect();
    let summary = coverage::summarize(
        &all_routes,
        &covered,
        scenarios_total,
        files_with_scenarios,
        unmatched,
    );

    let mut entries = Vec::new();
    if summary.scenarios_total == 0 {
        entries.push(ok("scenarios declared: 0"));
    } else {
        entries.push(ok(format!(
            "scenarios declared: {} across {} files",
            summary.scenarios_total, summary.files_total
        )));
    }
    entries.push(ok(format!(
        "routes with a scenario: {} / {} ({:.1}%)",
        summary.routes_with_scenario,
        summary.routes_total,
        summary.routes_with_scenario_pct()
    )));
    entries.push(ok(format!(
        "routes without a scenario: {}",
        summary.routes_without_scenario()
    )));
    if summary.unmatched_scenarios > 0 {
        entries.push(ok(format!(
            "scenarios with no matching route: {}",
            summary.unmatched_scenarios
        )));
    }
    if let Some(message) = discovery_error {
        entries.push(skip(message));
    }
    entries.push(plain(
        "run `marreta test --coverage` for per-route detail and pass/fail coverage",
    ));

    DoctorSection {
        title: "Tests".to_string(),
        entries,
    }
}

pub fn discover_project_intent(registry: &RouteRegistry) -> DoctorIntent {
    let mut intent = DoctorIntent {
        migrations: !registry.persistent_schemas.is_empty(),
        db: !registry.persistent_schemas.is_empty(),
        ..DoctorIntent::default()
    };

    for stmt in &registry.startup_stmts {
        visit_statement(stmt, &mut intent);
    }
    for route in &registry.routes {
        for stmt in &route.body {
            visit_statement(stmt, &mut intent);
        }
    }
    for consumer in &registry.consumers {
        intent.queue = true;
        visit_expression(&consumer.target, &mut intent);
        for stmt in &consumer.body {
            visit_statement(stmt, &mut intent);
        }
    }

    intent
}

fn visit_statement(stmt: &Statement, intent: &mut DoctorIntent) {
    match stmt {
        Statement::Assignment { value, .. } => visit_expression(value, intent),
        Statement::ConditionalAssignment {
            value, condition, ..
        } => {
            visit_expression(value, intent);
            visit_expression(condition, intent);
        }
        Statement::Require { condition, .. } | Statement::Reject { condition, .. } => {
            visit_expression(condition, intent)
        }
        Statement::While {
            condition, body, ..
        } => {
            visit_expression(condition, intent);
            for stmt in body {
                visit_statement(stmt, intent);
            }
        }
        Statement::TaskDef { body, .. } => visit_task_body(body, intent),
        Statement::ExpressionStatement { expression, .. } => visit_expression(expression, intent),
        Statement::Route { body, .. } => {
            for stmt in body {
                visit_statement(stmt, intent);
            }
        }
        Statement::Schema { .. } => {}
        Statement::AuthProvider { provider, .. } => {
            let fields = match provider {
                crate::ast::AuthProvider::Jwt(config)
                | crate::ast::AuthProvider::ApiKey(config) => &config.fields,
            };
            for field in fields {
                visit_expression(&field.value, intent);
            }
        }
        Statement::Reply {
            status_code,
            body,
            extra_headers,
            ..
        } => {
            visit_expression(status_code, intent);
            visit_expression(body, intent);
            if let Some(extra_headers) = extra_headers {
                visit_expression(extra_headers, intent);
            }
        }
        Statement::Fail { message, .. } => visit_expression(message, intent),
        Statement::Raise {
            message, condition, ..
        } => {
            visit_expression(message, intent);
            if let Some(condition) = condition {
                visit_expression(condition, intent);
            }
        }
        Statement::Export(inner) => visit_statement(inner, intent),
        Statement::Transaction { body, .. } => {
            for stmt in body {
                visit_statement(stmt, intent);
            }
        }
        Statement::OnQueue {
            queue_name, body, ..
        } => {
            intent.queue = true;
            visit_expression(queue_name, intent);
            for stmt in body {
                visit_statement(stmt, intent);
            }
        }
        Statement::OnTopic { pattern, body, .. } => {
            intent.queue = true;
            visit_expression(pattern, intent);
            for stmt in body {
                visit_statement(stmt, intent);
            }
        }
        Statement::Nack { condition, .. } => {
            if let Some(condition) = condition {
                visit_expression(condition, intent);
            }
        }
        Statement::Scenario { steps, .. } => {
            for step in steps {
                match step {
                    ScenarioStep::Given {
                        target, returns, ..
                    } => {
                        visit_expression(target, intent);
                        visit_expression(returns, intent);
                    }
                    ScenarioStep::When { body, headers, .. } => {
                        if let Some(body) = body {
                            visit_expression(body, intent);
                        }
                        if let Some(headers) = headers {
                            visit_expression(headers, intent);
                        }
                    }
                    ScenarioStep::ThenStatus { status, .. } => {
                        visit_expression(status, intent);
                    }
                    ScenarioStep::ThenResponse { expected, .. } => {
                        visit_expression(expected, intent);
                    }
                }
            }
        }
    }
}

fn visit_task_body(body: &TaskBody, intent: &mut DoctorIntent) {
    match body {
        TaskBody::Inline(expr) => visit_expression(expr, intent),
        TaskBody::Block(stmts, expr) => {
            for stmt in stmts {
                visit_statement(stmt, intent);
            }
            visit_expression(expr, intent);
        }
    }
}

fn visit_expression(expr: &Expression, intent: &mut DoctorIntent) {
    match expr {
        Expression::Identifier(name) => match name.as_str() {
            "db" => intent.db = true,
            "doc" => intent.doc = true,
            "cache" => intent.cache = true,
            "queue" => intent.queue = true,
            _ => {}
        },
        Expression::BinaryOp { left, right, .. } => {
            visit_expression(left, intent);
            visit_expression(right, intent);
        }
        Expression::UnaryOp { operand, .. } => visit_expression(operand, intent),
        Expression::PropertyAccess { object, .. } => visit_expression(object, intent),
        Expression::MethodCall {
            object, arguments, ..
        } => {
            visit_expression(object, intent);
            for arg in arguments {
                visit_argument(arg, intent);
            }
        }
        Expression::HttpClientResponseSchema { call, .. } => {
            visit_expression(call, intent);
        }
        Expression::FunctionCall { arguments, .. } => {
            for arg in arguments {
                visit_argument(arg, intent);
            }
        }
        Expression::TaskCall { .. } => {}
        Expression::Match { subject, arms } => {
            visit_expression(subject, intent);
            for arm in arms {
                visit_expression(&arm.value, intent);
            }
        }
        Expression::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expression(condition, intent);
            visit_task_body(then_branch, intent);
            if let Some(else_branch) = else_branch {
                visit_task_body(else_branch, intent);
            }
        }
        Expression::Subscript { object, key } => {
            visit_expression(object, intent);
            visit_expression(key, intent);
        }
        Expression::Pipeline { input, stages } => {
            visit_expression(input, intent);
            for stage in stages {
                visit_pipeline_stage(stage, intent);
            }
        }
        Expression::Broadcast { input, targets } => {
            visit_expression(input, intent);
            for target in targets {
                visit_expression(target, intent);
            }
        }
        Expression::Rescue { expr, handler } => {
            visit_expression(expr, intent);
            visit_expression(handler, intent);
        }
        Expression::QueuePush {
            queue_name,
            payload,
            ..
        } => {
            intent.queue = true;
            visit_expression(queue_name, intent);
            if let Some(payload) = payload {
                visit_expression(payload, intent);
            }
        }
        Expression::TopicPublish { topic, payload, .. } => {
            intent.queue = true;
            visit_expression(topic, intent);
            if let Some(payload) = payload {
                visit_expression(payload, intent);
            }
        }
        Expression::Integer(_)
        | Expression::Float(_)
        | Expression::StringLiteral(_)
        | Expression::Boolean(_)
        | Expression::Null => {}
        Expression::List(items) => {
            for item in items {
                visit_expression(item, intent);
            }
        }
        Expression::MapLiteral(items) => {
            for (_, value) in items {
                visit_expression(value, intent);
            }
        }
        Expression::SchemaConstructor { fields, .. } => {
            for (_, value) in fields {
                visit_expression(value, intent);
            }
        }
    }
}

fn visit_pipeline_stage(stage: &PipelineStage, intent: &mut DoctorIntent) {
    match stage {
        PipelineStage::Expression(expr) => visit_expression(expr, intent),
        PipelineStage::Map { body, .. } => {
            for stmt in body {
                visit_map_statement(stmt, intent);
            }
        }
        PipelineStage::Reduce { initial, body, .. } => {
            visit_expression(initial, intent);
            visit_task_body(body, intent);
        }
        PipelineStage::Rescue { handler } => match handler {
            RescueHandler::Inline(expr) => visit_expression(expr, intent),
            RescueHandler::Block(stmts) => {
                for stmt in stmts {
                    visit_statement(stmt, intent);
                }
            }
        },
    }
}

fn visit_map_statement(stmt: &MapStatement, intent: &mut DoctorIntent) {
    match stmt {
        MapStatement::Statement(stmt) => visit_statement(stmt, intent),
        MapStatement::Keep { value, condition } => {
            visit_expression(value, intent);
            if let Some(condition) = condition {
                visit_expression(condition, intent);
            }
        }
        MapStatement::Skip { condition } => visit_expression(condition, intent),
    }
}

fn visit_argument(arg: &Argument, intent: &mut DoctorIntent) {
    match arg {
        Argument::Positional(expr) => visit_expression(expr, intent),
        Argument::Named { value, .. } => visit_expression(value, intent),
    }
}

fn validate_db_config(config: &MarretaConfig) -> Result<Vec<DoctorEntry>, Vec<DoctorEntry>> {
    validate_runtime_config(
        config_errors_for_prefix(config, "MARRETA_DB_"),
        config.db.as_ref(),
        "db",
        "MARRETA_DB_PROVIDER is required for projects that use db.* or db: schemas",
        |db| {
            db.connection_url()?;
            Ok(vec![
                ok(format!("db provider = {}", db.provider_name())),
                ok(format!(
                    "db host = {}",
                    db.host.as_deref().unwrap_or("<missing>")
                )),
            ])
        },
    )
}

fn validate_doc_config(config: &MarretaConfig) -> Result<Vec<DoctorEntry>, Vec<DoctorEntry>> {
    validate_runtime_config(
        config_errors_for_prefix(config, "MARRETA_DOC_"),
        config.doc.as_ref(),
        "doc",
        "MARRETA_DOC_PROVIDER is required for projects that use doc.*",
        |doc| {
            doc.connection_url()?;
            Ok(vec![
                ok(format!("doc provider = {}", doc.provider_name())),
                ok(format!(
                    "doc host = {}",
                    doc.host.as_deref().unwrap_or("<missing>")
                )),
            ])
        },
    )
}

fn validate_cache_config(config: &MarretaConfig) -> Result<Vec<DoctorEntry>, Vec<DoctorEntry>> {
    validate_runtime_config(
        config_errors_for_prefix(config, "MARRETA_CACHE_"),
        config.cache.as_ref(),
        "cache",
        "MARRETA_CACHE_PROVIDER is required for projects that use cache.*",
        |cache| {
            cache.connection_url()?;
            Ok(vec![
                ok(format!("cache provider = {}", cache.provider_name())),
                ok(format!(
                    "cache host = {}",
                    cache.host.as_deref().unwrap_or("<missing>")
                )),
            ])
        },
    )
}

fn validate_queue_config(config: &MarretaConfig) -> Result<Vec<DoctorEntry>, Vec<DoctorEntry>> {
    validate_runtime_config(
        config_errors_for_prefix(config, "MARRETA_QUEUE_"),
        config.queue.as_ref(),
        "queue",
        "MARRETA_QUEUE_PROVIDER is required for projects that use queue.* or queue consumers",
        |queue| {
            queue.connection_url().map_err(|e| e.to_string())?;
            Ok(vec![
                ok(format!("queue provider = {}", queue.provider_name())),
                ok(format!(
                    "queue host = {}",
                    queue.host.as_deref().unwrap_or("<missing>")
                )),
            ])
        },
    )
}

fn validate_runtime_config<T>(
    config_errors: Vec<String>,
    value: Option<&T>,
    _name: &str,
    missing_provider_message: &str,
    validator: impl FnOnce(&T) -> Result<Vec<DoctorEntry>, String>,
) -> Result<Vec<DoctorEntry>, Vec<DoctorEntry>> {
    if !config_errors.is_empty() {
        return Err(config_errors.into_iter().map(error).collect());
    }

    let value = match value {
        Some(value) => value,
        None => return Err(vec![error(missing_provider_message)]),
    };

    validator(value).map_err(|message| vec![error(message)])
}

fn config_errors_for_prefix(config: &MarretaConfig, prefix: &str) -> Vec<String> {
    config
        .config_errors
        .iter()
        .filter(|message| message.contains(prefix))
        .cloned()
        .collect()
}

fn validate_auth_providers(registry: &RouteRegistry) -> Result<Vec<DoctorEntry>, Vec<DoctorEntry>> {
    let auth_registry = match build_auth_registry(&registry.auth_providers) {
        Ok(auth_registry) => auth_registry,
        Err(err) => return Err(vec![error(err.display_message())]),
    };

    let mut entries = Vec::new();
    let mut names = registry.auth_providers.keys().cloned().collect::<Vec<_>>();
    names.sort();

    for name in names {
        let Some(provider) = auth_registry.providers.get(&name) else {
            continue;
        };
        let Some(provider_decl) = registry.auth_providers.get(&name) else {
            continue;
        };

        match (provider, provider_decl) {
            (AuthProviderRuntimeConfig::Jwt(config), AuthProvider::Jwt(decl)) => {
                entries.push(ok(format!("jwt provider = {}", name)));
                entries.push(ok(format!("jwt issuer = {}", config.issuer)));
                entries.push(ok(format!("jwt audience = {}", config.audience)));
                entries.extend(jwt_validation_entries(&name, config, decl));
            }
            (AuthProviderRuntimeConfig::ApiKey(config), AuthProvider::ApiKey(decl)) => {
                entries.push(ok(format!("api_key provider = {}", name)));
                entries.push(ok(format!("api_key header = {}", config.header)));
                entries.extend(api_key_secret_entries(&name, decl));
            }
            _ => {
                entries.push(error(format!(
                    "auth provider '{}' has inconsistent runtime config",
                    name
                )));
            }
        }
    }
    Ok(entries)
}

fn jwt_validation_entries(
    provider_name: &str,
    config: &crate::auth::JwtAuthConfig,
    decl: &AuthProviderConfig,
) -> Vec<DoctorEntry> {
    match &config.validation_source {
        JwtValidationSource::OidcDiscovery => {
            vec![ok("jwt validation source = oidc discovery")]
        }
        JwtValidationSource::JwksUrl(url) => {
            vec![ok(format!("jwt jwks_url = {}", url))]
        }
        JwtValidationSource::PublicKeyPem(_) => {
            if auth_field_expr(decl, "public_key_pem_file").is_some() {
                let path = auth_field_string(provider_name, decl, "public_key_pem_file")
                    .unwrap_or_else(|| "<unresolved>".to_string());
                vec![
                    ok(format!("jwt public_key_pem_file = {}", path)),
                    ok("jwt public key file is readable"),
                    ok("jwt public key file contains a valid PEM public key"),
                ]
            } else {
                vec![ok("jwt public_key_pem = configured")]
            }
        }
        JwtValidationSource::Secret(_) => {
            vec![ok("jwt hmac secret = configured")]
        }
    }
}

fn api_key_secret_entries(provider_name: &str, decl: &AuthProviderConfig) -> Vec<DoctorEntry> {
    if auth_field_expr(decl, "secret_hash").is_some() {
        vec![ok("api_key secret_hash = configured")]
    } else if auth_field_expr(decl, "secret").is_some() {
        vec![ok("api_key secret = configured")]
    } else {
        vec![error(format!(
            "api_key provider '{}' has no configured secret source",
            provider_name
        ))]
    }
}

fn auth_field_expr<'a>(decl: &'a AuthProviderConfig, field: &str) -> Option<&'a Expression> {
    decl.fields
        .iter()
        .find(|candidate| candidate.name == field)
        .map(|field| &field.value)
}

fn auth_field_string(
    provider_name: &str,
    decl: &AuthProviderConfig,
    field: &str,
) -> Option<String> {
    match auth_field_expr(decl, field)? {
        Expression::StringLiteral(value) => Some(value.clone()),
        Expression::PropertyAccess { object, property } if matches!(object.as_ref(), Expression::Identifier(name) if name == "env") => {
            std::env::var(property).ok().or_else(|| {
                Some(format!(
                    "<missing env {} for provider {} field {}>",
                    property, provider_name, field
                ))
            })
        }
        _ => Some("<unsupported expression>".to_string()),
    }
}

fn connectivity_db(config: &MarretaConfig) -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let engine = rt
        .block_on(async { crate::db::DbEngine::from_config(config).await })
        .map_err(|e| e.to_string())?;
    if engine.is_some() {
        Ok("db connection".to_string())
    } else {
        Err("db connectivity skipped because no supported db provider is active".to_string())
    }
}

fn connectivity_doc(config: &MarretaConfig) -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let engine = rt
        .block_on(async { crate::doc::DocEngine::from_config(config).await })
        .map_err(|e| e.to_string())?;
    if engine.is_some() {
        Ok("doc connection".to_string())
    } else {
        Err("doc connectivity skipped because no supported doc provider is active".to_string())
    }
}

fn connectivity_cache(config: &MarretaConfig) -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let engine = rt
        .block_on(async { crate::cache::CacheEngine::from_config(config).await })
        .map_err(|e| e.to_string())?;
    match engine {
        Some(engine) => {
            rt.block_on(async { engine.driver.ping().await })
                .map_err(|e| e.to_string())?;
            Ok("cache connection".to_string())
        }
        None => Err(
            "cache connectivity skipped because no supported cache provider is active".to_string(),
        ),
    }
}

fn connectivity_queue(config: &MarretaConfig) -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let engine = rt
        .block_on(async { crate::queue::QueueEngine::from_config(config).await })
        .map_err(|e| e.to_string())?;
    if engine.is_some() {
        Ok("queue connection".to_string())
    } else {
        Err("queue connectivity skipped because no supported queue provider is active".to_string())
    }
}

/// Spec 067: report the inferred document indexes. Read-only, doctor never creates them (serve
/// does). With a live connection it compares the plan to the indexes present and reports present,
/// absent (not built yet or build failed, indistinguishable from a separate process), and orphan
/// (an owned index no longer inferred). The build lifecycle lives in the serve logs, not here.
fn doc_index_section(
    plan: &[crate::doc::index_inference::InferredIndex],
    config: &MarretaConfig,
    connect: bool,
    doc_ready: bool,
) -> Option<DoctorSection> {
    if plan.is_empty() {
        return None;
    }
    let mut entries = vec![ok(format!(
        "{} document index{} inferred from the query surface",
        plan.len(),
        if plan.len() == 1 { "" } else { "es" }
    ))];
    if connect && doc_ready {
        match doc_index_present_entries(plan, config) {
            Ok(mut e) => entries.append(&mut e),
            Err(msg) => entries.push(error(msg)),
        }
    } else {
        for idx in plan {
            entries.push(skip(format!(
                "{} on {} (not checked, no connection)",
                idx.name, idx.collection
            )));
        }
    }
    Some(DoctorSection {
        title: "Document indexes".to_string(),
        entries,
    })
}

/// Connect to the document provider and classify each inferred index against what is present.
fn doc_index_present_entries(
    plan: &[crate::doc::index_inference::InferredIndex],
    config: &MarretaConfig,
) -> Result<Vec<DoctorEntry>, String> {
    use std::collections::BTreeSet;
    let rt =
        tokio::runtime::Runtime::new().map_err(|e| format!("doc index check skipped: {}", e))?;
    let engine = match rt.block_on(async { crate::doc::DocEngine::from_config(config).await }) {
        Ok(Some(engine)) => engine,
        Ok(None) => return Err("doc index check skipped: no doc provider is active".to_string()),
        Err(e) => return Err(format!("doc index check failed to connect: {}", e)),
    };

    let collections: BTreeSet<&str> = plan.iter().map(|i| i.collection.as_str()).collect();
    let mut entries = Vec::new();
    for collection in collections {
        let present = match rt.block_on(async { engine.driver.list_index_names(collection).await })
        {
            Ok(names) => names,
            Err(e) => {
                entries.push(error(format!(
                    "{}: could not list indexes: {}",
                    collection, e
                )));
                continue;
            }
        };
        entries.append(&mut classify_collection_indexes(collection, plan, &present));
    }
    Ok(entries)
}

/// Pure classification of one collection's inferred indexes against the names actually present:
/// present, absent (not built yet or failed), and orphan (an owned index no longer inferred).
fn classify_collection_indexes(
    collection: &str,
    plan: &[crate::doc::index_inference::InferredIndex],
    present: &[String],
) -> Vec<DoctorEntry> {
    use std::collections::BTreeSet;
    let mut entries = Vec::new();
    for idx in plan.iter().filter(|i| i.collection == collection) {
        if present.iter().any(|n| n == &idx.name) {
            entries.push(ok(format!("{} present on {}", idx.name, collection)));
        } else {
            entries.push(skip(format!(
                "{} absent on {} (building or not yet ensured)",
                idx.name, collection
            )));
        }
    }
    let planned: BTreeSet<&str> = plan
        .iter()
        .filter(|i| i.collection == collection)
        .map(|i| i.name.as_str())
        .collect();
    for name in present {
        if crate::doc::index_inference::is_owned_index_name(name)
            && !planned.contains(name.as_str())
        {
            entries.push(skip(format!(
                "{} on {} is orphaned (owned, no longer inferred); verify nothing else uses it (doc.pipeline aggregations are not analyzed) before dropping",
                name, collection
            )));
        }
    }
    entries
}

fn ok(message: impl Into<String>) -> DoctorEntry {
    DoctorEntry {
        status: DoctorStatus::Ok,
        message: message.into(),
    }
}

fn error(message: impl Into<String>) -> DoctorEntry {
    DoctorEntry {
        status: DoctorStatus::Error,
        message: message.into(),
    }
}

fn skip(message: impl Into<String>) -> DoctorEntry {
    DoctorEntry {
        status: DoctorStatus::Skip,
        message: message.into(),
    }
}

fn plain(message: impl Into<String>) -> DoctorEntry {
    DoctorEntry {
        status: DoctorStatus::Plain,
        message: message.into(),
    }
}

fn display_value(value: &Value) -> String {
    match value {
        Value::String(v) => v.clone(),
        _ => format!("{}", value),
    }
}

fn build_persistence_entries(
    schemas: &std::collections::HashMap<String, SchemaDefinition>,
) -> Vec<DoctorEntry> {
    let mut entries = Vec::new();

    let mut schema_names = schemas.keys().cloned().collect::<Vec<_>>();
    schema_names.sort();

    for schema_name in &schema_names {
        if let Some(schema) = schemas.get(schema_name)
            && let Some(table) = &schema.db_table
        {
            entries.push(ok(format!(
                "schema {schema_name} persists to table {table}"
            )));
        }
    }

    for schema_name in &schema_names {
        let Some(schema) = schemas.get(schema_name) else {
            continue;
        };
        let Some(source_table) = &schema.db_table else {
            continue;
        };

        for field in &schema.fields {
            if let SchemaType::Reference(target_schema_name) = &field.field_type
                && let Some(target_schema) = schemas.get(target_schema_name)
                && let Some(target_table) = &target_schema.db_table
            {
                entries.push(ok(format!(
                    "schema {schema_name}.{} persists as {source_table}.{}_id -> {target_table}.id",
                    field.name, field.name
                )));
            }
        }
    }

    for schema_name in &schema_names {
        let Some(schema) = schemas.get(schema_name) else {
            continue;
        };
        let Some(_source_table) = &schema.db_table else {
            continue;
        };

        for field in &schema.fields {
            if let SchemaType::TypedList(inner) = &field.field_type
                && let SchemaType::Reference(target_schema_name) = inner.as_ref()
                && let Some(target_schema) = schemas.get(target_schema_name)
                && target_schema.db_table.is_some()
            {
                let inverse_fields = target_schema
                    .fields
                    .iter()
                    .filter(|target_field| {
                        matches!(
                            &target_field.field_type,
                            SchemaType::Reference(inverse_target) if inverse_target == schema_name
                        )
                    })
                    .map(|target_field| target_field.name.clone())
                    .collect::<Vec<_>>();

                if let [inverse_field] = inverse_fields.as_slice() {
                    entries.push(ok(format!(
                        "schema {schema_name}.{} is inferred from {target_schema_name}.{inverse_field}",
                        field.name
                    )));
                }
            }
        }
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doc_index_classification_present_absent_orphan() {
        use crate::doc::index_inference::InferredIndex;
        let plan = vec![
            InferredIndex {
                collection: "transactions".into(),
                keys: vec![("account_id".into(), true)],
                name: "idx_transactions_account_id".into(),
            },
            InferredIndex {
                collection: "transactions".into(),
                keys: vec![("ref".into(), true)],
                name: "idx_transactions_ref".into(),
            },
        ];
        let present = vec![
            "_id_".to_string(),                        // default, not owned -> ignored
            "idx_transactions_account_id".to_string(), // inferred and present
            "idx_transactions_old".to_string(),        // owned but not in plan -> orphan
        ];
        let entries = classify_collection_indexes("transactions", &plan, &present);
        assert!(entries.iter().any(|e| e.status == DoctorStatus::Ok
            && e.message.contains("idx_transactions_account_id present")));
        assert!(
            entries.iter().any(|e| e.status == DoctorStatus::Skip
                && e.message.contains("idx_transactions_ref absent"))
        );
        assert!(entries.iter().any(|e| e.status == DoctorStatus::Skip
            && e.message.contains("idx_transactions_old")
            && e.message.contains("orphaned")));
        // The default _id_ index is not owned, so it never shows as an orphan.
        assert!(!entries.iter().any(|e| e.message.contains("_id_")));
    }
    use crate::ast::{
        AuthProvider, AuthProviderConfig, AuthProviderField, Expression, RouteAuth, SchemaField,
        SchemaType, Statement,
    };
    use crate::environment::Environment;
    use crate::file_loader::ProjectRuntime;
    use crate::route_loader::{
        ConsumerDefinition, ConsumerKind, RouteDefinition, SchemaDefinition,
    };
    use std::collections::HashMap;

    fn empty_loaded() -> LoadedProject {
        let mut env = Environment::new();
        env.set("project_name".into(), Value::String("demo".into()));
        env.set("project_version".into(), Value::String("1.0.0".into()));
        LoadedProject {
            registry: RouteRegistry {
                routes: Vec::new(),
                schemas: HashMap::new(),
                persistent_schemas: HashMap::new(),
                startup_stmts: Vec::new(),
                consumers: Vec::new(),
                auth_providers: HashMap::new(),
            },
            runtime: ProjectRuntime::single(env, HashMap::new()),
            doc_index_plan: Vec::new(),
        }
    }

    fn auth_field(name: &str, value: Expression) -> AuthProviderField {
        AuthProviderField {
            name: name.into(),
            value,
            line: 1,
            column: 1,
        }
    }

    #[test]
    fn test_discover_intent_from_db_route_body() {
        let mut loaded = empty_loaded();
        loaded.registry.routes.push(RouteDefinition {
            verb: crate::ast::HttpVerb::Get,
            path: "/items".into(),
            auth: None,
            allow: vec![],
            take: Vec::new(),
            body: vec![Statement::ExpressionStatement {
                expression: Expression::MethodCall {
                    object: Box::new(Expression::PropertyAccess {
                        object: Box::new(Expression::Identifier("db".into())),
                        property: "items".into(),
                    }),
                    method: "find_all".into(),
                    arguments: Vec::new(),
                },
                line: 1,
                column: 1,
            }],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });

        let intent = discover_project_intent(&loaded.registry);
        assert!(intent.db);
        assert!(!intent.doc);
        assert!(!intent.cache);
        assert!(!intent.queue);
    }

    #[test]
    fn test_discover_intent_from_queue_consumer() {
        let mut loaded = empty_loaded();
        loaded.registry.consumers.push(ConsumerDefinition {
            kind: ConsumerKind::Queue,
            target: Expression::StringLiteral("orders".into()),
            binding: "msg".into(),
            schema: None,
            body: Vec::new(),
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });

        let intent = discover_project_intent(&loaded.registry);
        assert!(intent.queue);
    }

    #[test]
    fn test_discover_intent_marks_migrations_and_db_for_persistent_schemas() {
        let mut loaded = empty_loaded();
        loaded.registry.persistent_schemas.insert(
            "User".into(),
            SchemaDefinition {
                db_table: Some("users".into()),
                fields: Vec::new(),
            },
        );

        let intent = discover_project_intent(&loaded.registry);
        assert!(intent.migrations);
        assert!(intent.db);
    }

    #[test]
    fn test_doctor_reports_auth_provider_config() {
        let mut loaded = empty_loaded();
        loaded.registry.auth_providers.insert(
            "customer_auth".into(),
            AuthProvider::Jwt(AuthProviderConfig {
                name: "customer_auth".into(),
                fields: vec![
                    auth_field(
                        "issuer",
                        Expression::StringLiteral("https://issuer.example.test".into()),
                    ),
                    auth_field("audience", Expression::StringLiteral("shop-api".into())),
                ],
            }),
        );
        loaded.registry.routes.push(RouteDefinition {
            verb: crate::ast::HttpVerb::Get,
            path: "/orders".into(),
            auth: Some(RouteAuth {
                provider: "customer_auth".into(),
                line: 1,
                column: 5,
            }),
            allow: vec![],
            take: Vec::new(),
            body: Vec::new(),
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });

        let report = build_doctor_report(
            Path::new("app.marreta"),
            &loaded,
            &MarretaConfig::load(),
            false,
        );
        let auth_section = report
            .sections
            .iter()
            .find(|section| section.title == "Auth")
            .unwrap();
        assert!(
            auth_section
                .entries
                .iter()
                .any(|entry| entry.message == "jwt provider = customer_auth")
        );
        assert!(!report.has_errors);
    }

    #[test]
    fn test_doctor_reports_invalid_auth_provider_config() {
        let mut loaded = empty_loaded();
        loaded.registry.auth_providers.insert(
            "customer_auth".into(),
            AuthProvider::Jwt(AuthProviderConfig {
                name: "customer_auth".into(),
                fields: vec![auth_field(
                    "issuer",
                    Expression::StringLiteral("https://issuer.example.test".into()),
                )],
            }),
        );

        let report = build_doctor_report(
            Path::new("app.marreta"),
            &loaded,
            &MarretaConfig::load(),
            false,
        );
        let auth_section = report
            .sections
            .iter()
            .find(|section| section.title == "Auth")
            .unwrap();
        assert_eq!(auth_section.entries[0].status, DoctorStatus::Error);
        assert!(
            auth_section.entries[0]
                .message
                .contains("missing required auth field 'audience'")
        );
        assert!(report.has_errors);
    }

    #[test]
    fn test_doctor_reports_persistence_by_convention() {
        let mut loaded = empty_loaded();
        loaded.registry.persistent_schemas.insert(
            "User".into(),
            SchemaDefinition {
                db_table: Some("users".into()),
                fields: vec![
                    SchemaField {
                        name: "id".into(),
                        field_type: SchemaType::IntegerType,
                        optional: false,
                    },
                    SchemaField {
                        name: "orders".into(),
                        field_type: SchemaType::TypedList(Box::new(SchemaType::Reference(
                            "Order".into(),
                        ))),
                        optional: false,
                    },
                ],
            },
        );
        loaded.registry.persistent_schemas.insert(
            "Order".into(),
            SchemaDefinition {
                db_table: Some("orders".into()),
                fields: vec![
                    SchemaField {
                        name: "id".into(),
                        field_type: SchemaType::IntegerType,
                        optional: false,
                    },
                    SchemaField {
                        name: "customer".into(),
                        field_type: SchemaType::Reference("User".into()),
                        optional: false,
                    },
                ],
            },
        );

        let report = build_doctor_report(
            Path::new("app.marreta"),
            &loaded,
            &MarretaConfig::load(),
            false,
        );

        let section = report
            .sections
            .iter()
            .find(|section| section.title == "Persistence (db)")
            .unwrap();

        assert!(
            section
                .entries
                .iter()
                .any(|entry| entry.message == "schema User persists to table users")
        );
        assert!(
            section
                .entries
                .iter()
                .any(|entry| entry.message == "schema Order persists to table orders")
        );
        assert!(section.entries.iter().any(|entry| entry.message
            == "schema Order.customer persists as orders.customer_id -> users.id"));
        assert!(
            section
                .entries
                .iter()
                .any(|entry| entry.message == "schema User.orders is inferred from Order.customer")
        );
    }

    #[test]
    fn tests_section_reports_consolidated_presence() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app.marreta");
        std::fs::write(
            &app,
            r#"project_name = "doctor-tests"
project_version = "1.0.0"

route GET "/orders/:id"
    reply 200, { id: id }

route POST "/orders"
    reply 201, { ok: true }
"#,
        )
        .unwrap();
        let tests_dir = dir.path().join("tests");
        std::fs::create_dir(&tests_dir).unwrap();
        std::fs::write(
            tests_dir.join("orders_test.marreta"),
            "scenario \"get one\"\n    when GET \"/orders/7\"\n    then status 200\n",
        )
        .unwrap();

        let loaded = crate::file_loader::load_project(&app).unwrap();
        let section = build_tests_section(&app, &loaded.registry.routes);

        assert_eq!(section.title, "Tests");
        let messages: Vec<&str> = section
            .entries
            .iter()
            .map(|entry| entry.message.as_str())
            .collect();
        assert!(messages.contains(&"scenarios declared: 1 across 1 files"));
        assert!(messages.contains(&"routes with a scenario: 1 / 2 (50.0%)"));
        assert!(messages.contains(&"routes without a scenario: 1"));
        assert!(
            messages
                .iter()
                .any(|message| message.contains("marreta test --coverage"))
        );
        // Consolidated only: no route paths are listed.
        assert!(!messages.iter().any(|message| message.contains("/orders")));
    }

    #[test]
    fn tests_section_notes_unreadable_tests_directory() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app.marreta");
        std::fs::write(
            &app,
            r#"project_name = "doctor-io"
project_version = "1.0.0"

route GET "/a"
    reply 200, { ok: true }
"#,
        )
        .unwrap();
        // `tests` is a regular file, not a directory, so scenario discovery hits a
        // read_dir error. Project load ignores it (it is not a .marreta file), so
        // the doctor must surface it rather than hide it as "no tests".
        std::fs::write(dir.path().join("tests"), "not a directory").unwrap();

        let loaded = crate::file_loader::load_project(&app).unwrap();
        let section = build_tests_section(&app, &loaded.registry.routes);

        let note = section
            .entries
            .iter()
            .find(|entry| entry.status == DoctorStatus::Skip)
            .expect("expected a SKIP entry for the unreadable tests directory");
        assert!(
            note.message.contains("could not read the tests/ directory"),
            "unexpected skip message: {}",
            note.message
        );
        // The raw OS error must not leak into the user-facing message.
        assert!(
            !note.message.contains("os error"),
            "skip message leaks a raw OS error: {}",
            note.message
        );
    }
}
