use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::ast::{
    Argument, Expression, MapStatement, PipelineStage, RescueHandler, ScenarioStep, SchemaType,
    Statement, TaskBody,
};
use crate::error::MarretaError;
use crate::feature_flags::is_valid_feature_name;
use crate::lexer::Lexer;
use crate::parser::Parser;

#[derive(Debug)]
pub enum LintError {
    Io { path: PathBuf, source: io::Error },
    MissingProjectRoot(PathBuf),
    MissingFileArgument,
    InvalidPath(PathBuf),
}

impl fmt::Display for LintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LintError::Io { path, source } => write!(f, "{}: {}", path.display(), source),
            LintError::MissingProjectRoot(path) => write!(
                f,
                "no app.marreta found in {}; run marreta lint from a project root or pass explicit paths",
                path.display()
            ),
            LintError::MissingFileArgument => write!(f, "--stdin requires --file <path>"),
            LintError::InvalidPath(path) => write!(f, "invalid lint path: {}", path.display()),
        }
    }
}

impl std::error::Error for LintError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LintSeverity {
    Error,
    Warning,
    Info,
}

impl LintSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            LintSeverity::Error => "error",
            LintSeverity::Warning => "warning",
            LintSeverity::Info => "info",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintFormat {
    Human,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintDiagnostic {
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
    /// End of the diagnostic span. Defaults to the start (zero width); set via
    /// `spanning` when the offending token length is known, so editors can
    /// underline the whole token instead of a single character.
    pub end_line: usize,
    pub end_column: usize,
    pub severity: LintSeverity,
    pub code: &'static str,
    pub message: String,
    pub help: Option<String>,
}

impl LintDiagnostic {
    fn new(
        file: PathBuf,
        line: usize,
        column: usize,
        severity: LintSeverity,
        code: &'static str,
        message: impl Into<String>,
        help: Option<String>,
    ) -> Self {
        Self {
            file,
            line,
            column,
            end_line: line,
            end_column: column,
            severity,
            code,
            message: message.into(),
            help,
        }
    }

    /// Sets the end of the diagnostic span (same line, past the offending token).
    fn spanning(mut self, end_line: usize, end_column: usize) -> Self {
        self.end_line = end_line;
        self.end_column = end_column;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintReport {
    pub diagnostics: Vec<LintDiagnostic>,
}

impl LintReport {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diag| diag.severity == LintSeverity::Error)
    }

    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diag| diag.severity == LintSeverity::Warning)
    }

    pub fn should_fail(&self, strict: bool) -> bool {
        self.has_errors() || (strict && self.has_warnings())
    }

    pub fn render(&self, format: LintFormat) -> String {
        match format {
            LintFormat::Human => render_human(&self.diagnostics),
            LintFormat::Json => render_json(&self.diagnostics),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LintInput {
    pub path: PathBuf,
    pub source: String,
}

pub fn lint_project(root: &Path) -> Result<LintReport, LintError> {
    let app = root.join("app.marreta");
    if !app.is_file() {
        return Err(LintError::MissingProjectRoot(root.to_path_buf()));
    }

    lint_files(collect_project_files(root)?)
}

pub fn lint_paths(paths: &[PathBuf]) -> Result<LintReport, LintError> {
    lint_files(collect_explicit_files(paths)?)
}

pub fn lint_stdin(file: PathBuf, source: String) -> LintReport {
    let input = LintInput { path: file, source };
    lint_inputs(vec![input])
}

pub fn lint_project_stdin(
    root: &Path,
    file: PathBuf,
    source: String,
) -> Result<LintReport, LintError> {
    let app = root.join("app.marreta");
    if !app.is_file() {
        return Err(LintError::MissingProjectRoot(root.to_path_buf()));
    }

    let overlay_path = absolute_overlay_path(root, &file);
    let mut replaced = false;
    let mut inputs = Vec::new();
    for path in collect_project_files(root)? {
        if equivalent_path(&path, &overlay_path) {
            inputs.push(LintInput {
                path: file.clone(),
                source: source.clone(),
            });
            replaced = true;
        } else {
            let source = fs::read_to_string(&path).map_err(|source| LintError::Io {
                path: path.clone(),
                source,
            })?;
            inputs.push(LintInput { path, source });
        }
    }

    if !replaced {
        inputs.push(LintInput { path: file, source });
    }

    Ok(lint_inputs(inputs))
}

fn lint_files(paths: Vec<PathBuf>) -> Result<LintReport, LintError> {
    let mut inputs = Vec::new();
    for path in paths {
        let source = fs::read_to_string(&path).map_err(|source| LintError::Io {
            path: path.clone(),
            source,
        })?;
        inputs.push(LintInput { path, source });
    }
    Ok(lint_inputs(inputs))
}

fn lint_inputs(inputs: Vec<LintInput>) -> LintReport {
    let mut diagnostics = Vec::new();
    let mut parsed = Vec::new();

    for input in inputs {
        match parse_source(&input.source) {
            Ok(program) => parsed.push(ParsedFile {
                path: input.path,
                program,
            }),
            Err(err) => diagnostics.push(project_load_diagnostic(input.path, err)),
        }
    }

    if diagnostics
        .iter()
        .any(|d| d.severity == LintSeverity::Error)
    {
        return LintReport { diagnostics };
    }

    let schema_names = collect_schema_names(&parsed);
    let task_defs = collect_private_task_defs(&parsed);
    let task_calls = collect_task_calls(&parsed);

    lint_duplicate_routes(&parsed, &mut diagnostics);
    lint_schema_cycles(&parsed, &mut diagnostics);

    for file in &parsed {
        lint_statements(&file.path, &file.program, &schema_names, &mut diagnostics);
    }

    for (name, path, line, column) in task_defs {
        if !task_calls.bare_any.contains(&name) {
            diagnostics.push(LintDiagnostic::new(
                path,
                line,
                column,
                LintSeverity::Warning,
                "unused_private_task",
                format!("private task '{}' is never called", name),
                Some("Remove the task, export it, or call it from project code.".to_string()),
            ));
        }
    }

    // Spec 061: an exported task is used if referenced by its file-namespace anywhere
    // (`namespace.task`) or called bare within its own file. A bare call in another file
    // no longer resolves at runtime, so it must not count — hence the same-file check.
    let exported_defs = collect_exported_task_defs(&parsed);
    for (namespace, name, path, line, column) in exported_defs {
        let qualified = format!("{namespace}.{name}");
        if !task_calls.qualified.contains(&qualified)
            && !task_calls.bare_in_namespace(&namespace, &name)
        {
            diagnostics.push(LintDiagnostic::new(
                path,
                line,
                column,
                LintSeverity::Warning,
                "unused_exported_task",
                format!("exported task '{qualified}' is never referenced"),
                Some(format!(
                    "Call it as '{qualified}' from another file, use it in its own file, or remove the export."
                )),
            ));
        }
    }

    LintReport { diagnostics }
}

#[derive(Debug)]
struct ParsedFile {
    path: PathBuf,
    program: Vec<Statement>,
}

fn parse_source(source: &str) -> Result<Vec<Statement>, MarretaError> {
    let tokens = Lexer::new(source).tokenize()?;
    Parser::new(tokens).parse()
}

fn project_load_diagnostic(path: PathBuf, err: MarretaError) -> LintDiagnostic {
    LintDiagnostic::new(
        path,
        err.line().unwrap_or(1),
        err.column().unwrap_or(1),
        LintSeverity::Error,
        "source_load_error",
        err.to_string(),
        Some("Fix this source error before running the project.".to_string()),
    )
}

fn collect_project_files(root: &Path) -> Result<Vec<PathBuf>, LintError> {
    let mut files = Vec::new();
    collect_recursive(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_explicit_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, LintError> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_file() {
            if is_marreta_file(path) {
                files.push(path.clone());
            }
        } else if path.is_dir() {
            collect_recursive(path, &mut files)?;
        } else {
            return Err(LintError::InvalidPath(path.clone()));
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn collect_recursive(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), LintError> {
    let entries = fs::read_dir(path).map_err(|source| LintError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| LintError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let child = entry.path();
        if child.is_dir() {
            collect_recursive(&child, out)?;
        } else if is_marreta_file(&child) {
            out.push(child);
        }
    }
    Ok(())
}

fn is_marreta_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("marreta")
}

fn absolute_overlay_path(root: &Path, file: &Path) -> PathBuf {
    if file.is_absolute() {
        file.to_path_buf()
    } else {
        root.join(file)
    }
}

fn equivalent_path(left: &Path, right: &Path) -> bool {
    left == right || left.canonicalize().ok().as_deref() == right.canonicalize().ok().as_deref()
}

fn collect_schema_names(files: &[ParsedFile]) -> HashSet<String> {
    let mut names = HashSet::new();
    for file in files {
        collect_schema_names_from_statements(&file.program, &mut names);
    }
    names
}

fn collect_schema_names_from_statements(statements: &[Statement], names: &mut HashSet<String>) {
    for stmt in statements {
        match stmt {
            Statement::Schema { name, .. } => {
                names.insert(name.clone());
            }
            Statement::Export(inner) => {
                collect_schema_names_from_statements(std::slice::from_ref(inner.as_ref()), names)
            }
            _ => {}
        }
    }
}

fn collect_private_task_defs(files: &[ParsedFile]) -> Vec<(String, PathBuf, usize, usize)> {
    let mut defs = Vec::new();
    for file in files {
        for stmt in &file.program {
            if let Statement::TaskDef {
                name, line, column, ..
            } = stmt
            {
                defs.push((name.clone(), file.path.clone(), *line, *column));
            }
        }
    }
    defs
}

/// Top-level `export task` definitions, with the file-namespace they publish under
/// (the file stem). Spec 061: these are the cross-file surface (`namespace.task`).
fn collect_exported_task_defs(
    files: &[ParsedFile],
) -> Vec<(String, String, PathBuf, usize, usize)> {
    let mut defs = Vec::new();
    for file in files {
        let Some(namespace) = file_namespace(&file.path) else {
            continue;
        };
        for stmt in &file.program {
            if let Statement::Export(inner) = stmt
                && let Statement::TaskDef {
                    name, line, column, ..
                } = inner.as_ref()
            {
                defs.push((
                    namespace.clone(),
                    name.clone(),
                    file.path.clone(),
                    *line,
                    *column,
                ));
            }
        }
    }
    defs
}

/// The file-namespace (stem) for a `.marreta` path, e.g. `tasks/billing.marreta`
/// -> `billing`. The entrypoint `app.marreta` is the global scope, not a namespace.
fn file_namespace(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    if stem == "app" {
        return None;
    }
    Some(stem.to_string())
}

/// Index of task references across the project, partitioned so each lint can ask the
/// right question (Spec 061):
/// - `bare_any` — every bare call name, project-wide (a private task is used if called
///   bare anywhere; unchanged from before namespaces).
/// - `bare_by_namespace` — bare call names grouped by the file-namespace they occur in.
///   An exported `ns.task` counts a bare call as a use only when that bare call is in
///   `ns` itself (its own file): a bare cross-file call no longer resolves at runtime,
///   so it must not suppress the warning.
/// - `qualified` — `namespace.task` references, project-wide (a `file.task` call from
///   anywhere is a use).
#[derive(Default)]
struct TaskCallIndex {
    bare_any: BTreeSet<String>,
    bare_by_namespace: HashMap<String, BTreeSet<String>>,
    qualified: BTreeSet<String>,
}

impl TaskCallIndex {
    fn record_bare(&mut self, namespace: Option<&str>, name: &str) {
        self.bare_any.insert(name.to_string());
        if let Some(ns) = namespace {
            self.bare_by_namespace
                .entry(ns.to_string())
                .or_default()
                .insert(name.to_string());
        }
    }

    fn bare_in_namespace(&self, namespace: &str, name: &str) -> bool {
        self.bare_by_namespace
            .get(namespace)
            .is_some_and(|names| names.contains(name))
    }
}

fn collect_task_calls(files: &[ParsedFile]) -> TaskCallIndex {
    let mut index = TaskCallIndex::default();
    for file in files {
        let namespace = file_namespace(&file.path);
        collect_task_calls_from_statements(&file.program, namespace.as_deref(), &mut index);
    }
    index
}

fn collect_task_calls_from_statements(
    statements: &[Statement],
    namespace: Option<&str>,
    index: &mut TaskCallIndex,
) {
    for stmt in statements {
        walk_statement_expressions(stmt, &mut |expr, _, _| {
            record_task_call(expr, namespace, index);
        });
        match stmt {
            Statement::TaskDef { body, .. } => {
                collect_task_calls_from_task_body(body, namespace, index)
            }
            Statement::Route { body, .. }
            | Statement::While { body, .. }
            | Statement::Transaction { body, .. }
            | Statement::OnQueue { body, .. }
            | Statement::OnTopic { body, .. } => {
                collect_task_calls_from_statements(body, namespace, index)
            }
            Statement::Export(inner) => collect_task_calls_from_statements(
                std::slice::from_ref(inner.as_ref()),
                namespace,
                index,
            ),
            Statement::Scenario { steps, .. } => {
                for step in steps {
                    walk_scenario_step_expressions(step, &mut |expr| {
                        collect_task_calls_expr(expr, namespace, index)
                    });
                }
            }
            _ => {}
        }
    }
}

fn collect_task_calls_from_task_body(
    body: &TaskBody,
    namespace: Option<&str>,
    index: &mut TaskCallIndex,
) {
    match body {
        TaskBody::Inline(expr) => collect_task_calls_expr(expr, namespace, index),
        TaskBody::Block(statements, expr) => {
            collect_task_calls_from_statements(statements, namespace, index);
            collect_task_calls_expr(expr, namespace, index);
        }
    }
}

fn collect_task_calls_expr(expr: &Expression, namespace: Option<&str>, index: &mut TaskCallIndex) {
    walk_expression(expr, &mut |expr| {
        record_task_call(expr, namespace, index);
    });
}

/// Records a task reference into the index. Bare calls (`foo(..)`, `>> foo`, `-> foo`)
/// are recorded by name and tagged with the calling file's `namespace`; a file-namespace
/// call (`ns.foo` as a method call or a pipeline/broadcast `PropertyAccess` stage) is
/// recorded qualified as `"ns.foo"`. Only the qualified form is recorded for `ns.foo` so
/// a built-in or value method (`cache.get`, `list.length`) never masks a same-named task.
fn record_task_call(expr: &Expression, namespace: Option<&str>, index: &mut TaskCallIndex) {
    match expr {
        Expression::FunctionCall { name, .. } | Expression::TaskCall { name } => {
            index.record_bare(namespace, name);
        }
        Expression::MethodCall { object, method, .. } => {
            if let Expression::Identifier(ns) = object.as_ref() {
                index.qualified.insert(format!("{ns}.{method}"));
            }
        }
        Expression::PropertyAccess { object, property } => {
            if let Expression::Identifier(ns) = object.as_ref() {
                index.qualified.insert(format!("{ns}.{property}"));
            }
        }
        // A bare identifier in a pipeline stage (`>> task`) or broadcast target
        // (`-> task`) is a task reference; record it (qualified `ns.task` stages and
        // call-shaped stages are recorded when the walk descends into them).
        Expression::Pipeline { stages, .. } => {
            for stage in stages {
                if let PipelineStage::Expression(Expression::Identifier(name)) = stage {
                    index.record_bare(namespace, name);
                }
            }
        }
        Expression::Broadcast { targets, .. } => {
            for target in targets {
                if let Expression::Identifier(name) = target {
                    index.record_bare(namespace, name);
                }
            }
        }
        _ => {}
    }
}

fn lint_duplicate_routes(files: &[ParsedFile], diagnostics: &mut Vec<LintDiagnostic>) {
    let mut seen = HashSet::new();
    for file in files {
        for stmt in &file.program {
            lint_duplicate_routes_in_statement(&file.path, stmt, &mut seen, diagnostics);
        }
    }
}

fn lint_duplicate_routes_in_statement(
    path: &Path,
    stmt: &Statement,
    seen: &mut HashSet<(String, String)>,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    match stmt {
        Statement::Route {
            verb,
            path: route_path,
            line,
            column,
            ..
        } => {
            let key = (verb.to_string(), route_path.clone());
            if !seen.insert(key) {
                diagnostics.push(LintDiagnostic::new(
                    path.to_path_buf(),
                    *line,
                    *column,
                    LintSeverity::Error,
                    "duplicate_route",
                    format!("duplicate route {} {}", verb, route_path),
                    Some("Remove one route or change its method/path.".to_string()),
                ));
            }
        }
        Statement::Export(inner) => {
            lint_duplicate_routes_in_statement(path, inner, seen, diagnostics);
        }
        _ => {}
    }
}

fn lint_schema_cycles(files: &[ParsedFile], diagnostics: &mut Vec<LintDiagnostic>) {
    let mut refs: HashMap<String, Vec<String>> = HashMap::new();
    let mut locations: HashMap<String, (PathBuf, usize, usize)> = HashMap::new();
    let mut persistent: HashSet<String> = HashSet::new();

    for file in files {
        collect_schema_refs_from_statements(
            &file.path,
            &file.program,
            &mut refs,
            &mut locations,
            &mut persistent,
        );
    }

    // The relation-aware cycle rule is shared with the loader (Spec 062): only a cycle
    // lying entirely within value schemas is flagged. A reference to a persistent (`db:`)
    // schema is a relation the validator lets pass, so any cycle through a persistent
    // schema is broken at that edge and is allowed. The helper returns the first such
    // (all-value) cycle.
    if let Some(cycle) = crate::schema_cycle::find_disallowed_cycle(&refs, &persistent) {
        let anchor = cycle.first().expect("a cycle has at least one node");
        if let Some((path, line, column)) = locations.get(anchor) {
            diagnostics.push(LintDiagnostic::new(
                path.clone(),
                *line,
                *column,
                LintSeverity::Error,
                "source_load_error",
                format!("circular schema reference: {}", cycle.join(" -> ")),
                Some("Break the schema reference cycle.".to_string()),
            ));
        }
    }
}

fn collect_schema_refs_from_statements(
    path: &Path,
    statements: &[Statement],
    refs: &mut HashMap<String, Vec<String>>,
    locations: &mut HashMap<String, (PathBuf, usize, usize)>,
    persistent: &mut HashSet<String>,
) {
    for stmt in statements {
        match stmt {
            Statement::Schema {
                name,
                db_table,
                fields,
                line,
                column,
            } => {
                let mut schema_refs = Vec::new();
                for field in fields {
                    collect_schema_type_refs(&field.field_type, &mut schema_refs);
                }
                refs.insert(name.clone(), schema_refs);
                locations.insert(name.clone(), (path.to_path_buf(), *line, *column));
                if db_table.is_some() {
                    persistent.insert(name.clone());
                }
            }
            Statement::Export(inner) => {
                collect_schema_refs_from_statements(
                    path,
                    std::slice::from_ref(inner.as_ref()),
                    refs,
                    locations,
                    persistent,
                );
            }
            _ => {}
        }
    }
}

fn collect_schema_type_refs(schema_type: &SchemaType, refs: &mut Vec<String>) {
    match schema_type {
        SchemaType::Reference(name) => refs.push(name.clone()),
        SchemaType::TypedList(inner) => collect_schema_type_refs(inner, refs),
        _ => {}
    }
}

fn lint_statements(
    path: &Path,
    statements: &[Statement],
    schema_names: &HashSet<String>,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    let is_app_file = path.file_name().and_then(|name| name.to_str()) == Some("app.marreta");
    lint_statement_block(
        path,
        statements,
        None,
        is_app_file,
        schema_names,
        diagnostics,
    );
}

fn lint_statement_block(
    path: &Path,
    statements: &[Statement],
    final_expression: Option<&Expression>,
    allow_project_metadata: bool,
    schema_names: &HashSet<String>,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    lint_unreachable(path, statements, diagnostics);
    lint_unused_variables(
        path,
        statements,
        final_expression,
        allow_project_metadata,
        diagnostics,
    );

    for stmt in statements {
        lint_schema_references_in_statement(path, stmt, schema_names, diagnostics);
        lint_feature_flags_in_statement(path, stmt, diagnostics);
        lint_self_recursion(path, stmt, diagnostics);
        match stmt {
            Statement::TaskDef {
                body: TaskBody::Block(body, final_expr),
                ..
            } => {
                lint_statement_block(
                    path,
                    body,
                    Some(final_expr),
                    false,
                    schema_names,
                    diagnostics,
                );
            }
            Statement::Route { body, .. }
            | Statement::While { body, .. }
            | Statement::Transaction { body, .. }
            | Statement::OnQueue { body, .. }
            | Statement::OnTopic { body, .. } => {
                lint_statement_block(path, body, None, false, schema_names, diagnostics);
            }
            Statement::Export(inner) => lint_statement_block(
                path,
                std::slice::from_ref(inner.as_ref()),
                None,
                false,
                schema_names,
                diagnostics,
            ),
            _ => {}
        }
    }
}

fn lint_unreachable(path: &Path, statements: &[Statement], diagnostics: &mut Vec<LintDiagnostic>) {
    let mut terminal: Option<&'static str> = None;
    for stmt in statements {
        if let Some(kind) = terminal {
            let (line, column) = statement_location(stmt);
            diagnostics.push(LintDiagnostic::new(
                path.to_path_buf(),
                line,
                column,
                LintSeverity::Warning,
                "unreachable_statement",
                format!("statement is unreachable after {}", kind),
                Some(format!(
                    "Remove the statement or move it before the {}.",
                    kind
                )),
            ));
            continue;
        }
        terminal = terminal_statement_kind(stmt);
    }
}

fn terminal_statement_kind(stmt: &Statement) -> Option<&'static str> {
    match stmt {
        Statement::Reply { .. } => Some("reply"),
        Statement::Fail { .. } => Some("fail"),
        Statement::Raise {
            condition: None, ..
        } => Some("raise"),
        _ => None,
    }
}

fn lint_unused_variables(
    path: &Path,
    statements: &[Statement],
    final_expression: Option<&Expression>,
    allow_project_metadata: bool,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    for (index, stmt) in statements.iter().enumerate() {
        let Some((target, line, column)) = assignment_target(stmt) else {
            continue;
        };
        if allow_project_metadata
            && matches!(
                target,
                "project_name" | "project_version" | "requires_marreta"
            )
        {
            continue;
        }

        let mut later_reads = HashSet::new();
        for later in &statements[index + 1..] {
            collect_identifier_reads_from_statement(later, &mut later_reads);
        }
        if let Some(expr) = final_expression {
            collect_identifier_reads_from_expression(expr, &mut later_reads);
        }

        if !later_reads.contains(target) {
            diagnostics.push(
                LintDiagnostic::new(
                    path.to_path_buf(),
                    line,
                    column,
                    LintSeverity::Warning,
                    "unused_variable",
                    format!("variable '{}' is assigned but never used", target),
                    Some(
                        "Remove the assignment or use the variable later in the block.".to_string(),
                    ),
                )
                .spanning(line, column + target.chars().count()),
            );
        }
    }
}

fn assignment_target(stmt: &Statement) -> Option<(&str, usize, usize)> {
    match stmt {
        Statement::Assignment {
            target,
            line,
            column,
            ..
        }
        | Statement::ConditionalAssignment {
            target,
            line,
            column,
            ..
        } => Some((target.as_str(), *line, *column)),
        _ => None,
    }
}

fn collect_identifier_reads_from_statement(stmt: &Statement, reads: &mut HashSet<String>) {
    walk_statement_expressions(stmt, &mut |expr, _, _| {
        collect_identifier_reads_from_expression(expr, reads);
    });
}

fn collect_identifier_reads_from_expression(expr: &Expression, reads: &mut HashSet<String>) {
    walk_expression(expr, &mut |expr| match expr {
        Expression::Identifier(name) => {
            reads.insert(name.clone());
        }
        // Interpolation is preserved in the string literal and resolved at runtime,
        // so the AST has no sub-expression for `#{name}`. Scan the literal so a
        // variable used only inside an interpolation is not reported as unused.
        Expression::StringLiteral(text) => collect_interpolation_identifiers(text, reads),
        _ => {}
    });
}

/// Collects identifiers referenced inside `#{...}` interpolation placeholders of a
/// string literal. Conservative on purpose: it over-collects identifier-like words,
/// which is safe because it can only mark a variable as used, never the reverse.
fn collect_interpolation_identifiers(text: &str, reads: &mut HashSet<String>) {
    let mut rest = text;
    while let Some(open) = rest.find("#{") {
        let after = &rest[open + 2..];
        let end = after.find('}').unwrap_or(after.len());
        collect_identifier_words(&after[..end], reads);
        rest = &after[end..];
    }
}

/// Adds every identifier-like word (`[A-Za-z_][A-Za-z0-9_]*`) in `fragment` to
/// `reads`. Numbers and punctuation are skipped; keywords and method/property names
/// are harmlessly over-collected.
fn collect_identifier_words(fragment: &str, reads: &mut HashSet<String>) {
    let mut word = String::new();
    for ch in fragment.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            word.push(ch);
        } else if !word.is_empty() {
            push_identifier_word(&mut word, reads);
        }
    }
    if !word.is_empty() {
        push_identifier_word(&mut word, reads);
    }
}

fn push_identifier_word(word: &mut String, reads: &mut HashSet<String>) {
    if word
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
    {
        reads.insert(word.clone());
    }
    word.clear();
}

// Each arm extracts an optional schema field with an explicit `if let`; merging
// it into the match pattern would split each variant into Some/None arms and
// obscure the one-field-per-statement intent.
#[allow(clippy::collapsible_match)]
fn lint_schema_references_in_statement(
    path: &Path,
    stmt: &Statement,
    schema_names: &HashSet<String>,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    match stmt {
        Statement::TaskDef { params, .. } => {
            let (line, column) = statement_location(stmt);
            for param in params {
                if let Some(schema) = &param.schema {
                    push_unknown_schema(path, line, column, schema, schema_names, diagnostics);
                }
            }
        }
        Statement::Schema {
            fields,
            line,
            column,
            ..
        } => {
            for field in fields {
                lint_schema_type_references(
                    path,
                    *line,
                    *column,
                    &field.field_type,
                    schema_names,
                    diagnostics,
                );
            }
        }
        Statement::Route {
            schema,
            line,
            column,
            ..
        } => {
            if let Some(schema) = schema {
                push_unknown_schema(path, *line, *column, schema, schema_names, diagnostics);
            }
        }
        Statement::Reply {
            response_schema,
            line,
            column,
            ..
        } => {
            if let Some(schema) = response_schema {
                push_unknown_schema(path, *line, *column, schema, schema_names, diagnostics);
            }
        }
        Statement::OnQueue {
            schema,
            line,
            column,
            ..
        }
        | Statement::OnTopic {
            schema,
            line,
            column,
            ..
        } => {
            if let Some(schema) = schema {
                push_unknown_schema(path, *line, *column, schema, schema_names, diagnostics);
            }
        }
        Statement::Export(inner) => {
            lint_schema_references_in_statement(path, inner, schema_names, diagnostics)
        }
        _ => {}
    }

    walk_statement_local_expressions(stmt, &mut |expr, line, column| {
        lint_schema_references_in_expression(path, expr, line, column, schema_names, diagnostics);
    });
}

fn lint_schema_type_references(
    path: &Path,
    line: usize,
    column: usize,
    schema_type: &SchemaType,
    schema_names: &HashSet<String>,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    match schema_type {
        SchemaType::Reference(schema) => {
            push_unknown_schema(path, line, column, schema, schema_names, diagnostics);
        }
        SchemaType::TypedList(inner) => {
            lint_schema_type_references(path, line, column, inner, schema_names, diagnostics);
        }
        _ => {}
    }
}

fn lint_schema_references_in_expression(
    path: &Path,
    expr: &Expression,
    line: usize,
    column: usize,
    schema_names: &HashSet<String>,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    match expr {
        Expression::SchemaConstructor { schema_name, .. }
        | Expression::HttpClientResponseSchema { schema_name, .. }
        | Expression::QueuePush {
            schema: Some(schema_name),
            ..
        }
        | Expression::TopicPublish {
            schema: Some(schema_name),
            ..
        } => push_unknown_schema(path, line, column, schema_name, schema_names, diagnostics),
        _ => {}
    }
}

fn push_unknown_schema(
    path: &Path,
    line: usize,
    column: usize,
    schema: &str,
    schema_names: &HashSet<String>,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    if schema_names.contains(schema) {
        return;
    }
    diagnostics.push(LintDiagnostic::new(
        path.to_path_buf(),
        line,
        column,
        LintSeverity::Error,
        "unknown_schema_reference",
        format!("schema '{}' is not declared", schema),
        Some("Declare the schema or fix the schema name.".to_string()),
    ));
}

fn lint_feature_flags_in_statement(
    path: &Path,
    stmt: &Statement,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    walk_statement_local_expressions(stmt, &mut |expr, line, column| {
        if let Expression::MethodCall {
            object,
            method,
            arguments,
        } = expr
            && matches!(object.as_ref(), Expression::Identifier(name) if name == "feature")
            && method == "enabled"
            && let Some(Argument::Positional(Expression::StringLiteral(name))) = arguments.first()
            && !is_valid_feature_name(name)
        {
            diagnostics.push(LintDiagnostic::new(
                path.to_path_buf(),
                line,
                column,
                LintSeverity::Error,
                "invalid_feature_flag_name",
                format!("feature flag name '{}' is invalid", name),
                Some(
                    "Use lower_snake_case in code, without double or trailing underscores."
                        .to_string(),
                ),
            ));
        }
    });
}

fn walk_statement_local_expressions(
    stmt: &Statement,
    visitor: &mut dyn FnMut(&Expression, usize, usize),
) {
    let (line, column) = statement_location(stmt);
    match stmt {
        Statement::Assignment { value, .. } => {
            walk_expression_with_location(value, line, column, visitor)
        }
        Statement::ConditionalAssignment {
            value, condition, ..
        } => {
            walk_expression_with_location(value, line, column, visitor);
            walk_expression_with_location(condition, line, column, visitor);
        }
        Statement::Require { condition, .. } | Statement::Reject { condition, .. } => {
            walk_expression_with_location(condition, line, column, visitor)
        }
        Statement::While { condition, .. } => {
            walk_expression_with_location(condition, line, column, visitor)
        }
        Statement::TaskDef { body, .. } => {
            walk_task_body_local_expressions(body, line, column, visitor)
        }
        Statement::ExpressionStatement { expression, .. } => {
            walk_expression_with_location(expression, line, column, visitor)
        }
        Statement::Route { allow, .. } => {
            for expr in allow {
                walk_expression_with_location(expr, line, column, visitor);
            }
        }
        Statement::AuthProvider { provider, .. } => {
            for field in provider.fields() {
                walk_expression_with_location(&field.value, field.line, field.column, visitor);
            }
        }
        Statement::Reply {
            status_code,
            body,
            extra_headers,
            ..
        } => {
            walk_expression_with_location(status_code, line, column, visitor);
            walk_expression_with_location(body, line, column, visitor);
            if let Some(headers) = extra_headers {
                walk_expression_with_location(headers, line, column, visitor);
            }
        }
        Statement::Fail { message, .. } | Statement::Raise { message, .. } => {
            walk_expression_with_location(message, line, column, visitor);
            if let Statement::Raise {
                condition: Some(condition),
                ..
            } = stmt
            {
                walk_expression_with_location(condition, line, column, visitor);
            }
        }
        Statement::OnQueue { queue_name, .. } => {
            walk_expression_with_location(queue_name, line, column, visitor);
        }
        Statement::OnTopic { pattern, .. } => {
            walk_expression_with_location(pattern, line, column, visitor);
        }
        Statement::Nack {
            condition: Some(condition),
            ..
        } => walk_expression_with_location(condition, line, column, visitor),
        Statement::Scenario { steps, .. } => {
            for step in steps {
                walk_scenario_step_expressions(step, &mut |expr| {
                    walk_expression_with_location(expr, line, column, visitor)
                });
            }
        }
        _ => {}
    }
}

fn lint_self_recursion(path: &Path, stmt: &Statement, diagnostics: &mut Vec<LintDiagnostic>) {
    let Statement::TaskDef {
        name,
        body: TaskBody::Inline(Expression::FunctionCall { name: called, .. }),
        line,
        column,
        ..
    } = stmt
    else {
        return;
    };
    if name == called {
        diagnostics.push(LintDiagnostic::new(
            path.to_path_buf(),
            *line,
            *column,
            LintSeverity::Warning,
            "suspicious_self_recursive_task",
            format!(
                "task '{}' calls itself directly without an obvious guard",
                name
            ),
            Some("Add a conditional guard or rewrite the recursive task.".to_string()),
        ));
    }
}

fn statement_location(stmt: &Statement) -> (usize, usize) {
    match stmt {
        Statement::Assignment { line, column, .. }
        | Statement::ConditionalAssignment { line, column, .. }
        | Statement::Require { line, column, .. }
        | Statement::Reject { line, column, .. }
        | Statement::While { line, column, .. }
        | Statement::TaskDef { line, column, .. }
        | Statement::ExpressionStatement { line, column, .. }
        | Statement::Route { line, column, .. }
        | Statement::Schema { line, column, .. }
        | Statement::AuthProvider { line, column, .. }
        | Statement::Reply { line, column, .. }
        | Statement::Fail { line, column, .. }
        | Statement::Raise { line, column, .. }
        | Statement::Transaction { line, column, .. }
        | Statement::OnQueue { line, column, .. }
        | Statement::OnTopic { line, column, .. }
        | Statement::Nack { line, column, .. }
        | Statement::Scenario { line, column, .. } => (*line, *column),
        Statement::Export(inner) => statement_location(inner),
    }
}

fn walk_statement_expressions(
    stmt: &Statement,
    visitor: &mut dyn FnMut(&Expression, usize, usize),
) {
    let (line, column) = statement_location(stmt);
    match stmt {
        Statement::Assignment { value, .. } => {
            walk_expression_with_location(value, line, column, visitor)
        }
        Statement::ConditionalAssignment {
            value, condition, ..
        } => {
            walk_expression_with_location(value, line, column, visitor);
            walk_expression_with_location(condition, line, column, visitor);
        }
        Statement::Require { condition, .. } | Statement::Reject { condition, .. } => {
            walk_expression_with_location(condition, line, column, visitor)
        }
        Statement::While {
            condition, body, ..
        } => {
            walk_expression_with_location(condition, line, column, visitor);
            for stmt in body {
                walk_statement_expressions(stmt, visitor);
            }
        }
        Statement::TaskDef { body, .. } => walk_task_body_expressions(body, line, column, visitor),
        Statement::ExpressionStatement { expression, .. } => {
            walk_expression_with_location(expression, line, column, visitor)
        }
        Statement::Route { allow, body, .. } => {
            for expr in allow {
                walk_expression_with_location(expr, line, column, visitor);
            }
            for stmt in body {
                walk_statement_expressions(stmt, visitor);
            }
        }
        Statement::AuthProvider { provider, .. } => {
            for field in provider.fields() {
                walk_expression_with_location(&field.value, field.line, field.column, visitor);
            }
        }
        Statement::Reply {
            status_code,
            body,
            extra_headers,
            ..
        } => {
            walk_expression_with_location(status_code, line, column, visitor);
            walk_expression_with_location(body, line, column, visitor);
            if let Some(headers) = extra_headers {
                walk_expression_with_location(headers, line, column, visitor);
            }
        }
        Statement::Fail { message, .. } | Statement::Raise { message, .. } => {
            walk_expression_with_location(message, line, column, visitor);
            if let Statement::Raise {
                condition: Some(condition),
                ..
            } = stmt
            {
                walk_expression_with_location(condition, line, column, visitor);
            }
        }
        Statement::Transaction { body, .. } => {
            for stmt in body {
                walk_statement_expressions(stmt, visitor);
            }
        }
        Statement::Export(inner) => walk_statement_expressions(inner, visitor),
        Statement::OnQueue {
            queue_name, body, ..
        } => {
            walk_expression_with_location(queue_name, line, column, visitor);
            for stmt in body {
                walk_statement_expressions(stmt, visitor);
            }
        }
        Statement::OnTopic { pattern, body, .. } => {
            walk_expression_with_location(pattern, line, column, visitor);
            for stmt in body {
                walk_statement_expressions(stmt, visitor);
            }
        }
        Statement::Nack {
            condition: Some(condition),
            ..
        } => walk_expression_with_location(condition, line, column, visitor),
        Statement::Scenario { steps, .. } => {
            for step in steps {
                walk_scenario_step_expressions(step, &mut |expr| {
                    walk_expression_with_location(expr, line, column, visitor)
                });
            }
        }
        _ => {}
    }
}

fn walk_task_body_expressions(
    body: &TaskBody,
    line: usize,
    column: usize,
    visitor: &mut dyn FnMut(&Expression, usize, usize),
) {
    match body {
        TaskBody::Inline(expr) => walk_expression_with_location(expr, line, column, visitor),
        TaskBody::Block(statements, expr) => {
            for stmt in statements {
                walk_statement_expressions(stmt, visitor);
            }
            walk_expression_with_location(expr, line, column, visitor);
        }
    }
}

fn walk_task_body_local_expressions(
    body: &TaskBody,
    line: usize,
    column: usize,
    visitor: &mut dyn FnMut(&Expression, usize, usize),
) {
    match body {
        TaskBody::Inline(expr) | TaskBody::Block(_, expr) => {
            walk_expression_with_location(expr, line, column, visitor)
        }
    }
}

fn walk_scenario_step_expressions(step: &ScenarioStep, visitor: &mut dyn FnMut(&Expression)) {
    match step {
        ScenarioStep::Given {
            target, returns, ..
        } => {
            visitor(target);
            visitor(returns);
        }
        ScenarioStep::When { body, headers, .. } => {
            if let Some(body) = body {
                visitor(body);
            }
            if let Some(headers) = headers {
                visitor(headers);
            }
        }
        ScenarioStep::ThenStatus { status, .. } => visitor(status),
        ScenarioStep::ThenResponse { expected, .. } => visitor(expected),
    }
}

fn walk_expression_with_location(
    expr: &Expression,
    line: usize,
    column: usize,
    visitor: &mut dyn FnMut(&Expression, usize, usize),
) {
    visitor(expr, line, column);
    walk_expression_children(expr, &mut |child| {
        walk_expression_with_location(child, line, column, visitor)
    });
}

fn walk_expression(expr: &Expression, visitor: &mut dyn FnMut(&Expression)) {
    visitor(expr);
    walk_expression_children(expr, &mut |child| walk_expression(child, visitor));
}

fn walk_expression_children(expr: &Expression, visitor: &mut dyn FnMut(&Expression)) {
    match expr {
        Expression::List(items) => items.iter().for_each(visitor),
        Expression::MapLiteral(items) | Expression::SchemaConstructor { fields: items, .. } => {
            items.iter().for_each(|(_, value)| visitor(value))
        }
        Expression::BinaryOp { left, right, .. } => {
            visitor(left);
            visitor(right);
        }
        Expression::UnaryOp { operand, .. } => visitor(operand),
        Expression::PropertyAccess { object, .. } => visitor(object),
        Expression::MethodCall {
            object, arguments, ..
        } => {
            visitor(object);
            arguments.iter().for_each(|arg| walk_argument(arg, visitor));
        }
        Expression::HttpClientResponseSchema { call, .. } => visitor(call),
        Expression::FunctionCall { arguments, .. } => {
            arguments.iter().for_each(|arg| walk_argument(arg, visitor));
        }
        Expression::Match { subject, arms } => {
            visitor(subject);
            arms.iter().for_each(|arm| {
                if let crate::ast::MatchPattern::Literal(expr) = &arm.pattern {
                    visitor(expr);
                }
                visitor(&arm.value);
            });
        }
        Expression::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visitor(condition);
            walk_task_body_expression_children(then_branch, visitor);
            if let Some(else_branch) = else_branch {
                walk_task_body_expression_children(else_branch, visitor);
            }
        }
        Expression::Subscript { object, key } => {
            visitor(object);
            visitor(key);
        }
        Expression::Pipeline { input, stages } => {
            visitor(input);
            for stage in stages {
                walk_pipeline_stage(stage, visitor);
            }
        }
        Expression::Broadcast { input, targets } => {
            visitor(input);
            targets.iter().for_each(visitor);
        }
        Expression::Rescue { expr, handler } => {
            visitor(expr);
            visitor(handler);
        }
        Expression::QueuePush {
            queue_name,
            payload,
            ..
        } => {
            visitor(queue_name);
            if let Some(payload) = payload {
                visitor(payload);
            }
        }
        Expression::TopicPublish { topic, payload, .. } => {
            visitor(topic);
            if let Some(payload) = payload {
                visitor(payload);
            }
        }
        _ => {}
    }
}

