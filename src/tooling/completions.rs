use serde_json::{Value as JsonValue, json};

use crate::tooling::catalog::{CatalogEntry, CatalogKind, catalog, entries_for_namespace};
use crate::tooling::symbols::ToolingSymbol;

pub fn completions_json(
    source: &str,
    line: usize,
    column: usize,
    project_symbols: &[ToolingSymbol],
) -> JsonValue {
    JsonValue::Array(
        completions(source, line, column, project_symbols)
            .into_iter()
            .collect(),
    )
}

pub fn completions(
    source: &str,
    line: usize,
    column: usize,
    project_symbols: &[ToolingSymbol],
) -> Vec<JsonValue> {
    let prefix = line_prefix(source, line, column);
    let trimmed = prefix.trim_end();

    // db/doc are table/collection-driven (`db.TABLE.operation`), so the catalog has
    // no fixed members for `db.`. After a table/collection (`db.users.`), offer the
    // store operations.
    if is_db_doc_member_position(trimmed) {
        return DB_DOC_OPERATIONS
            .iter()
            .map(|op| db_doc_operation_completion(op))
            .collect();
    }

    if let Some(namespace) = dotted_namespace(trimmed) {
        let entries = entries_for_namespace(namespace);
        if !entries.is_empty() {
            return entries
                .into_iter()
                .map(completion_from_catalog_entry)
                .collect();
        }
        // Spec 061 file-namespace: after `file.`, offer that file's exported tasks
        // (only exported — private tasks are not reachable cross-file).
        let tasks: Vec<JsonValue> = project_symbols
            .iter()
            .filter(|symbol| {
                symbol.kind == "task" && symbol.exported && symbol.namespace() == namespace
            })
            .map(completion_from_symbol)
            .collect();
        if !tasks.is_empty() {
            return tasks;
        }
    }

    if wants_schema_completion(trimmed) {
        return project_symbols
            .iter()
            .filter(|symbol| symbol.kind == "schema")
            .map(completion_from_symbol)
            .collect();
    }

    if is_statement_position(trimmed) || trimmed.is_empty() {
        let mut items = catalog()
            .iter()
            .filter(|entry| matches!(entry.kind, CatalogKind::Keyword | CatalogKind::Namespace))
            .map(completion_from_catalog_entry)
            .collect::<Vec<_>>();
        items.extend(
            project_symbols
                .iter()
                .filter(|symbol| symbol.kind == "task")
                .map(completion_from_symbol),
        );
        return items;
    }

    if let Some(partial) = trailing_identifier(trimmed)
        && !partial.is_empty()
    {
        let mut items = project_symbols
            .iter()
            .filter(|symbol| {
                matches!(symbol.kind, "task" | "schema") && symbol.name.starts_with(partial)
            })
            .map(completion_from_symbol)
            .collect::<Vec<_>>();
        items.extend(
            catalog()
                .iter()
                .filter(|entry| entry.name.starts_with(partial))
                .map(completion_from_catalog_entry),
        );
        return items;
    }

    Vec::new()
}

fn completion_from_catalog_entry(entry: &CatalogEntry) -> JsonValue {
    json!({
        "label": entry.completion_label(),
        "kind": entry.kind.as_str(),
        "detail": entry.signature,
        "documentation": entry.summary,
        "insert_text": entry.insert_text,
        "source": if entry.kind == CatalogKind::Keyword { "keyword" } else { "builtin" },
        "sort_text": sort_text(entry),
    })
}

fn completion_from_symbol(symbol: &ToolingSymbol) -> JsonValue {
    json!({
        "label": symbol.name,
        "kind": symbol.kind,
        "detail": symbol.detail,
        "documentation": format!("Defined in {}:{}.", symbol.file, symbol.line),
        "insert_text": symbol_insert_text(symbol),
        "source": "project",
    })
}

fn symbol_insert_text(symbol: &ToolingSymbol) -> String {
    if symbol.kind == "task" {
        format!("{}(${{1:arg}})", symbol.name)
    } else {
        symbol.name.clone()
    }
}

fn sort_text(entry: &CatalogEntry) -> String {
    let prefix = match entry.kind {
        CatalogKind::Keyword => "010",
        CatalogKind::Namespace => "020",
        CatalogKind::Function => "030",
        CatalogKind::Method => "040",
    };
    format!("{prefix}_{}", entry.name)
}

fn dotted_namespace(trimmed: &str) -> Option<&str> {
    let before_dot = trimmed.strip_suffix('.')?;
    trailing_identifier(before_dot)
}

/// Operations supported by `db.TABLE.op(...)` and `doc.COLLECTION.op(...)`.
const DB_DOC_OPERATIONS: &[&str] = &["save", "find", "find_all", "update", "delete"];

