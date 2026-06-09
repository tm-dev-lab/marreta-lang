//! Go-to-definition for the editor's Definition provider (Spec 059, §3.3).
//!
//! Resolves the **token at the cursor** using the lexer position plus the
//! surrounding tokens to classify the reference, then looks the name up in the
//! project/buffer symbol table. It is not AST-only (the AST does not carry a precise
//! column span on every identifier) and not a name-only guess: a name only resolves
//! when it sits in a recognized reference site. Anything else returns null.
//!
//! v1 reference sites:
//!   - task: a call `name(...)`, a pipeline stage (`>> name`), a broadcast target,
//!     or a `-> name` arm of a broadcast / pipeline `map`.
//!   - schema: a name after `as` (take/reply/http_client/param), a constructor
//!     `Name { ... }`, or a nested schema field type (`field: OtherSchema`, only
//!     inside a schema block — not a map value like `{ value: OtherSchema }`).
//!   - auth: the provider name in `require auth <provider>`.

use serde_json::{Value as JsonValue, json};

use crate::lexer::Lexer;
use crate::token::{Token, TokenKind};

use super::symbols::ToolingSymbol;

/// Resolves the definition at `line`/`column` (1-based) against the given symbols,
/// returning `{ file, line, column, kind }` or null.
pub fn definition_json(
    source: &str,
    line: usize,
    column: usize,
    symbols: &[ToolingSymbol],
) -> JsonValue {
    match resolve(source, line, column, symbols) {
        Some(symbol) => json!({
            "file": symbol.file,
            "line": symbol.line,
            "column": symbol.column,
            "kind": symbol.kind,
        }),
        None => JsonValue::Null,
    }
}

fn resolve<'a>(
    source: &str,
    line: usize,
    column: usize,
    symbols: &'a [ToolingSymbol],
) -> Option<&'a ToolingSymbol> {
    let tokens = Lexer::new(source).tokenize().ok()?;
    let idx = token_at(&tokens, line, column)?;
    let name = match &tokens[idx].kind {
        TokenKind::Identifier(name) => name.as_str(),
        _ => return None,
    };
    let kind = classify(&tokens, idx, in_schema_block(source, line))?;
    // Spec 061: when the reference is `ns.task`, resolve to that file-namespace's
    // exported task so same-named tasks in different files disambiguate correctly.
    if kind == "task"
        && let Some(ns) = preceding_namespace(&tokens, idx)
        && let Some(symbol) = symbols.iter().find(|symbol| {
            symbol.kind == "task"
                && symbol.exported
                && symbol.name == name
                && symbol.namespace() == ns
        })
    {
        return Some(symbol);
    }
    symbols
        .iter()
        .find(|symbol| symbol.kind == kind && symbol.name == name)
}

/// When the identifier at `idx` is the `task` in `ns.task`, returns `ns`.
fn preceding_namespace(tokens: &[Token], idx: usize) -> Option<&str> {
    let dot = prev_significant_idx(tokens, idx)?;
    if !matches!(tokens[dot].kind, TokenKind::Dot) {
        return None;
    }
    let ns = prev_significant_idx(tokens, dot)?;
    match &tokens[ns].kind {
        TokenKind::Identifier(name) => Some(name.as_str()),
        _ => None,
    }
}

fn prev_significant_idx(tokens: &[Token], idx: usize) -> Option<usize> {
    tokens[..idx].iter().rposition(|t| !is_layout(&t.kind))
}

/// Whether `line` (1-based) sits inside a `schema` declaration block, i.e. its
/// nearest less-indented ancestor line is a `schema` header. Used to accept a
/// `field: OtherSchema` type only inside a schema, never a map value like
/// `reply 200, { value: OtherSchema }`.
fn in_schema_block(source: &str, line: usize) -> bool {
    let lines: Vec<&str> = source.lines().collect();
    if line == 0 || line > lines.len() {
        return false;
    }
    let current_indent = indent_of(lines[line - 1]);
    for prev in lines[..line - 1].iter().rev() {
        let trimmed = prev.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if indent_of(prev) < current_indent {
            return trimmed.starts_with("schema ") || trimmed.starts_with("export schema ");
        }
    }
    false
}

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|ch| *ch == ' ').count()
}

/// Index of the identifier token whose span contains the 1-based cursor position.
fn token_at(tokens: &[Token], line: usize, column: usize) -> Option<usize> {
    tokens.iter().position(|token| {
        matches!(token.kind, TokenKind::Identifier(_))
            && token.line == line
            && column >= token.column
            && column <= token.column + token.lexeme.chars().count()
    })
}

/// Classifies the reference at `idx` into the kind of symbol it points at, or `None`
/// when the token is not in a recognized reference site. `in_schema` gates the
/// `field: Type` case so a `:` only resolves to a schema inside a schema block.
fn classify(tokens: &[Token], idx: usize, in_schema: bool) -> Option<&'static str> {
    if let Some(next) = next_significant(tokens, idx) {
        match next.kind {
            TokenKind::LeftParen => return Some("task"),
            // `Name { ... }` constructor; resolves only if a schema by that name
            // exists (the caller filters by kind), so declaration names like
            // `auth jwt foo {` simply find nothing.
            TokenKind::LeftBrace => return Some("schema"),
            _ => {}
        }
    }
    let prev = prev_significant(tokens, idx)?;
    match &prev.kind {
        // Pipeline stage (`>> task`), broadcast first target, or a `-> task` arm of a
        // pipeline `map`/broadcast.
        TokenKind::Pipeline | TokenKind::Broadcast | TokenKind::Arrow => Some("task"),
        TokenKind::As => Some("schema"),
        // A nested schema field type (`field: OtherSchema`) — only inside a schema
        // block, so a map value like `reply 200, { value: Address }` does not match.
        TokenKind::Colon if in_schema => Some("schema"),
        // `require auth <provider>`: the token before the name is `auth`.
        _ if prev.lexeme == "auth" => Some("auth"),
        _ => None,
    }
}

