use crate::error::MarretaError;
use crate::token::{Token, TokenKind, keyword_lookup};

/// Tokenizer for MarretaLang source code.
///
/// Handles significant indentation, string interpolation (preserved for runtime),
/// newline suppression on continuation operators, and `#` comments.
pub struct Lexer {
    source: Vec<char>,
    tokens: Vec<Token>,
    pos: usize,
    line: usize,
    column: usize,
    indent_stack: Vec<usize>,
    at_line_start: bool,
    /// Tracks nesting depth across `(`, `[`, `{`. When > 0, newlines and
    /// indentation changes are suppressed so multi-line expressions inside
    /// delimiters are treated as a single logical line.
    nesting_depth: usize,
}

impl Lexer {
    /// Creates a new lexer for the given source string.
    ///
    /// Line endings are normalized to `\n` first, so source authored on Windows
    /// (CRLF) or classic macOS (CR) lexes identically to Unix (LF). Without this,
    /// the indentation logic would misread a `\r\n` blank line and reject valid
    /// programs (notably for Windows/WSL users).
    pub fn new(source: &str) -> Self {
        let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
        Self {
            source: normalized.chars().collect(),
            tokens: Vec::new(),
            pos: 0,
            line: 1,
            column: 1,
            indent_stack: vec![0],
            at_line_start: true,
            nesting_depth: 0,
        }
    }

    /// Tokenizes the full source and returns the token list.
    pub fn tokenize(&mut self) -> Result<Vec<Token>, MarretaError> {
        while !self.is_at_end() {
            if self.at_line_start {
                self.handle_indentation()?;
                self.at_line_start = false;
            }

            let ch = self.current();
            match ch {
                // Whitespace (non-newline) — skip
                ' ' | '\t' | '\r' => {
                    self.advance();
                }

                // Newline
                '\n' => {
                    self.handle_newline();
                }

                // Comment
                '#' => {
                    // Only treat as comment if not inside a string (we're at top level here)
                    self.skip_comment();
                }

                // String literal
                '"' => {
                    self.read_string()?;
                }

                // Operators and delimiters
                '+' => {
                    self.emit_single(TokenKind::Plus, "+");
                }
                '-' => {
                    if self.peek() == Some('>') {
                        self.emit_double(TokenKind::Arrow, "->");
                    } else {
                        self.emit_single(TokenKind::Minus, "-");
                    }
                }
                '*' => {
                    if self.peek() == Some('>') && self.peek_at(2) == Some('>') {
                        self.emit_triple(TokenKind::Broadcast, "*>>");
                    } else {
                        self.emit_single(TokenKind::Star, "*");
                    }
                }
                '/' => {
                    self.emit_single(TokenKind::Slash, "/");
                }
                '%' => {
                    self.emit_single(TokenKind::Percent, "%");
                }
                '=' => {
                    if self.peek() == Some('=') {
                        self.emit_double(TokenKind::Equal, "==");
                    } else if self.peek() == Some('>') {
                        self.emit_double(TokenKind::FatArrow, "=>");
                    } else {
                        self.emit_single(TokenKind::Assign, "=");
                    }
                }
                '!' => {
                    if self.peek() == Some('=') {
                        self.emit_double(TokenKind::NotEqual, "!=");
                    } else {
                        return Err(MarretaError::UnexpectedCharacter {
                            char: '!',
                            line: self.line,
                            column: self.column,
                        });
                    }
                }
                '>' => {
                    if self.peek() == Some('>') {
                        self.emit_double(TokenKind::Pipeline, ">>");
                    } else if self.peek() == Some('=') {
                        self.emit_double(TokenKind::GreaterEqual, ">=");
                    } else {
                        self.emit_single(TokenKind::Greater, ">");
                    }
                }
                '<' => {
                    if self.peek() == Some('=') {
                        self.emit_double(TokenKind::LessEqual, "<=");
                    } else {
                        self.emit_single(TokenKind::Less, "<");
                    }
                }

                // Delimiters
                '(' => {
                    self.nesting_depth += 1;
                    self.emit_single(TokenKind::LeftParen, "(");
                }
                ')' => {
                    self.nesting_depth = self.nesting_depth.saturating_sub(1);
                    self.emit_single(TokenKind::RightParen, ")");
                }
                '[' => {
                    self.nesting_depth += 1;
                    self.emit_single(TokenKind::LeftBracket, "[");
                }
                ']' => {
                    self.nesting_depth = self.nesting_depth.saturating_sub(1);
                    self.emit_single(TokenKind::RightBracket, "]");
                }
                '{' => {
                    self.nesting_depth += 1;
                    self.emit_single(TokenKind::LeftBrace, "{");
                }
                '}' => {
                    self.nesting_depth = self.nesting_depth.saturating_sub(1);
                    self.emit_single(TokenKind::RightBrace, "}");
                }
                ',' => self.emit_single(TokenKind::Comma, ","),
                '.' => self.emit_single(TokenKind::Dot, "."),
                ':' => self.emit_single(TokenKind::Colon, ":"),

                // Numbers
                '0'..='9' => {
                    self.read_number()?;
                }

                // Identifiers and keywords
                'a'..='z' | 'A'..='Z' | '_' => {
                    self.read_identifier();
                }

                _ => {
                    return Err(MarretaError::UnexpectedCharacter {
                        char: ch,
                        line: self.line,
                        column: self.column,
                    });
                }
            }
        }

        // Emit remaining dedents at EOF
        while self.indent_stack.len() > 1 {
            self.indent_stack.pop();
            self.tokens
                .push(Token::new(TokenKind::Dedent, self.line, self.column, ""));
        }

        self.tokens.push(Token::eof(self.line, self.column));
        Ok(self.tokens.clone())
    }

