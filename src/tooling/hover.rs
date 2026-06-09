use serde_json::{Value as JsonValue, json};

use crate::tooling::catalog::find_entry;
use crate::tooling::symbols::ToolingSymbol;

pub fn hover_json(
    source: &str,
    line: usize,
    column: usize,
    project_symbols: &[ToolingSymbol],
) -> JsonValue {
    hover(source, line, column, project_symbols).unwrap_or(JsonValue::Null)
}

pub fn hover(
    source: &str,
    line: usize,
    column: usize,
    project_symbols: &[ToolingSymbol],
) -> Option<JsonValue> {
    let line_text = source.lines().nth(line.checked_sub(1)?)?;
    let range = word_range(line_text, column)?;
    let word = &line_text[range.0..range.1];
    let dotted = dotted_name_at(line_text, range.0, range.1);

    if let Some(name) = dotted.as_deref()
        && let Some(entry) = find_entry(name)
    {
        return Some(catalog_hover(
            entry.signature,
            entry.summary,
            entry.example,
            line,
            range,
        ));
    }

    if let Some(entry) = find_entry(word) {
        return Some(catalog_hover(
            entry.signature,
            entry.summary,
            entry.example,
            line,
            range,
        ));
    }

    if let Some(symbol) = project_symbols.iter().find(|symbol| symbol.name == word) {
        return Some(symbol_hover(symbol, line, range));
    }

    None
}

fn catalog_hover(
    signature: &str,
    summary: &str,
    example: &str,
    line: usize,
    range: (usize, usize),
) -> JsonValue {
    let mut value = format!("### {signature}\n\n{summary}");
    if !example.is_empty() {
        value.push_str("\n\n```marreta\n");
        value.push_str(example);
        value.push_str("\n```");
    }
    json!({
        "contents": [{ "kind": "markdown", "value": value }],
        "range": range_json(line, range),
    })
}

fn symbol_hover(symbol: &ToolingSymbol, line: usize, range: (usize, usize)) -> JsonValue {
    json!({
        "contents": [{
            "kind": "markdown",
            "value": format!("### {}\n\nDefined in `{}:{}`.", symbol.detail, symbol.file, symbol.line)
        }],
        "range": range_json(line, range),
    })
}

fn range_json(line: usize, range: (usize, usize)) -> JsonValue {
    json!({
        "start": { "line": line, "column": range.0 + 1 },
        "end": { "line": line, "column": range.1 + 1 },
    })
}

fn dotted_name_at(line: &str, start: usize, end: usize) -> Option<String> {
    let bytes = line.as_bytes();
    if start >= 2 && bytes.get(start - 1) == Some(&b'.') {
        let ns_start = scan_ident_left(line, start - 1);
        return Some(format!(
            "{}.{}",
            &line[ns_start..start - 1],
            &line[start..end]
        ));
    }
    if bytes.get(end) == Some(&b'.') {
        let method_end = scan_ident_right(line, end + 1);
        if method_end > end + 1 {
            return Some(format!(
                "{}.{}",
                &line[start..end],
                &line[end + 1..method_end]
            ));
        }
    }
    None
}

fn word_range(line: &str, column: usize) -> Option<(usize, usize)> {
    let target = column.saturating_sub(1).min(line.len());
    let start = scan_ident_left(line, target);
    let end = scan_ident_right(line, target);
    if start == end {
        None
    } else {
        Some((start, end))
    }
}

fn scan_ident_left(line: &str, mut pos: usize) -> usize {
    pos = pos.min(line.len());
    while pos > 0 {
        let ch = line[..pos].chars().next_back().unwrap();
        if is_ident(ch) {
            pos -= ch.len_utf8();
        } else {
            break;
        }
    }
    pos
}

fn scan_ident_right(line: &str, mut pos: usize) -> usize {
    pos = pos.min(line.len());
    while pos < line.len() {
        let ch = line[pos..].chars().next().unwrap();
        if is_ident(ch) {
            pos += ch.len_utf8();
        } else {
            break;
        }
    }
    pos
}

fn is_ident(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hovers_builtin_operation() {
        let result = hover_json("value = cache.get(\"x\")", 1, 15, &[]);
        let text = result["contents"][0]["value"].as_str().unwrap();
        assert!(text.contains("cache.get"));
    }

    #[test]
    fn hovers_project_symbol() {
        let symbols = vec![ToolingSymbol {
            name: "greet".into(),
            kind: "task",
            file: "tasks/greetings.marreta".into(),
            line: 2,
            column: 1,
            detail: "task greet(name)".into(),
            exported: true,
        }];
        let result = hover_json("result = greet(\"Thiago\")", 1, 11, &symbols);
        let text = result["contents"][0]["value"].as_str().unwrap();
        assert!(text.contains("task greet"));
    }

    #[test]
    fn unknown_hover_returns_null() {
        assert_eq!(hover_json("x = 1", 1, 1, &[]), JsonValue::Null);
    }
}
