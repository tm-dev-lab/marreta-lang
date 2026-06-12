use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::ast::{
    Argument, Expression, MapStatement, MatchPattern, PipelineStage, RescueHandler, ScenarioStep,
    SchemaType, Statement, TakeBinding, TaskBody,
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

/// Spec 071: the single source of every lint code, its default severity, and a one-line summary.
/// It backs the `reference/lint` docs page and the editor `codeDescription` links, and it makes the
/// lint drift-proof the same way the Spec 068 catalog-to-token test made the lexer drift-proof: a
/// code emitted but not catalogued trips the `LintDiagnostic::new` debug assertion, and a code
/// catalogued but not documented trips the catalog-to-docs test.
#[derive(Debug, Clone, Copy)]
pub struct LintRule {
    pub code: &'static str,
    pub default_severity: LintSeverity,
    pub summary: &'static str,
}

const CATALOG: &[LintRule] = &[
    LintRule {
        code: "source_load_error",
        default_severity: LintSeverity::Error,
        summary: "The file failed to load (a syntax, schema, or configuration error); the project cannot run until it is fixed.",
    },
    LintRule {
        code: "duplicate_route",
        default_severity: LintSeverity::Error,
        summary: "Two routes share the same verb and path pattern, so one shadows the other.",
    },
    LintRule {
        code: "unknown_schema_reference",
        default_severity: LintSeverity::Error,
        summary: "A schema field type or `as` binding references a schema name that is not declared.",
    },
    LintRule {
        code: "invalid_feature_flag_name",
        default_severity: LintSeverity::Error,
        summary: "A feature flag name is not a valid `MARRETA_FEATURE_*` identifier.",
    },
    LintRule {
        code: "unused_variable",
        default_severity: LintSeverity::Warning,
        summary: "A local variable is assigned but never read.",
    },
    LintRule {
        code: "unused_private_task",
        default_severity: LintSeverity::Warning,
        summary: "A file-private task is declared but never called.",
    },
    LintRule {
        code: "unused_exported_task",
        default_severity: LintSeverity::Warning,
        summary: "An exported task is never called from any file.",
    },
    LintRule {
        code: "unreachable_statement",
        default_severity: LintSeverity::Warning,
        summary: "A statement follows a terminating statement (reply, fail, raise) and can never run.",
    },
    LintRule {
        code: "suspicious_self_recursive_task",
        default_severity: LintSeverity::Warning,
        summary: "A task calls itself with no visible base case, a likely infinite recursion.",
    },
    LintRule {
        code: "shadows_injected_binding",
        default_severity: LintSeverity::Warning,
        summary: "A local in a route reuses the name of a runtime-injected binding (params, query, headers, auth, or a take binding), hiding it.",
    },
    LintRule {
        code: "route_without_response",
        default_severity: LintSeverity::Warning,
        summary: "A route path can finish without reply or fail, returning a silent 204 with no body.",
    },
    LintRule {
        code: "match_without_fallback",
        default_severity: LintSeverity::Warning,
        summary: "A match whose value is used has no fallback arm, so an unmatched value silently becomes null.",
    },
    LintRule {
        code: "non_literal_sql_identifier",
        default_severity: LintSeverity::Warning,
        summary: "A db order_by clause, select alias, or like/in field is built from a runtime value rather than a literal, which is a SQL injection vector.",
    },
    LintRule {
        code: "unused_schema",
        default_severity: LintSeverity::Warning,
        summary: "A non-persistent schema is declared but never referenced by a validation, response, field type, or constructor.",
    },
    LintRule {
        code: "unused_auth_provider",
        default_severity: LintSeverity::Warning,
        summary: "An auth provider is declared but never required by any route.",
    },
];

/// Every lint rule in the catalog, in declaration order.
pub fn catalog() -> &'static [LintRule] {
    CATALOG
}

/// The catalog entry for a lint code, if any.
pub fn rule(code: &str) -> Option<&'static LintRule> {
    CATALOG.iter().find(|r| r.code == code)
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
        // Spec 071: a code must exist in the catalog, so a new rule cannot ship without its catalog
        // entry (and, through the catalog-to-docs test, its docs anchor).
        debug_assert!(
            rule(code).is_some(),
            "lint code '{code}' is not in the catalog; add it to CATALOG (Spec 071)"
        );
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
    // Spec 071: per-file inline suppressions (`# marreta: allow <code>`), built from the raw source
    // so they survive even when a file fails to parse.
    let mut suppressions: HashMap<PathBuf, LineSuppressions> = HashMap::new();

    for input in inputs {
        suppressions.insert(input.path.clone(), parse_suppressions(&input.source));
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
        lint_shadowed_injected_bindings(&file.path, &file.program, None, &mut diagnostics);
        lint_route_without_response(&file.path, &file.program, &mut diagnostics);
        lint_match_without_fallback(&file.path, &file.program, &mut diagnostics);
        lint_non_literal_sql_identifier(&file.path, &file.program, &mut diagnostics);
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

    // Spec 071: a non-persistent schema that nothing references. Persistent (`db:`) schemas define a
    // table and are excluded, since a table can be in use without an explicit `as Schema` reference.
    let schema_refs = collect_schema_references(&parsed);
    for (name, path, line, column, persistent) in collect_schema_decls(&parsed) {
        if !persistent && !schema_refs.contains(&name) {
            diagnostics.push(LintDiagnostic::new(
                path,
                line,
                column,
                LintSeverity::Warning,
                "unused_schema",
                format!("schema '{name}' is declared but never referenced"),
                Some("Use it to validate or shape a payload, reference it from another schema, or remove it.".to_string()),
            ));
        }
    }

    // Spec 071: an auth provider no route requires.
    let provider_refs = collect_auth_provider_refs(&parsed);
    for (name, path, line, column) in collect_auth_provider_decls(&parsed) {
        if !provider_refs.contains(&name) {
            diagnostics.push(LintDiagnostic::new(
                path,
                line,
                column,
                LintSeverity::Warning,
                "unused_auth_provider",
                format!("auth provider '{name}' is declared but never required by a route"),
                Some("Require it on a route with `require auth {name}`, or remove it.".to_string()),
            ));
        }
    }

    // Spec 071: drop diagnostics silenced by an inline `# marreta: allow <code>` directive.
    diagnostics.retain(|d| !is_suppressed(&suppressions, d));
    LintReport { diagnostics }
}