    // --- Character access helpers ---

    fn is_at_end(&self) -> bool {
        self.pos >= self.source.len()
    }

    fn current(&self) -> char {
        self.source[self.pos]
    }

    fn peek(&self) -> Option<char> {
        self.source.get(self.pos + 1).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.source.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> char {
        let ch = self.source[self.pos];
        self.pos += 1;
        self.column += 1;
        ch
    }

    // --- Token emission helpers ---

    fn emit_single(&mut self, kind: TokenKind, lexeme: &str) {
        let tok = Token::new(kind, self.line, self.column, lexeme);
        self.tokens.push(tok);
        self.advance();
    }

    fn emit_double(&mut self, kind: TokenKind, lexeme: &str) {
        // `>>` and `*>>` at the start of a line: remove the preceding Newline so
        // the pipeline step is treated as a continuation of the previous expression.
        if matches!(kind, TokenKind::Pipeline | TokenKind::Broadcast) {
            let last_meaningful = self
                .tokens
                .iter()
                .rposition(|t| !matches!(t.kind, TokenKind::Indent | TokenKind::Dedent));
            if let Some(idx) = last_meaningful
                && matches!(self.tokens[idx].kind, TokenKind::Newline)
            {
                self.tokens.remove(idx);
            }
        }
        let tok = Token::new(kind, self.line, self.column, lexeme);
        self.tokens.push(tok);
        self.advance();
        self.advance();
    }

    fn emit_triple(&mut self, kind: TokenKind, lexeme: &str) {
        let tok = Token::new(kind, self.line, self.column, lexeme);
        self.tokens.push(tok);
        self.advance();
        self.advance();
        self.advance();
    }

    // --- Indentation ---

    fn handle_indentation(&mut self) -> Result<(), MarretaError> {
        let mut spaces = 0;
        while !self.is_at_end() && self.current() == ' ' {
            spaces += 1;
            self.advance();
        }

        // Skip blank lines and comment-only lines
        if self.is_at_end() || self.current() == '\n' || self.current() == '#' {
            return Ok(());
        }

        let current_indent = *self.indent_stack.last().unwrap_or(&0);

        if spaces > current_indent {
            self.indent_stack.push(spaces);
            self.tokens
                .push(Token::new(TokenKind::Indent, self.line, 1, ""));
        } else if spaces < current_indent {
            while self.indent_stack.len() > 1 && *self.indent_stack.last().unwrap_or(&0) > spaces {
                self.indent_stack.pop();
                self.tokens
                    .push(Token::new(TokenKind::Dedent, self.line, 1, ""));
            }
            // Validate that we landed on a known indentation level
            let current = *self.indent_stack.last().unwrap_or(&0);
            if current != spaces {
                return Err(MarretaError::InvalidIndentation {
                    line: self.line,
                    expected: current,
                    got: spaces,
                });
            }
        }

        Ok(())
    }

    // --- Newlines ---

    fn handle_newline(&mut self) {
        // Suppress newline when inside delimiters (multi-line expressions)
        if self.nesting_depth > 0 {
            self.pos += 1;
            self.line += 1;
            self.column = 1;
            self.at_line_start = false; // don't trigger indentation handling
            return;
        }
        // Suppress newline if last non-control token is a continuation operator
        if !self.should_suppress_newline() {
            // Avoid duplicate newlines
            let last_is_newline = self
                .tokens
                .last()
                .is_some_and(|t| matches!(t.kind, TokenKind::Newline));
            if !last_is_newline && !self.tokens.is_empty() {
                self.tokens.push(Token::new(
                    TokenKind::Newline,
                    self.line,
                    self.column,
                    "\\n",
                ));
            }
        }

        self.pos += 1;
        self.line += 1;
        self.column = 1;
        self.at_line_start = true;
    }

    /// Returns true if the last meaningful token is a continuation operator.
    fn should_suppress_newline(&self) -> bool {
        // Walk backwards past Indent/Dedent/Newline to find the last meaningful token
        for tok in self.tokens.iter().rev() {
            match tok.kind {
                TokenKind::Indent | TokenKind::Dedent | TokenKind::Newline => continue,
                TokenKind::Pipeline
                | TokenKind::Broadcast
                | TokenKind::Arrow
                | TokenKind::FatArrow
                | TokenKind::Comma => return true,
                _ => return false,
            }
        }
        // No tokens yet — suppress (don't emit newline at start of file)
        true
    }

    // --- Comments ---

    fn skip_comment(&mut self) {
        while !self.is_at_end() && self.current() != '\n' {
            self.advance();
        }
    }

    // --- String literals ---

    fn read_string(&mut self) -> Result<(), MarretaError> {
        let start_line = self.line;
        let start_col = self.column;
        self.advance(); // consume opening "

        let mut value = String::new();
        while !self.is_at_end() && self.current() != '"' {
            if self.current() == '\n' {
                return Err(MarretaError::UnterminatedString {
                    line: start_line,
                    column: start_col,
                });
            }
            if self.current() == '\\' {
                self.advance(); // consume backslash
                if self.is_at_end() {
                    return Err(MarretaError::UnterminatedString {
                        line: start_line,
                        column: start_col,
                    });
                }
                match self.current() {
                    'n' => value.push('\n'),
                    't' => value.push('\t'),
                    'r' => value.push('\r'),
                    '\\' => value.push('\\'),
                    '"' => value.push('"'),
                    '#' => value.push('#'),
                    other => {
                        value.push('\\');
                        value.push(other);
                    }
                }
                self.advance();
            } else {
                value.push(self.advance());
            }
        }

        if self.is_at_end() {
            return Err(MarretaError::UnterminatedString {
                line: start_line,
                column: start_col,
            });
        }

        self.advance(); // consume closing "

        let lexeme = format!("\"{}\"", value);
        self.tokens.push(Token::new(
            TokenKind::StringLiteral(value),
            start_line,
            start_col,
            lexeme,
        ));
        Ok(())
    }

    // --- Numbers ---

    fn read_number(&mut self) -> Result<(), MarretaError> {
        let start_col = self.column;
        let start_pos = self.pos;
        let mut has_dot = false;

        while !self.is_at_end() && (self.current().is_ascii_digit() || self.current() == '.') {
            if self.current() == '.' {
                // Check if next char is a digit — otherwise it's a method call like `5.abs()`
                if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    if has_dot {
                        let lexeme: String = self.source[start_pos..=self.pos].iter().collect();
                        return Err(MarretaError::InvalidNumber {
                            lexeme,
                            line: self.line,
                            column: start_col,
                        });
                    }
                    has_dot = true;
                } else {
                    break; // stop before `.` — it's property access
                }
            }
            self.advance();
        }

        let lexeme: String = self.source[start_pos..self.pos].iter().collect();

        if has_dot {
            let n: f64 = lexeme.parse().map_err(|_| MarretaError::InvalidNumber {
                lexeme: lexeme.clone(),
                line: self.line,
                column: start_col,
            })?;
            self.tokens.push(Token::new(
                TokenKind::Float(n),
                self.line,
                start_col,
                &lexeme,
            ));
        } else {
            let n: i64 = lexeme.parse().map_err(|_| MarretaError::InvalidNumber {
                lexeme: lexeme.clone(),
                line: self.line,
                column: start_col,
            })?;
            self.tokens.push(Token::new(
                TokenKind::Integer(n),
                self.line,
                start_col,
                &lexeme,
            ));
        }

        Ok(())
    }

