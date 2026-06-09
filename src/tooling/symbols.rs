use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value as JsonValue, json};

use crate::ast::{AuthProvider, Expression, Statement, TaskBody};
use crate::error::MarretaError;
use crate::lexer::Lexer;
use crate::parser::Parser;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolingSymbol {
    pub name: String,
    pub kind: &'static str,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub detail: String,
    /// True when the declaration is `export`ed (Spec 061): an exported task is the
    /// project's cross-file surface, reached as `file.task`.
    pub exported: bool,
}

impl ToolingSymbol {
    /// The file-namespace this symbol belongs to (its file stem, e.g.
    /// `tasks/billing.marreta` -> `billing`). Spec 061: exported tasks are reached
    /// cross-file as `namespace.task`.
    pub fn namespace(&self) -> &str {
        let stem = self.file.rsplit('/').next().unwrap_or(&self.file);
        stem.strip_suffix(".marreta").unwrap_or(stem)
    }

    pub fn to_json(&self) -> JsonValue {
        json!({
            "name": self.name,
            "kind": self.kind,
            "file": self.file,
            "line": self.line,
            "column": self.column,
            "detail": self.detail,
            "exported": self.exported,
            "namespace": self.namespace(),
        })
    }
}

pub fn symbols_json(root: &Path) -> Result<JsonValue, MarretaError> {
    let symbols = collect_project_symbols(root)?;
    Ok(JsonValue::Array(
        symbols.iter().map(ToolingSymbol::to_json).collect(),
    ))
}

pub fn collect_project_symbols(root: &Path) -> Result<Vec<ToolingSymbol>, MarretaError> {
    let mut files = Vec::new();
    collect_marreta_files(root, &mut files).map_err(|err| MarretaError::IoError {
        message: format!("failed to read '{}': {err}", root.display()),
    })?;
    files.sort();

    let mut out = Vec::new();
    for file in files {
        let source = fs::read_to_string(&file).map_err(|err| MarretaError::IoError {
            message: format!("cannot read '{}': {err}", file.display()),
        })?;
        let program = parse_source(&source)?;
        let rel = display_path(file.strip_prefix(root).unwrap_or(&file));
        collect_statement_symbols(&program, &rel, false, &mut out);
    }
    out.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.line.cmp(&b.line))
            .then(a.column.cmp(&b.column))
            .then(a.name.cmp(&b.name))
    });
    Ok(out)
}

pub fn parse_source(source: &str) -> Result<Vec<Statement>, MarretaError> {
    let tokens = Lexer::new(source).tokenize()?;
    Parser::new(tokens).parse()
}

/// Collects symbols from a single in-memory source (no project on disk). Used for
/// project-less tooling over `--stdin` and to resolve same-file references from the
/// current buffer.
pub fn collect_source_symbols(source: &str, file: &str) -> Vec<ToolingSymbol> {
    let Ok(program) = parse_source(source) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_statement_symbols(&program, file, false, &mut out);
    out
}

/// JSON symbols for a single in-memory source — the project-less fallback for
/// `marreta tooling symbols --stdin`.
pub fn symbols_json_from_source(source: &str, file: &str) -> JsonValue {
    JsonValue::Array(
        collect_source_symbols(source, file)
            .iter()
            .map(ToolingSymbol::to_json)
            .collect(),
    )
}

fn collect_marreta_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), std::io::Error> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if should_skip_dir(&path) {
                continue;
            }
            collect_marreta_files(&path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("marreta") {
            out.push(path);
        }
    }
    Ok(())
}

fn should_skip_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(".git" | "target")
    )
}