fn walk_argument(arg: &Argument, visitor: &mut dyn FnMut(&Expression)) {
    match arg {
        Argument::Positional(expr) | Argument::Named { value: expr, .. } => visitor(expr),
    }
}

fn walk_task_body_expression_children(body: &TaskBody, visitor: &mut dyn FnMut(&Expression)) {
    match body {
        TaskBody::Inline(expr) => visitor(expr),
        TaskBody::Block(statements, expr) => {
            for stmt in statements {
                walk_statement_expressions(stmt, &mut |expr, _, _| visitor(expr));
            }
            visitor(expr);
        }
    }
}

fn walk_pipeline_stage(stage: &PipelineStage, visitor: &mut dyn FnMut(&Expression)) {
    match stage {
        PipelineStage::Expression(expr) => visitor(expr),
        PipelineStage::Map { body, .. } => {
            for stmt in body {
                match stmt {
                    MapStatement::Statement(stmt) => {
                        walk_statement_expressions(stmt, &mut |expr, _, _| visitor(expr))
                    }
                    MapStatement::Keep { value, condition } => {
                        visitor(value);
                        if let Some(condition) = condition {
                            visitor(condition);
                        }
                    }
                    MapStatement::Skip { condition } => visitor(condition),
                }
            }
        }
        PipelineStage::Reduce { initial, body, .. } => {
            visitor(initial);
            walk_task_body_expression_children(body, visitor);
        }
        PipelineStage::Rescue { handler } => match handler {
            RescueHandler::Inline(expr) => visitor(expr),
            RescueHandler::Block(statements) => {
                for stmt in statements {
                    walk_statement_expressions(stmt, &mut |expr, _, _| visitor(expr));
                }
            }
        },
    }
}

