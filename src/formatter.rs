use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::MarretaError;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::token::TokenKind;

#[derive(Debug)]
pub enum FormatError {
    Parse(MarretaError),
    Io {
        path: PathBuf,
        source: io::Error,
    },
    MissingProjectRoot(PathBuf),
    MissingFileArgument,
    InvalidPath(PathBuf),
    /// Formatting changed the program's meaning (its significant token stream). This should
    /// never happen for a correct formatter; it is surfaced rather than written out.
    AstDivergence,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::Parse(err) => write!(f, "{}", err),
            FormatError::Io { path, source } => write!(f, "{}: {}", path.display(), source),
            FormatError::MissingProjectRoot(path) => write!(
                f,
                "no app.marreta found in {}; run marreta fmt from a project root or pass explicit paths",
                path.display()
            ),
            FormatError::MissingFileArgument => {
                write!(f, "--stdin requires --file <path>")
            }
            FormatError::InvalidPath(path) => write!(f, "invalid format path: {}", path.display()),
            FormatError::AstDivergence => write!(
                f,
                "internal formatter error: formatting changed the program's token stream"
            ),
        }
    }
}

impl std::error::Error for FormatError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatResult {
    pub changed: bool,
    pub output: String,
}

pub fn format_source(source: &str) -> Result<FormatResult, FormatError> {
    parse_source(source)?;
    let before = significant_tokens(source)?;
    let formatted = normalize_source(source);
    parse_source(&formatted)?;
    let after = significant_tokens(&formatted)?;
    // The AST carries source positions (line/column), so comparing parsed `Program`s would
    // always differ after reindenting. The meaning-preserving invariant is instead the full
    // token stream including the layout tokens Indent/Dedent/Newline (block structure and
    // statement separation are semantic in Marreta); only the Eof sentinel is dropped.
    // Formatting changes whitespace, not tokens, so this must be identical before and after.
    if before != after {
        return Err(FormatError::AstDivergence);
    }
    Ok(FormatResult {
        changed: formatted != source,
        output: formatted,
    })
}

/// The meaning-bearing token stream: the full token sequence including the semantic layout
/// tokens (Indent, Dedent, Newline — block structure and statement separation), with the Eof
/// sentinel and the file-terminal Newline dropped.
///
/// Spec 072 (2.3): the lexer collapses interior consecutive newlines, but the file-terminal
/// newline still differs between an input with a final `\n` and one without, and at the end of an
/// indented file it sits *behind* the synthesized block-closing Dedents: `... Newline Dedent* Eof`
/// (with `\n`) versus `... Dedent* Eof` (without). That terminal Newline separates nothing (no
/// statement follows it, and a Dedent is a synthesized block close, not a statement), so it is not
/// meaning-bearing. We walk back over the terminal Dedent run and drop the single Newline behind
/// it. This lets the formatter normalize the final newline (add one when missing, keep exactly
/// one) without tripping the divergence guard, while every interior Newline and every Dedent stays
/// snapshotted and protected.
fn significant_tokens(source: &str) -> Result<Vec<TokenKind>, FormatError> {
    let tokens = Lexer::new(source).tokenize().map_err(FormatError::Parse)?;
    let mut kinds: Vec<TokenKind> = tokens
        .into_iter()
        .map(|token| token.kind)
        .filter(|kind| !matches!(kind, TokenKind::Eof))
        .collect();
    let mut end = kinds.len();
    while end > 0 && matches!(kinds[end - 1], TokenKind::Dedent) {
        end -= 1;
    }
    if end > 0 && matches!(kinds[end - 1], TokenKind::Newline) {
        kinds.remove(end - 1);
    }
    Ok(kinds)
}

pub fn discover_project_files(root: &Path) -> Result<Vec<PathBuf>, FormatError> {
    let app = root.join("app.marreta");
    if !app.is_file() {
        return Err(FormatError::MissingProjectRoot(root.to_path_buf()));
    }

    // Spec 072: share the loader's recursive discovery so `marreta fmt` formats exactly what the
    // runtime loads. The loader walks the whole root and excludes the entrypoint (it parses
    // `app.marreta` separately); fmt formats it too, so we append it. The old four-directory list
    // (routes/schemas/tasks/tests) silently skipped files in custom folders like `auth/`.
    let mut files = crate::file_loader::collect_marreta_files(root, &app);
    files.push(app);
    files.sort();
    Ok(files)
}