    // --- Identifiers and keywords ---

    fn read_identifier(&mut self) {
        let start_col = self.column;
        let start_pos = self.pos;

        while !self.is_at_end()
            && (self.current().is_ascii_alphanumeric()
                || self.current() == '_'
                || self.current() == '?')
        {
            self.advance();
        }

        let word: String = self.source[start_pos..self.pos].iter().collect();

        let kind = keyword_lookup(&word).unwrap_or_else(|| TokenKind::Identifier(word.clone()));

        self.tokens
            .push(Token::new(kind, self.line, start_col, &word));
    }
}

#[cfg(test)]
#[allow(clippy::approx_constant)] // sample float literals (3.14…), not the PI constant
mod tests {
    use super::*;

    fn tokenize(source: &str) -> Vec<Token> {
        Lexer::new(source).tokenize().unwrap()
    }

    fn kinds(source: &str) -> Vec<TokenKind> {
        tokenize(source).into_iter().map(|t| t.kind).collect()
    }

    // --- Basic literals ---

    #[test]
    fn test_integer() {
        let k = kinds("42");
        assert_eq!(k, vec![TokenKind::Integer(42), TokenKind::Eof]);
    }

    #[test]
    fn test_float() {
        let k = kinds("3.14");
        assert_eq!(k, vec![TokenKind::Float(3.14), TokenKind::Eof]);
    }