/// Per-line set of lint codes suppressed by inline directives, keyed by 1-based line number.
type LineSuppressions = HashMap<usize, HashSet<String>>;

/// Spec 071: scans a file's raw source for `# marreta: allow <code> [<code> ...]` directives. A
/// standalone comment line silences the **next** line; a trailing comment silences **its own** line.
fn parse_suppressions(source: &str) -> LineSuppressions {
    let mut map: LineSuppressions = HashMap::new();
    for (idx, raw) in source.lines().enumerate() {
        if let Some(codes) = parse_allow_directive(raw) {
            let line_no = idx + 1;
            let target = if raw.trim_start().starts_with('#') {
                line_no + 1 // standalone directive: applies to the line below it
            } else {
                line_no // trailing directive: applies to its own line
            };
            map.entry(target).or_default().extend(codes);
        }
    }
    map
}

/// Parses the codes from a `# marreta: allow ...` directive on a line, or `None` if absent.
/// `#{...}` interpolation never matches (the text after `#` is `{`, not `marreta:`).
fn parse_allow_directive(line: &str) -> Option<Vec<String>> {
    let hash = comment_start_index(line)?;
    let after = line[hash + 1..].trim_start();
    let rest = after.strip_prefix("marreta:")?.trim_start();
    let rest = rest.strip_prefix("allow")?;
    // Require a boundary after `allow` so a word like `allowance` is not a directive.
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let codes: Vec<String> = rest
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    (!codes.is_empty()).then_some(codes)
}

/// The byte index of the `#` that starts a comment on a line, skipping any `#` inside a string
/// literal (including a `#{...}` interpolation, which lives between the quotes). String-aware so an
/// inline directive after a string, `x = "a#b"  # marreta: allow ...`, still parses.
fn comment_start_index(line: &str) -> Option<usize> {
    let mut in_string = false;
    let mut chars = line.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' if in_string => {
                chars.next(); // skip the escaped character
            }
            '"' => in_string = !in_string,
            '#' if !in_string => return Some(i),
            _ => {}
        }
    }
    None
}

fn is_suppressed(suppressions: &HashMap<PathBuf, LineSuppressions>, diag: &LintDiagnostic) -> bool {
    suppressions
        .get(&diag.file)
        .and_then(|lines| lines.get(&diag.line))
        .is_some_and(|codes| codes.contains(diag.code))
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

/// Spec 071: a schema declaration with its location and whether it is persistent (`db:`).
fn collect_schema_decls(files: &[ParsedFile]) -> Vec<(String, PathBuf, usize, usize, bool)> {
    let mut out = Vec::new();
    for file in files {
        collect_schema_decls_in(&file.program, &file.path, &mut out);
    }
    out
}

fn collect_schema_decls_in(
    statements: &[Statement],
    path: &Path,
    out: &mut Vec<(String, PathBuf, usize, usize, bool)>,
) {
    for stmt in statements {
        match stmt {
            Statement::Schema {
                name,
                db_table,
                line,
                column,
                ..
            } => out.push((
                name.clone(),
                path.to_path_buf(),
                *line,
                *column,
                db_table.is_some(),
            )),
            Statement::Export(inner) => {
                collect_schema_decls_in(std::slice::from_ref(inner.as_ref()), path, out)
            }
            _ => {}
        }
    }
}

/// Every schema name referenced anywhere (across files): a validation/response/param `as Schema`, a
/// schema field type, a constructor, or a queue/topic payload schema. Comprehensive on purpose: a
/// missed reference would falsely flag a used schema as unused, so it descends into nested blocks
/// (an `if`/`match` branch, a pipeline `map`/`reduce`/`rescue` block) as well as statement bodies.
///
/// Intentional asymmetry, do not "unify" with the route-termination walker: reference collection
/// descends as far as possible, **including** `rescue` bodies, because a missed reference is a false
/// "unused" (its bad direction). Route termination (`block_terminates`) is deliberately shallow and
/// **excludes** `rescue` bodies, because counting too much is a false "has a response" (its bad
/// direction). The two conservatisms point opposite ways by design.
fn collect_schema_references(files: &[ParsedFile]) -> HashSet<String> {
    let mut refs = HashSet::new();
    for file in files {
        for stmt in &file.program {
            collect_schema_refs_in_statement(stmt, &mut refs);
        }
    }
    refs
}

fn collect_schema_refs_in_statement(stmt: &Statement, refs: &mut HashSet<String>) {
    extract_statement_field_schema_refs(stmt, refs);

    // Expression-level references, and statement-field references of statements nested in this
    // statement's expressions (a conditional `reply ... as Schema`, for example). The walk is deep,
    // so it visits every `if`/pipeline node at any depth; taking each node's direct block statements
    // therefore reaches all nested statements.
    walk_statement_local_expressions(stmt, &mut |expr, _, _| {
        extract_expression_field_schema_refs(expr, refs);
        for nested in direct_block_statements(expr) {
            extract_statement_field_schema_refs(nested, refs);
        }
    });

    for body_stmt in direct_body_statements(stmt) {
        collect_schema_refs_in_statement(body_stmt, refs);
    }
}

fn extract_statement_field_schema_refs(stmt: &Statement, refs: &mut HashSet<String>) {
    match stmt {
        Statement::Reply {
            response_schema: Some(schema),
            ..
        }
        | Statement::Route {
            schema: Some(schema),
            ..
        }
        | Statement::OnQueue {
            schema: Some(schema),
            ..
        }
        | Statement::OnTopic {
            schema: Some(schema),
            ..
        } => {
            refs.insert(schema.clone());
        }
        Statement::TaskDef { params, .. } => {
            for param in params {
                if let Some(schema) = &param.schema {
                    refs.insert(schema.clone());
                }
            }
        }
        Statement::Schema { fields, .. } => {
            for field in fields {
                collect_schema_type_refs_into(&field.field_type, refs);
            }
        }
        _ => {}
    }
}

fn extract_expression_field_schema_refs(expr: &Expression, refs: &mut HashSet<String>) {
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
        } => {
            refs.insert(schema_name.clone());
        }
        _ => {}
    }
}