/// True when the cursor is right after `db.<table>.` or `doc.<collection>.`.
fn is_db_doc_member_position(trimmed: &str) -> bool {
    let Some(before_dot) = trimmed.strip_suffix('.') else {
        return false;
    };
    let Some(table) = trailing_identifier(before_dot) else {
        return false;
    };
    let before_table = before_dot[..before_dot.len() - table.len()].trim_end();
    before_table.ends_with("db.") || before_table.ends_with("doc.")
}

fn db_doc_operation_completion(op: &str) -> JsonValue {
    let insert_text = match op {
        "save" => "save(${1:value})".to_string(),
        "find" | "delete" => format!("{op}(${{1:id}})"),
        "find_all" => "find_all()".to_string(),
        "update" => "update(${1:id}, ${2:changes})".to_string(),
        _ => format!("{op}()"),
    };
    json!({
        "label": op,
        "kind": "method",
        "detail": "db/doc operation",
        "documentation": "Relational/document store operation.",
        "insert_text": insert_text,
        "source": "builtin",
        "sort_text": format!("040_{op}"),
    })
}

fn wants_schema_completion(trimmed: &str) -> bool {
    trimmed.ends_with(" as")
        || trimmed.ends_with(" as ")
        || trimmed.ends_with("take payload as")
        || trimmed.ends_with("take form as")
        || trimmed.ends_with("take raw as")
        || trimmed.ends_with("reply 200 as")
        || trimmed.contains(" as ")
}

fn is_statement_position(trimmed: &str) -> bool {
    let lower = trimmed.trim_start();
    lower.is_empty() || lower == "require" || lower == "reject" || lower == "if" || lower == "else"
}

fn trailing_identifier(input: &str) -> Option<&str> {
    let end = input.len();
    let start = input
        .char_indices()
        .rev()
        .find_map(|(idx, ch)| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                None
            } else {
                Some(idx + ch.len_utf8())
            }
        })
        .unwrap_or(0);
    let ident = &input[start..end];
    if ident
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
    {
        Some(ident)
    } else {
        None
    }
}

fn line_prefix(source: &str, line: usize, column: usize) -> String {
    let Some(line_text) = source.lines().nth(line.saturating_sub(1)) else {
        return String::new();
    };
    line_text
        .chars()
        .take(column.saturating_sub(1))
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completes_namespace_methods_after_dot() {
        let items = completions("value = cache.", 1, 15, &[]);
        assert!(
            items
                .iter()
                .any(|item| item["label"] == "get" && item["source"] == "builtin")
        );
    }

    #[test]
    fn completes_schema_names_after_as() {
        let symbols = vec![ToolingSymbol {
            name: "Greeting".into(),
            kind: "schema",
            file: "schemas/greetings.marreta".into(),
            line: 1,
            column: 1,
            detail: "schema Greeting".into(),
            exported: true,
        }];
        let items = completions("route POST \"/x\" take payload as ", 1, 34, &symbols);
        assert!(items.iter().any(|item| item["label"] == "Greeting"));
    }

    #[test]
    fn completes_exported_tasks_after_file_namespace() {
        let symbols = vec![
            ToolingSymbol {
                name: "charge".into(),
                kind: "task",
                file: "tasks/billing.marreta".into(),
                line: 1,
                column: 1,
                detail: "task charge(order)".into(),
                exported: true,
            },
            ToolingSymbol {
                name: "secret".into(),
                kind: "task",
                file: "tasks/billing.marreta".into(),
                line: 5,
                column: 1,
                detail: "task secret(x)".into(),
                exported: false,
            },
        ];
        let items = completions("    total = billing.", 1, 21, &symbols);
        assert!(
            items.iter().any(|item| item["label"] == "charge"),
            "exported task should be offered after the file-namespace"
        );
        assert!(
            !items.iter().any(|item| item["label"] == "secret"),
            "private task must not be offered cross-file"
        );
    }

    #[test]
    fn unknown_context_returns_empty_list() {
        let items = completions("1 + ", 1, 5, &[]);
        assert!(items.is_empty());
    }

    #[test]
    fn completes_db_operations_after_table() {
        let items = completions("    item = db.users.", 1, 21, &[]);
        let labels: Vec<_> = items.iter().map(|item| item["label"].clone()).collect();
        for op in ["save", "find", "find_all", "update", "delete"] {
            assert!(labels.iter().any(|l| l == op), "missing db op '{op}'");
        }
    }

    #[test]
    fn completes_doc_operations_after_collection() {
        let items = completions("    e = doc.events.", 1, 20, &[]);
        assert!(items.iter().any(|item| item["label"] == "save"));
    }

    #[test]
    fn does_not_complete_operations_directly_after_db() {
        // `db.` is a table position (dynamic name), not an operation position.
        let items = completions("    x = db.", 1, 12, &[]);
        assert!(!items.iter().any(|item| item["label"] == "find"));
    }
}