fn render_human(diagnostics: &[LintDiagnostic]) -> String {
    if diagnostics.is_empty() {
        return "No lint diagnostics.\n".to_string();
    }
    let mut out = String::new();
    for diag in diagnostics {
        out.push_str(&format!(
            "{} {} at {}:{}:{}\n{}\n",
            diag.severity.as_str(),
            diag.code,
            display_path(&diag.file),
            diag.line,
            diag.column,
            diag.message
        ));
        if let Some(help) = &diag.help {
            out.push_str(&format!("help: {}\n", help));
        }
        out.push('\n');
    }
    out
}

fn render_json(diagnostics: &[LintDiagnostic]) -> String {
    let items: Vec<_> = diagnostics
        .iter()
        .map(|diag| {
            json!({
                "file": display_path(&diag.file),
                "line": diag.line,
                "column": diag.column,
                "end_line": diag.end_line,
                "end_column": diag.end_column,
                "severity": diag.severity.as_str(),
                "code": diag.code,
                "message": diag.message,
                "help": diag.help,
            })
        })
        .collect();
    format!("{}\n", serde_json::to_string_pretty(&items).unwrap())
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

trait AuthProviderFields {
    fn fields(&self) -> &[crate::ast::AuthProviderField];
}

impl AuthProviderFields for crate::ast::AuthProvider {
    fn fields(&self) -> &[crate::ast::AuthProviderField] {
        match self {
            crate::ast::AuthProvider::Jwt(config) | crate::ast::AuthProvider::ApiKey(config) => {
                &config.fields
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lint_source(source: &str) -> LintReport {
        lint_stdin(PathBuf::from("test.marreta"), source.to_string())
    }

    fn lint_multi(files: &[(&str, &str)]) -> LintReport {
        lint_inputs(
            files
                .iter()
                .map(|(path, source)| LintInput {
                    path: PathBuf::from(path),
                    source: source.to_string(),
                })
                .collect(),
        )
    }

    #[test]
    fn requires_marreta_is_not_flagged_as_unused() {
        // Spec 063: `requires_marreta` is project metadata in the entrypoint, not an
        // unused variable.
        let report = lint_multi(&[(
            "app.marreta",
            "project_name = \"x\"\nproject_version = \"0.1.0\"\nrequires_marreta = \">=0.2.0\"\nroute GET \"/h\"\n    reply 200, { ok: true }\n",
        )]);
        assert!(
            report
                .diagnostics
                .iter()
                .all(|d| !d.message.contains("requires_marreta")),
            "requires_marreta must not be flagged, got {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn reports_unused_exported_task() {
        let report = lint_multi(&[
            (
                "app.marreta",
                "project_name = \"x\"\nproject_version = \"0.1.0\"\n",
            ),
            ("tasks/billing.marreta", "export task charge(x) => x\n"),
        ]);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "unused_exported_task" && d.message.contains("billing.charge")),
            "an exported task referenced nowhere should warn, got {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn bare_cross_file_call_does_not_suppress_unused_exported_task() {
        // A bare `charge()` in another file no longer resolves to `billing.charge` at
        // runtime, so it must not count as a use (review finding: avoid the false
        // negative from a project-wide bare-name match).
        let report = lint_multi(&[
            (
                "app.marreta",
                "project_name = \"x\"\nproject_version = \"0.1.0\"\n",
            ),
            ("tasks/billing.marreta", "export task charge(x) => x\n"),
            (
                "routes/other.marreta",
                "route GET \"/o\"\n    y = charge(5)\n    reply 200, { y: y }\n",
            ),
        ]);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "unused_exported_task" && d.message.contains("billing.charge")),
            "a bare call in a different file must not suppress the warning, got {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn exported_task_called_bare_in_its_own_file_is_used() {
        let report = lint_multi(&[
            (
                "app.marreta",
                "project_name = \"x\"\nproject_version = \"0.1.0\"\n",
            ),
            (
                "tasks/billing.marreta",
                "export task charge(x) => x\nroute GET \"/b\"\n    reply 200, { v: charge(5) }\n",
            ),
        ]);
        assert!(
            report
                .diagnostics
                .iter()
                .all(|d| d.code != "unused_exported_task"),
            "a bare call within the declaring file counts as a use, got {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn exported_task_used_via_namespace_is_not_unused() {
        let report = lint_multi(&[
            (
                "app.marreta",
                "project_name = \"x\"\nproject_version = \"0.1.0\"\n",
            ),
            ("tasks/billing.marreta", "export task charge(x) => x\n"),
            (
                "routes/orders.marreta",
                "route GET \"/o\"\n    reply 200, { v: billing.charge(5) }\n",
            ),
        ]);
        assert!(
            report
                .diagnostics
                .iter()
                .all(|d| d.code != "unused_exported_task"),
            "a `namespace.task` reference counts as a use, got {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn exported_task_used_via_pipeline_namespace_is_not_unused() {
        let report = lint_multi(&[
            (
                "app.marreta",
                "project_name = \"x\"\nproject_version = \"0.1.0\"\n",
            ),
            ("tasks/calc.marreta", "export task double(x) => x * 2\n"),
            (
                "routes/run.marreta",
                "route GET \"/r\"\n    v = 5 >> calc.double\n    reply 200, { v: v }\n",
            ),
        ]);
        assert!(
            report
                .diagnostics
                .iter()
                .all(|d| d.code != "unused_exported_task"),
            "a `>> namespace.task` stage counts as a use, got {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn renders_json_diagnostics() {
        let report = LintReport {
            diagnostics: vec![LintDiagnostic::new(
                PathBuf::from("routes/a.marreta"),
                2,
                4,
                LintSeverity::Warning,
                "unreachable_statement",
                "statement is unreachable after reply",
                Some("Move it.".to_string()),
            )],
        };
        let json: serde_json::Value =
            serde_json::from_str(&report.render(LintFormat::Json)).unwrap();
        assert_eq!(json[0]["file"], "routes/a.marreta");
        assert_eq!(json[0]["severity"], "warning");
        assert_eq!(json[0]["code"], "unreachable_statement");
    }

    #[test]
    fn reports_unreachable_statement_after_reply() {
        let report =
            lint_source("route GET \"/x\"\n    reply 200, null\n    log.info(\"never\")\n");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diag| diag.code == "unreachable_statement")
        );
    }

    #[test]
    fn reports_invalid_feature_flag_literal() {
        let report = lint_source(
            "route GET \"/x\"\n    enabled = feature.enabled(\"Bad__Name\")\n    reply 200, enabled\n",
        );
        let count = report
            .diagnostics
            .iter()
            .filter(|diag| diag.code == "invalid_feature_flag_name")
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn reports_nested_feature_flag_literal_once() {
        let report = lint_source(
            "route GET \"/x\"\n    if feature.enabled(\"Bad__Name\")\n        reply 200, { enabled: true }\n    reply 200, { enabled: false }\n",
        );
        let count = report
            .diagnostics
            .iter()
            .filter(|diag| diag.code == "invalid_feature_flag_name")
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn reports_unknown_schema_reference() {
        let report =
            lint_source("route GET \"/x\"\n    reply 200 as MissingSchema, { ok: true }\n");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diag| diag.code == "unknown_schema_reference")
        );
    }

    #[test]
    fn reports_schema_constructor_reference_once_inside_route() {
        let report = lint_source(
            "route GET \"/x\"\n    payload = MissingSchema { ok: true }\n    reply 200, payload\n",
        );
        let count = report
            .diagnostics
            .iter()
            .filter(|diag| diag.code == "unknown_schema_reference")
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn reports_unknown_schema_reference_in_schema_field() {
        let report = lint_source("schema Order\n    address: MissingSchema\n");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diag| diag.code == "unknown_schema_reference")
        );
    }

    #[test]
    fn stdin_overlay_uses_project_schema_context() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("app.marreta"),
            "project_name = \"lint\"\nproject_version = \"0.1.0\"\n",
        )
        .unwrap();
        fs::create_dir(dir.path().join("schemas")).unwrap();
        fs::write(
            dir.path().join("schemas/contracts.marreta"),
            "export schema GreetingResponse\n    message: string\n",
        )
        .unwrap();
        fs::create_dir(dir.path().join("routes")).unwrap();
        fs::write(
            dir.path().join("routes/greetings.marreta"),
            "route GET \"/saved\"\n    reply 200, { ok: true }\n",
        )
        .unwrap();

        let report = lint_project_stdin(
            dir.path(),
            PathBuf::from("routes/greetings.marreta"),
            "route GET \"/x\"\n    reply 200 as GreetingResponse, { message: \"Hello\" }\n"
                .to_string(),
        )
        .unwrap();
        assert!(
            report
                .diagnostics
                .iter()
                .all(|diag| diag.code != "unknown_schema_reference")
        );
    }

    #[test]
    fn reports_circular_schema_reference() {
        let report = lint_source("schema A\n    b: B\n\nschema B\n    a: A\n");
        assert!(report.diagnostics.iter().any(|diag| {
            diag.code == "source_load_error" && diag.message.contains("circular schema reference")
        }));
    }

    #[test]
    fn allows_circular_reference_between_persistent_schemas() {
        // A mutual reference between `db:` schemas is a relation (foreign key), not value
        // embedding, so it is a legitimate relational graph the loader accepts.
        let report = lint_source(
            "schema DbUser\n    db: users\n    orders: list of DbOrder\n\nschema DbOrder\n    db: orders\n    customer: DbUser\n",
        );
        assert!(
            report
                .diagnostics
                .iter()
                .all(|diag| !diag.message.contains("circular schema reference")),
            "persistent-schema relations must not be flagged as a cycle, got {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn allows_mixed_cycle_through_a_persistent_schema() {
        // A cycle that passes through a persistent schema (DbUser <-> Profile) is broken
        // at the relation edge by the validator (the DbUser reference is let through), so
        // it never loops and is not flagged (Spec 062, relation-aware). Only an all-value
        // cycle is flagged.
        let report = lint_source(
            "schema DbUser\n    db: users\n    profile: Profile\n\nschema Profile\n    user: DbUser\n",
        );
        assert!(
            report
                .diagnostics
                .iter()
                .all(|diag| !diag.message.contains("circular schema reference")),
            "a cycle through a persistent schema must not be flagged, got {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn reports_unused_private_task() {
        let report =
            lint_source("task helper() => 1\nroute GET \"/x\"\n    reply 200, { ok: true }\n");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diag| diag.code == "unused_private_task")
        );
    }

    #[test]
    fn called_private_task_is_not_reported() {
        let report = lint_source("task helper() => 1\nroute GET \"/x\"\n    reply 200, helper()\n");
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|diag| diag.code == "unused_private_task")
        );
    }

    #[test]
    fn suspicious_direct_self_recursion_is_reported() {
        let report = lint_source("task loop() => loop()\n");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diag| diag.code == "suspicious_self_recursive_task")
        );
    }

    #[test]
    fn strict_mode_fails_on_warnings() {
        let report = lint_source("task helper() => 1\n");
        assert!(!report.has_errors());
        assert!(report.should_fail(true));
        assert!(!report.should_fail(false));
    }

    #[test]
    fn reports_unused_variable_in_straight_line_block() {
        let report =
            lint_source("route GET \"/x\"\n    message = \"unused\"\n    reply 200, null\n");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diag| diag.code == "unused_variable")
        );
    }

    #[test]
    fn does_not_report_variable_used_by_later_expression() {
        let report =
            lint_source("route GET \"/x\"\n    message = \"used\"\n    reply 200, message\n");
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|diag| diag.code == "unused_variable")
        );
    }

    #[test]
    fn does_not_report_variable_used_only_in_string_interpolation() {
        let report = lint_source(
            "route GET \"/x\"\n    name = \"world\"\n    reply 200, \"Hello #{name}\"\n",
        );
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|diag| diag.code == "unused_variable"),
            "variable used inside an interpolation must not be flagged unused"
        );
    }

    #[test]
    fn reports_unused_variable_not_referenced_in_interpolation() {
        // The interpolation scan must not mask a genuinely unused variable.
        let report =
            lint_source("route GET \"/x\"\n    unused = \"x\"\n    reply 200, \"Hello #{name}\"\n");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diag| diag.code == "unused_variable")
        );
    }

    #[test]
    fn does_not_report_task_block_variable_used_as_implicit_return() {
        let report = lint_source("task greet()\n    message = \"used\"\n    message\n");
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|diag| diag.code == "unused_variable")
        );
    }
}