/// The statements directly inside a statement's own body (not via its expressions).
fn direct_body_statements(stmt: &Statement) -> Vec<&Statement> {
    match stmt {
        Statement::Route { body, .. }
        | Statement::While { body, .. }
        | Statement::Transaction { body, .. }
        | Statement::OnQueue { body, .. }
        | Statement::OnTopic { body, .. } => body.iter().collect(),
        Statement::TaskDef {
            body: TaskBody::Block(body, _),
            ..
        } => body.iter().collect(),
        Statement::Export(inner) => vec![inner.as_ref()],
        _ => Vec::new(),
    }
}

/// The statements directly inside an expression node's blocks (`if`/`match` branches, pipeline
/// `map`/`reduce`/`rescue` blocks). Direct only: a deep expression walk visits every node, so deeper
/// blocks are reached through their own node.
fn direct_block_statements(expr: &Expression) -> Vec<&Statement> {
    let mut out: Vec<&Statement> = Vec::new();
    match expr {
        Expression::If {
            then_branch,
            else_branch,
            ..
        } => {
            if let TaskBody::Block(stmts, _) = then_branch.as_ref() {
                out.extend(stmts.iter());
            }
            if let Some(TaskBody::Block(stmts, _)) = else_branch.as_deref() {
                out.extend(stmts.iter());
            }
        }
        Expression::Pipeline { stages, .. } => {
            for stage in stages {
                match stage {
                    PipelineStage::Map { body, .. } => {
                        for ms in body {
                            if let MapStatement::Statement(s) = ms {
                                out.push(s);
                            }
                        }
                    }
                    PipelineStage::Reduce {
                        body: TaskBody::Block(stmts, _),
                        ..
                    } => out.extend(stmts.iter()),
                    PipelineStage::Rescue {
                        handler: RescueHandler::Block(stmts),
                    } => out.extend(stmts.iter()),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    out
}

fn collect_schema_type_refs_into(schema_type: &SchemaType, refs: &mut HashSet<String>) {
    match schema_type {
        SchemaType::Reference(name) => {
            refs.insert(name.clone());
        }
        SchemaType::TypedList(inner) => collect_schema_type_refs_into(inner, refs),
        _ => {}
    }
}

/// Auth provider declarations with location, and the set of provider names any route requires.
fn collect_auth_provider_decls(files: &[ParsedFile]) -> Vec<(String, PathBuf, usize, usize)> {
    let mut out = Vec::new();
    for file in files {
        for stmt in &file.program {
            let inner = match stmt {
                Statement::AuthProvider { .. } => Some(stmt),
                Statement::Export(b) => {
                    matches!(b.as_ref(), Statement::AuthProvider { .. }).then_some(b.as_ref())
                }
                _ => None,
            };
            if let Some(Statement::AuthProvider {
                provider,
                line,
                column,
            }) = inner
            {
                out.push((
                    provider.name().to_string(),
                    file.path.clone(),
                    *line,
                    *column,
                ));
            }
        }
    }
    out
}

fn collect_auth_provider_refs(files: &[ParsedFile]) -> HashSet<String> {
    let mut refs = HashSet::new();
    for file in files {
        collect_auth_provider_refs_in(&file.program, &mut refs);
    }
    refs
}

fn collect_auth_provider_refs_in(statements: &[Statement], refs: &mut HashSet<String>) {
    for stmt in statements {
        match stmt {
            Statement::Route {
                auth: Some(auth), ..
            } => {
                refs.insert(auth.provider.clone());
            }
            Statement::Export(inner) => {
                collect_auth_provider_refs_in(std::slice::from_ref(inner.as_ref()), refs)
            }
            _ => {}
        }
    }
}

/// Spec 071: the runtime-injected bindings always live in a route scope.
const ALWAYS_INJECTED_BINDINGS: &[&str] = &["params", "query", "headers"];

fn route_injected_bindings(has_auth: bool, take: &[TakeBinding]) -> HashSet<String> {
    let mut live: HashSet<String> = ALWAYS_INJECTED_BINDINGS
        .iter()
        .map(|s| s.to_string())
        .collect();
    if has_auth {
        live.insert("auth".to_string());
    }
    for binding in take {
        live.insert(take_binding_name(binding).to_string());
    }
    live
}

fn take_binding_name(binding: &TakeBinding) -> &str {
    match binding {
        TakeBinding::Payload(n)
        | TakeBinding::Query(n)
        | TakeBinding::Headers(n)
        | TakeBinding::Form(n)
        | TakeBinding::Raw(n) => n,
    }
}

/// Spec 071: flags a local that shadows a runtime-injected binding (`params`, `query`, `headers`,
/// `auth`, or a `take` binding). Scope-aware: only inside a route, only against the bindings live in
/// that route. Recurses into `while`/`transaction` blocks (same route scope), and clears the scope
/// at a `task` body (a task receives arguments, not injections) and at a consumer (its `take`
/// binding is developer-named, not a fixed injection). Matches the existing lint's depth: assignment
/// targets at statement level, not those buried inside `if`/`match` expression branches.
fn lint_shadowed_injected_bindings(
    path: &Path,
    statements: &[Statement],
    live: Option<&HashSet<String>>,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    for stmt in statements {
        if let Some(live) = live {
            if let Some((target, line, column)) = assignment_target(stmt) {
                if live.contains(target) {
                    diagnostics.push(LintDiagnostic::new(
                        path.to_path_buf(),
                        line,
                        column,
                        LintSeverity::Warning,
                        "shadows_injected_binding",
                        format!("local '{target}' shadows the injected binding '{target}'"),
                        Some(format!(
                            "Rename the local; '{target}' is provided by the runtime in this scope."
                        )),
                    ));
                }
            }
        }

        match stmt {
            Statement::Route {
                auth,
                allow,
                take,
                body,
                ..
            } => {
                let route_live = route_injected_bindings(auth.is_some() || !allow.is_empty(), take);
                lint_shadowed_injected_bindings(path, body, Some(&route_live), diagnostics);
            }
            Statement::While { body, .. } | Statement::Transaction { body, .. } => {
                lint_shadowed_injected_bindings(path, body, live, diagnostics);
            }
            Statement::OnQueue { body, .. } | Statement::OnTopic { body, .. } => {
                lint_shadowed_injected_bindings(path, body, None, diagnostics);
            }
            Statement::TaskDef {
                body: TaskBody::Block(body, _),
                ..
            } => {
                lint_shadowed_injected_bindings(path, body, None, diagnostics);
            }
            Statement::Export(inner) => {
                lint_shadowed_injected_bindings(
                    path,
                    std::slice::from_ref(inner.as_ref()),
                    live,
                    diagnostics,
                );
            }
            _ => {}
        }
    }
}

/// Spec 071: flags a `match` with no `fallback` arm whose value is **consumed**. The runtime returns
/// a silent `null` when no arm matches, which then propagates far from its origin. A bare `match`
/// statement (the top expression of an expression statement) discards its value and is exempt - that
/// is the effect-only form. Every other position (an assignment value, a `reply`/`require` argument,
/// a task return expression, a nested subexpression) consumes the value and is checked.
fn lint_match_without_fallback(
    path: &Path,
    statements: &[Statement],
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    for stmt in statements {
        match stmt {
            // A bare match statement discards its value (effect-only): exempt the top match, but
            // still check its subject and arm values for nested consumed matches.
            Statement::ExpressionStatement {
                expression: Expression::Match { subject, arms },
                line,
                column,
            } => {
                flag_consumed_matches(subject, path, *line, *column, diagnostics);
                for arm in arms {
                    flag_consumed_matches(&arm.value, path, *line, *column, diagnostics);
                }
            }
            // A task's return expression is consumed by the caller; its body statements are recursed.
            Statement::TaskDef {
                body, line, column, ..
            } => match body {
                TaskBody::Inline(expr) => {
                    flag_consumed_matches(expr, path, *line, *column, diagnostics)
                }
                TaskBody::Block(_, final_expr) => {
                    flag_consumed_matches(final_expr, path, *line, *column, diagnostics)
                }
            },
            // Every other statement: its directly-bound expressions are consumed contexts. This
            // walks only the statement's own expressions, never a nested body (those are recursed).
            other => {
                walk_statement_local_expressions(other, &mut |expr, l, c| {
                    if let Expression::Match { arms, .. } = expr {
                        if !has_fallback(arms) {
                            diagnostics.push(match_without_fallback_diagnostic(path, l, c));
                        }
                    }
                });
            }
        }

        match stmt {
            Statement::Route { body, .. }
            | Statement::While { body, .. }
            | Statement::Transaction { body, .. }
            | Statement::OnQueue { body, .. }
            | Statement::OnTopic { body, .. } => {
                lint_match_without_fallback(path, body, diagnostics);
            }
            Statement::TaskDef {
                body: TaskBody::Block(stmts, _),
                ..
            } => {
                lint_match_without_fallback(path, stmts, diagnostics);
            }
            Statement::Export(inner) => {
                lint_match_without_fallback(
                    path,
                    std::slice::from_ref(inner.as_ref()),
                    diagnostics,
                );
            }
            _ => {}
        }
    }
}

fn has_fallback(arms: &[crate::ast::MatchArm]) -> bool {
    arms.iter()
        .any(|arm| matches!(arm.pattern, MatchPattern::Fallback))
}

fn match_without_fallback_diagnostic(path: &Path, line: usize, column: usize) -> LintDiagnostic {
    LintDiagnostic::new(
        path.to_path_buf(),
        line,
        column,
        LintSeverity::Warning,
        "match_without_fallback",
        "match has no fallback arm; an unmatched value becomes a silent null".to_string(),
        Some(
            "Add a `fallback ->` arm, or use the match as a statement if its value is unused."
                .to_string(),
        ),
    )
}

/// Flags every consumed `match` without a `fallback` reachable in an expression (descending into
/// nested expressions, including `if`/`match` branches).
fn flag_consumed_matches(
    expr: &Expression,
    path: &Path,
    line: usize,
    column: usize,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    walk_expression(expr, &mut |e| {
        if let Expression::Match { arms, .. } = e {
            if !has_fallback(arms) {
                diagnostics.push(match_without_fallback_diagnostic(path, line, column));
            }
        }
    });
}

/// Spec 071: flags a SQL identifier built from a runtime value in a `db` pipeline. Filter *values*
/// are parameterized, but the identifier (`order_by` clause, a `select` computed alias, a `like`/`in`
/// field) is interpolated into the SQL string raw (`query_builder.rs`), so a non-literal there is an
/// injection vector. `order_by` and `select` are db-only operations; `like`/`in` also exist on `doc`
/// pipelines (a different engine), so they are checked only when the pipeline is not doc-rooted. This
/// lint warns, it does not sanitize: the runtime guard is the named "db identifier hardening"
/// follow-up (SPEC.md §1.4).
fn lint_non_literal_sql_identifier(
    path: &Path,
    statements: &[Statement],
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    for stmt in statements {
        let (line, column) = statement_location(stmt);
        match stmt {
            Statement::TaskDef { body, .. } => match body {
                TaskBody::Inline(expr) => {
                    check_sql_pipelines_in_expr(expr, path, line, column, diagnostics)
                }
                TaskBody::Block(_, final_expr) => {
                    check_sql_pipelines_in_expr(final_expr, path, line, column, diagnostics)
                }
            },
            other => {
                walk_statement_local_expressions(other, &mut |expr, l, c| {
                    if let Expression::Pipeline { input, stages } = expr {
                        check_sql_identifier_stages(input, stages, path, l, c, diagnostics);
                    }
                });
            }
        }

        match stmt {
            Statement::Route { body, .. }
            | Statement::While { body, .. }
            | Statement::Transaction { body, .. }
            | Statement::OnQueue { body, .. }
            | Statement::OnTopic { body, .. } => {
                lint_non_literal_sql_identifier(path, body, diagnostics);
            }
            Statement::TaskDef {
                body: TaskBody::Block(stmts, _),
                ..
            } => {
                lint_non_literal_sql_identifier(path, stmts, diagnostics);
            }
            Statement::Export(inner) => {
                lint_non_literal_sql_identifier(
                    path,
                    std::slice::from_ref(inner.as_ref()),
                    diagnostics,
                );
            }
            _ => {}
        }
    }
}

fn check_sql_pipelines_in_expr(
    expr: &Expression,
    path: &Path,
    line: usize,
    column: usize,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    walk_expression(expr, &mut |e| {
        if let Expression::Pipeline { input, stages } = e {
            check_sql_identifier_stages(input, stages, path, line, column, diagnostics);
        }
    });
}

fn check_sql_identifier_stages(
    input: &Expression,
    stages: &[PipelineStage],
    path: &Path,
    line: usize,
    column: usize,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    let is_doc = expression_root_identifier(input) == Some("doc");
    for stage in stages {
        let PipelineStage::Expression(Expression::FunctionCall { name, arguments }) = stage else {
            continue;
        };
        match name.as_str() {
            "order_by" => check_first_positional_identifier(
                arguments,
                "order_by",
                path,
                line,
                column,
                diagnostics,
            ),
            "like" | "in" if !is_doc => {
                check_first_positional_identifier(arguments, name, path, line, column, diagnostics)
            }
            "select" => {
                for arg in arguments {
                    if let Argument::Named { value, .. } = arg {
                        if !is_safe_sql_identifier(value) {
                            diagnostics
                                .push(non_literal_sql_diagnostic("select", path, line, column));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn check_first_positional_identifier(
    arguments: &[Argument],
    op: &str,
    path: &Path,
    line: usize,
    column: usize,
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    if let Some(Argument::Positional(expr)) = arguments.first() {
        if !is_safe_sql_identifier(expr) {
            diagnostics.push(non_literal_sql_diagnostic(op, path, line, column));
        }
    }
}

/// A SQL identifier is safe only as a string literal with no interpolation. A bare literal is
/// developer-authored; a variable, an expression, or an interpolated string (`"#{x}"`) carries a
/// runtime value into the SQL string.
///
/// This is intentionally stricter than the index-inference `literal_str` check, because the two ask
/// different questions. Index inference asks "can I know the field statically?" (an interpolation is
/// unknowable, so it skips the shape, which is safe). This lint asks "does a runtime value reach the
/// SQL string?" (an interpolation does, so it must warn). `order_by("id #{dir}")` is exactly the
/// vector: the whole clause is concatenated into the SQL raw, not parameterized.
fn is_safe_sql_identifier(expr: &Expression) -> bool {
    matches!(expr, Expression::StringLiteral(s) if !s.contains("#{"))
}

fn non_literal_sql_diagnostic(op: &str, path: &Path, line: usize, column: usize) -> LintDiagnostic {
    LintDiagnostic::new(
        path.to_path_buf(),
        line,
        column,
        LintSeverity::Warning,
        "non_literal_sql_identifier",
        format!(
            "the SQL identifier in `{op}` is built from a runtime value, which is an injection risk"
        ),
        Some(
            "Use a literal string for the identifier. Filter values are already parameterized; only \
             the identifier is interpolated raw."
                .to_string(),
        ),
    )
}

/// The leftmost identifier an access/pipeline expression is rooted at (`db` for `db.users.find`,
/// `doc` for `doc.items >> ...`, a variable for a relation pipeline like `user.orders`).
fn expression_root_identifier(expr: &Expression) -> Option<&str> {
    match expr {
        Expression::Identifier(name) => Some(name),
        Expression::PropertyAccess { object, .. }
        | Expression::MethodCall { object, .. }
        | Expression::Subscript { object, .. } => expression_root_identifier(object),
        Expression::Pipeline { input, .. } => expression_root_identifier(input),
        _ => None,
    }
}

/// Spec 071: flags a route whose body can finish without `reply`/`fail`, returning a silent 204.
/// The analysis is conservative and local to the route body (see the termination helpers): it never
/// looks inside `rescue` bodies, so a `fail` reachable only on an error-recovery path does not save
/// the happy path.
fn lint_route_without_response(
    path: &Path,
    statements: &[Statement],
    diagnostics: &mut Vec<LintDiagnostic>,
) {
    for stmt in statements {
        match stmt {
            Statement::Route {
                body, line, column, ..
            } => {
                if !block_terminates(body) {
                    diagnostics.push(LintDiagnostic::new(
                        path.to_path_buf(),
                        *line,
                        *column,
                        LintSeverity::Warning,
                        "route_without_response",
                        "route can finish without a reply or fail, returning a silent 204"
                            .to_string(),
                        Some(
                            "End every path with reply or fail, or suppress with \
                             `# marreta: allow route_without_response` if the empty 204 is intended."
                                .to_string(),
                        ),
                    ));
                }
            }
            Statement::Export(inner) => {
                lint_route_without_response(
                    path,
                    std::slice::from_ref(inner.as_ref()),
                    diagnostics,
                );
            }
            _ => {}
        }
    }
}

/// A block terminates if any statement in it is guaranteed to reach `reply`/`fail` (or an
/// unconditional `raise`, which is still a response, a 500).
fn block_terminates(statements: &[Statement]) -> bool {
    statements.iter().any(statement_terminates)
}

fn statement_terminates(stmt: &Statement) -> bool {
    match stmt {
        Statement::Reply { .. } | Statement::Fail { .. } => true,
        Statement::Raise {
            condition: None, ..
        } => true,
        Statement::Transaction { body, .. } => block_terminates(body),
        Statement::ExpressionStatement { expression, .. } => expression_terminates(expression),
        Statement::Assignment { value, .. } => expression_terminates(value),
        _ => false,
    }
}

/// An expression terminates only via `fail` in expression position (`__fail__`), an `if` with both
/// branches terminating, or a `match` with every arm plus a `fallback` terminating. It deliberately
/// does not recurse into pipelines or `rescue` bodies.
fn expression_terminates(expr: &Expression) -> bool {
    match expr {
        Expression::FunctionCall { name, .. } => name == "__fail__",
        Expression::If {
            then_branch,
            else_branch,
            ..
        } => {
            else_branch
                .as_ref()
                .is_some_and(|e| task_body_terminates(e))
                && task_body_terminates(then_branch)
        }
        Expression::Match { arms, .. } => {
            arms.iter()
                .any(|arm| matches!(arm.pattern, MatchPattern::Fallback))
                && arms.iter().all(|arm| expression_terminates(&arm.value))
        }
        _ => false,
    }
}

fn task_body_terminates(body: &TaskBody) -> bool {
    match body {
        TaskBody::Block(stmts, final_expr) => {
            block_terminates(stmts) || expression_terminates(final_expr)
        }
        TaskBody::Inline(expr) => expression_terminates(expr),
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
    fn every_catalogued_code_is_documented() {
        // Spec 071: the drift-proof guardrail. Each code must have a `### <code>` anchor on the lint
        // reference page, so a new rule cannot ship without its docs entry (and the editor's
        // codeDescription link, which points at that anchor, never 404s).
        let page = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/guide/reference/lint.md"
        ))
        .expect("lint reference page must exist");
        for r in catalog() {
            assert!(
                page.contains(&format!("### {}", r.code)),
                "lint code '{}' has no `### {}` anchor in docs/guide/reference/lint.md",
                r.code,
                r.code
            );
        }
    }

    #[test]
    fn catalog_codes_are_unique_and_described() {
        let mut seen = HashSet::new();
        for r in catalog() {
            assert!(
                seen.insert(r.code),
                "duplicate lint code in catalog: {}",
                r.code
            );
            assert!(
                !r.summary.is_empty(),
                "lint code {} has an empty summary",
                r.code
            );
            assert!(
                rule(r.code).is_some(),
                "rule() cannot resolve catalogued code {}",
                r.code
            );
        }
    }

    fn has_code(report: &LintReport, code: &str) -> bool {
        report.diagnostics.iter().any(|d| d.code == code)
    }

    #[test]
    fn unsuppressed_unused_variable_warns() {
        let report = lint_source("route GET \"/x\"\n    y = 1\n    reply 200, { ok: true }\n");
        assert!(has_code(&report, "unused_variable"));
    }

    #[test]
    fn standalone_allow_suppresses_the_next_line() {
        let report = lint_source(
            "route GET \"/x\"\n    # marreta: allow unused_variable\n    y = 1\n    reply 200, { ok: true }\n",
        );
        assert!(!has_code(&report, "unused_variable"));
    }

    #[test]
    fn trailing_allow_suppresses_its_own_line() {
        let report = lint_source(
            "route GET \"/x\"\n    y = 1  # marreta: allow unused_variable\n    reply 200, { ok: true }\n",
        );
        assert!(!has_code(&report, "unused_variable"));
    }

    #[test]
    fn allow_of_a_different_code_does_not_suppress() {
        let report = lint_source(
            "route GET \"/x\"\n    # marreta: allow duplicate_route\n    y = 1\n    reply 200, { ok: true }\n",
        );
        assert!(has_code(&report, "unused_variable"));
    }

    #[test]
    fn interpolation_is_not_a_suppression_directive() {
        // A `#{...}` interpolation must not be read as a directive.
        assert!(parse_allow_directive("    msg = \"hi #{name}\"").is_none());
        assert!(parse_allow_directive("    # marreta: allow unused_variable").is_some());
    }

    #[test]
    fn allow_directive_is_string_aware() {
        // A `#` inside a string literal must not hide the real comment directive after it.
        assert_eq!(
            parse_allow_directive("    y = \"a#b\"  # marreta: allow unused_variable"),
            Some(vec!["unused_variable".to_string()])
        );
        let report = lint_source(
            "route GET \"/x\"\n    y = \"a#b\"  # marreta: allow unused_variable\n    reply 200, \"ok\"\n",
        );
        assert!(!has_code(&report, "unused_variable"));
    }

    #[test]
    fn shadowing_params_is_flagged_in_any_route() {
        let report = lint_source("route GET \"/x\"\n    params = 1\n    reply 200, params\n");
        assert!(has_code(&report, "shadows_injected_binding"));
    }

    #[test]
    fn shadowing_a_take_binding_is_flagged() {
        let report = lint_source(
            "route POST \"/x\" take payload\n    payload = 1\n    reply 200, payload\n",
        );
        assert!(has_code(&report, "shadows_injected_binding"));
    }

    #[test]
    fn an_untaken_binding_name_is_not_a_shadow() {
        // No `take payload`, so `payload` is just a free local name here.
        let report = lint_source("route GET \"/x\"\n    payload = 1\n    reply 200, payload\n");
        assert!(!has_code(&report, "shadows_injected_binding"));
    }

    #[test]
    fn auth_shadow_only_when_the_route_has_auth() {
        let no_auth = lint_source("route GET \"/x\"\n    auth = 1\n    reply 200, auth\n");
        assert!(!has_code(&no_auth, "shadows_injected_binding"));
        let with_auth = lint_source(
            "route GET \"/x\"\n    require auth k\n    auth = 1\n    reply 200, auth\n",
        );
        assert!(has_code(&with_auth, "shadows_injected_binding"));
    }

    #[test]
    fn shadow_inside_a_transaction_shares_the_route_scope() {
        let report = lint_source(
            "route POST \"/x\" take payload\n    transaction\n        payload = 1\n    reply 200, payload\n",
        );
        assert!(has_code(&report, "shadows_injected_binding"));
    }

    #[test]
    fn injected_names_are_free_inside_a_task_body() {
        let report = lint_source("task helper()\n    params = 1\n    params\n");
        assert!(!has_code(&report, "shadows_injected_binding"));
    }

    #[test]
    fn route_ending_in_reply_is_clean() {
        let report = lint_source("route GET \"/x\"\n    reply 200, { ok: true }\n");
        assert!(!has_code(&report, "route_without_response"));
    }

    #[test]
    fn route_without_a_response_is_flagged() {
        let report = lint_source("route GET \"/x\"\n    y = compute()\n");
        assert!(has_code(&report, "route_without_response"));
    }

    #[test]
    fn route_with_if_else_both_responding_is_clean() {
        let report = lint_source(
            "route GET \"/x\"\n    if params.id\n        reply 200, \"a\"\n    else\n        reply 404, \"b\"\n",
        );
        assert!(!has_code(&report, "route_without_response"));
    }

    #[test]
    fn route_with_if_and_no_else_is_flagged() {
        let report = lint_source("route GET \"/x\"\n    if params.id\n        reply 200, \"a\"\n");
        assert!(has_code(&report, "route_without_response"));
    }

    #[test]
    fn fail_only_inside_rescue_still_flags_the_route() {
        // Deciding case: the only fail lives in a rescue block, so the happy path 204s.
        let report = lint_source(
            "route GET \"/x\"\n    result = doc.x.find(params.id) >> rescue\n        fail 503, \"down\"\n",
        );
        assert!(has_code(&report, "route_without_response"));
    }

    #[test]
    fn match_with_all_arms_and_fallback_terminating_is_clean() {
        // Deciding case: every arm plus fallback fails, so the match terminates the route.
        let report = lint_source(
            "route GET \"/x\"\n    match params.code\n        \"a\" -> fail 400, \"bad\"\n        fallback -> fail 404, \"nope\"\n",
        );
        assert!(!has_code(&report, "route_without_response"));
    }

    #[test]
    fn match_without_fallback_does_not_terminate_the_route() {
        let report = lint_source(
            "route GET \"/x\"\n    match params.code\n        \"a\" -> fail 400, \"bad\"\n        \"b\" -> fail 404, \"nope\"\n",
        );
        assert!(has_code(&report, "route_without_response"));
    }

    #[test]
    fn consumed_match_without_fallback_is_flagged() {
        let report = lint_source(
            "route GET \"/x\"\n    label = match params.code\n        \"a\" -> \"A\"\n        \"b\" -> \"B\"\n    reply 200, label\n",
        );
        assert!(has_code(&report, "match_without_fallback"));
    }

    #[test]
    fn consumed_match_with_fallback_is_clean() {
        let report = lint_source(
            "route GET \"/x\"\n    label = match params.code\n        \"a\" -> \"A\"\n        fallback -> \"?\"\n    reply 200, label\n",
        );
        assert!(!has_code(&report, "match_without_fallback"));
    }

    #[test]
    fn bare_effect_match_without_fallback_is_exempt() {
        // A match used as a statement discards its value, so a missing fallback is not a footgun.
        let report = lint_source(
            "route GET \"/x\"\n    match params.code\n        \"a\" -> log.info(\"a\")\n        \"b\" -> log.info(\"b\")\n    reply 200, \"ok\"\n",
        );
        assert!(!has_code(&report, "match_without_fallback"));
    }

    #[test]
    fn literal_order_by_is_clean() {
        let report = lint_source(
            "route GET \"/x\"\n    rows = db.items >> order_by(\"id asc\") >> fetch\n    reply 200, rows\n",
        );
        assert!(!has_code(&report, "non_literal_sql_identifier"));
    }

    #[test]
    fn runtime_order_by_is_flagged() {
        let report = lint_source(
            "route GET \"/x\"\n    rows = db.items >> order_by(query.sort) >> fetch\n    reply 200, rows\n",
        );
        assert!(has_code(&report, "non_literal_sql_identifier"));
    }

    #[test]
    fn interpolated_order_by_is_flagged() {
        let report = lint_source(
            "route GET \"/x\"\n    rows = db.items >> order_by(\"id #{query.dir}\") >> fetch\n    reply 200, rows\n",
        );
        assert!(has_code(&report, "non_literal_sql_identifier"));
    }

    #[test]
    fn doc_like_with_runtime_field_is_not_a_sql_identifier() {
        // like/in on a doc pipeline is a different engine, not SQL; do not flag it.
        let report = lint_source(
            "route GET \"/x\"\n    rows = doc.items >> like(query.field, \"x\") >> fetch_all\n    reply 200, rows\n",
        );
        assert!(!has_code(&report, "non_literal_sql_identifier"));
    }

    #[test]
    fn db_like_with_runtime_field_is_flagged() {
        let report = lint_source(
            "route GET \"/x\"\n    rows = db.items >> like(query.field, \"x\") >> fetch\n    reply 200, rows\n",
        );
        assert!(has_code(&report, "non_literal_sql_identifier"));
    }

    #[test]
    fn referenced_schema_is_clean() {
        let report = lint_source(
            "schema NewItem\n    name: string\n\nroute POST \"/x\" take payload as NewItem\n    reply 201, payload\n",
        );
        assert!(!has_code(&report, "unused_schema"));
    }

    #[test]
    fn unreferenced_schema_is_flagged() {
        let report = lint_source(
            "schema Orphan\n    name: string\n\nroute GET \"/x\"\n    reply 200, \"ok\"\n",
        );
        assert!(has_code(&report, "unused_schema"));
    }

    #[test]
    fn schema_referenced_in_a_conditional_reply_is_not_unused() {
        // A reference nested inside an `if` branch must still count, or a used schema is falsely
        // flagged (the omni_hub `OrderDetails` regression).
        let report = lint_source(
            "schema View\n    id: string\n\nroute GET \"/x\"\n    if params.id\n        reply 200 as View, { id: params.id }\n    reply 404, \"nope\"\n",
        );
        assert!(!has_code(&report, "unused_schema"));
    }

    #[test]
    fn unreferenced_persistent_schema_is_exempt() {
        let report = lint_source(
            "schema Account\n    db: accounts\n    owner: string\n\nroute GET \"/x\"\n    reply 200, \"ok\"\n",
        );
        assert!(!has_code(&report, "unused_schema"));
    }

    #[test]
    fn required_auth_provider_is_clean() {
        let report = lint_source(
            "auth api_key k {\n    header: \"x-api-key\"\n    secret_hash: env.H\n}\n\nroute GET \"/x\"\n    require auth k\n    reply 200, \"ok\"\n",
        );
        assert!(!has_code(&report, "unused_auth_provider"));
    }

    #[test]
    fn unrequired_auth_provider_is_flagged() {
        let report = lint_source(
            "auth api_key k {\n    header: \"x-api-key\"\n    secret_hash: env.H\n}\n\nroute GET \"/x\"\n    reply 200, \"ok\"\n",
        );
        assert!(has_code(&report, "unused_auth_provider"));
    }

    #[test]
    fn select_with_runtime_alias_is_flagged_but_columns_are_not() {
        let runtime_alias = lint_source(
            "route GET \"/x\"\n    rows = db.orders >> select(id, net: query.expr) >> fetch\n    reply 200, rows\n",
        );
        assert!(has_code(&runtime_alias, "non_literal_sql_identifier"));
        let literal_alias = lint_source(
            "route GET \"/x\"\n    rows = db.orders >> select(id, status, net: \"total * 0.9\") >> fetch\n    reply 200, rows\n",
        );
        assert!(!has_code(&literal_alias, "non_literal_sql_identifier"));
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