pub fn discover_explicit_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, FormatError> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_file() {
            if is_marreta_file(path) {
                files.push(path.clone());
            }
        } else if path.is_dir() {
            collect_marreta_files(path, &mut files)?;
        } else {
            return Err(FormatError::InvalidPath(path.clone()));
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

pub fn format_file(path: &Path, check: bool) -> Result<bool, FormatError> {
    let source = fs::read_to_string(path).map_err(|source| FormatError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let result = format_source(&source)?;
    if result.changed && !check {
        fs::write(path, result.output).map_err(|source| FormatError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    }
    Ok(result.changed)
}

fn parse_source(source: &str) -> Result<(), FormatError> {
    let tokens = Lexer::new(source).tokenize().map_err(FormatError::Parse)?;
    Parser::new(tokens).parse().map_err(FormatError::Parse)?;
    Ok(())
}

fn normalize_source(source: &str) -> String {
    let mut stack = vec![0usize];
    let mut lines = Vec::new();

    for raw_line in source.lines() {
        let line = raw_line.trim_end();
        if line.trim().is_empty() {
            lines.push(FormattedLine::blank());
            continue;
        }

        let raw_indent = leading_indent_width(line);
        let stripped = line.trim_start_matches([' ', '\t']);
        let is_comment = stripped.starts_with('#');
        let depth = if is_comment {
            depth_for_comment(raw_indent, &stack)
        } else {
            depth_for_code(raw_indent, &mut stack)
        };

        lines.push(FormattedLine::new(depth, stripped));
    }

    // Spec 072 (2.2-2.4): collapse runs of 2+ blank lines to one and strip blanks at
    // both file edges. Blank lines carry no significant tokens (the lexer collapses
    // consecutive newlines), so this never changes meaning; the `format_source` guard
    // proves it on every run anyway.
    let lines = normalize_blank_lines(lines);

    let mut output = apply_top_level_spacing(&lines).join("\n");
    // Spec 072 (2.3): always end with exactly one final newline, regardless of input.
    output.push('\n');
    output
}

/// Collapse consecutive blank lines to at most one and remove blank lines at the
/// start and end of the file. Non-blank lines pass through untouched and in order.
fn normalize_blank_lines(lines: Vec<FormattedLine>) -> Vec<FormattedLine> {
    let mut result = Vec::with_capacity(lines.len());
    for line in lines {
        if line.is_blank {
            // Drop leading blanks (nothing emitted yet) and collapse runs.
            if result
                .last()
                .is_none_or(|last: &FormattedLine| last.is_blank)
            {
                continue;
            }
        }
        result.push(line);
    }
    // Drop the single trailing blank a collapsed run may have left.
    while result.last().is_some_and(|last| last.is_blank) {
        result.pop();
    }
    result
}

#[derive(Debug, Clone)]
struct FormattedLine {
    text: String,
    depth: usize,
    is_blank: bool,
    is_comment: bool,
    is_top_level_declaration: bool,
}

impl FormattedLine {
    fn blank() -> Self {
        Self {
            text: String::new(),
            depth: 0,
            is_blank: true,
            is_comment: false,
            is_top_level_declaration: false,
        }
    }

    fn new(depth: usize, stripped: &str) -> Self {
        let is_comment = stripped.starts_with('#');
        // Comment-only lines keep their content (no reflow); code lines get canonical
        // intra-line spacing. Comments get only the leading `#`-spacing normalization.
        let content = if is_comment {
            normalize_comment_spacing(stripped)
        } else {
            respace_line(stripped)
        };
        let text = format!("{}{}", " ".repeat(depth * 4), content);
        Self {
            text,
            depth,
            is_blank: false,
            is_comment,
            is_top_level_declaration: depth == 0 && is_top_level_declaration(stripped),
        }
    }
}

/// Spec 072 (2.5): a leading `#` directly followed by a non-space, non-`#` character
/// gains exactly one space (`#comment` -> `# comment`). A bare `#` and `##`-style
/// comments (dividers, doc-headings) are left untouched, and the comment content beyond
/// the first character is never modified (no reflow). The caller guarantees the line
/// starts with `#`, which is one byte, so slicing at 1 is on a char boundary.
fn normalize_comment_spacing(comment: &str) -> String {
    match comment[1..].chars().next() {
        Some(next) if next != ' ' && next != '#' => format!("# {}", &comment[1..]),
        _ => comment.to_string(),
    }
}

fn apply_top_level_spacing(lines: &[FormattedLine]) -> Vec<String> {
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if should_insert_blank_before(lines, index, &out) {
            out.push(String::new());
        }
        out.push(line.text.clone());
    }
    out
}

fn should_insert_blank_before(lines: &[FormattedLine], index: usize, out: &[String]) -> bool {
    let line = &lines[index];
    if line.is_blank || out.is_empty() || out.last().is_some_and(|last| last.is_empty()) {
        return false;
    }

    if line.depth != 0 {
        return false;
    }

    if line.is_comment {
        // Only the first line of a comment group can get a separating blank, and
        // only when the group leads a top-level declaration. The previous non-blank
        // line being a comment means this is an interior line of the group, which
        // must never get a blank inserted before it (that would split the comment).
        if previous_nonblank_line(lines, index).is_none_or(|previous| previous.is_comment) {
            return false;
        }
        return comment_group_leads_to_top_level_declaration(lines, index);
    }

    if line.is_top_level_declaration {
        return !previous_nonblank_line(lines, index).is_some_and(|previous| previous.is_comment);
    }

    false
}

/// Whether the comment group starting at `index` (this line and any consecutive
/// comment lines) is immediately followed by a top-level declaration. Blank lines
/// or non-comment content break the group, so the comment is not a leading comment.
fn comment_group_leads_to_top_level_declaration(lines: &[FormattedLine], index: usize) -> bool {
    for line in &lines[index..] {
        if line.is_comment {
            continue;
        }
        return !line.is_blank && line.is_top_level_declaration;
    }
    false
}

fn previous_nonblank_line(lines: &[FormattedLine], index: usize) -> Option<&FormattedLine> {
    lines[..index].iter().rev().find(|line| !line.is_blank)
}

// --- Intra-line spacing (Spec 065) ---
//
// Reindentation aside, each code line's content is respaced to the house style: one space
// around binary operators and `=`, one space after `,` and `:` (none before), one space inside
// `{ }`, none inside `( )`/`[ ]`, and member access `.` kept tight. String literals and
// trailing comments are copied verbatim, so escapes and `#{...}` interpolation are untouched.
// The `significant_tokens` guard in `format_source` guarantees this never changes meaning.

enum Atom {
    Word(String),
    Str(String),
    Comment(String),
    Op(String),
}

fn respace_line(line: &str) -> String {
    join_atoms(&scan_atoms(line))
}

fn scan_atoms(line: &str) -> Vec<Atom> {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut atoms = Vec::new();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c == ' ' || c == '\t' {
            i += 1;
        } else if c == '"' {
            let start = i;
            i += 1;
            while i < n {
                if chars[i] == '\\' {
                    i += 2;
                } else if chars[i] == '"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            atoms.push(Atom::Str(chars[start..i.min(n)].iter().collect()));
        } else if c == '#' {
            atoms.push(Atom::Comment(chars[i..].iter().collect()));
            break;
        } else if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            atoms.push(Atom::Word(chars[start..i].iter().collect()));
        } else {
            let three: String = chars[i..n.min(i + 3)].iter().collect();
            let two: String = chars[i..n.min(i + 2)].iter().collect();
            if three == "*>>" {
                atoms.push(Atom::Op("*>>".to_string()));
                i += 3;
            } else if matches!(two.as_str(), ">>" | "->" | "=>" | "==" | "!=" | ">=" | "<=") {
                atoms.push(Atom::Op(two));
                i += 2;
            } else {
                atoms.push(Atom::Op(c.to_string()));
                i += 1;
            }
        }
    }
    atoms
}

fn text_of(a: &Atom) -> &str {
    match a {
        Atom::Word(s) | Atom::Str(s) | Atom::Comment(s) | Atom::Op(s) => s,
    }
}

fn op_str(a: &Atom) -> Option<&str> {
    match a {
        Atom::Op(s) => Some(s.as_str()),
        _ => None,
    }
}

fn is_keyword_operator(word: &str) -> bool {
    matches!(word, "and" | "or" | "not" | "in")
}

fn is_operand_end(a: &Atom) -> bool {
    match a {
        // Keyword operators (and/or/not/in) are operators, not operands, so a following
        // `(`/`[` is a grouped expression (spaced), not a call/index, and `+`/`-` after them
        // is unary.
        Atom::Word(w) => !is_keyword_operator(w),
        Atom::Str(_) => true,
        Atom::Op(s) => s == ")" || s == "]" || s == "}",
        Atom::Comment(_) => false,
    }
}

fn is_binary_spaced_op(s: &str, prev_is_operand: bool) -> bool {
    match s {
        "=" | "==" | "!=" | ">=" | "<=" | "<" | ">" | "*" | "/" | "%" | ">>" | "*>>" | "->"
        | "=>" => true,
        // `+`/`-` are binary (spaced) only after an operand; otherwise they are unary signs.
        "+" | "-" => prev_is_operand,
        _ => false,
    }
}

fn join_atoms(atoms: &[Atom]) -> String {
    let mut out = String::new();
    for k in 0..atoms.len() {
        if let Atom::Comment(c) = &atoms[k] {
            if !out.is_empty() {
                out.push_str("  ");
            }
            out.push_str(c);
            continue;
        }
        if k == 0 {
            out.push_str(text_of(&atoms[k]));
            continue;
        }
        if separator(atoms, k) {
            out.push(' ');
        }
        out.push_str(text_of(&atoms[k]));
    }
    out
}

// Whether a single space goes between atoms[k-1] and atoms[k].
fn separator(atoms: &[Atom], k: usize) -> bool {
    let prev = &atoms[k - 1];
    let cur = &atoms[k];
    let p = op_str(prev);
    let c = op_str(cur);

    // Member access and optional marker are tight.
    if c == Some(".") || p == Some(".") || c == Some("?") || p == Some("?") {
        return false;
    }
    // Open bracket hugs what follows; close bracket hugs what precedes.
    if p == Some("(") || p == Some("[") || c == Some(")") || c == Some("]") {
        return false;
    }
    // Comma / colon: no space before, one space after.
    if c == Some(",") || c == Some(":") {
        return false;
    }
    if p == Some(",") || p == Some(":") {
        return true;
    }
    // Map braces: `{}` tight, otherwise one inner space.
    if p == Some("{") && c == Some("}") {
        return false;
    }
    if p == Some("{") || c == Some("}") {
        return true;
    }
    // `(`/`[` after an operand is a call/index (tight); after an operator it is a group/array.
    if c == Some("(") || c == Some("[") {
        return !is_operand_end(prev);
    }
    // Binary operator gets a space on the side facing this boundary; a unary sign stays tight.
    if let Some(cs) = c {
        if is_binary_spaced_op(cs, is_operand_end(prev)) {
            return true;
        }
    }
    if let Some(ps) = p {
        let prev_prev_operand = k >= 2 && is_operand_end(&atoms[k - 2]);
        if is_binary_spaced_op(ps, prev_prev_operand) {
            return true;
        }
        if (ps == "+" || ps == "-") && !prev_prev_operand {
            return false;
        }
    }
    // Default: one space (word and operand boundaries like `route GET`, `as Schema`).
    true
}

fn is_top_level_declaration(stripped: &str) -> bool {
    stripped.starts_with("route ")
        || stripped.starts_with("task ")
        || stripped.starts_with("schema ")
        || stripped.starts_with("auth ")
        || stripped.starts_with("on queue ")
        || stripped.starts_with("on topic ")
        || stripped.starts_with("export task ")
        || stripped.starts_with("export schema ")
}

fn leading_indent_width(line: &str) -> usize {
    line.chars()
        .take_while(|ch| *ch == ' ' || *ch == '\t')
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum()
}

fn depth_for_comment(raw_indent: usize, stack: &[usize]) -> usize {
    if let Some(depth) = stack.iter().rposition(|indent| *indent == raw_indent) {
        return depth;
    }
    let current = *stack.last().unwrap_or(&0);
    if raw_indent > current {
        return stack.len();
    }
    stack.len().saturating_sub(1)
}

fn depth_for_code(raw_indent: usize, stack: &mut Vec<usize>) -> usize {
    let current = *stack.last().unwrap_or(&0);
    if raw_indent > current {
        stack.push(raw_indent);
    } else if raw_indent < current {
        while stack.len() > 1 && *stack.last().unwrap_or(&0) > raw_indent {
            stack.pop();
        }
    }
    stack.len().saturating_sub(1)
}

fn collect_marreta_files(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), FormatError> {
    let entries = fs::read_dir(path).map_err(|source| FormatError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| FormatError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let child = entry.path();
        if child.is_dir() {
            collect_marreta_files(&child, out)?;
        } else if is_marreta_file(&child) {
            out.push(child);
        }
    }
    Ok(())
}

fn is_marreta_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("marreta")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(src: &str) -> String {
        format_source(src).unwrap().output
    }

    fn touch(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    // --- Spec 072: blank-line pass (2.2-2.4) ---

    #[test]
    fn collapses_runs_of_blank_lines_to_one() {
        assert_eq!(fmt("a = 1\n\n\n\n\nb = 2\n"), "a = 1\n\nb = 2\n");
    }

    #[test]
    fn preserves_a_single_blank_line() {
        assert_eq!(fmt("a = 1\n\nb = 2\n"), "a = 1\n\nb = 2\n");
    }

    #[test]
    fn adds_missing_final_newline() {
        assert_eq!(fmt("a = 1"), "a = 1\n");
    }

    #[test]
    fn keeps_a_single_final_newline() {
        assert_eq!(fmt("a = 1\n"), "a = 1\n");
        assert_eq!(fmt("a = 1\n\n\n"), "a = 1\n");
    }

    // Spec 072 (2.3): an indented file with no final newline ends in `... Newline Dedent* Eof`
    // vs `... Dedent* Eof`, so the terminal Newline sits behind the synthesized Dedents. The
    // significant_tokens snapshot must skip those Dedents to find and drop it, or adding the
    // final newline trips the divergence guard. The corpus cannot guard this (every corpus file
    // already ends in a newline), so these two unit tests are the guardian.
    #[test]
    fn adds_final_newline_to_indented_body_without_one() {
        assert_eq!(
            fmt("route GET \"/x\"\n    reply 200, 1"),
            "route GET \"/x\"\n    reply 200, 1\n"
        );
    }

    #[test]
    fn indented_body_with_final_newline_is_idempotent() {
        let formatted = "route GET \"/x\"\n    reply 200, 1\n";
        assert_eq!(fmt(formatted), formatted);
    }

    #[test]
    fn strips_blank_lines_at_file_edges() {
        assert_eq!(fmt("\n\na = 1\nb = 2\n\n\n"), "a = 1\nb = 2\n");
    }

    // --- Spec 072: comment spacing (2.5) ---

    #[test]
    fn adds_space_after_hash_in_comment() {
        assert_eq!(fmt("#comment\nx = 1\n"), "# comment\nx = 1\n");
    }

    #[test]
    fn leaves_already_spaced_and_special_comments_untouched() {
        // already spaced, `##` heading-style, and a bare `#` all pass through.
        assert_eq!(fmt("# spaced\nx = 1\n"), "# spaced\nx = 1\n");
        assert_eq!(fmt("## heading\nx = 1\n"), "## heading\nx = 1\n");
        assert_eq!(fmt("#\nx = 1\n"), "#\nx = 1\n");
    }

    #[test]
    fn normalize_comment_spacing_edge_rule() {
        assert_eq!(normalize_comment_spacing("#x"), "# x");
        assert_eq!(normalize_comment_spacing("#==="), "# ===");
        assert_eq!(normalize_comment_spacing("# x"), "# x");
        assert_eq!(normalize_comment_spacing("## x"), "## x");
        assert_eq!(normalize_comment_spacing("#"), "#");
    }

    #[test]
    fn normalizes_only_the_leading_hash_not_comment_content() {
        // The `#` inside the comment body is content, never touched.
        assert_eq!(fmt("#note #2\nx = 1\n"), "# note #2\nx = 1\n");
    }

    #[test]
    fn normalizes_indentation_to_four_spaces() {
        let source = "route GET \"/x\"\n  value = 1\n  reply 200, value\n";
        assert_eq!(
            fmt(source),
            "route GET \"/x\"\n    value = 1\n    reply 200, value\n"
        );
    }

    #[test]
    fn formatting_is_idempotent() {
        let source = "route GET \"/x\"\n  value = { ok: true }\n  reply 200, value\n";
        let once = fmt(source);
        let twice = fmt(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn normalizes_intra_line_spacing() {
        assert_eq!(
            respace_line("message=greetings.build_greeting( \"Marreta\" )"),
            "message = greetings.build_greeting(\"Marreta\")"
        );
        assert_eq!(respace_line("{message:message}"), "{ message: message }");
        assert_eq!(
            respace_line("reply 200,{ msgs: msgs }"),
            "reply 200, { msgs: msgs }"
        );
        assert_eq!(respace_line("{}"), "{}");
        assert_eq!(respace_line("x = -1"), "x = -1");
        assert_eq!(respace_line("rate = price*0.9"), "rate = price * 0.9");
        assert_eq!(respace_line("data = params.id *>>"), "data = params.id *>>");
    }

    #[test]
    fn spaces_keyword_operators_before_groups() {
        // and/or/not/in are operators, so a following `(`/`[` is a grouped expression.
        assert_eq!(
            respace_line("item in[1,2] and ok and(flag)"),
            "item in [1, 2] and ok and (flag)"
        );
        assert_eq!(respace_line("x = not(flag)"), "x = not (flag)");
    }

    #[test]
    fn preserves_strings_and_comments() {
        // Trailing comment is normalized to two leading spaces and kept verbatim.
        assert_eq!(respace_line("a = 1 # note"), "a = 1  # note");
        // A `#` inside a string is not a comment.
        assert_eq!(respace_line("x = \"a#b\""), "x = \"a#b\"");
        // Escapes and interpolation inside strings are untouched.
        assert_eq!(
            respace_line("greet=\"Hi #{name}\\n\""),
            "greet = \"Hi #{name}\\n\""
        );
    }

    #[test]
    fn full_line_and_grouped_comments_survive() {
        let source = "# leading\n# group\nroute GET \"/x\"\n    reply 200, null\n";
        let out = fmt(source);
        assert!(out.contains("# leading\n# group\n"), "got: {out}");
    }

    #[test]
    fn examples_corpus_formats_idempotently_without_changing_meaning() {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        walk(&p, out);
                    } else if is_marreta_file(&p) {
                        out.push(p);
                    }
                }
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/examples");
        let mut files = Vec::new();
        walk(&root, &mut files);
        assert!(!files.is_empty(), "no example .marreta files found");
        for f in files {
            let src = std::fs::read_to_string(&f).unwrap();
            // Skip files that do not parse on their own (fixtures expecting load errors).
            if parse_source(&src).is_err() {
                continue;
            }
            let once = match format_source(&src) {
                Ok(r) => r.output,
                Err(e) => panic!("fmt failed on {}: {}", f.display(), e),
            };
            let twice = format_source(&once).unwrap().output;
            assert_eq!(once, twice, "not idempotent: {}", f.display());
        }
    }

    #[test]
    fn preserves_comments_and_removes_trailing_whitespace() {
        let source =
            "# top   \nroute GET \"/x\"\n  # route comment   \n  reply 200, { ok: true }   \n";
        assert_eq!(
            fmt(source),
            "# top\nroute GET \"/x\"\n    # route comment\n    reply 200, { ok: true }\n"
        );
    }

    #[test]
    fn preserves_multiline_map_shape() {
        let source = "route GET \"/x\"\n  reply 200, {\n    ok: true,\n    message: \"hi\"\n  }\n";
        assert_eq!(
            fmt(source),
            "route GET \"/x\"\n    reply 200, {\n        ok: true,\n        message: \"hi\"\n    }\n"
        );
    }

    #[test]
    fn inserts_blank_lines_between_top_level_declarations() {
        let source = "project_name = \"fmt-test\"\nproject_version = \"0.1.0\"\nroute GET \"/x\"\n  reply 200, null\ntask greet() => \"hi\"\n";
        assert_eq!(
            fmt(source),
            "project_name = \"fmt-test\"\nproject_version = \"0.1.0\"\n\nroute GET \"/x\"\n    reply 200, null\n\ntask greet() => \"hi\"\n"
        );
    }

    #[test]
    fn keeps_top_level_comments_attached_to_following_declaration() {
        let source =
            "route GET \"/a\"\n  reply 200, null\n# route b\nroute GET \"/b\"\n  reply 200, null\n";
        assert_eq!(
            fmt(source),
            "route GET \"/a\"\n    reply 200, null\n\n# route b\nroute GET \"/b\"\n    reply 200, null\n"
        );
    }

    #[test]
    fn does_not_split_multi_line_top_level_comment() {
        // A multi-line comment preceding a top-level declaration gets a single
        // separating blank before its first line, and is never split internally.
        let source = "route GET \"/a\"\n  reply 200, null\n# comment line one\n# comment line two\nroute GET \"/b\"\n  reply 200, null\n";
        assert_eq!(
            fmt(source),
            "route GET \"/a\"\n    reply 200, null\n\n# comment line one\n# comment line two\nroute GET \"/b\"\n    reply 200, null\n"
        );
    }

    #[test]
    fn parse_errors_are_reported() {
        let err = format_source("route GET \"/x\"\n  reply 200, null\n x = 1\n").unwrap_err();
        assert!(matches!(err, FormatError::Parse(_)));
    }

    #[test]
    fn project_discovery_requires_app_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let err = discover_project_files(dir.path()).unwrap_err();
        assert!(matches!(err, FormatError::MissingProjectRoot(_)));
    }

    fn relative_to(root: &Path, files: &[PathBuf]) -> Vec<String> {
        files
            .iter()
            .map(|path| {
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect()
    }

    // Spec 072 (2.1): fmt discovery recurses every directory the loader would load,
    // not just the four canonical ones. A project with an `auth/` directory (the
    // organization spec 024 suggests) or any custom folder must be reachable by fmt.
    #[test]
    fn project_discovery_recurses_every_directory_like_the_loader() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("app.marreta"), "project_name = \"fmt\"\n");
        touch(
            &root.join("routes/api.marreta"),
            "route GET \"/x\"\n    reply 200, null\n",
        );
        touch(
            &root.join("routes/admin/ping.marreta"),
            "route GET \"/admin/ping\"\n    reply 200, null\n",
        );
        touch(
            &root.join("schemas/models.marreta"),
            "schema Item\n    name: string\n",
        );
        touch(
            &root.join("tasks/jobs.marreta"),
            "task ping() => \"pong\"\n",
        );
        touch(
            &root.join("tests/api_test.marreta"),
            "scenario \"ok\"\n    assert true\n",
        );
        // The bug cases: a non-canonical `auth/` dir and a fully custom `lib/` dir.
        touch(
            &root.join("auth/customer_auth.marreta"),
            "auth_provider Customer\n    header \"X-Token\"\n",
        );
        touch(&root.join("lib/y.marreta"), "task helper() => 1\n");

        let files = discover_project_files(root).unwrap();

        assert_eq!(
            relative_to(root, &files),
            vec![
                "app.marreta",
                "auth/customer_auth.marreta",
                "lib/y.marreta",
                "routes/admin/ping.marreta",
                "routes/api.marreta",
                "schemas/models.marreta",
                "tasks/jobs.marreta",
                "tests/api_test.marreta",
            ]
        );
    }

    // Spec 072 (2.1): pin fmt discovery to the loader's discovery so the two lists
    // cannot drift again (the 068 catalog-to-token pattern, applied to file discovery).
    #[test]
    fn fmt_discovery_equals_loader_discovery_plus_entrypoint() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let app = root.join("app.marreta");
        touch(&app, "project_name = \"fmt\"\n");
        touch(
            &root.join("routes/api.marreta"),
            "route GET \"/x\"\n    reply 200, null\n",
        );
        touch(
            &root.join("auth/customer_auth.marreta"),
            "auth_provider Customer\n    header \"X-Token\"\n",
        );
        touch(&root.join("lib/y.marreta"), "task helper() => 1\n");

        let fmt_files = discover_project_files(root).unwrap();

        let mut loader_files = crate::file_loader::collect_marreta_files(root, &app);
        loader_files.push(app);
        loader_files.sort();

        assert_eq!(fmt_files, loader_files);
    }

    #[test]
    fn explicit_discovery_accepts_files_and_dirs_without_project_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let scratch = root.join("scratch");
        touch(&scratch.join("one.marreta"), "print(1)\n");
        touch(&scratch.join("nested/two.marreta"), "print(2)\n");
        touch(&scratch.join("notes.txt"), "ignore me\n");
        touch(&root.join("single.marreta"), "print(3)\n");

        let files =
            discover_explicit_files(&[scratch.clone(), root.join("single.marreta")]).unwrap();
        let relative: Vec<_> = files
            .iter()
            .map(|path| {
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();

        assert_eq!(
            relative,
            vec![
                "scratch/nested/two.marreta",
                "scratch/one.marreta",
                "single.marreta"
            ]
        );
    }

    #[test]
    fn explicit_discovery_rejects_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.marreta");
        let err = discover_explicit_files(std::slice::from_ref(&missing)).unwrap_err();
        assert!(matches!(err, FormatError::InvalidPath(path) if path == missing));
    }
}