fn next_significant(tokens: &[Token], idx: usize) -> Option<&Token> {
    tokens[idx + 1..].iter().find(|t| !is_layout(&t.kind))
}

fn prev_significant(tokens: &[Token], idx: usize) -> Option<&Token> {
    tokens[..idx].iter().rev().find(|t| !is_layout(&t.kind))
}

fn is_layout(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(
        name: &str,
        kind: &'static str,
        file: &str,
        line: usize,
        column: usize,
    ) -> ToolingSymbol {
        ToolingSymbol {
            name: name.to_string(),
            kind,
            file: file.to_string(),
            line,
            column,
            detail: String::new(),
            exported: false,
        }
    }

    fn symbols() -> Vec<ToolingSymbol> {
        vec![
            sym("square", "task", "tasks.marreta", 2, 1),
            sym("NewUser", "schema", "schemas.marreta", 1, 8),
            sym("e2e_key", "auth", "auth.marreta", 1, 1),
        ]
    }

    /// 1-based column of the first occurrence of `needle` on the given 1-based line.
    fn col(src: &str, line: usize, needle: &str) -> usize {
        src.lines().nth(line - 1).unwrap().find(needle).unwrap() + 1
    }

    fn def_kind(src: &str, line: usize, needle: &str) -> JsonValue {
        definition_json(src, line, col(src, line, needle), &symbols())
    }

    #[test]
    fn resolves_task_call() {
        let src = "route GET \"/x\"\n    reply 200, { v: square(1) }\n";
        let j = def_kind(src, 2, "square");
        assert_eq!(j["kind"], "task");
        assert_eq!(j["file"], "tasks.marreta");
    }

    #[test]
    fn resolves_pipeline_stage_task() {
        let src = "route GET \"/x\"\n    y = x >> square\n    reply 200, { y: y }\n";
        assert_eq!(def_kind(src, 2, "square")["kind"], "task");
    }

    #[test]
    fn resolves_namespaced_task_to_its_file() {
        // Spec 061: `billing.charge` and `payments.charge` are different tasks; the
        // qualified call must resolve to the matching file-namespace.
        let symbols = vec![
            ToolingSymbol {
                name: "charge".into(),
                kind: "task",
                file: "tasks/billing.marreta".into(),
                line: 1,
                column: 1,
                detail: String::new(),
                exported: true,
            },
            ToolingSymbol {
                name: "charge".into(),
                kind: "task",
                file: "tasks/payments.marreta".into(),
                line: 1,
                column: 1,
                detail: String::new(),
                exported: true,
            },
        ];
        let src = "route GET \"/x\"\n    reply 200, { v: payments.charge(1) }\n";
        let column = col(src, 2, "charge");
        let j = definition_json(src, 2, column, &symbols);
        assert_eq!(j["kind"], "task");
        assert_eq!(j["file"], "tasks/payments.marreta");
    }

    #[test]
    fn resolves_schema_after_as() {
        let src = "route POST \"/x\" take payload as NewUser\n    reply 201, { ok: true }\n";
        let j = def_kind(src, 1, "NewUser");
        assert_eq!(j["kind"], "schema");
        assert_eq!(j["file"], "schemas.marreta");
    }

    #[test]
    fn resolves_auth_provider_in_require() {
        let src = "route GET \"/x\"\n    require auth e2e_key\n    reply 200, { ok: true }\n";
        let j = def_kind(src, 2, "e2e_key");
        assert_eq!(j["kind"], "auth");
        assert_eq!(j["file"], "auth.marreta");
    }

    #[test]
    fn returns_null_for_non_reference_identifier() {
        // A plain variable read is not a reference site.
        let src = "route GET \"/x\"\n    y = x\n    reply 200, { y: y }\n";
        assert_eq!(def_kind(src, 2, "x"), JsonValue::Null);
    }

    #[test]
    fn returns_null_for_unknown_name() {
        let src = "route GET \"/x\"\n    reply 200, { v: nope(1) }\n";
        assert_eq!(def_kind(src, 2, "nope"), JsonValue::Null);
    }

    #[test]
    fn resolves_broadcast_arrow_target_task() {
        let src = "route POST \"/x\" take payload\n    r = payload *>>\n        -> square\n";
        assert_eq!(def_kind(src, 3, "square")["kind"], "task");
    }

    #[test]
    fn resolves_nested_schema_field_type() {
        let src = "schema Order\n    customer: NewUser\n";
        let j = def_kind(src, 2, "NewUser");
        assert_eq!(j["kind"], "schema");
        assert_eq!(j["file"], "schemas.marreta");
    }

    #[test]
    fn returns_null_for_non_schema_value_after_colon() {
        // A map value after `:` that is not a schema resolves to nothing (the lookup
        // filters by kind), so this is not a name-only guess.
        let src = "route GET \"/x\"\n    reply 200, { v: square }\n";
        assert_eq!(def_kind(src, 2, "square"), JsonValue::Null);
    }

    #[test]
    fn returns_null_for_schema_name_as_map_value_outside_schema_block() {
        // `NewUser` IS a schema, but here it is a map value in a reply, not a
        // `field: NewUser` inside a schema declaration — must not resolve.
        let src = "route GET \"/x\"\n    reply 200, { value: NewUser }\n";
        assert_eq!(def_kind(src, 2, "NewUser"), JsonValue::Null);
    }
}