    #[test]
    fn test_string() {
        let k = kinds("\"hello\"");
        assert_eq!(
            k,
            vec![TokenKind::StringLiteral("hello".into()), TokenKind::Eof]
        );
    }

    #[test]
    fn test_string_with_interpolation_preserved() {
        let k = kinds("\"hello #{name}\"");
        assert_eq!(
            k,
            vec![
                TokenKind::StringLiteral("hello #{name}".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_string_escape_sequences() {
        let k = kinds("\"a\\nb\\tc\"");
        assert_eq!(
            k,
            vec![TokenKind::StringLiteral("a\nb\tc".into()), TokenKind::Eof]
        );
    }

    #[test]
    fn test_boolean_and_null() {
        let k = kinds("true false null");
        assert_eq!(
            k,
            vec![
                TokenKind::True,
                TokenKind::False,
                TokenKind::Null,
                TokenKind::Eof
            ]
        );
    }

    // --- Operators ---

    #[test]
    fn test_arithmetic_operators() {
        let k = kinds("+ - * / %");
        assert_eq!(
            k,
            vec![
                TokenKind::Plus,
                TokenKind::Minus,
                TokenKind::Star,
                TokenKind::Slash,
                TokenKind::Percent,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_comparison_operators() {
        let k = kinds("== != > < >= <=");
        assert_eq!(
            k,
            vec![
                TokenKind::Equal,
                TokenKind::NotEqual,
                TokenKind::Greater,
                TokenKind::Less,
                TokenKind::GreaterEqual,
                TokenKind::LessEqual,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_pipeline_operators() {
        let k = kinds(">> *>>");
        assert_eq!(
            k,
            vec![TokenKind::Pipeline, TokenKind::Broadcast, TokenKind::Eof]
        );
    }

    #[test]
    fn test_arrows() {
        let k = kinds("-> =>");
        assert_eq!(
            k,
            vec![TokenKind::Arrow, TokenKind::FatArrow, TokenKind::Eof]
        );
    }

    #[test]
    fn test_assign() {
        let k = kinds("x = 5");
        assert_eq!(
            k,
            vec![
                TokenKind::Identifier("x".into()),
                TokenKind::Assign,
                TokenKind::Integer(5),
                TokenKind::Eof,
            ]
        );
    }

    // --- Delimiters ---

    #[test]
    fn test_delimiters() {
        let k = kinds("( ) [ ] { } , . :");
        assert_eq!(
            k,
            vec![
                TokenKind::LeftParen,
                TokenKind::RightParen,
                TokenKind::LeftBracket,
                TokenKind::RightBracket,
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
                TokenKind::Comma,
                TokenKind::Dot,
                TokenKind::Colon,
                TokenKind::Eof,
            ]
        );
    }

    // --- Keywords ---

    #[test]
    fn test_core_keywords() {
        let k = kinds("task match fallback map keep require reject if else");
        assert_eq!(
            k,
            vec![
                TokenKind::Task,
                TokenKind::Match,
                TokenKind::Fallback,
                TokenKind::Map,
                TokenKind::Keep,
                TokenKind::Require,
                TokenKind::Reject,
                TokenKind::If,
                TokenKind::Else,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_logical_keywords() {
        let k = kinds("and or not");
        assert_eq!(
            k,
            vec![
                TokenKind::And,
                TokenKind::Or,
                TokenKind::Not,
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_http_keywords() {
        let k = kinds("route take reply fail listen GET POST PUT PATCH DELETE");
        assert_eq!(
            k,
            vec![
                TokenKind::Route,
                TokenKind::Take,
                TokenKind::Reply,
                TokenKind::Fail,
                TokenKind::Listen,
                TokenKind::Get,
                TokenKind::Post,
                TokenKind::Put,
                TokenKind::Patch,
                TokenKind::Delete,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_infra_keywords() {
        let k = kinds("db queue cache");
        assert_eq!(
            k,
            vec![
                TokenKind::Db,
                TokenKind::Queue,
                TokenKind::Cache,
                TokenKind::Eof
            ]
        );
    }

    // --- Identifiers ---

    #[test]
    fn test_identifiers() {
        let k = kinds("foo bar_baz _private camelCase");
        assert_eq!(
            k,
            vec![
                TokenKind::Identifier("foo".into()),
                TokenKind::Identifier("bar_baz".into()),
                TokenKind::Identifier("_private".into()),
                TokenKind::Identifier("camelCase".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_identifier_with_question_mark() {
        let k = kinds("empty?");
        assert_eq!(
            k,
            vec![TokenKind::Identifier("empty?".into()), TokenKind::Eof]
        );
    }

    // --- Comments ---

    #[test]
    fn test_line_comment() {
        let k = kinds("x = 5 # this is a comment");
        assert_eq!(
            k,
            vec![
                TokenKind::Identifier("x".into()),
                TokenKind::Assign,
                TokenKind::Integer(5),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_comment_only_line() {
        let k = kinds("# full line comment\nx = 1");
        assert_eq!(
            k,
            vec![
                TokenKind::Identifier("x".into()),
                TokenKind::Assign,
                TokenKind::Integer(1),
                TokenKind::Eof,
            ]
        );
    }

    // --- Newlines ---

    #[test]
    fn test_newlines_between_statements() {
        let k = kinds("x = 1\ny = 2");
        assert_eq!(
            k,
            vec![
                TokenKind::Identifier("x".into()),
                TokenKind::Assign,
                TokenKind::Integer(1),
                TokenKind::Newline,
                TokenKind::Identifier("y".into()),
                TokenKind::Assign,
                TokenKind::Integer(2),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_newline_suppression_after_pipeline() {
        let k = kinds("items >>\ntask(double)");
        assert_eq!(
            k,
            vec![
                TokenKind::Identifier("items".into()),
                TokenKind::Pipeline,
                TokenKind::Task,
                TokenKind::LeftParen,
                TokenKind::Identifier("double".into()),
                TokenKind::RightParen,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_newline_suppression_after_arrow() {
        let k = kinds("\"VIP\" ->\n0.0");
        assert_eq!(
            k,
            vec![
                TokenKind::StringLiteral("VIP".into()),
                TokenKind::Arrow,
                TokenKind::Float(0.0),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_newline_suppression_after_comma() {
        let k = kinds("func(a,\nb)");
        assert_eq!(
            k,
            vec![
                TokenKind::Identifier("func".into()),
                TokenKind::LeftParen,
                TokenKind::Identifier("a".into()),
                TokenKind::Comma,
                TokenKind::Identifier("b".into()),
                TokenKind::RightParen,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_no_duplicate_newlines() {
        let k = kinds("x = 1\n\n\ny = 2");
        assert_eq!(
            k,
            vec![
                TokenKind::Identifier("x".into()),
                TokenKind::Assign,
                TokenKind::Integer(1),
                TokenKind::Newline,
                TokenKind::Identifier("y".into()),
                TokenKind::Assign,
                TokenKind::Integer(2),
                TokenKind::Eof,
            ]
        );
    }

    // --- Indentation ---

    #[test]
    fn test_indent_dedent() {
        let src = "task double(n)\n    n * 2\n";
        let k = kinds(src);
        assert_eq!(
            k,
            vec![
                TokenKind::Task,
                TokenKind::Identifier("double".into()),
                TokenKind::LeftParen,
                TokenKind::Identifier("n".into()),
                TokenKind::RightParen,
                TokenKind::Newline,
                TokenKind::Indent,
                TokenKind::Identifier("n".into()),
                TokenKind::Star,
                TokenKind::Integer(2),
                TokenKind::Newline,
                TokenKind::Dedent,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_nested_indent() {
        let src = "a\n    b\n        c\n    d\ne";
        let k = kinds(src);
        assert_eq!(
            k,
            vec![
                TokenKind::Identifier("a".into()),
                TokenKind::Newline,
                TokenKind::Indent,
                TokenKind::Identifier("b".into()),
                TokenKind::Newline,
                TokenKind::Indent,
                TokenKind::Identifier("c".into()),
                TokenKind::Newline,
                TokenKind::Dedent,
                TokenKind::Identifier("d".into()),
                TokenKind::Newline,
                TokenKind::Dedent,
                TokenKind::Identifier("e".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_multiple_dedents_at_once() {
        let src = "a\n    b\n        c\nd";
        let k = kinds(src);
        assert_eq!(
            k,
            vec![
                TokenKind::Identifier("a".into()),
                TokenKind::Newline,
                TokenKind::Indent,
                TokenKind::Identifier("b".into()),
                TokenKind::Newline,
                TokenKind::Indent,
                TokenKind::Identifier("c".into()),
                TokenKind::Newline,
                TokenKind::Dedent,
                TokenKind::Dedent,
                TokenKind::Identifier("d".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_dedents_at_eof() {
        let src = "a\n    b\n        c";
        let k = kinds(src);
        assert_eq!(
            k,
            vec![
                TokenKind::Identifier("a".into()),
                TokenKind::Newline,
                TokenKind::Indent,
                TokenKind::Identifier("b".into()),
                TokenKind::Newline,
                TokenKind::Indent,
                TokenKind::Identifier("c".into()),
                TokenKind::Dedent,
                TokenKind::Dedent,
                TokenKind::Eof,
            ]
        );
    }

    // --- Full expressions ---

    #[test]
    fn test_assignment_expression() {
        let k = kinds("name = \"Marreta\"");
        assert_eq!(
            k,
            vec![
                TokenKind::Identifier("name".into()),
                TokenKind::Assign,
                TokenKind::StringLiteral("Marreta".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_require_statement() {
        let k = kinds("require payload.items else fail 400, \"Cart is empty\"");
        assert_eq!(
            k,
            vec![
                TokenKind::Require,
                TokenKind::Identifier("payload".into()),
                TokenKind::Dot,
                TokenKind::Identifier("items".into()),
                TokenKind::Else,
                TokenKind::Fail,
                TokenKind::Integer(400),
                TokenKind::Comma,
                TokenKind::StringLiteral("Cart is empty".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_match_expression() {
        let src = "fee = match client.type\n    \"VIP\" -> 0.0\n    fallback -> 15.0";
        let k = kinds(src);
        assert_eq!(
            k,
            vec![
                TokenKind::Identifier("fee".into()),
                TokenKind::Assign,
                TokenKind::Match,
                TokenKind::Identifier("client".into()),
                TokenKind::Dot,
                TokenKind::Identifier("type".into()),
                TokenKind::Newline,
                TokenKind::Indent,
                TokenKind::StringLiteral("VIP".into()),
                TokenKind::Arrow,
                TokenKind::Float(0.0),
                TokenKind::Newline,
                TokenKind::Fallback,
                TokenKind::Arrow,
                TokenKind::Float(15.0),
                TokenKind::Dedent,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_conditional_suffix() {
        let k = kinds("status = \"approved\" if balance > 100");
        assert_eq!(
            k,
            vec![
                TokenKind::Identifier("status".into()),
                TokenKind::Assign,
                TokenKind::StringLiteral("approved".into()),
                TokenKind::If,
                TokenKind::Identifier("balance".into()),
                TokenKind::Greater,
                TokenKind::Integer(100),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_list_literal() {
        let k = kinds("[1, 2, 3]");
        assert_eq!(
            k,
            vec![
                TokenKind::LeftBracket,
                TokenKind::Integer(1),
                TokenKind::Comma,
                TokenKind::Integer(2),
                TokenKind::Comma,
                TokenKind::Integer(3),
                TokenKind::RightBracket,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_map_literal() {
        let k = kinds("{ name: \"Ana\", age: 30 }");
        assert_eq!(
            k,
            vec![
                TokenKind::LeftBrace,
                TokenKind::Identifier("name".into()),
                TokenKind::Colon,
                TokenKind::StringLiteral("Ana".into()),
                TokenKind::Comma,
                TokenKind::Identifier("age".into()),
                TokenKind::Colon,
                TokenKind::Integer(30),
                TokenKind::RightBrace,
                TokenKind::Eof,
            ]
        );
    }

    // --- Line/column tracking ---

    #[test]
    fn test_line_column_tracking() {
        let tokens = tokenize("x = 1\ny = 2");
        assert_eq!(tokens[0].line, 1); // x
        assert_eq!(tokens[0].column, 1);
        assert_eq!(tokens[2].line, 1); // 1
        assert_eq!(tokens[2].column, 5);
        assert_eq!(tokens[4].line, 2); // y
        assert_eq!(tokens[4].column, 1);
    }

    // --- Error cases ---

    #[test]
    fn test_unterminated_string() {
        let result = Lexer::new("\"hello").tokenize();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MarretaError::UnterminatedString { .. }
        ));
    }

    #[test]
    fn test_invalid_indentation() {
        let src = "a\n    b\n  c"; // dedent to 2 spaces which was never an indent level
        let result = Lexer::new(src).tokenize();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MarretaError::InvalidIndentation { .. }
        ));
    }

    #[test]
    fn test_number_before_dot_method() {
        let k = kinds("5.abs()");
        assert_eq!(k[0], TokenKind::Integer(5));
        assert_eq!(k[1], TokenKind::Dot);
        assert_eq!(k[2], TokenKind::Identifier("abs".into()));
    }

    #[test]
    fn test_crlf_lexes_like_lf() {
        // A route body with a blank line is exactly what broke under CRLF: the
        // indentation logic must see normalized LF, so Windows/WSL source lexes
        // identically to Unix source.
        let lf = "route GET \"/x\"\n    require ok else fail 400, \"x\"\n\n    reply 200, { ok: true }\n";
        let crlf = lf.replace('\n', "\r\n");
        let cr = lf.replace('\n', "\r");
        assert_eq!(kinds(lf), kinds(&crlf));
        assert_eq!(kinds(lf), kinds(&cr));
    }
}