fn collect_statement_symbols(
    program: &[Statement],
    file: &str,
    exported: bool,
    out: &mut Vec<ToolingSymbol>,
) {
    for stmt in program {
        match stmt {
            // `export <decl>` marks only the immediate declaration as exported;
            // anything nested inside a body stays file-private.
            Statement::Export(inner) => {
                collect_statement_symbols(std::slice::from_ref(inner.as_ref()), file, true, out)
            }
            Statement::TaskDef {
                name,
                params,
                body,
                line,
                column,
            } => {
                let params = params
                    .iter()
                    .map(|param| param.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push(ToolingSymbol {
                    name: name.clone(),
                    kind: "task",
                    file: file.to_string(),
                    line: *line,
                    column: *column,
                    detail: format!("task {name}({params})"),
                    exported,
                });
                if let TaskBody::Block(stmts, _) = body {
                    collect_statement_symbols(stmts, file, false, out);
                }
            }
            Statement::Schema {
                name, line, column, ..
            } => out.push(ToolingSymbol {
                name: name.clone(),
                kind: "schema",
                file: file.to_string(),
                line: *line,
                column: *column,
                detail: format!("schema {name}"),
                exported,
            }),
            Statement::AuthProvider {
                provider,
                line,
                column,
            } => {
                let kind_label = match provider {
                    AuthProvider::Jwt(_) => "jwt",
                    AuthProvider::ApiKey(_) => "api_key",
                };
                out.push(ToolingSymbol {
                    name: provider.name().to_string(),
                    kind: "auth",
                    file: file.to_string(),
                    line: *line,
                    column: *column,
                    detail: format!("auth {kind_label} {}", provider.name()),
                    exported: false,
                });
            }
            Statement::Route {
                verb,
                path,
                body,
                line,
                column,
                ..
            } => {
                let name = format!("{verb} {path}");
                out.push(ToolingSymbol {
                    name,
                    kind: "route",
                    file: file.to_string(),
                    line: *line,
                    column: *column,
                    detail: format!("route {verb} \"{path}\""),
                    exported: false,
                });
                collect_statement_symbols(body, file, false, out);
            }
            Statement::OnQueue {
                queue_name,
                body,
                line,
                column,
                ..
            } => {
                let target = literal_label(queue_name).unwrap_or_else(|| "<dynamic>".to_string());
                out.push(ToolingSymbol {
                    name: target.clone(),
                    kind: "consumer",
                    file: file.to_string(),
                    line: *line,
                    column: *column,
                    detail: format!("on queue \"{target}\""),
                    exported: false,
                });
                collect_statement_symbols(body, file, false, out);
            }
            Statement::OnTopic {
                pattern,
                body,
                line,
                column,
                ..
            } => {
                let target = literal_label(pattern).unwrap_or_else(|| "<dynamic>".to_string());
                out.push(ToolingSymbol {
                    name: target.clone(),
                    kind: "consumer",
                    file: file.to_string(),
                    line: *line,
                    column: *column,
                    detail: format!("on topic \"{target}\""),
                    exported: false,
                });
                collect_statement_symbols(body, file, false, out);
            }
            Statement::Scenario {
                name, line, column, ..
            } => out.push(ToolingSymbol {
                name: name.clone(),
                kind: "scenario",
                file: file.to_string(),
                line: *line,
                column: *column,
                detail: format!("scenario \"{name}\""),
                exported: false,
            }),
            Statement::Transaction { body, .. } | Statement::While { body, .. } => {
                collect_statement_symbols(body, file, false, out);
            }
            _ => {}
        }
    }
}

fn literal_label(expr: &Expression) -> Option<String> {
    match expr {
        Expression::StringLiteral(value) => Some(value.clone()),
        _ => None,
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn collects_project_symbols_without_executing_runtime() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            r#"project_name = "demo"
project_version = "0.1.0"

route GET "/greetings"
    reply 200, { ok: true }
"#,
        );
        write(
            dir.path(),
            "tasks/greetings.marreta",
            r#"export task greet(name)
    "Hello " + name

schema Greeting
    message: string

on queue "jobs" take job
    log.info(job)
"#,
        );

        let symbols = collect_project_symbols(dir.path()).unwrap();
        assert!(
            symbols
                .iter()
                .any(|s| s.kind == "route" && s.name == "GET /greetings")
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.kind == "task" && s.name == "greet")
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.kind == "schema" && s.name == "Greeting")
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.kind == "consumer" && s.detail == "on queue \"jobs\"")
        );
    }

    #[test]
    fn emits_auth_provider_symbols() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "app.marreta",
            "project_name = \"demo\"\nproject_version = \"0.1.0\"\n",
        );
        write(
            dir.path(),
            "routes/auth.marreta",
            "auth api_key k {\n    header: \"x-api-key\"\n}\n\nauth jwt j {\n    issuer: \"i\"\n}\n",
        );

        let symbols = collect_project_symbols(dir.path()).unwrap();
        assert!(
            symbols
                .iter()
                .any(|s| s.kind == "auth" && s.name == "k" && s.detail == "auth api_key k"),
            "api_key auth provider should be an auth symbol"
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.kind == "auth" && s.name == "j" && s.detail == "auth jwt j"),
            "jwt auth provider should be an auth symbol"
        );
    }
}
