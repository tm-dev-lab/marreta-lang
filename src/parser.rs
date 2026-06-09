use crate::ast::*;
use crate::error::MarretaError;
use crate::token::{Token, TokenKind};

/// Parser for MarretaLang — transforms a token stream into an AST.
///
/// Uses a Pratt parser (top-down operator precedence) for expressions
/// and recursive descent for statements and blocks.
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Tracks whether we are currently inside a `transaction` block.
    /// Used to detect nesting at parse time and emit a startup error.
    inside_transaction: bool,
}

// --- Operator precedence levels (lowest to highest) ---
const PREC_NONE: u8 = 0;
const PREC_OR: u8 = 1;
const PREC_AND: u8 = 2;
const PREC_EQUALITY: u8 = 4;
const PREC_COMPARISON: u8 = 5;
const PREC_ADDITION: u8 = 6;
const PREC_MULTIPLICATION: u8 = 7;
const PREC_UNARY: u8 = 8;
const PREC_CALL: u8 = 9;

impl Parser {
    /// Creates a new parser for the given token stream.
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            inside_transaction: false,
        }
    }

    /// Parses the full token stream into a program (list of statements).
    pub fn parse(&mut self) -> Result<Program, MarretaError> {
        let mut statements = Vec::new();
        self.skip_newlines();

        while !self.is_at_end() {
            let stmt = self.parse_statement()?;
            statements.push(stmt);
            self.skip_newlines();
        }

        Ok(statements)
    }

    // =========================================================================
    // Statement parsing
    // =========================================================================

    fn parse_statement(&mut self) -> Result<Statement, MarretaError> {
        match self.current_kind() {
            TokenKind::Require => self.parse_require(),
            TokenKind::Reject => self.parse_reject(),
            TokenKind::While => self.parse_while(),
            TokenKind::Task => self.parse_task_def(),
            TokenKind::Route => self.parse_route(),
            TokenKind::Schema => self.parse_schema(),
            TokenKind::Reply => self.parse_reply(),
            TokenKind::Fail => self.parse_fail(),
            TokenKind::Export => self.parse_export(),
            TokenKind::Transaction => self.parse_transaction(),
            TokenKind::Raise => self.parse_raise(),
            TokenKind::On => self.parse_on(),
            TokenKind::Nack => self.parse_nack(),
            TokenKind::Identifier(name)
                if name == "auth"
                    && matches!(self.peek_kind(), Some(TokenKind::Identifier(kind)) if kind == "jwt" || kind == "api_key") =>
            {
                self.parse_auth_provider()
            }
            TokenKind::Identifier(name)
                if name == "scenario"
                    && matches!(self.peek_kind(), Some(TokenKind::StringLiteral(_))) =>
            {
                self.parse_scenario()
            }
            TokenKind::Identifier(_) if self.peek_kind() == Some(&TokenKind::Assign) => {
                self.parse_assignment()
            }
            // An Indent can only legitimately follow a block opener (consumed by expect(Indent)).
            // Reaching one here means the line is indented deeper than its block expects, so
            // report it as an indentation error instead of a generic "expected expression".
            TokenKind::Indent => Err(MarretaError::UnexpectedIndentation {
                line: self.current().line,
            }),
            _ => {
                let line = self.current().line;
                let column = self.current().column;
                let expr = self.parse_expression(PREC_NONE)?;
                Ok(Statement::ExpressionStatement {
                    expression: expr,
                    line,
                    column,
                })
            }
        }
    }

    /// `export task|schema|assignment` — wraps the inner statement in Statement::Export.
    /// `export route` is a parse error — routes are always globally visible by nature.
    fn parse_export(&mut self) -> Result<Statement, MarretaError> {
        let (line, column) = {
            let t = &self.tokens[self.pos];
            (t.line, t.column)
        };
        self.advance(); // consume `export`

        match self.current_kind() {
            TokenKind::Task => {
                let inner = self.parse_task_def()?;
                Ok(Statement::Export(Box::new(inner)))
            }
            TokenKind::Schema => {
                let inner = self.parse_schema()?;
                Ok(Statement::Export(Box::new(inner)))
            }
            TokenKind::Identifier(_) if self.peek_kind() == Some(&TokenKind::Assign) => {
                let inner = self.parse_assignment()?;
                Ok(Statement::Export(Box::new(inner)))
            }
            TokenKind::Route => Err(MarretaError::UnexpectedToken {
                expected: "task, schema, or assignment".to_string(),
                got_lexeme: "route".to_string(),
                line,
                column,
            }),
            _ => Err(MarretaError::UnexpectedToken {
                expected: "task, schema, or assignment after export".to_string(),
                got_lexeme: self.tokens[self.pos].lexeme.clone(),
                line,
                column,
            }),
        }
    }

    /// `name = expression` or `name = expression if condition`
    fn parse_assignment(&mut self) -> Result<Statement, MarretaError> {
        let (name, line, column) = self.expect_identifier()?;
        self.expect(TokenKind::Assign)?;

        // Check for `name = match ...` or `name = if ...`
        if matches!(self.current_kind(), TokenKind::Match | TokenKind::If) {
            let expr = if matches!(self.current_kind(), TokenKind::Match) {
                self.parse_match_expression()?
            } else {
                self.parse_if_expression()?
            };
            let expr = self.maybe_parse_pipeline(expr)?;
            return Ok(Statement::Assignment {
                target: name,
                value: expr,
                line,
                column,
            });
        }

        let value = self.parse_expression(PREC_NONE)?;

        // Check for suffix `if`
        if matches!(self.current_kind(), TokenKind::If) {
            self.advance(); // consume `if`
            let condition = self.parse_expression(PREC_NONE)?;
            return Ok(Statement::ConditionalAssignment {
                target: name,
                value,
                condition,
                line,
                column,
            });
        }

        // Check for pipeline on the value
        let value = self.maybe_parse_pipeline(value)?;

        Ok(Statement::Assignment {
            target: name,
            value,
            line,
            column,
        })
    }

    /// `require EXPR else fail CODE, MSG` or `require EXPR else raise MSG`
    fn parse_require(&mut self) -> Result<Statement, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `require`

        let condition = self.parse_expression(PREC_NONE)?;
        self.expect(TokenKind::Else)?;

        if matches!(self.current_kind(), TokenKind::Nack) {
            // `require X else nack [requeue]` — nack fires only when condition is falsy
            self.advance(); // consume `nack`
            let requeue = self.check(&TokenKind::Requeue);
            if requeue {
                self.advance();
            }
            return Ok(Statement::Nack {
                requeue,
                condition: Some(crate::ast::Expression::UnaryOp {
                    operator: crate::ast::UnaryOperator::Not,
                    operand: Box::new(condition),
                }),
                line,
                column,
            });
        }

        if matches!(self.current_kind(), TokenKind::Raise) {
            // `require X else raise MSG` — emits a Raise statement when condition is falsy
            self.advance(); // consume `raise`
            let msg_expr = self.parse_expression(PREC_NONE)?;
            // We wrap this as a Raise with an inverted condition so it only fires when falsy
            return Ok(Statement::Raise {
                message: msg_expr,
                // condition = NOT(original condition) — raise when require would fail
                condition: Some(crate::ast::Expression::UnaryOp {
                    operator: crate::ast::UnaryOperator::Not,
                    operand: Box::new(condition),
                }),
                line,
                column,
            });
        }

        self.expect(TokenKind::Fail)?;

        let error_code = self.expect_integer()?;
        self.expect(TokenKind::Comma)?;
        let error_message = self.expect_string()?;

        Ok(Statement::Require {
            condition,
            error_code,
            error_message,
            line,
            column,
        })
    }

    /// `reject EXPR else fail CODE, MSG`
    fn parse_reject(&mut self) -> Result<Statement, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `reject`

        let condition = self.parse_expression(PREC_NONE)?;
        self.expect(TokenKind::Else)?;
        self.expect(TokenKind::Fail)?;

        let error_code = self.expect_integer()?;
        self.expect(TokenKind::Comma)?;
        let error_message = self.expect_string()?;

        Ok(Statement::Reject {
            condition,
            error_code,
            error_message,
            line,
            column,
        })
    }

    /// `while CONDITION\n  BODY`
    fn parse_while(&mut self) -> Result<Statement, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `while`
        let condition = self.parse_expression(PREC_NONE)?;
        self.expect(TokenKind::Newline)?;
        self.expect(TokenKind::Indent)?;

        let mut body = Vec::new();
        self.skip_newlines();
        while !self.check(&TokenKind::Dedent) && !self.is_at_end() {
            body.push(self.parse_statement()?);
            self.skip_newlines();
        }
        self.expect(TokenKind::Dedent)?;

        Ok(Statement::While {
            condition,
            body,
            line,
            column,
        })
    }

    /// `transaction\n  INDENT body DEDENT` — atomic sequential block.
    /// Nested `transaction` blocks are a parse-time error.
    fn parse_transaction(&mut self) -> Result<Statement, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;

        if self.inside_transaction {
            return Err(MarretaError::UnexpectedToken {
                expected: "statement (not transaction — nested transactions are not allowed)"
                    .to_string(),
                got_lexeme: "transaction".to_string(),
                line,
                column,
            });
        }

        self.advance(); // consume `transaction`
        self.expect(TokenKind::Newline)?;
        self.expect(TokenKind::Indent)?;

        self.inside_transaction = true;
        let mut body = Vec::new();
        self.skip_newlines();
        while !self.check(&TokenKind::Dedent) && !self.is_at_end() {
            body.push(self.parse_statement()?);
            self.skip_newlines();
        }
        self.inside_transaction = false;

        self.expect(TokenKind::Dedent)?;
        Ok(Statement::Transaction { body, line, column })
    }

    // ── Queue statements (v0.8) ───────────────────────────────────────────────

    /// `on queue "name" take binding [as schema]\n  body`
    /// `on topic "name" take binding [as schema]\n  body`
    fn parse_on(&mut self) -> Result<Statement, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `on`

        let is_topic = match self.current().kind.clone() {
            TokenKind::Queue => {
                self.advance();
                false
            }
            TokenKind::Topic => {
                self.advance();
                true
            }
            _ => {
                return Err(MarretaError::UnexpectedToken {
                    expected: "queue or topic".into(),
                    got_lexeme: self.current().lexeme.clone(),
                    line: self.current().line,
                    column: self.current().column,
                });
            }
        };

        let target = self.parse_expression(PREC_NONE)?;
        if is_topic
            && let Expression::StringLiteral(topic) = &target
            && (topic.contains('*') || topic.contains('#'))
        {
            return Err(MarretaError::UnexpectedToken {
                expected: "exact topic string without '*' or '#' wildcards".into(),
                got_lexeme: topic.clone(),
                line,
                column,
            });
        }

        self.expect(TokenKind::Take)?;

        let binding = match self.current().kind.clone() {
            TokenKind::Identifier(name) => {
                self.advance();
                name
            }
            _ => {
                return Err(MarretaError::UnexpectedToken {
                    expected: "identifier (binding name)".into(),
                    got_lexeme: self.current().lexeme.clone(),
                    line: self.current().line,
                    column: self.current().column,
                });
            }
        };

        let schema = if self.check(&TokenKind::As) {
            self.advance(); // consume `as`
            match self.current().kind.clone() {
                TokenKind::Identifier(name) => {
                    self.advance();
                    Some(name)
                }
                _ => {
                    return Err(MarretaError::UnexpectedToken {
                        expected: "schema name".into(),
                        got_lexeme: self.current().lexeme.clone(),
                        line: self.current().line,
                        column: self.current().column,
                    });
                }
            }
        } else {
            None
        };

        self.expect(TokenKind::Newline)?;
        self.expect(TokenKind::Indent)?;
        let mut body = Vec::new();
        self.skip_newlines();
        while !self.check(&TokenKind::Dedent) && !self.is_at_end() {
            body.push(self.parse_statement()?);
            self.skip_newlines();
        }
        self.expect(TokenKind::Dedent)?;

        if is_topic {
            Ok(Statement::OnTopic {
                pattern: target,
                binding,
                schema,
                body,
                line,
                column,
            })
        } else {
            Ok(Statement::OnQueue {
                queue_name: target,
                binding,
                schema,
                body,
                line,
                column,
            })
        }
    }

    /// `nack` or `nack requeue`
    fn parse_nack(&mut self) -> Result<Statement, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `nack`
        let requeue = self.check(&TokenKind::Requeue);
        if requeue {
            self.advance();
        }
        Ok(Statement::Nack {
            requeue,
            condition: None,
            line,
            column,
        })
    }

    /// `scenario "name"\n  INDENT given/when/then DEDENT`
    fn parse_scenario(&mut self) -> Result<Statement, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `scenario`

        let name = self.expect_string()?;
        self.expect(TokenKind::Newline)?;
        self.expect(TokenKind::Indent)?;

        let mut steps = Vec::new();
        self.skip_newlines();
        while !self.check(&TokenKind::Dedent) && !self.is_at_end() {
            let step = match self.current_kind() {
                TokenKind::Identifier(name) if name == "given" => self.parse_scenario_given()?,
                TokenKind::Identifier(name) if name == "when" => self.parse_scenario_when()?,
                TokenKind::Identifier(name) if name == "then" => self.parse_scenario_then()?,
                _ => {
                    return Err(MarretaError::UnexpectedToken {
                        expected: "given, when, or then inside scenario".to_string(),
                        got_lexeme: self.current().lexeme.clone(),
                        line: self.current().line,
                        column: self.current().column,
                    });
                }
            };
            steps.push(step);
            self.skip_newlines();
        }
        self.expect(TokenKind::Dedent)?;

        let when_count = steps
            .iter()
            .filter(|step| matches!(step, ScenarioStep::When { .. }))
            .count();
        let then_count = steps
            .iter()
            .filter(|step| {
                matches!(
                    step,
                    ScenarioStep::ThenStatus { .. } | ScenarioStep::ThenResponse { .. }
                )
            })
            .count();
        if when_count != 1 {
            return Err(MarretaError::UnexpectedToken {
                expected: "exactly one when request inside scenario".to_string(),
                got_lexeme: format!("{when_count} when requests"),
                line,
                column,
            });
        }
        if then_count == 0 {
            return Err(MarretaError::UnexpectedToken {
                expected: "at least one then assertion inside scenario".to_string(),
                got_lexeme: "no then assertions".to_string(),
                line,
                column,
            });
        }

        Ok(Statement::Scenario {
            name,
            steps,
            line,
            column,
        })
    }

    fn parse_scenario_given(&mut self) -> Result<ScenarioStep, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `given`
        let target = self.parse_expression(PREC_NONE)?;
        self.expect_identifier_lexeme("returns")?;
        let returns = self.parse_expression(PREC_NONE)?;
        Ok(ScenarioStep::Given {
            target,
            returns,
            line,
            column,
        })
    }

    fn parse_scenario_when(&mut self) -> Result<ScenarioStep, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `when`
        let verb = self.parse_http_verb()?;
        let path = self.expect_string()?;

        let body = if self.check_identifier_lexeme("with") {
            self.advance();
            // Stop before the scenario-level `and headers` clause instead of
            // parsing it as a boolean expression attached to the request body.
            Some(self.parse_expression(PREC_AND + 1)?)
        } else {
            None
        };

        let headers = if matches!(self.current_kind(), TokenKind::And) {
            self.advance(); // consume `and`
            self.expect_identifier_lexeme("headers")?;
            Some(self.parse_expression(PREC_NONE)?)
        } else {
            None
        };

        Ok(ScenarioStep::When {
            verb,
            path,
            body,
            headers,
            line,
            column,
        })
    }

    fn parse_scenario_then(&mut self) -> Result<ScenarioStep, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `then`

        if self.check_identifier_lexeme("status") {
            self.advance();
            let status = self.parse_expression(PREC_NONE)?;
            return Ok(ScenarioStep::ThenStatus {
                status,
                line,
                column,
            });
        }

        if self.check_identifier_lexeme("response") {
            self.advance();
            self.expect_identifier_lexeme("is")?;
            let expected = self.parse_expression(PREC_NONE)?;
            return Ok(ScenarioStep::ThenResponse {
                expected,
                line,
                column,
            });
        }

        Err(MarretaError::UnexpectedToken {
            expected: "status or response after then".to_string(),
            got_lexeme: self.current().lexeme.clone(),
            line: self.current().line,
            column: self.current().column,
        })
    }

    fn parse_http_verb(&mut self) -> Result<HttpVerb, MarretaError> {
        match self.current_kind() {
            TokenKind::Get => {
                self.advance();
                Ok(HttpVerb::Get)
            }
            TokenKind::Post => {
                self.advance();
                Ok(HttpVerb::Post)
            }
            TokenKind::Put => {
                self.advance();
                Ok(HttpVerb::Put)
            }
            TokenKind::Patch => {
                self.advance();
                Ok(HttpVerb::Patch)
            }
            TokenKind::Delete => {
                self.advance();
                Ok(HttpVerb::Delete)
            }
            _ => {
                let tok = self.current();
                Err(MarretaError::UnexpectedToken {
                    expected: "HTTP verb (GET, POST, PUT, PATCH, DELETE)".into(),
                    got_lexeme: tok.lexeme.clone(),
                    line: tok.line,
                    column: tok.column,
                })
            }
        }
    }

    /// `raise MSG [if CONDITION]`
    fn parse_raise(&mut self) -> Result<Statement, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `raise`

        let message = self.parse_expression(PREC_NONE)?;

        // suffix `if` — `raise MSG if CONDITION`
        let condition = if matches!(self.current_kind(), TokenKind::If) {
            self.advance(); // consume `if`
            Some(self.parse_expression(PREC_NONE)?)
        } else {
            None
        };

        Ok(Statement::Raise {
            message,
            condition,
            line,
            column,
        })
    }

    /// `route VERB "path" [take VAR]\n  INDENT [require auth provider] [allow expr]* body DEDENT`
    fn parse_route(&mut self) -> Result<Statement, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `route`

        let verb = self.parse_http_verb()?;

        let path = self.expect_string()?;

        let take = if matches!(self.current_kind(), TokenKind::Take) {
            self.advance(); // consume `take`
            let mut bindings = Vec::new();
            loop {
                let (name, _, _) = self.expect_identifier()?;
                let binding = match name.as_str() {
                    "query" => TakeBinding::Query(name),
                    "headers" => TakeBinding::Headers(name),
                    "form" => TakeBinding::Form(name),
                    "raw" => TakeBinding::Raw(name),
                    _ => TakeBinding::Payload(name),
                };
                bindings.push(binding);
                if matches!(self.current_kind(), TokenKind::Comma) {
                    self.advance(); // consume `,`
                } else {
                    break;
                }
            }
            bindings
        } else {
            vec![]
        };

        // Optional `as SchemaName` — binds a schema for payload validation
        let schema = if matches!(self.current_kind(), TokenKind::As) {
            self.advance(); // consume `as`
            let (name, _, _) = self.expect_identifier()?;
            Some(name)
        } else {
            None
        };

        self.expect(TokenKind::Newline)?;
        self.expect(TokenKind::Indent)?;

        let mut auth = None;
        let mut allow = Vec::new();
        let mut body = Vec::new();
        self.skip_newlines();

        if matches!(self.current_kind(), TokenKind::Require)
            && self.check_next_identifier_lexeme("auth")
        {
            auth = Some(self.parse_route_auth()?);
            self.skip_newlines();
        }

        while self.check_identifier_lexeme("allow") {
            allow.push(self.parse_route_allow()?);
            self.skip_newlines();
        }

        while !self.check(&TokenKind::Dedent) && !self.is_at_end() {
            if self.check_identifier_lexeme("allow") {
                return Err(MarretaError::UnexpectedToken {
                    expected:
                        "route body statement (allow clauses must appear before the route body)"
                            .to_string(),
                    got_lexeme: self.current().lexeme.clone(),
                    line: self.current().line,
                    column: self.current().column,
                });
            }
            if matches!(self.current_kind(), TokenKind::Require)
                && self.check_next_identifier_lexeme("auth")
            {
                return Err(MarretaError::UnexpectedToken {
                    expected: "route body statement (require auth must appear before allow clauses and body)".to_string(),
                    got_lexeme: self.current().lexeme.clone(),
                    line: self.current().line,
                    column: self.current().column,
                });
            }
            body.push(self.parse_statement()?);
            self.skip_newlines();
        }
        self.expect(TokenKind::Dedent)?;

        Ok(Statement::Route {
            verb,
            path,
            auth,
            allow,
            take,
            schema,
            body,
            line,
            column,
        })
    }

    fn parse_route_auth(&mut self) -> Result<RouteAuth, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `require`
        self.expect_identifier_lexeme("auth")?;
        let (provider, _, _) = self.expect_identifier()?;
        Ok(RouteAuth {
            provider,
            line,
            column,
        })
    }

    fn parse_route_allow(&mut self) -> Result<Expression, MarretaError> {
        self.advance(); // consume `allow`
        self.parse_expression(PREC_NONE)
    }

    /// `auth jwt name { key: value, ... }` or `auth api_key name { ... }`
    fn parse_auth_provider(&mut self) -> Result<Statement, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `auth`

        let (kind, _, _) = self.expect_identifier()?;
        let (name, _, _) = self.expect_identifier()?;
        self.expect(TokenKind::LeftBrace)?;

        let mut fields = Vec::new();
        self.skip_newlines();
        while !self.check(&TokenKind::RightBrace) && !self.is_at_end() {
            let (field_name, field_line, field_column) =
                self.expect_identifier_or_keyword_as_key()?;
            self.expect(TokenKind::Colon)?;
            let value = self.parse_expression(PREC_NONE)?;
            fields.push(AuthProviderField {
                name: field_name,
                value,
                line: field_line,
                column: field_column,
            });

            if self.check(&TokenKind::Comma) {
                self.advance();
            }
            self.skip_newlines();
        }
        self.expect(TokenKind::RightBrace)?;

        let config = AuthProviderConfig { name, fields };
        let provider = match kind.as_str() {
            "jwt" => AuthProvider::Jwt(config),
            "api_key" => AuthProvider::ApiKey(config),
            _ => {
                return Err(MarretaError::UnexpectedToken {
                    expected: "jwt or api_key".to_string(),
                    got_lexeme: kind,
                    line,
                    column,
                });
            }
        };

        Ok(Statement::AuthProvider {
            provider,
            line,
            column,
        })
    }

    /// `schema Name\n  INDENT field: type\n  ... DEDENT`
    /// Fields are indented, one per line. Optional fields have a `?` suffix on the type.
    /// Uses indentation-based scoping (consistent with route, task, match).
    fn parse_schema(&mut self) -> Result<Statement, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `schema`

        let (name, _, _) = self.expect_identifier()?;

        self.expect(TokenKind::Newline)?;
        self.expect(TokenKind::Indent)?;

        let mut db_table = None;
        let mut fields = Vec::new();
        self.skip_newlines();
        while !self.check(&TokenKind::Dedent) && !self.is_at_end() {
            if matches!(self.current_kind(), TokenKind::Db)
                && self.peek_kind() == Some(&TokenKind::Colon)
            {
                self.advance();
                self.expect(TokenKind::Colon)?;
                let (table_name, _, _) = self.expect_identifier()?;
                db_table = Some(table_name);
                self.skip_newlines();
                continue;
            }

            let (raw_name, _, _) = self.expect_identifier()?;
            // `email?` → optional field; strip the trailing `?` from the name
            let (field_name, optional) = if raw_name.ends_with('?') {
                (raw_name.trim_end_matches('?').to_string(), true)
            } else {
                (raw_name, false)
            };
            self.expect(TokenKind::Colon)?;
            let (field_type, _) = self.parse_schema_type()?;
            fields.push(SchemaField {
                name: field_name,
                field_type,
                optional,
            });
            self.skip_newlines();
        }

        self.expect(TokenKind::Dedent)?;

        Ok(Statement::Schema {
            name,
            db_table,
            fields,
            line,
            column,
        })
    }

    /// Parses a schema field type keyword and optional `?` suffix.
    /// Returns `(SchemaType, is_optional)`.
    ///
    /// Supports:
    /// - Primitive types: `string`, `integer`, `float`, `decimal`, `boolean`, `map`
    /// - Typed list: `list of <type>` (v0.4.0)
    /// - Schema reference: any identifier not matching a primitive (v0.4.0)
    fn parse_schema_type(&mut self) -> Result<(SchemaType, bool), MarretaError> {
        let field_type = match self.current_kind() {
            TokenKind::TypeString => {
                self.advance();
                SchemaType::StringType
            }
            TokenKind::TypeInteger => {
                self.advance();
                SchemaType::IntegerType
            }
            TokenKind::TypeFloat => {
                self.advance();
                SchemaType::FloatType
            }
            TokenKind::Identifier(name) if name == "decimal" => {
                self.advance();
                SchemaType::DecimalType
            }
            TokenKind::Identifier(name) if name == "enum" => self.parse_enum_schema_type()?,
            TokenKind::TypeBoolean => {
                self.advance();
                SchemaType::BooleanType
            }
            TokenKind::TypeInstant => {
                self.advance();
                SchemaType::InstantType
            }
            TokenKind::TypeDate => {
                self.advance();
                SchemaType::DateType
            }
            TokenKind::Time => {
                self.advance();
                SchemaType::TimeType
            }
            TokenKind::TypeDuration => {
                self.advance();
                SchemaType::DurationType
            }
            TokenKind::TypeInterval => {
                self.advance();
                SchemaType::IntervalType
            }
            // `map` comes through as TokenKind::Map (pipeline keyword)
            TokenKind::Map => {
                self.advance();
                SchemaType::MapType
            }
            // `list` comes through as Identifier — may be bare `list` or `list of <type>`
            TokenKind::Identifier(name) if name == "list" => {
                self.advance();
                if matches!(self.current_kind(), TokenKind::Of) {
                    self.advance(); // consume `of`
                    let (inner, _) = self.parse_schema_type()?;
                    SchemaType::TypedList(Box::new(inner))
                } else {
                    SchemaType::ListType
                }
            }
            // `map` as identifier fallback
            TokenKind::Identifier(name) if name == "map" => {
                self.advance();
                SchemaType::MapType
            }
            // Any other identifier is a schema reference: `billing: address`
            TokenKind::Identifier(_) => {
                let (schema_name, _, _) = self.expect_identifier()?;
                SchemaType::Reference(schema_name)
            }
            _ => {
                let tok = self.current();
                return Err(MarretaError::UnexpectedToken {
                    expected: "schema type (string, integer, float, decimal, boolean, instant, date, time, duration, interval, enum, list, map, or schema name)".into(),
                    got_lexeme: tok.lexeme.clone(),
                    line: tok.line,
                    column: tok.column,
                });
            }
        };

        // Optionality is indicated by `?` on the field name (e.g., `email?:`), not the type.
        // The second tuple element is always false here; callers use it for forward-compat.
        Ok((field_type, false))
    }

    fn parse_enum_schema_type(&mut self) -> Result<SchemaType, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `enum`
        self.expect(TokenKind::LeftBracket)?;

        let mut values = Vec::new();
        let mut seen = std::collections::HashSet::new();
        if self.check(&TokenKind::RightBracket) {
            return Err(MarretaError::UnexpectedToken {
                expected: "at least one enum string value".to_string(),
                got_lexeme: "]".to_string(),
                line,
                column,
            });
        }

        loop {
            let value = self.expect_string()?;
            if value.is_empty() {
                return Err(MarretaError::UnexpectedToken {
                    expected: "non-empty enum string value".to_string(),
                    got_lexeme: "\"\"".to_string(),
                    line,
                    column,
                });
            }
            if !seen.insert(value.clone()) {
                return Err(MarretaError::UnexpectedToken {
                    expected: "unique enum string values".to_string(),
                    got_lexeme: value,
                    line,
                    column,
                });
            }
            values.push(value);

            if self.check(&TokenKind::RightBracket) {
                break;
            }
            self.expect(TokenKind::Comma)?;
        }

        self.expect(TokenKind::RightBracket)?;
        Ok(SchemaType::EnumType(values))
    }

    /// `reply [html|text] CODE, expression [, headers_map]`
    fn parse_reply(&mut self) -> Result<Statement, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `reply`

        // Optional content-type modifier: `html` or `text` (parsed as identifiers)
        let content_type = if let TokenKind::Identifier(_) = self.current_kind() {
            let name = self.current().lexeme.clone();
            match name.as_str() {
                "html" => {
                    self.advance();
                    ReplyContentType::Html
                }
                "text" => {
                    self.advance();
                    ReplyContentType::Text
                }
                _ => ReplyContentType::Json,
            }
        } else {
            ReplyContentType::Json
        };

        let status_code = self.parse_expression(PREC_NONE)?;

        // Optional `as schema_name` before the comma: `reply 201 as order_result, value`
        let response_schema = if matches!(self.current_kind(), TokenKind::As) {
            self.advance(); // consume `as`
            let (name, _, _) = self.expect_identifier()?;
            Some(name)
        } else {
            None
        };

        self.expect(TokenKind::Comma)?;
        let body = self.parse_expression(PREC_NONE)?;

        // Optional third argument: extra headers map `{ Location: "..." }`
        let extra_headers = if matches!(self.current_kind(), TokenKind::Comma) {
            self.advance(); // consume `,`
            Some(self.parse_expression(PREC_NONE)?)
        } else {
            None
        };

        Ok(Statement::Reply {
            status_code,
            content_type,
            body,
            response_schema,
            extra_headers,
            line,
            column,
        })
    }

    /// `fail CODE, expression`
    fn parse_fail(&mut self) -> Result<Statement, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `fail`

        let status_code = self.expect_integer()?;
        self.expect(TokenKind::Comma)?;
        let message = self.parse_expression(PREC_NONE)?;

        Ok(Statement::Fail {
            status_code,
            message,
            line,
            column,
        })
    }

    /// `task name(params) => expr` or `task name(params)\n  INDENT body DEDENT`
    fn parse_task_def(&mut self) -> Result<Statement, MarretaError> {
        let line = self.current().line;
        let column = self.current().column;
        self.advance(); // consume `task`

        let (name, _, _) = self.expect_identifier()?;
        self.expect(TokenKind::LeftParen)?;
        let params = self.parse_param_list()?;
        self.expect(TokenKind::RightParen)?;
        let body = self.parse_task_like_body()?;

        Ok(Statement::TaskDef {
            name,
            params,
            body,
            line,
            column,
        })
    }

    fn parse_task_like_body(&mut self) -> Result<TaskBody, MarretaError> {
        if matches!(self.current_kind(), TokenKind::FatArrow) {
            self.advance(); // consume `=>`
            let expr = self.parse_expression(PREC_NONE)?;
            return Ok(TaskBody::Inline(expr));
        }

        self.parse_expression_block_body()
    }

    fn parse_expression_block_body(&mut self) -> Result<TaskBody, MarretaError> {
        self.expect(TokenKind::Newline)?;
        self.expect(TokenKind::Indent)?;

        let mut body_stmts = Vec::new();
        self.skip_newlines();

        while !self.check(&TokenKind::Dedent) && !self.is_at_end() {
            let stmt = self.parse_statement()?;
            body_stmts.push(stmt);
            self.skip_newlines();
        }

        self.expect(TokenKind::Dedent)?;

        let final_expr = match body_stmts.pop() {
            Some(Statement::ExpressionStatement { expression, .. }) => expression,
            Some(other) => {
                body_stmts.push(other);
                Expression::Null
            }
            None => Expression::Null,
        };

        Ok(TaskBody::Block(body_stmts, final_expr))
    }

    fn parse_param_list(&mut self) -> Result<Vec<ParamDef>, MarretaError> {
        let mut params = Vec::new();
        if !self.check(&TokenKind::RightParen) {
            let (name, _, _) = self.expect_identifier()?;
            // Optional schema contract: `param as schema_name`
            let schema = if matches!(self.current_kind(), TokenKind::As) {
                self.advance(); // consume `as`
                let (schema_name, _, _) = self.expect_identifier()?;
                Some(schema_name)
            } else {
                None
            };
            params.push(ParamDef { name, schema });
            while self.check(&TokenKind::Comma) {
                self.advance(); // consume `,`
                let (name, _, _) = self.expect_identifier()?;
                let schema = if matches!(self.current_kind(), TokenKind::As) {
                    self.advance();
                    let (schema_name, _, _) = self.expect_identifier()?;
                    Some(schema_name)
                } else {
                    None
                };
                params.push(ParamDef { name, schema });
            }
        }
        Ok(params)
    }

    // =========================================================================
    // Expression parsing — Pratt parser
    // =========================================================================

    fn parse_queue_producer_args(
        &mut self,
    ) -> Result<(Expression, Option<String>, Option<Box<Expression>>), MarretaError> {
        let name = self.parse_expression(PREC_NONE)?;
        let schema = if self.check(&TokenKind::As) {
            self.advance();
            match self.current().kind.clone() {
                TokenKind::Identifier(name) => {
                    self.advance();
                    Some(name)
                }
                _ => {
                    return Err(MarretaError::UnexpectedToken {
                        expected: "schema name".into(),
                        got_lexeme: self.current().lexeme.clone(),
                        line: self.current().line,
                        column: self.current().column,
                    });
                }
            }
        } else {
            None
        };
        let payload = if self.check(&TokenKind::Comma) {
            self.advance();
            Some(Box::new(self.parse_expression(PREC_NONE)?))
        } else {
            None
        };
        Ok((name, schema, payload))
    }

    fn parse_expression(&mut self, min_prec: u8) -> Result<Expression, MarretaError> {
        let mut left = self.parse_prefix()?;

        while !self.is_at_end() {
            let prec = self.current_infix_precedence();
            if prec <= min_prec {
                break;
            }
            left = self.parse_infix(left, prec)?;
        }

        if min_prec == PREC_NONE
            && matches!(self.current_kind(), TokenKind::As)
            && Self::is_http_client_method_call(&left)
        {
            self.advance(); // consume `as`
            let (schema_name, _, _) = self.expect_identifier()?;
            left = Expression::HttpClientResponseSchema {
                call: Box::new(left),
                schema_name,
            };
        }

        // `rescue` as lowest-precedence infix modifier: `expr rescue handler`
        // Only parse at PREC_NONE level (not when called with higher min_prec)
        if min_prec == PREC_NONE && matches!(self.current_kind(), TokenKind::Rescue) {
            self.advance(); // consume `rescue`
            let handler = self.parse_expression(PREC_NONE)?;
            left = Expression::Rescue {
                expr: Box::new(left),
                handler: Box::new(handler),
            };
        }

        Ok(left)
    }

    /// Parses a prefix expression (literal, identifier, unary, grouped, list, map).
    fn parse_prefix(&mut self) -> Result<Expression, MarretaError> {
        match self.current_kind().clone() {
            TokenKind::Integer(n) => {
                self.advance();
                Ok(Expression::Integer(n))
            }
            TokenKind::Float(n) => {
                self.advance();
                Ok(Expression::Float(n))
            }
            TokenKind::StringLiteral(s) => {
                self.advance();
                Ok(Expression::StringLiteral(s))
            }
            TokenKind::True => {
                self.advance();
                Ok(Expression::Boolean(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(Expression::Boolean(false))
            }
            TokenKind::Null => {
                self.advance();
                Ok(Expression::Null)
            }

            // Unary `not`
            TokenKind::Not => {
                self.advance();
                let operand = self.parse_expression(PREC_UNARY)?;
                Ok(Expression::UnaryOp {
                    operator: UnaryOperator::Not,
                    operand: Box::new(operand),
                })
            }

            // Unary `-`
            TokenKind::Minus => {
                self.advance();
                let operand = self.parse_expression(PREC_UNARY)?;
                Ok(Expression::UnaryOp {
                    operator: UnaryOperator::Negate,
                    operand: Box::new(operand),
                })
            }

            // Grouped expression `(expr)`
            TokenKind::LeftParen => {
                self.advance(); // consume `(`
                let expr = self.parse_expression(PREC_NONE)?;
                self.expect(TokenKind::RightParen)?;
                Ok(expr)
            }

            // List literal `[expr, ...]`
            TokenKind::LeftBracket => self.parse_list_literal(),

            // Map literal `{ key: value, ... }`
            TokenKind::LeftBrace => self.parse_map_literal(),

            // `task(name)` in pipeline context
            TokenKind::Task if self.peek_kind() == Some(&TokenKind::LeftParen) => {
                self.advance(); // consume `task`
                self.advance(); // consume `(`
                let (name, _, _) = self.expect_identifier()?;
                self.expect(TokenKind::RightParen)?;
                Ok(Expression::TaskCall { name })
            }

            // Identifier — could be simple, function call, or property access
            TokenKind::Identifier(name) => {
                self.advance();
                if self.check(&TokenKind::LeftBrace) {
                    return self.parse_schema_constructor(name);
                }
                // Function call: `name(args)`
                if self.check(&TokenKind::LeftParen) {
                    self.advance(); // consume `(`
                    let arguments = self.parse_argument_list()?;
                    self.expect(TokenKind::RightParen)?;
                    Ok(Expression::FunctionCall { name, arguments })
                } else {
                    Ok(Expression::Identifier(name))
                }
            }

            // Match expression as a standalone expression
            TokenKind::Match => self.parse_match_expression(),

            // If expression as a standalone expression
            TokenKind::If => self.parse_if_expression(),

            // Infrastructure keywords used as identifiers (db, queue, cache, fs)
            TokenKind::Db => {
                self.advance();
                Ok(Expression::Identifier("db".into()))
            }
            // queue.push — point-to-point queue producer (v0.8)
            TokenKind::Queue => {
                self.advance(); // consume `queue`
                if self.check(&TokenKind::Dot) {
                    self.advance(); // consume `.`
                    match self.current().kind.clone() {
                        TokenKind::Identifier(method) if method == "push" => {
                            self.advance(); // consume `push`
                            let (queue_name, schema, payload) = self.parse_queue_producer_args()?;
                            Ok(Expression::QueuePush {
                                queue_name: Box::new(queue_name),
                                schema,
                                payload,
                            })
                        }
                        // `queue.publish` was renamed to `topic.publish` — point the
                        // way rather than just saying "expected push".
                        TokenKind::Identifier(method) if method == "publish" => {
                            Err(MarretaError::UnexpectedToken {
                                expected: "push (use topic.publish to publish to a topic)".into(),
                                got_lexeme: self.current().lexeme.clone(),
                                line: self.current().line,
                                column: self.current().column,
                            })
                        }
                        _ => Err(MarretaError::UnexpectedToken {
                            expected: "push".into(),
                            got_lexeme: self.current().lexeme.clone(),
                            line: self.current().line,
                            column: self.current().column,
                        }),
                    }
                } else {
                    Ok(Expression::Identifier("queue".into()))
                }
            }
            // topic.publish — pub/sub topic producer (Spec 060)
            TokenKind::Topic => {
                self.advance(); // consume `topic`
                if self.check(&TokenKind::Dot) {
                    self.advance(); // consume `.`
                    match self.current().kind.clone() {
                        TokenKind::Identifier(method) if method == "publish" => {
                            self.advance(); // consume `publish`
                            let (topic, schema, payload) = self.parse_queue_producer_args()?;
                            Ok(Expression::TopicPublish {
                                topic: Box::new(topic),
                                schema,
                                payload,
                            })
                        }
                        _ => Err(MarretaError::UnexpectedToken {
                            expected: "publish".into(),
                            got_lexeme: self.current().lexeme.clone(),
                            line: self.current().line,
                            column: self.current().column,
                        }),
                    }
                } else {
                    Ok(Expression::Identifier("topic".into()))
                }
            }
            TokenKind::Cache => {
                self.advance();
                Ok(Expression::Identifier("cache".into()))
            }
            TokenKind::Fs => {
                self.advance();
                Ok(Expression::Identifier("fs".into()))
            }
            TokenKind::Json => {
                self.advance();
                Ok(Expression::Identifier("json".into()))
            }
            TokenKind::Base64 => {
                self.advance();
                Ok(Expression::Identifier("base64".into()))
            }
            TokenKind::Uuid => {
                self.advance();
                Ok(Expression::Identifier("uuid".into()))
            }
            TokenKind::Log => {
                self.advance();
                Ok(Expression::Identifier("log".into()))
            }
            TokenKind::Time => {
                self.advance();
                Ok(Expression::Identifier("time".into()))
            }
            TokenKind::Math => {
                self.advance();
                Ok(Expression::Identifier("math".into()))
            }
            TokenKind::HttpClient => {
                self.advance();
                Ok(Expression::Identifier("http_client".into()))
            }

            // `fail CODE, MSG` used in expression position (e.g. rescue handler)
            TokenKind::Fail => {
                self.advance(); // consume `fail`
                let code = self.expect_integer()?;
                self.expect(TokenKind::Comma)?;
                let msg = self.parse_expression(PREC_NONE)?;
                // Encode as FunctionCall to a synthetic built-in that the interpreter handles
                Ok(Expression::FunctionCall {
                    name: "__fail__".to_string(),
                    arguments: vec![
                        Argument::Positional(Expression::Integer(code)),
                        Argument::Positional(msg),
                    ],
                })
            }

            _ => Err(MarretaError::UnexpectedToken {
                expected: "expression".into(),
                got_lexeme: self.current().lexeme.clone(),
                line: self.current().line,
                column: self.current().column,
            }),
        }
    }

    /// Parses an infix expression (binary op, property access, method call, pipeline).
    fn parse_infix(&mut self, left: Expression, prec: u8) -> Result<Expression, MarretaError> {
        match self.current_kind().clone() {
            TokenKind::Identifier(name) if name == "in" => {
                self.advance();
                let right = self.parse_expression(prec)?;
                Ok(Expression::BinaryOp {
                    left: Box::new(left),
                    operator: BinaryOperator::In,
                    right: Box::new(right),
                })
            }

            // Binary operators
            TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Percent
            | TokenKind::Equal
            | TokenKind::NotEqual
            | TokenKind::Greater
            | TokenKind::Less
            | TokenKind::GreaterEqual
            | TokenKind::LessEqual
            | TokenKind::And
            | TokenKind::Or => {
                let operator = self.token_to_binop()?;
                self.advance();
                let right = self.parse_expression(prec)?;
                Ok(Expression::BinaryOp {
                    left: Box::new(left),
                    operator,
                    right: Box::new(right),
                })
            }

            // Subscript access: `expr[key]`
            TokenKind::LeftBracket => {
                self.advance(); // consume `[`
                let key = self.parse_expression(PREC_NONE)?;
                self.expect(TokenKind::RightBracket)?;
                Ok(Expression::Subscript {
                    object: Box::new(left),
                    key: Box::new(key),
                })
            }

            // Property access / method call: `.field` or `.method(args)`
            TokenKind::Dot => {
                self.advance(); // consume `.`
                let (name, _, _) = self.parse_member_name()?;
                if self.check(&TokenKind::LeftParen) {
                    self.advance(); // consume `(`
                    let arguments = self.parse_argument_list()?;
                    self.expect(TokenKind::RightParen)?;
                    Ok(Expression::MethodCall {
                        object: Box::new(left),
                        method: name,
                        arguments,
                    })
                } else {
                    Ok(Expression::PropertyAccess {
                        object: Box::new(left),
                        property: name,
                    })
                }
            }

            // Pipeline: `expr >> stage`
            TokenKind::Pipeline => {
                let pipeline = self.parse_pipeline_stages(left)?;
                Ok(pipeline)
            }

            // Broadcast: `expr *>> destinations`
            TokenKind::Broadcast => {
                self.advance(); // consume `*>>`
                self.skip_newlines();
                let mut targets = Vec::new();

                if self.check(&TokenKind::Indent) {
                    self.advance(); // consume Indent
                    self.skip_newlines();
                    while self.check(&TokenKind::Arrow) {
                        self.advance(); // consume `->`
                        let target = self.parse_expression(PREC_NONE)?;
                        targets.push(target);
                        self.skip_newlines();
                    }
                    self.expect(TokenKind::Dedent)?;
                } else {
                    // Single-line broadcast: `expr *>> -> target`
                    self.expect(TokenKind::Arrow)?;
                    let target = self.parse_expression(PREC_NONE)?;
                    targets.push(target);
                }

                Ok(Expression::Broadcast {
                    input: Box::new(left),
                    targets,
                })
            }

            _ => Ok(left),
        }
    }

    /// Returns the infix precedence of the current token.
    fn current_infix_precedence(&self) -> u8 {
        if self.is_at_end() {
            return PREC_NONE;
        }
        match self.current_kind() {
            TokenKind::Or => PREC_OR,
            TokenKind::And => PREC_AND,
            TokenKind::Equal | TokenKind::NotEqual => PREC_EQUALITY,
            TokenKind::Greater
            | TokenKind::Less
            | TokenKind::GreaterEqual
            | TokenKind::LessEqual => PREC_COMPARISON,
            TokenKind::Identifier(name) if name == "in" => PREC_COMPARISON,
            TokenKind::Plus | TokenKind::Minus => PREC_ADDITION,
            TokenKind::Star | TokenKind::Slash | TokenKind::Percent => PREC_MULTIPLICATION,
            TokenKind::Dot | TokenKind::LeftBracket => PREC_CALL,
            TokenKind::Pipeline | TokenKind::Broadcast => PREC_OR, // low precedence, binds loosely
            _ => PREC_NONE,
        }
    }

    fn token_to_binop(&self) -> Result<BinaryOperator, MarretaError> {
        match self.current_kind() {
            TokenKind::Plus => Ok(BinaryOperator::Add),
            TokenKind::Minus => Ok(BinaryOperator::Subtract),
            TokenKind::Star => Ok(BinaryOperator::Multiply),
            TokenKind::Slash => Ok(BinaryOperator::Divide),
            TokenKind::Percent => Ok(BinaryOperator::Modulo),
            TokenKind::Equal => Ok(BinaryOperator::Equal),
            TokenKind::NotEqual => Ok(BinaryOperator::NotEqual),
            TokenKind::Greater => Ok(BinaryOperator::Greater),
            TokenKind::Less => Ok(BinaryOperator::Less),
            TokenKind::GreaterEqual => Ok(BinaryOperator::GreaterEqual),
            TokenKind::LessEqual => Ok(BinaryOperator::LessEqual),
            TokenKind::Identifier(name) if name == "in" => Ok(BinaryOperator::In),
            TokenKind::And => Ok(BinaryOperator::And),
            TokenKind::Or => Ok(BinaryOperator::Or),
            _ => Err(MarretaError::UnexpectedToken {
                expected: "binary operator".into(),
                got_lexeme: self.current().lexeme.clone(),
                line: self.current().line,
                column: self.current().column,
            }),
        }
    }

    // =========================================================================
    // Pipeline parsing
    // =========================================================================

    fn parse_pipeline_stages(&mut self, input: Expression) -> Result<Expression, MarretaError> {
        let mut stages = Vec::new();

        while self.check(&TokenKind::Pipeline) {
            self.advance(); // consume `>>`
            self.skip_newlines();

            if matches!(self.current_kind(), TokenKind::Map) {
                stages.push(self.parse_map_stage()?);
            } else if matches!(self.current_kind(), TokenKind::Reduce) {
                stages.push(self.parse_reduce_stage()?);
            } else if matches!(self.current_kind(), TokenKind::Rescue) {
                stages.push(self.parse_rescue_stage()?);
                // rescue is always terminal — stop parsing more stages
                break;
            } else {
                let expr = self.parse_expression(PREC_CALL - 1)?; // include dot-access but not >>
                stages.push(PipelineStage::Expression(expr));
            }
            self.skip_newlines();
        }

        Ok(Expression::Pipeline {
            input: Box::new(input),
            stages,
        })
    }

    /// `rescue [EXPR]` or `rescue\n  INDENT body DEDENT` — terminal pipeline error-capture stage.
    fn parse_rescue_stage(&mut self) -> Result<PipelineStage, MarretaError> {
        self.advance(); // consume `rescue`

        // Check if next token is a newline followed by indent — block form
        let saved = self.pos;
        self.skip_newlines();
        if self.check(&TokenKind::Indent) {
            self.advance(); // consume indent
            let mut body: Vec<Statement> = Vec::new();
            self.skip_newlines();
            while !self.check(&TokenKind::Dedent) && !self.is_at_end() {
                body.push(self.parse_statement()?);
                self.skip_newlines();
            }
            self.expect(TokenKind::Dedent)?;
            return Ok(PipelineStage::Rescue {
                handler: RescueHandler::Block(body),
            });
        }
        // Not a block — restore and check if there's an inline handler on this line
        self.pos = saved;

        // Inline: if there's an expression on the same logical line, parse it
        if !matches!(
            self.current_kind(),
            TokenKind::Newline | TokenKind::Dedent | TokenKind::Eof
        ) {
            let expr = self.parse_expression(PREC_NONE)?;
            return Ok(PipelineStage::Rescue {
                handler: RescueHandler::Inline(expr),
            });
        }

        // No handler at all — treat as `rescue null` (silences error)
        Ok(PipelineStage::Rescue {
            handler: RescueHandler::Inline(Expression::Null),
        })
    }

    /// `map variable\n  INDENT body DEDENT`
    /// Body contains regular statements plus `keep [expr] [if cond]` and `skip if cond`.
    fn parse_map_stage(&mut self) -> Result<PipelineStage, MarretaError> {
        self.advance(); // consume `map`
        let (variable, _, _) = self.expect_identifier()?;
        self.expect(TokenKind::Newline)?;
        self.expect(TokenKind::Indent)?;

        let mut body: Vec<MapStatement> = Vec::new();
        self.skip_newlines();

        while !self.check(&TokenKind::Dedent) && !self.is_at_end() {
            if self.check(&TokenKind::Keep) {
                self.advance(); // consume `keep`
                let value = self.parse_expression(PREC_NONE)?;
                let condition = if self.check(&TokenKind::If) {
                    self.advance(); // consume `if`
                    Some(self.parse_expression(PREC_NONE)?)
                } else {
                    None
                };
                body.push(MapStatement::Keep { value, condition });
            } else if self.check(&TokenKind::Skip) {
                self.advance(); // consume `skip`
                self.expect(TokenKind::If)?;
                let condition = self.parse_expression(PREC_NONE)?;
                body.push(MapStatement::Skip { condition });
            } else {
                let stmt = self.parse_statement()?;
                body.push(MapStatement::Statement(stmt));
            }
            self.skip_newlines();
        }

        self.expect(TokenKind::Dedent)?;

        Ok(PipelineStage::Map { variable, body })
    }

    /// `reduce(INITIAL) acc, item\n  BODY`
    fn parse_reduce_stage(&mut self) -> Result<PipelineStage, MarretaError> {
        self.advance(); // consume `reduce`
        self.expect(TokenKind::LeftParen)?;
        let initial = self.parse_expression(PREC_NONE)?;
        self.expect(TokenKind::RightParen)?;
        let (accumulator, _, _) = self.expect_identifier()?;
        self.expect(TokenKind::Comma)?;
        let (item, _, _) = self.expect_identifier()?;
        let body = self.parse_task_like_body()?;

        Ok(PipelineStage::Reduce {
            initial,
            accumulator,
            item,
            body,
        })
    }

    /// Wraps an expression in a pipeline if `>>` follows (possibly after newline+indent).
    fn maybe_parse_pipeline(&mut self, expr: Expression) -> Result<Expression, MarretaError> {
        // Pipeline/broadcast may start on the next indented line
        let saved = self.pos;
        self.skip_newlines();
        if self.check(&TokenKind::Indent) {
            self.advance();
            if self.check(&TokenKind::Pipeline) {
                return self.parse_pipeline_stages(expr);
            } else if self.check(&TokenKind::Broadcast) {
                return self.parse_infix(expr, PREC_OR);
            }
            // Not a pipeline — rewind
            self.pos = saved;
        } else if self.check(&TokenKind::Pipeline) {
            return self.parse_pipeline_stages(expr);
        } else if self.check(&TokenKind::Broadcast) {
            return self.parse_infix(expr, PREC_OR);
        } else {
            self.pos = saved;
        }
        Ok(expr)
    }

    // =========================================================================
    // Match expression
    // =========================================================================

    fn parse_match_expression(&mut self) -> Result<Expression, MarretaError> {
        self.advance(); // consume `match`
        let subject = self.parse_expression(PREC_NONE)?;
        self.expect(TokenKind::Newline)?;
        self.expect(TokenKind::Indent)?;

        let mut arms = Vec::new();
        self.skip_newlines();

        while !self.check(&TokenKind::Dedent) && !self.is_at_end() {
            let pattern = if matches!(self.current_kind(), TokenKind::Fallback) {
                self.advance();
                MatchPattern::Fallback
            } else {
                let expr = self.parse_expression(PREC_NONE)?;
                MatchPattern::Literal(expr)
            };

            self.expect(TokenKind::Arrow)?;
            let value = self.parse_expression(PREC_NONE)?;
            arms.push(MatchArm { pattern, value });
            self.skip_newlines();
        }

        self.expect(TokenKind::Dedent)?;

        Ok(Expression::Match {
            subject: Box::new(subject),
            arms,
        })
    }

    fn parse_if_expression(&mut self) -> Result<Expression, MarretaError> {
        self.advance(); // consume `if`
        let condition = self.parse_expression(PREC_NONE)?;
        let then_branch = Box::new(self.parse_expression_block_body()?);

        let else_branch = if self.check(&TokenKind::Else) {
            self.advance(); // consume `else`
            if self.check(&TokenKind::If) {
                let nested = self.parse_if_expression()?;
                Some(Box::new(TaskBody::Inline(nested)))
            } else {
                Some(Box::new(self.parse_expression_block_body()?))
            }
        } else {
            None
        };

        Ok(Expression::If {
            condition: Box::new(condition),
            then_branch,
            else_branch,
        })
    }

    // =========================================================================
    // Collection literals
    // =========================================================================

    fn parse_list_literal(&mut self) -> Result<Expression, MarretaError> {
        self.advance(); // consume `[`
        let mut elements = Vec::new();

        while !self.check(&TokenKind::RightBracket) && !self.is_at_end() {
            let expr = self.parse_expression(PREC_NONE)?;
            elements.push(expr);
            if !self.check(&TokenKind::RightBracket) {
                self.expect(TokenKind::Comma)?;
            }
        }
        self.expect(TokenKind::RightBracket)?;
        Ok(Expression::List(elements))
    }

    fn parse_map_literal(&mut self) -> Result<Expression, MarretaError> {
        self.advance(); // consume `{`
        let mut pairs = Vec::new();

        while !self.check(&TokenKind::RightBrace) && !self.is_at_end() {
            // Map keys may be plain identifiers OR reserved keywords used as keys
            // (e.g. `{ match: ... }`, `{ limit: ... }`, `{ count: ... }`).
            let (key, _, _) = self.expect_identifier_or_keyword_as_key()?;
            self.expect(TokenKind::Colon)?;
            let value = self.parse_expression(PREC_NONE)?;
            pairs.push((key, value));
            if !self.check(&TokenKind::RightBrace) {
                self.expect(TokenKind::Comma)?;
            }
        }
        self.expect(TokenKind::RightBrace)?;
        Ok(Expression::MapLiteral(pairs))
    }

    fn parse_schema_constructor(
        &mut self,
        schema_name: String,
    ) -> Result<Expression, MarretaError> {
        self.expect(TokenKind::LeftBrace)?;
        let mut fields = Vec::new();

        while !self.check(&TokenKind::RightBrace) && !self.is_at_end() {
            self.skip_newlines();
            if self.check(&TokenKind::RightBrace) {
                break;
            }
            let (key, _, _) = self.expect_identifier_or_keyword_as_key()?;
            self.expect(TokenKind::Colon)?;
            let value = self.parse_expression(PREC_NONE)?;
            fields.push((key, value));
            if self.check(&TokenKind::Comma) {
                self.advance();
            }
            self.skip_newlines();
        }
        self.expect(TokenKind::RightBrace)?;

        Ok(Expression::SchemaConstructor {
            schema_name,
            fields,
        })
    }

    fn is_http_client_method_call(expr: &Expression) -> bool {
        matches!(
            expr,
            Expression::MethodCall { object, .. }
                if matches!(object.as_ref(), Expression::Identifier(name) if name == "http_client")
        )
    }

    // =========================================================================
    // Argument list
    // =========================================================================

    fn parse_argument_list(&mut self) -> Result<Vec<Argument>, MarretaError> {
        let mut args = Vec::new();
        if self.check(&TokenKind::RightParen) {
            return Ok(args);
        }

        loop {
            // Check for named argument: `name: expr`
            // Allows some keywords as arg names: `as:`, `on:` (used in join/pipeline ops)
            let named_arg_name: Option<String> = match self.current_kind().clone() {
                TokenKind::Identifier(ref n) if self.peek_kind() == Some(&TokenKind::Colon) => {
                    Some(n.clone())
                }
                TokenKind::As if self.peek_kind() == Some(&TokenKind::Colon) => {
                    Some("as".to_string())
                }
                TokenKind::On if self.peek_kind() == Some(&TokenKind::Colon) => {
                    Some("on".to_string())
                }
                _ => None,
            };
            if let Some(name) = named_arg_name {
                self.advance(); // consume identifier / keyword
                self.advance(); // consume `:`
                let value = self.parse_expression(PREC_NONE)?;
                args.push(Argument::Named { name, value });
                if self.check(&TokenKind::RightParen) {
                    break;
                }
                self.expect(TokenKind::Comma)?;
                continue;
            }

            let expr = self.parse_expression(PREC_NONE)?;
            args.push(Argument::Positional(expr));
            if !self.check(&TokenKind::RightParen) {
                self.expect(TokenKind::Comma)?;
            }

            if self.check(&TokenKind::RightParen) {
                break;
            }
        }

        Ok(args)
    }

    fn parse_member_name(&mut self) -> Result<(String, usize, usize), MarretaError> {
        match self.current_kind().clone() {
            TokenKind::Identifier(name) => {
                let line = self.current().line;
                let column = self.current().column;
                self.advance();
                Ok((name, line, column))
            }
            TokenKind::Time => {
                let line = self.current().line;
                let column = self.current().column;
                self.advance();
                Ok(("time".into(), line, column))
            }
            TokenKind::Base64 => {
                let line = self.current().line;
                let column = self.current().column;
                self.advance();
                Ok(("base64".into(), line, column))
            }
            TokenKind::Uuid => {
                let line = self.current().line;
                let column = self.current().column;
                self.advance();
                Ok(("uuid".into(), line, column))
            }
            TokenKind::Log => {
                let line = self.current().line;
                let column = self.current().column;
                self.advance();
                Ok(("log".into(), line, column))
            }
            TokenKind::TypeInstant => {
                let line = self.current().line;
                let column = self.current().column;
                self.advance();
                Ok(("instant".into(), line, column))
            }
            TokenKind::TypeDate => {
                let line = self.current().line;
                let column = self.current().column;
                self.advance();
                Ok(("date".into(), line, column))
            }
            TokenKind::TypeDuration => {
                let line = self.current().line;
                let column = self.current().column;
                self.advance();
                Ok(("duration".into(), line, column))
            }
            TokenKind::TypeInterval => {
                let line = self.current().line;
                let column = self.current().column;
                self.advance();
                Ok(("interval".into(), line, column))
            }
            TokenKind::On => {
                let line = self.current().line;
                let column = self.current().column;
                self.advance();
                Ok(("on".into(), line, column))
            }
            _ => Err(MarretaError::UnexpectedToken {
                expected: "identifier".into(),
                got_lexeme: self.current().lexeme.clone(),
                line: self.current().line,
                column: self.current().column,
            }),
        }
    }

    // =========================================================================
    // Token navigation helpers
    // =========================================================================

    fn is_at_end(&self) -> bool {
        self.pos >= self.tokens.len() || matches!(self.tokens[self.pos].kind, TokenKind::Eof)
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn current_kind(&self) -> &TokenKind {
        &self.current().kind
    }

    fn peek_kind(&self) -> Option<&TokenKind> {
        self.tokens.get(self.pos + 1).map(|t| &t.kind)
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn check(&self, kind: &TokenKind) -> bool {
        !self.is_at_end()
            && std::mem::discriminant(self.current_kind()) == std::mem::discriminant(kind)
    }

    fn skip_newlines(&mut self) {
        while !self.is_at_end() && matches!(self.current_kind(), TokenKind::Newline) {
            self.advance();
        }
    }

    fn expect(&mut self, kind: TokenKind) -> Result<&Token, MarretaError> {
        if self.is_at_end() {
            return Err(MarretaError::UnexpectedEndOfInput {
                expected: format!("{:?}", kind),
            });
        }
        if std::mem::discriminant(self.current_kind()) == std::mem::discriminant(&kind) {
            Ok(self.advance())
        } else {
            Err(MarretaError::UnexpectedToken {
                expected: format!("{:?}", kind),
                got_lexeme: self.current().lexeme.clone(),
                line: self.current().line,
                column: self.current().column,
            })
        }
    }

    fn expect_identifier(&mut self) -> Result<(String, usize, usize), MarretaError> {
        if self.is_at_end() {
            return Err(MarretaError::UnexpectedEndOfInput {
                expected: "identifier".into(),
            });
        }
        match self.current_kind().clone() {
            TokenKind::Identifier(name) => {
                let line = self.current().line;
                let col = self.current().column;
                self.advance();
                Ok((name, line, col))
            }
            _ => Err(MarretaError::UnexpectedToken {
                expected: "identifier".into(),
                got_lexeme: self.current().lexeme.clone(),
                line: self.current().line,
                column: self.current().column,
            }),
        }
    }

    fn check_identifier_lexeme(&self, expected: &str) -> bool {
        matches!(self.current_kind(), TokenKind::Identifier(name) if name == expected)
    }

    fn check_next_identifier_lexeme(&self, expected: &str) -> bool {
        matches!(self.peek_kind(), Some(TokenKind::Identifier(name)) if name == expected)
    }

    fn expect_identifier_lexeme(&mut self, expected: &str) -> Result<(), MarretaError> {
        if self.check_identifier_lexeme(expected) {
            self.advance();
            Ok(())
        } else {
            Err(MarretaError::UnexpectedToken {
                expected: expected.to_string(),
                got_lexeme: self.current().lexeme.clone(),
                line: self.current().line,
                column: self.current().column,
            })
        }
    }

    /// Like `expect_identifier` but also accepts reserved keyword tokens as map keys.
    /// Returns the keyword's lexeme as the key string.
    fn expect_identifier_or_keyword_as_key(
        &mut self,
    ) -> Result<(String, usize, usize), MarretaError> {
        if self.is_at_end() {
            return Err(MarretaError::UnexpectedEndOfInput {
                expected: "identifier".into(),
            });
        }
        let line = self.current().line;
        let col = self.current().column;
        let lexeme = self.current().lexeme.clone();
        match self.current_kind().clone() {
            TokenKind::Identifier(name) => {
                self.advance();
                Ok((name, line, col))
            }
            // Allow any keyword to be used as a map key
            TokenKind::Match
            | TokenKind::If
            | TokenKind::Else
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Null
            | TokenKind::And
            | TokenKind::Or
            | TokenKind::Not
            | TokenKind::As
            | TokenKind::Of
            | TokenKind::Skip
            | TokenKind::Map
            | TokenKind::Keep
            | TokenKind::Require
            | TokenKind::Reject
            | TokenKind::Task
            | TokenKind::Schema
            | TokenKind::Export
            | TokenKind::Fallback
            | TokenKind::Listen
            | TokenKind::Cache
            | TokenKind::Fs
            | TokenKind::Json
            | TokenKind::Base64
            | TokenKind::Log
            | TokenKind::Time
            | TokenKind::Math
            | TokenKind::HttpClient
            | TokenKind::Transaction
            | TokenKind::Raise
            | TokenKind::Rescue => {
                self.advance();
                Ok((lexeme, line, col))
            }
            _ => Err(MarretaError::UnexpectedToken {
                expected: "identifier".into(),
                got_lexeme: lexeme,
                line,
                column: col,
            }),
        }
    }

    fn expect_integer(&mut self) -> Result<i64, MarretaError> {
        if let TokenKind::Integer(n) = self.current_kind() {
            let n = *n;
            self.advance();
            Ok(n)
        } else {
            Err(MarretaError::UnexpectedToken {
                expected: "integer".into(),
                got_lexeme: self.current().lexeme.clone(),
                line: self.current().line,
                column: self.current().column,
            })
        }
    }

    fn expect_string(&mut self) -> Result<String, MarretaError> {
        if let TokenKind::StringLiteral(s) = self.current_kind().clone() {
            self.advance();
            Ok(s)
        } else {
            Err(MarretaError::UnexpectedToken {
                expected: "string".into(),
                got_lexeme: self.current().lexeme.clone(),
                line: self.current().line,
                column: self.current().column,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(source: &str) -> Program {
        let tokens = Lexer::new(source).tokenize().unwrap();
        Parser::new(tokens).parse().unwrap()
    }

    fn parse_err(source: &str) -> MarretaError {
        let tokens = Lexer::new(source).tokenize().unwrap();
        Parser::new(tokens).parse().unwrap_err()
    }

    #[test]
    fn over_indented_line_reports_indentation_error() {
        // `b = 2` is indented deeper than the route body expects: an indentation error,
        // not the generic "expected expression" fallthrough (Spec 065).
        let err = parse_err("route GET \"/x\"\n    a = 1\n        b = 2\n");
        assert!(
            matches!(err, MarretaError::UnexpectedIndentation { line } if line == 3),
            "got {err:?}"
        );
    }

    // --- Assignments ---

    #[test]
    fn test_simple_assignment() {
        let prog = parse("x = 42");
        assert_eq!(prog.len(), 1);
        match &prog[0] {
            Statement::Assignment { target, value, .. } => {
                assert_eq!(target, "x");
                assert_eq!(*value, Expression::Integer(42));
            }
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_string_assignment() {
        let prog = parse("name = \"Marreta\"");
        match &prog[0] {
            Statement::Assignment { value, .. } => {
                assert_eq!(*value, Expression::StringLiteral("Marreta".into()));
            }
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_conditional_assignment() {
        let prog = parse("status = \"ok\" if active");
        match &prog[0] {
            Statement::ConditionalAssignment {
                target,
                value,
                condition,
                ..
            } => {
                assert_eq!(target, "status");
                assert_eq!(*value, Expression::StringLiteral("ok".into()));
                assert_eq!(*condition, Expression::Identifier("active".into()));
            }
            _ => panic!("expected ConditionalAssignment"),
        }
    }

    #[test]
    fn test_multiple_assignments() {
        let prog = parse("x = 1\ny = 2\nz = 3");
        assert_eq!(prog.len(), 3);
    }

    // --- Expressions ---

    #[test]
    fn test_binary_arithmetic() {
        let prog = parse("x = 1 + 2 * 3");
        match &prog[0] {
            Statement::Assignment { value, .. } => {
                // Should be: 1 + (2 * 3) due to precedence
                match value {
                    Expression::BinaryOp {
                        left,
                        operator,
                        right,
                    } => {
                        assert_eq!(**left, Expression::Integer(1));
                        assert_eq!(*operator, BinaryOperator::Add);
                        match right.as_ref() {
                            Expression::BinaryOp { operator, .. } => {
                                assert_eq!(*operator, BinaryOperator::Multiply);
                            }
                            _ => panic!("expected nested BinaryOp"),
                        }
                    }
                    _ => panic!("expected BinaryOp"),
                }
            }
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_comparison() {
        let prog = parse("x = a > 5");
        match &prog[0] {
            Statement::Assignment { value, .. } => {
                assert!(matches!(
                    value,
                    Expression::BinaryOp {
                        operator: BinaryOperator::Greater,
                        ..
                    }
                ));
            }
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_logical_operators() {
        let prog = parse("x = a and b or c");
        match &prog[0] {
            Statement::Assignment { value, .. } => {
                // `and` has higher precedence than `or`, so: (a and b) or c
                match value {
                    Expression::BinaryOp {
                        operator: BinaryOperator::Or,
                        left,
                        ..
                    } => {
                        assert!(matches!(
                            left.as_ref(),
                            Expression::BinaryOp {
                                operator: BinaryOperator::And,
                                ..
                            }
                        ));
                    }
                    _ => panic!("expected Or wrapping And"),
                }
            }
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_unary_not() {
        let prog = parse("x = not true");
        match &prog[0] {
            Statement::Assignment { value, .. } => {
                assert!(matches!(
                    value,
                    Expression::UnaryOp {
                        operator: UnaryOperator::Not,
                        ..
                    }
                ));
            }
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_unary_negate() {
        let prog = parse("x = -5");
        match &prog[0] {
            Statement::Assignment { value, .. } => {
                assert!(matches!(
                    value,
                    Expression::UnaryOp {
                        operator: UnaryOperator::Negate,
                        ..
                    }
                ));
            }
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_grouped_expression() {
        let prog = parse("x = (1 + 2) * 3");
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::BinaryOp {
                    operator: BinaryOperator::Multiply,
                    left,
                    ..
                } => {
                    assert!(matches!(
                        left.as_ref(),
                        Expression::BinaryOp {
                            operator: BinaryOperator::Add,
                            ..
                        }
                    ));
                }
                _ => panic!("expected Multiply at top"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    // --- Literals ---

    #[test]
    fn test_list_literal() {
        let prog = parse("x = [1, 2, 3]");
        match &prog[0] {
            Statement::Assignment { value, .. } => {
                if let Expression::List(items) = value {
                    assert_eq!(items.len(), 3);
                } else {
                    panic!("expected List");
                }
            }
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_map_literal() {
        let prog = parse("x = { name: \"Ana\", age: 30 }");
        match &prog[0] {
            Statement::Assignment { value, .. } => {
                if let Expression::MapLiteral(pairs) = value {
                    assert_eq!(pairs.len(), 2);
                    assert_eq!(pairs[0].0, "name");
                    assert_eq!(pairs[1].0, "age");
                } else {
                    panic!("expected MapLiteral");
                }
            }
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_schema_constructor_expression() {
        let prog = parse("x = User { name: \"Ana\", age: 30 }");
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::SchemaConstructor {
                    schema_name,
                    fields,
                } => {
                    assert_eq!(schema_name, "User");
                    assert_eq!(fields.len(), 2);
                    assert_eq!(fields[0].0, "name");
                    assert_eq!(fields[1].0, "age");
                }
                _ => panic!("expected SchemaConstructor"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_http_client_response_schema_expression() {
        let prog = parse(r#"x = http_client.get("https://example.test") as UserProfile"#);
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::HttpClientResponseSchema { call, schema_name } => {
                    assert_eq!(schema_name, "UserProfile");
                    assert!(matches!(
                        call.as_ref(),
                        Expression::MethodCall { method, .. } if method == "get"
                    ));
                }
                _ => panic!("expected HttpClientResponseSchema"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    // --- Property access and method calls ---

    #[test]
    fn test_property_access() {
        let prog = parse("x = obj.field");
        match &prog[0] {
            Statement::Assignment { value, .. } => {
                assert!(
                    matches!(value, Expression::PropertyAccess { property, .. } if property == "field")
                );
            }
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_chained_property_access() {
        let prog = parse("x = a.b.c");
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::PropertyAccess { object, property } => {
                    assert_eq!(property, "c");
                    assert!(
                        matches!(object.as_ref(), Expression::PropertyAccess { property, .. } if property == "b")
                    );
                }
                _ => panic!("expected PropertyAccess"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_method_call() {
        let prog = parse("x = name.upper()");
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::MethodCall {
                    method, arguments, ..
                } => {
                    assert_eq!(method, "upper");
                    assert!(arguments.is_empty());
                }
                _ => panic!("expected MethodCall"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_method_call_with_args() {
        let prog = parse("x = text.replace(\"a\", \"b\")");
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::MethodCall {
                    method, arguments, ..
                } => {
                    assert_eq!(method, "replace");
                    assert_eq!(arguments.len(), 2);
                }
                _ => panic!("expected MethodCall"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_function_call() {
        let prog = parse("x = double(21)");
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::FunctionCall { name, arguments } => {
                    assert_eq!(name, "double");
                    assert_eq!(arguments.len(), 1);
                }
                _ => panic!("expected FunctionCall"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_named_arguments() {
        let prog = parse("x = find(limit: 10, offset: 5)");
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::FunctionCall { arguments, .. } => {
                    assert!(
                        matches!(&arguments[0], Argument::Named { name, .. } if name == "limit")
                    );
                    assert!(
                        matches!(&arguments[1], Argument::Named { name, .. } if name == "offset")
                    );
                }
                _ => panic!("expected FunctionCall"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    // --- Require / Reject ---

    #[test]
    fn test_require() {
        let prog = parse("require payload.items else fail 400, \"Cart is empty\"");
        match &prog[0] {
            Statement::Require {
                error_code,
                error_message,
                ..
            } => {
                assert_eq!(*error_code, 400);
                assert_eq!(error_message, "Cart is empty");
            }
            _ => panic!("expected Require"),
        }
    }

    #[test]
    fn test_reject() {
        let prog = parse("reject client.delinquent else fail 402, \"Payment pending\"");
        match &prog[0] {
            Statement::Reject {
                error_code,
                error_message,
                ..
            } => {
                assert_eq!(*error_code, 402);
                assert_eq!(error_message, "Payment pending");
            }
            _ => panic!("expected Reject"),
        }
    }

    #[test]
    fn test_while_statement() {
        let prog = parse("counter = 0\nwhile counter < 3\n    counter = counter + 1");
        match &prog[1] {
            Statement::While {
                condition, body, ..
            } => {
                assert!(matches!(condition, Expression::BinaryOp { .. }));
                assert_eq!(body.len(), 1);
            }
            _ => panic!("expected While"),
        }
    }

    // --- Tasks ---

    #[test]
    fn test_inline_task() {
        let prog = parse("task double(n) => n * 2");
        match &prog[0] {
            Statement::TaskDef {
                name, params, body, ..
            } => {
                assert_eq!(name, "double");
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].name, "n");
                assert_eq!(params[0].schema, None);
                assert!(matches!(body, TaskBody::Inline(_)));
            }
            _ => panic!("expected TaskDef"),
        }
    }

    #[test]
    fn test_block_task() {
        let src = "task calc(a, b)\n    x = a + b\n    x * 2";
        let prog = parse(src);
        match &prog[0] {
            Statement::TaskDef {
                name, params, body, ..
            } => {
                assert_eq!(name, "calc");
                assert_eq!(params.len(), 2);
                match body {
                    TaskBody::Block(stmts, final_expr) => {
                        assert_eq!(stmts.len(), 1); // x = a + b
                        assert!(matches!(final_expr, Expression::BinaryOp { .. })); // x * 2
                    }
                    _ => panic!("expected Block"),
                }
            }
            _ => panic!("expected TaskDef"),
        }
    }

    #[test]
    fn test_task_no_params() {
        let prog = parse("task noop() => null");
        match &prog[0] {
            Statement::TaskDef { params, .. } => {
                assert!(params.is_empty());
            }
            _ => panic!("expected TaskDef"),
        }
    }

    // --- Match ---

    #[test]
    fn test_match_expression() {
        let src =
            "fee = match kind\n    \"VIP\" -> 0.0\n    \"BASIC\" -> 10.0\n    fallback -> 5.0";
        let prog = parse(src);
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::Match { arms, .. } => {
                    assert_eq!(arms.len(), 3);
                    assert!(matches!(arms[0].pattern, MatchPattern::Literal(_)));
                    assert!(matches!(arms[2].pattern, MatchPattern::Fallback));
                }
                _ => panic!("expected Match"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_if_expression_assignment() {
        let src = "x = if active\n    1\nelse\n    2";
        let prog = parse(src);
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    assert!(
                        matches!(condition.as_ref(), Expression::Identifier(name) if name == "active")
                    );
                    assert!(matches!(
                        then_branch.as_ref(),
                        TaskBody::Block(_, Expression::Integer(1))
                    ));
                    assert!(
                        matches!(else_branch.as_ref(), Some(branch) if matches!(branch.as_ref(), TaskBody::Block(_, Expression::Integer(2))))
                    );
                }
                _ => panic!("expected If expression"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_if_expression_else_if() {
        let src = "x = if score > 90\n    \"excellent\"\nelse if score > 70\n    \"good\"\nelse\n    \"regular\"";
        let prog = parse(src);
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::If { else_branch, .. } => {
                    assert!(matches!(
                        else_branch.as_ref(),
                        Some(branch) if matches!(branch.as_ref(), TaskBody::Inline(Expression::If { .. }))
                    ));
                }
                _ => panic!("expected If expression"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_if_expression_with_pipeline() {
        let src = "x = if active\n    1\nelse\n    2\n>> inc";
        let prog = parse(src);
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::Pipeline { input, stages } => {
                    assert!(matches!(input.as_ref(), Expression::If { .. }));
                    assert_eq!(stages.len(), 1);
                }
                _ => panic!("expected Pipeline wrapping If expression"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_time_on_method_parses_after_dot() {
        let src = "x = opens_at.on(billing_date)";
        let prog = parse(src);
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::MethodCall {
                    object,
                    method,
                    arguments,
                } => {
                    assert!(
                        matches!(object.as_ref(), Expression::Identifier(name) if name == "opens_at")
                    );
                    assert_eq!(method, "on");
                    assert_eq!(arguments.len(), 1);
                }
                _ => panic!("expected method call"),
            },
            _ => panic!("expected assignment"),
        }
    }

    // --- Pipelines ---

    #[test]
    fn test_simple_pipeline() {
        let prog = parse("x = items >> task(double)");
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::Pipeline { stages, .. } => {
                    assert_eq!(stages.len(), 1);
                    assert!(
                        matches!(&stages[0], PipelineStage::Expression(Expression::TaskCall { name }) if name == "double")
                    );
                }
                _ => panic!("expected Pipeline"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_multi_stage_pipeline() {
        let prog = parse("x = items >> task(double) >> task(plus_one)");
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::Pipeline { stages, .. } => {
                    assert_eq!(stages.len(), 2);
                }
                _ => panic!("expected Pipeline"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_pipeline_with_map() {
        let src = "x = items >> map item\n    y = item * 2\n    keep y";
        let prog = parse(src);
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::Pipeline { stages, .. } => {
                    assert_eq!(stages.len(), 1);
                    match &stages[0] {
                        PipelineStage::Map { variable, body } => {
                            assert_eq!(variable, "item");
                            // body has 1 statement + 1 keep
                            assert_eq!(body.len(), 2);
                            assert!(matches!(body[0], MapStatement::Statement(_)));
                            assert!(matches!(
                                body[1],
                                MapStatement::Keep {
                                    condition: None,
                                    ..
                                }
                            ));
                        }
                        _ => panic!("expected Map stage"),
                    }
                }
                _ => panic!("expected Pipeline"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_pipeline_with_reduce_block() {
        let src = "x = items >> reduce(0) acc, item\n    sum = acc + item\n    sum";
        let prog = parse(src);
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::Pipeline { stages, .. } => {
                    assert_eq!(stages.len(), 1);
                    match &stages[0] {
                        PipelineStage::Reduce {
                            initial,
                            accumulator,
                            item,
                            body,
                        } => {
                            assert_eq!(accumulator, "acc");
                            assert_eq!(item, "item");
                            assert!(matches!(initial, Expression::Integer(0)));
                            match body {
                                TaskBody::Block(stmts, final_expr) => {
                                    assert_eq!(stmts.len(), 1);
                                    assert!(
                                        matches!(final_expr, Expression::Identifier(name) if name == "sum")
                                    );
                                }
                                _ => panic!("expected reduce block body"),
                            }
                        }
                        _ => panic!("expected Reduce stage"),
                    }
                }
                _ => panic!("expected Pipeline"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    #[test]
    fn test_pipeline_with_reduce_inline() {
        let prog = parse("x = items >> reduce(0) acc, item => acc + item");
        match &prog[0] {
            Statement::Assignment { value, .. } => match value {
                Expression::Pipeline { stages, .. } => match &stages[0] {
                    PipelineStage::Reduce { body, .. } => {
                        assert!(matches!(
                            body,
                            TaskBody::Inline(Expression::BinaryOp { .. })
                        ));
                    }
                    _ => panic!("expected Reduce stage"),
                },
                _ => panic!("expected Pipeline"),
            },
            _ => panic!("expected Assignment"),
        }
    }

    // --- Broadcast ---

    #[test]
    fn test_broadcast() {
        let src = "data *>>\n    -> target_a\n    -> target_b";
        let prog = parse(src);
        match &prog[0] {
            Statement::ExpressionStatement { expression, .. } => match expression {
                Expression::Broadcast { targets, .. } => {
                    assert_eq!(targets.len(), 2);
                }
                _ => panic!("expected Broadcast"),
            },
            _ => panic!("expected ExpressionStatement"),
        }
    }

    // --- Or as fallback ---

    #[test]
    fn test_or_as_default_value() {
        let prog = parse("x = val or 10");
        match &prog[0] {
            Statement::Assignment { value, .. } => {
                assert!(matches!(
                    value,
                    Expression::BinaryOp {
                        operator: BinaryOperator::Or,
                        ..
                    }
                ));
            }
            _ => panic!("expected Assignment"),
        }
    }

    // --- Infrastructure keywords as identifiers ---

    #[test]
    fn test_db_as_identifier() {
        let prog = parse("x = db.users.find(id)");
        match &prog[0] {
            Statement::Assignment { value, .. } => {
                // db.users.find(id) → MethodCall { object: PropertyAccess { object: Identifier("db"), property: "users" }, method: "find", ... }
                assert!(matches!(value, Expression::MethodCall { method, .. } if method == "find"));
            }
            _ => panic!("expected Assignment"),
        }
    }

    // --- Error cases ---

    #[test]
    fn test_unexpected_token_error() {
        let err = parse_err("= 5");
        assert!(matches!(err, MarretaError::UnexpectedToken { .. }));
    }

    #[test]
    fn test_missing_closing_paren() {
        let err = parse_err("x = func(1, 2");
        assert!(matches!(
            err,
            MarretaError::UnexpectedEndOfInput { .. } | MarretaError::UnexpectedToken { .. }
        ));
    }

    // --- HTTP: route ---

    #[test]
    fn test_parse_route_get_no_take() {
        let prog = parse("route GET \"/hello\"\n    reply 200, null\n");
        assert_eq!(prog.len(), 1);
        if let Statement::Route {
            verb,
            path,
            take,
            body,
            ..
        } = &prog[0]
        {
            assert_eq!(*verb, HttpVerb::Get);
            assert_eq!(path, "/hello");
            assert!(take.is_empty());
            assert_eq!(body.len(), 1);
        } else {
            panic!("expected Route");
        }
    }

    #[test]
    fn test_parse_route_post_take_payload() {
        let prog = parse("route POST \"/users\" take payload\n    reply 201, null\n");
        if let Statement::Route {
            verb, path, take, ..
        } = &prog[0]
        {
            assert_eq!(*verb, HttpVerb::Post);
            assert_eq!(path, "/users");
            assert!(matches!(take.first(), Some(TakeBinding::Payload(n)) if n == "payload"));
        } else {
            panic!("expected Route");
        }
    }

    #[test]
    fn test_parse_route_get_take_query() {
        let prog = parse("route GET \"/search\" take query\n    reply 200, null\n");
        if let Statement::Route { take, .. } = &prog[0] {
            assert!(matches!(take.first(), Some(TakeBinding::Query(n)) if n == "query"));
        } else {
            panic!("expected Route");
        }
    }

    #[test]
    fn test_parse_route_get_take_headers() {
        let prog = parse("route GET \"/protected\" take headers\n    reply 200, null\n");
        if let Statement::Route { take, .. } = &prog[0] {
            assert!(matches!(take.first(), Some(TakeBinding::Headers(n)) if n == "headers"));
        } else {
            panic!("expected Route");
        }
    }

    #[test]
    fn test_parse_all_http_verbs() {
        for (src, expected_verb) in [
            ("route GET \"/a\"\n    reply 200, null\n", HttpVerb::Get),
            ("route POST \"/a\"\n    reply 200, null\n", HttpVerb::Post),
            ("route PUT \"/a\"\n    reply 200, null\n", HttpVerb::Put),
            ("route PATCH \"/a\"\n    reply 200, null\n", HttpVerb::Patch),
            (
                "route DELETE \"/a\"\n    reply 200, null\n",
                HttpVerb::Delete,
            ),
        ] {
            let prog = parse(src);
            if let Statement::Route { verb, .. } = &prog[0] {
                assert_eq!(*verb, expected_verb);
            } else {
                panic!("expected Route for {}", src);
            }
        }
    }

    #[test]
    fn test_parse_route_with_url_param_path() {
        let prog = parse("route GET \"/users/:id\"\n    reply 200, null\n");
        if let Statement::Route { path, .. } = &prog[0] {
            assert_eq!(path, "/users/:id");
        } else {
            panic!("expected Route");
        }
    }

    #[test]
    fn test_parse_route_invalid_verb_error() {
        let err = parse_err("route CONNECT \"/test\"\n    reply 200, null\n");
        assert!(matches!(err, MarretaError::UnexpectedToken { .. }));
    }

    // --- HTTP: reply ---

    #[test]
    fn test_parse_reply_integer_body() {
        let prog = parse("reply 200, 42");
        if let Statement::Reply {
            status_code, body, ..
        } = &prog[0]
        {
            assert_eq!(*status_code, Expression::Integer(200));
            assert_eq!(*body, Expression::Integer(42));
        } else {
            panic!("expected Reply");
        }
    }

    #[test]
    fn test_parse_reply_map_body() {
        let prog = parse("reply 201, { id: 1 }");
        if let Statement::Reply {
            status_code, body, ..
        } = &prog[0]
        {
            assert_eq!(*status_code, Expression::Integer(201));
            assert!(matches!(body, Expression::MapLiteral(_)));
        } else {
            panic!("expected Reply");
        }
    }

    #[test]
    fn test_parse_reply_null_body() {
        let prog = parse("reply 204, null");
        if let Statement::Reply {
            status_code, body, ..
        } = &prog[0]
        {
            assert_eq!(*status_code, Expression::Integer(204));
            assert_eq!(*body, Expression::Null);
        } else {
            panic!("expected Reply");
        }
    }

    #[test]
    fn test_parse_reply_identifier_body() {
        let prog = parse("reply 200, user");
        if let Statement::Reply { body, .. } = &prog[0] {
            assert!(matches!(body, Expression::Identifier(n) if n == "user"));
        } else {
            panic!("expected Reply");
        }
    }

    // --- HTTP: fail ---

    #[test]
    fn test_parse_fail_string_message() {
        let prog = parse("fail 404, \"Not found\"");
        if let Statement::Fail {
            status_code,
            message,
            ..
        } = &prog[0]
        {
            assert_eq!(*status_code, 404);
            assert!(matches!(message, Expression::StringLiteral(s) if s == "Not found"));
        } else {
            panic!("expected Fail");
        }
    }

    #[test]
    fn test_parse_fail_400() {
        let prog = parse("fail 400, \"Bad request\"");
        if let Statement::Fail { status_code, .. } = &prog[0] {
            assert_eq!(*status_code, 400);
        } else {
            panic!("expected Fail");
        }
    }

    #[test]
    fn test_parse_route_body_has_multiple_statements() {
        let src = "route GET \"/test\"\n    x = 1\n    reply 200, x\n";
        let prog = parse(src);
        if let Statement::Route { body, .. } = &prog[0] {
            assert_eq!(body.len(), 2);
            assert!(matches!(&body[0], Statement::Assignment { .. }));
            assert!(matches!(&body[1], Statement::Reply { .. }));
        } else {
            panic!("expected Route");
        }
    }

    #[test]
    fn test_parse_route_body_with_require() {
        let src = "route POST \"/checkout\" take payload\n    require payload.items else fail 400, \"Empty\"\n    reply 200, null\n";
        let prog = parse(src);
        if let Statement::Route { body, .. } = &prog[0] {
            assert_eq!(body.len(), 2);
            assert!(matches!(&body[0], Statement::Require { .. }));
        } else {
            panic!("expected Route");
        }
    }

    // --- v0.2.1: multiple take bindings ---

    #[test]
    fn test_parse_route_multiple_take_payload_headers() {
        let prog = parse("route POST \"/checkout\" take payload, headers\n    reply 200, null\n");
        if let Statement::Route { take, .. } = &prog[0] {
            assert_eq!(take.len(), 2);
            assert!(matches!(&take[0], TakeBinding::Payload(_)));
            assert!(matches!(&take[1], TakeBinding::Headers(_)));
        } else {
            panic!("expected Route");
        }
    }

    #[test]
    fn test_parse_route_multiple_take_query_payload() {
        let prog = parse("route GET \"/search\" take query, payload\n    reply 200, null\n");
        if let Statement::Route { take, .. } = &prog[0] {
            assert_eq!(take.len(), 2);
            assert!(matches!(&take[0], TakeBinding::Query(_)));
            assert!(matches!(&take[1], TakeBinding::Payload(_)));
        } else {
            panic!("expected Route");
        }
    }

    #[test]
    fn test_parse_route_take_form() {
        let prog = parse("route POST \"/contact\" take form\n    reply 200, null\n");
        if let Statement::Route { take, .. } = &prog[0] {
            assert!(matches!(take.first(), Some(TakeBinding::Form(_))));
        } else {
            panic!("expected Route");
        }
    }

    #[test]
    fn test_parse_route_take_raw() {
        let prog = parse("route POST \"/webhook\" take raw\n    reply 200, null\n");
        if let Statement::Route { take, .. } = &prog[0] {
            assert!(matches!(take.first(), Some(TakeBinding::Raw(_))));
        } else {
            panic!("expected Route");
        }
    }

    #[test]
    fn test_parse_route_take_raw_and_headers() {
        let prog = parse("route POST \"/webhook\" take raw, headers\n    reply 200, null\n");
        if let Statement::Route { take, .. } = &prog[0] {
            assert_eq!(take.len(), 2);
            assert!(matches!(&take[0], TakeBinding::Raw(_)));
            assert!(matches!(&take[1], TakeBinding::Headers(_)));
        } else {
            panic!("expected Route");
        }
    }

    // --- v0.2.1: reply content-type modifiers ---

    #[test]
    fn test_parse_reply_html_modifier() {
        let prog = parse("reply html 200, \"<h1>hi</h1>\"");
        if let Statement::Reply {
            content_type,
            status_code,
            ..
        } = &prog[0]
        {
            assert_eq!(*content_type, ReplyContentType::Html);
            assert_eq!(*status_code, Expression::Integer(200));
        } else {
            panic!("expected Reply");
        }
    }

    #[test]
    fn test_parse_reply_text_modifier() {
        let prog = parse("reply text 201, \"ok\"");
        if let Statement::Reply {
            content_type,
            status_code,
            ..
        } = &prog[0]
        {
            assert_eq!(*content_type, ReplyContentType::Text);
            assert_eq!(*status_code, Expression::Integer(201));
        } else {
            panic!("expected Reply");
        }
    }

    #[test]
    fn test_parse_reply_default_is_json() {
        let prog = parse("reply 200, { ok: true }");
        if let Statement::Reply { content_type, .. } = &prog[0] {
            assert_eq!(*content_type, ReplyContentType::Json);
        } else {
            panic!("expected Reply");
        }
    }

    #[test]
    fn test_parse_reply_with_extra_headers() {
        let prog = parse("reply 302, null, { Location: \"https://example.com\" }");
        if let Statement::Reply {
            status_code,
            extra_headers,
            ..
        } = &prog[0]
        {
            assert_eq!(*status_code, Expression::Integer(302));
            assert!(extra_headers.is_some());
        } else {
            panic!("expected Reply");
        }
    }

    // --- reply ... as schema_name ---

    #[test]
    fn test_parse_reply_with_response_schema() {
        let prog = parse("reply 201 as order_result, result");
        if let Statement::Reply {
            status_code,
            response_schema,
            body,
            ..
        } = &prog[0]
        {
            assert_eq!(*status_code, Expression::Integer(201));
            assert_eq!(response_schema.as_deref(), Some("order_result"));
            assert!(matches!(body, Expression::Identifier(n) if n == "result"));
        } else {
            panic!("expected Reply");
        }
    }

    #[test]
    fn test_parse_reply_without_response_schema_is_none() {
        let prog = parse("reply 200, { ok: true }");
        if let Statement::Reply {
            response_schema, ..
        } = &prog[0]
        {
            assert!(response_schema.is_none());
        } else {
            panic!("expected Reply");
        }
    }

    #[test]
    fn test_parse_reply_schema_with_html_modifier() {
        // html modifier + as schema is not meaningful, but the parser should not crash
        let prog = parse("reply 200 as page_schema, content");
        if let Statement::Reply {
            response_schema,
            status_code,
            ..
        } = &prog[0]
        {
            assert_eq!(*status_code, Expression::Integer(200));
            assert_eq!(response_schema.as_deref(), Some("page_schema"));
        } else {
            panic!("expected Reply");
        }
    }

    // --- Schema & `as` binding tests ---

    #[test]
    fn test_parse_schema_basic() {
        let src = "schema UserPayload\n    name: string\n    age: integer";
        let prog = parse(src);
        assert_eq!(prog.len(), 1);
        if let Statement::Schema { name, fields, .. } = &prog[0] {
            assert_eq!(name, "UserPayload");
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].name, "name");
            assert_eq!(fields[0].field_type, SchemaType::StringType);
            assert!(!fields[0].optional);
            assert_eq!(fields[1].name, "age");
            assert_eq!(fields[1].field_type, SchemaType::IntegerType);
        } else {
            panic!("expected Schema");
        }
    }

    #[test]
    fn test_parse_schema_optional_field() {
        // `?` on the field name marks it as optional (Ruby-style)
        let src = "schema Req\n    email?: string\n    active: boolean";
        let prog = parse(src);
        if let Statement::Schema { fields, .. } = &prog[0] {
            assert_eq!(fields[0].name, "email"); // `?` stripped from name
            assert!(fields[0].optional);
            assert!(!fields[1].optional);
        } else {
            panic!("expected Schema");
        }
    }

    #[test]
    fn test_parse_schema_all_types() {
        let src = "schema All\n    s: string\n    i: integer\n    f: float\n    amount: decimal\n    b: boolean\n    at: instant\n    d: date\n    t: time\n    ttl: duration\n    window: interval";
        let prog = parse(src);
        if let Statement::Schema { fields, .. } = &prog[0] {
            assert_eq!(fields[0].field_type, SchemaType::StringType);
            assert_eq!(fields[1].field_type, SchemaType::IntegerType);
            assert_eq!(fields[2].field_type, SchemaType::FloatType);
            assert_eq!(fields[3].field_type, SchemaType::DecimalType);
            assert_eq!(fields[4].field_type, SchemaType::BooleanType);
            assert_eq!(fields[5].field_type, SchemaType::InstantType);
            assert_eq!(fields[6].field_type, SchemaType::DateType);
            assert_eq!(fields[7].field_type, SchemaType::TimeType);
            assert_eq!(fields[8].field_type, SchemaType::DurationType);
            assert_eq!(fields[9].field_type, SchemaType::IntervalType);
        } else {
            panic!("expected Schema");
        }
    }

    #[test]
    fn test_parse_schema_inline_enum() {
        let src = "schema Payment\n    status: enum [\"pending\", \"paid\", \"cancelled\"]";
        let prog = parse(src);
        if let Statement::Schema { fields, .. } = &prog[0] {
            assert_eq!(
                fields[0].field_type,
                SchemaType::EnumType(vec!["pending".into(), "paid".into(), "cancelled".into()])
            );
        } else {
            panic!("expected Schema");
        }
    }

    #[test]
    fn test_parse_schema_inline_enum_rejects_duplicate_values() {
        let err = parse_err("schema Payment\n    status: enum [\"pending\", \"pending\"]");
        assert!(matches!(err, MarretaError::UnexpectedToken { .. }));
    }

    #[test]
    fn test_parse_schema_with_db_table_metadata() {
        let src = "schema User\n    db: users\n    id: integer";
        let prog = parse(src);
        if let Statement::Schema {
            db_table, fields, ..
        } = &prog[0]
        {
            assert_eq!(db_table.as_deref(), Some("users"));
            assert_eq!(fields.len(), 1);
        } else {
            panic!("expected Schema");
        }
    }

    #[test]
    fn test_parse_route_with_schema_binding() {
        let src = "route POST \"/users\" take payload as UserPayload\n    reply 201, { ok: true }";
        let prog = parse(src);
        if let Statement::Route { schema, take, .. } = &prog[0] {
            assert_eq!(schema.as_deref(), Some("UserPayload"));
            assert!(matches!(take.first(), Some(TakeBinding::Payload(_))));
        } else {
            panic!("expected Route");
        }
    }

    #[test]
    fn test_parse_route_without_schema_binding() {
        let src = "route POST \"/users\" take payload\n    reply 201, { ok: true }";
        let prog = parse(src);
        if let Statement::Route { schema, .. } = &prog[0] {
            assert!(schema.is_none());
        } else {
            panic!("expected Route");
        }
    }

    #[test]
    fn test_parse_route_schema_no_take() {
        // Schema binding only valid with take, but parser shouldn't crash without it
        let src = "route GET \"/health\"\n    reply 200, { ok: true }";
        let prog = parse(src);
        if let Statement::Route { schema, take, .. } = &prog[0] {
            assert!(schema.is_none());
            assert!(take.is_empty());
        } else {
            panic!("expected Route");
        }
    }

    #[test]
    fn test_parse_auth_jwt_provider() {
        let src = r#"auth jwt customer_auth {
    issuer: env.MARRETA_AUTH_CUSTOMER_ISSUER
    audience: env.MARRETA_AUTH_CUSTOMER_AUDIENCE
}
"#;
        let prog = parse(src);
        match &prog[0] {
            Statement::AuthProvider {
                provider: AuthProvider::Jwt(config),
                ..
            } => {
                assert_eq!(config.name, "customer_auth");
                assert_eq!(config.fields.len(), 2);
                assert_eq!(config.fields[0].name, "issuer");
                assert_eq!(config.fields[1].name, "audience");
            }
            other => panic!("expected jwt auth provider, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_auth_api_key_provider() {
        let src = r#"auth api_key internal_auth {
    header: "x-api-key"
    secret_hash: env.MARRETA_AUTH_INTERNAL_API_KEY_HASH
}
"#;
        let prog = parse(src);
        match &prog[0] {
            Statement::AuthProvider {
                provider: AuthProvider::ApiKey(config),
                ..
            } => {
                assert_eq!(config.name, "internal_auth");
                assert_eq!(config.fields.len(), 2);
                assert_eq!(config.fields[0].name, "header");
                assert_eq!(config.fields[1].name, "secret_hash");
            }
            other => panic!("expected api_key auth provider, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_route_auth_and_allow_clauses() {
        let src = r#"route GET "/orders"
    require auth customer_auth
    allow "admin" in auth.user.roles
    reply 200, { ok: true }
"#;
        let prog = parse(src);
        match &prog[0] {
            Statement::Route {
                auth, allow, body, ..
            } => {
                assert_eq!(
                    auth.as_ref().map(|a| a.provider.as_str()),
                    Some("customer_auth")
                );
                assert_eq!(allow.len(), 1);
                assert_eq!(body.len(), 1);
                assert!(matches!(
                    allow[0],
                    Expression::BinaryOp {
                        operator: BinaryOperator::In,
                        ..
                    }
                ));
            }
            other => panic!("expected Route, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_route_rejects_late_allow_clause() {
        let err = parse_err(
            r#"route GET "/orders"
    reply 200, { ok: true }
    allow "admin" in auth.user.roles
"#,
        );
        assert!(matches!(err, MarretaError::UnexpectedToken { .. }));
    }

    // --- Export ---

    #[test]
    fn test_export_task() {
        let prog = parse("export task double(x) => x * 2");
        assert_eq!(prog.len(), 1);
        match &prog[0] {
            Statement::Export(inner) => match inner.as_ref() {
                Statement::TaskDef { name, params, .. } => {
                    assert_eq!(name, "double");
                    assert_eq!(params.len(), 1);
                    assert_eq!(params[0].name, "x");
                    assert_eq!(params[0].schema, None);
                }
                _ => panic!("expected TaskDef inside Export"),
            },
            _ => panic!("expected Export"),
        }
    }

    #[test]
    fn test_export_schema() {
        let prog = parse("export schema order_payload\n    total: float\n    paid: boolean\n");
        assert_eq!(prog.len(), 1);
        match &prog[0] {
            Statement::Export(inner) => match inner.as_ref() {
                Statement::Schema { name, fields, .. } => {
                    assert_eq!(name, "order_payload");
                    assert_eq!(fields.len(), 2);
                }
                _ => panic!("expected Schema inside Export"),
            },
            _ => panic!("expected Export"),
        }
    }

    #[test]
    fn test_export_assignment() {
        let prog = parse("export tax_rate = 0.1");
        assert_eq!(prog.len(), 1);
        match &prog[0] {
            Statement::Export(inner) => match inner.as_ref() {
                Statement::Assignment { target, .. } => {
                    assert_eq!(target, "tax_rate");
                }
                _ => panic!("expected Assignment inside Export"),
            },
            _ => panic!("expected Export"),
        }
    }

    #[test]
    fn test_export_route_is_error() {
        let err = parse_err("export route GET \"/health\"\n    reply 200, { ok: true }\n");
        assert!(matches!(err, MarretaError::UnexpectedToken { .. }));
    }

    // --- API Scenario Testing ---

    #[test]
    fn test_parse_api_scenario_basic() {
        let src = r#"scenario "health"
    when GET "/health"
    then status 200
"#;
        let prog = parse(src);
        assert_eq!(prog.len(), 1);
        match &prog[0] {
            Statement::Scenario { name, steps, .. } => {
                assert_eq!(name, "health");
                assert_eq!(steps.len(), 2);
                assert!(
                    matches!(steps[0], ScenarioStep::When { verb: HttpVerb::Get, ref path, .. } if path == "/health")
                );
                assert!(matches!(
                    steps[1],
                    ScenarioStep::ThenStatus {
                        status: Expression::Integer(200),
                        ..
                    }
                ));
            }
            _ => panic!("expected Scenario"),
        }
    }

    #[test]
    fn test_parse_api_scenario_with_given_when_body_headers_and_response() {
        let src = r#"scenario "create order"
    given db.products.find(10) returns { id: 10, price: 100 }
    given db.orders.save(anything) returns { id: 99, total: 100 }
    when POST "/orders" with { product_id: 10, quantity: 1 } and headers { authorization: "Bearer test" }
    then response is { status: 201, body: { id: anything, total: 100 } }
"#;
        let prog = parse(src);
        match &prog[0] {
            Statement::Scenario { name, steps, .. } => {
                assert_eq!(name, "create order");
                assert_eq!(steps.len(), 4);
                assert!(matches!(steps[0], ScenarioStep::Given { .. }));
                assert!(matches!(steps[1], ScenarioStep::Given { .. }));
                assert!(
                    matches!(steps[2], ScenarioStep::When { verb: HttpVerb::Post, ref path, body: Some(_), headers: Some(_), .. } if path == "/orders")
                );
                assert!(matches!(steps[3], ScenarioStep::ThenResponse { .. }));
            }
            _ => panic!("expected Scenario"),
        }
    }

    #[test]
    fn test_parse_api_scenario_queue_given_productive_style() {
        let src = r#"scenario "queue producer"
    given queue.push "orders.created", anything returns true
    when POST "/queue/push" with { order_id: "ord-1" }
    then status 202
"#;
        let prog = parse(src);
        match &prog[0] {
            Statement::Scenario { steps, .. } => {
                assert!(matches!(
                    &steps[0],
                    ScenarioStep::Given {
                        target: Expression::QueuePush {
                            payload: Some(_),
                            ..
                        },
                        ..
                    }
                ));
            }
            _ => panic!("expected Scenario"),
        }
    }

    #[test]
    fn test_parse_api_scenario_rejects_loose_statement() {
        let err = parse_err(
            "scenario \"invalid\"\n    x = 999\n    when GET \"/health\"\n    then status 200\n",
        );
        assert!(matches!(err, MarretaError::UnexpectedToken { .. }));
        assert!(
            err.display_message()
                .contains("given, when, or then inside scenario")
        );
    }

    #[test]
    fn test_parse_api_scenario_words_are_contextual_identifiers() {
        let prog = parse(
            r#"returns = 42
given = "ok"
when = true
then = returns
scenario = "not a scenario"
"#,
        );
        assert_eq!(prog.len(), 5);
        assert!(matches!(
            &prog[0],
            Statement::Assignment { target, .. } if target == "returns"
        ));
        assert!(matches!(
            &prog[4],
            Statement::Assignment { target, .. } if target == "scenario"
        ));
    }

    #[test]
    fn test_parse_api_scenario_requires_one_when() {
        let err = parse_err("scenario \"invalid\"\n    then status 200\n");
        assert!(matches!(err, MarretaError::UnexpectedToken { .. }));
    }

    #[test]
    fn test_parse_api_scenario_requires_then() {
        let err = parse_err("scenario \"invalid\"\n    when GET \"/health\"\n");
        assert!(matches!(err, MarretaError::UnexpectedToken { .. }));
    }

    // =========================================================================
    // v0.4.0 — Advanced Schemas & Task Contracts
    // =========================================================================

    #[test]
    fn test_schema_reference_type() {
        let src = "schema user_payload\n    name: string\n    billing: address\n";
        let prog = parse(src);
        match &prog[0] {
            Statement::Schema { fields, .. } => {
                assert_eq!(fields[1].name, "billing");
                assert_eq!(
                    fields[1].field_type,
                    SchemaType::Reference("address".into())
                );
                assert!(!fields[1].optional);
            }
            _ => panic!("expected Schema"),
        }
    }

    #[test]
    fn test_schema_typed_list_of_schema() {
        let src = "schema order_payload\n    client_id: integer\n    items: list of order_item\n";
        let prog = parse(src);
        match &prog[0] {
            Statement::Schema { fields, .. } => {
                assert_eq!(fields[1].name, "items");
                assert_eq!(
                    fields[1].field_type,
                    SchemaType::TypedList(Box::new(SchemaType::Reference("order_item".into())))
                );
            }
            _ => panic!("expected Schema"),
        }
    }

    #[test]
    fn test_schema_typed_list_of_primitive() {
        let src = "schema tag_list\n    tags: list of string\n";
        let prog = parse(src);
        match &prog[0] {
            Statement::Schema { fields, .. } => {
                assert_eq!(fields[0].name, "tags");
                assert_eq!(
                    fields[0].field_type,
                    SchemaType::TypedList(Box::new(SchemaType::StringType))
                );
            }
            _ => panic!("expected Schema"),
        }
    }

    #[test]
    fn test_schema_typed_list_optional() {
        let src = "schema order_payload\n    client_id: integer\n    tags?: list of string\n";
        let prog = parse(src);
        match &prog[0] {
            Statement::Schema { fields, .. } => {
                assert_eq!(fields[1].name, "tags");
                assert!(fields[1].optional);
                assert_eq!(
                    fields[1].field_type,
                    SchemaType::TypedList(Box::new(SchemaType::StringType))
                );
            }
            _ => panic!("expected Schema"),
        }
    }

    #[test]
    fn test_task_param_with_schema_contract() {
        let prog = parse("task apply_taxes(order as order_payload) => order.total * 1.15");
        match &prog[0] {
            Statement::TaskDef { name, params, .. } => {
                assert_eq!(name, "apply_taxes");
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].name, "order");
                assert_eq!(params[0].schema, Some("order_payload".into()));
            }
            _ => panic!("expected TaskDef"),
        }
    }

    #[test]
    fn test_task_mixed_params_with_and_without_schema() {
        let prog = parse(
            "task process(order as order_payload, discount) => order.total * (1.0 - discount)",
        );
        match &prog[0] {
            Statement::TaskDef { params, .. } => {
                assert_eq!(params.len(), 2);
                assert_eq!(params[0].name, "order");
                assert_eq!(params[0].schema, Some("order_payload".into()));
                assert_eq!(params[1].name, "discount");
                assert_eq!(params[1].schema, None);
            }
            _ => panic!("expected TaskDef"),
        }
    }

    #[test]
    fn test_task_param_without_schema_is_none() {
        let prog = parse("task double(n) => n * 2");
        match &prog[0] {
            Statement::TaskDef { params, .. } => {
                assert_eq!(params[0].schema, None);
            }
            _ => panic!("expected TaskDef"),
        }
    }
}

// ─── Additional parser error tests ────────────────────────────────────────────

#[cfg(test)]
mod tests_errors {
    use super::*;
    use crate::error::MarretaError;
    use crate::lexer::Lexer;

    fn parse_err(src: &str) -> MarretaError {
        let tokens = Lexer::new(src).tokenize().unwrap();
        Parser::new(tokens).parse().unwrap_err()
    }

    fn parse_ok(src: &str) -> Program {
        let tokens = Lexer::new(src).tokenize().unwrap();
        Parser::new(tokens).parse().unwrap()
    }

    // ─── Unexpected token errors ──────────────────────────────────────────────

    #[test]
    fn test_bare_operator_is_unexpected_token() {
        let err = parse_err("= 5");
        assert!(matches!(err, MarretaError::UnexpectedToken { .. }));
    }

    #[test]
    fn test_queue_publish_error_suggests_topic_publish() {
        // queue.publish was renamed to topic.publish; the error must point the way.
        let err = parse_err(r#"queue.publish "t", { a: 1 }"#);
        match err {
            MarretaError::UnexpectedToken { expected, .. } => {
                assert!(
                    expected.contains("topic.publish"),
                    "error should suggest topic.publish, got: {expected}"
                );
            }
            other => panic!("expected UnexpectedToken, got {other:?}"),
        }
    }

    #[test]
    fn test_missing_closing_bracket_errors() {
        let err = parse_err("x = [1, 2, 3");
        assert!(matches!(
            err,
            MarretaError::UnexpectedEndOfInput { .. } | MarretaError::UnexpectedToken { .. }
        ));
    }

    #[test]
    fn test_missing_closing_brace_errors() {
        let err = parse_err("x = {a: 1, b: 2");
        assert!(matches!(
            err,
            MarretaError::UnexpectedEndOfInput { .. } | MarretaError::UnexpectedToken { .. }
        ));
    }

    #[test]
    fn test_missing_closing_paren_errors() {
        let err = parse_err("x = func(1, 2");
        assert!(matches!(
            err,
            MarretaError::UnexpectedEndOfInput { .. } | MarretaError::UnexpectedToken { .. }
        ));
    }

    #[test]
    fn test_unexpected_eof_in_expression() {
        // Trailing operator with no right-hand side
        let err = parse_err("x = 1 +");
        assert!(matches!(
            err,
            MarretaError::UnexpectedEndOfInput { .. } | MarretaError::UnexpectedToken { .. }
        ));
    }

    #[test]
    fn test_double_operator_is_error() {
        let err = parse_err("x = 1 + + 2");
        // The second '+' is an unexpected token in prefix position
        assert!(matches!(
            err,
            MarretaError::UnexpectedToken { .. } | MarretaError::UnexpectedEndOfInput { .. }
        ));
    }

    // ─── Valid edge cases that must parse without error ───────────────────────

    #[test]
    fn test_empty_list_literal() {
        let prog = parse_ok("x = []");
        assert_eq!(prog.len(), 1);
    }

    #[test]
    fn test_empty_map_literal() {
        let prog = parse_ok("x = {}");
        assert_eq!(prog.len(), 1);
    }

    #[test]
    fn test_nested_list_in_map() {
        let prog = parse_ok("x = {items: [1, 2, 3]}");
        assert_eq!(prog.len(), 1);
    }

    #[test]
    fn test_deeply_nested_map() {
        let prog = parse_ok("x = {a: {b: {c: 42}}}");
        assert_eq!(prog.len(), 1);
    }

    #[test]
    fn test_chained_method_calls_parses() {
        let prog = parse_ok("x = \"hello world\".split(\" \").length()");
        assert_eq!(prog.len(), 1);
    }

    #[test]
    fn test_pipeline_chain_parses() {
        let prog = parse_ok("task f(x) => x\nresult = 1 >> f >> f >> f");
        assert_eq!(prog.len(), 2);
    }

    #[test]
    fn test_negative_number_literal_parses() {
        let prog = parse_ok("x = -42");
        assert_eq!(prog.len(), 1);
    }

    #[test]
    fn test_boolean_expression_parses() {
        let prog = parse_ok("x = true and false or not true");
        assert_eq!(prog.len(), 1);
    }

    #[test]
    fn test_multiline_map_parses() {
        let src = "x = {\n  name: \"Ana\",\n  age: 30\n}";
        let prog = parse_ok(src);
        assert_eq!(prog.len(), 1);
    }

    #[test]
    fn test_conditional_assignment_parses() {
        let prog = parse_ok("y = 42 if true");
        assert_eq!(prog.len(), 1);
    }

    #[test]
    fn test_or_default_parses() {
        let prog = parse_ok("x = maybe or \"default\"");
        assert_eq!(prog.len(), 1);
    }

    #[test]
    fn test_subscript_access_parses() {
        let prog = parse_ok("x = items[0]");
        assert_eq!(prog.len(), 1);
    }

    #[test]
    fn test_raise_statement_parses() {
        let prog = parse_ok("raise \"error\"");
        assert!(matches!(prog[0], Statement::Raise { .. }));
    }

    #[test]
    fn test_raise_with_condition_parses() {
        let prog = parse_ok("raise \"error\" if true");
        if let Statement::Raise { condition, .. } = &prog[0] {
            assert!(condition.is_some());
        } else {
            panic!("expected Raise");
        }
    }

    // ── Queue (v0.8) parser tests ─────────────────────────────────────────────

    #[test]
    fn test_on_queue_no_schema() {
        let src = "on queue \"orders\" take message\n    x = 1\n";
        let prog = parse_ok(src);
        assert_eq!(prog.len(), 1);
        if let Statement::OnQueue {
            queue_name,
            binding,
            schema,
            body,
            ..
        } = &prog[0]
        {
            assert!(matches!(queue_name, Expression::StringLiteral(s) if s == "orders"));
            assert_eq!(binding, "message");
            assert!(schema.is_none());
            assert_eq!(body.len(), 1);
        } else {
            panic!("expected OnQueue");
        }
    }

    #[test]
    fn test_on_queue_with_schema() {
        let src = "on queue \"orders\" take message as order_payload\n    x = 1\n";
        let prog = parse_ok(src);
        if let Statement::OnQueue { schema, .. } = &prog[0] {
            assert_eq!(schema.as_deref(), Some("order_payload"));
        } else {
            panic!("expected OnQueue");
        }
    }

    #[test]
    fn test_on_topic_no_schema() {
        let src = "on topic \"payments.approved\" take event\n    x = 1\n";
        let prog = parse_ok(src);
        if let Statement::OnTopic {
            pattern,
            binding,
            schema,
            ..
        } = &prog[0]
        {
            assert!(matches!(pattern, Expression::StringLiteral(s) if s == "payments.approved"));
            assert_eq!(binding, "event");
            assert!(schema.is_none());
        } else {
            panic!("expected OnTopic");
        }
    }

    #[test]
    fn test_on_topic_with_schema() {
        let src = "on topic \"payments.approved\" take event as payment_event\n    x = 1\n";
        let prog = parse_ok(src);
        if let Statement::OnTopic { schema, .. } = &prog[0] {
            assert_eq!(schema.as_deref(), Some("payment_event"));
        } else {
            panic!("expected OnTopic");
        }
    }

    #[test]
    fn test_on_topic_rejects_wildcards() {
        let err = parse_err("on topic \"payments.*\" take event\n    x = 1\n");
        assert!(matches!(
            err,
            MarretaError::UnexpectedToken { expected, got_lexeme, .. }
                if expected.contains("exact topic") && got_lexeme == "payments.*"
        ));

        let err = parse_err("on topic \"payments.#\" take event\n    x = 1\n");
        assert!(matches!(
            err,
            MarretaError::UnexpectedToken { expected, got_lexeme, .. }
                if expected.contains("exact topic") && got_lexeme == "payments.#"
        ));
    }

    #[test]
    fn test_nack_no_requeue() {
        let prog = parse_ok("nack");
        if let Statement::Nack { requeue, .. } = &prog[0] {
            assert!(!requeue);
        } else {
            panic!("expected Nack");
        }
    }

    #[test]
    fn test_nack_with_requeue() {
        let prog = parse_ok("nack requeue");
        if let Statement::Nack { requeue, .. } = &prog[0] {
            assert!(requeue);
        } else {
            panic!("expected Nack");
        }
    }

    #[test]
    fn test_queue_push_no_schema() {
        let prog = parse_ok("queue.push \"orders\", { id: 1 }");
        if let Statement::ExpressionStatement { expression, .. } = &prog[0] {
            assert!(matches!(
                expression,
                Expression::QueuePush { schema: None, .. }
            ));
        } else {
            panic!("expected QueuePush expression statement");
        }
    }

    #[test]
    fn test_queue_push_with_schema() {
        let prog = parse_ok("queue.push \"orders\" as order_payload, { id: 1 }");
        if let Statement::ExpressionStatement { expression, .. } = &prog[0] {
            if let Expression::QueuePush {
                schema, queue_name, ..
            } = expression
            {
                assert_eq!(schema.as_deref(), Some("order_payload"));
                assert!(
                    matches!(queue_name.as_ref(), Expression::StringLiteral(s) if s == "orders")
                );
            } else {
                panic!("expected QueuePush");
            }
        } else {
            panic!("expected expression statement");
        }
    }

    #[test]
    fn test_queue_push_pipeline_no_payload() {
        let prog = parse_ok("x >> queue.push \"orders\"");
        if let Statement::ExpressionStatement { expression, .. } = &prog[0] {
            if let Expression::Pipeline { stages, .. } = expression {
                assert_eq!(stages.len(), 1);
                if let PipelineStage::Expression(Expression::QueuePush { payload, .. }) = &stages[0]
                {
                    assert!(payload.is_none());
                } else {
                    panic!("expected QueuePush in pipeline stage");
                }
            } else {
                panic!("expected Pipeline");
            }
        } else {
            panic!("expected expression statement");
        }
    }

    #[test]
    fn test_queue_publish_pipeline_no_payload() {
        let prog = parse_ok("x >> topic.publish \"order.created\"");
        if let Statement::ExpressionStatement { expression, .. } = &prog[0] {
            if let Expression::Pipeline { stages, .. } = expression {
                assert_eq!(stages.len(), 1);
                if let PipelineStage::Expression(Expression::TopicPublish { payload, .. }) =
                    &stages[0]
                {
                    assert!(payload.is_none());
                } else {
                    panic!("expected TopicPublish in pipeline stage");
                }
            } else {
                panic!("expected Pipeline");
            }
        } else {
            panic!("expected expression statement");
        }
    }

    #[test]
    fn test_cache_set_pipeline() {
        let prog = parse_ok("x >> cache.set(\"mykey\")");
        if let Statement::ExpressionStatement { expression, .. } = &prog[0] {
            if let Expression::Pipeline { stages, .. } = expression {
                assert_eq!(stages.len(), 1);
                if let PipelineStage::Expression(Expression::MethodCall {
                    object, method, ..
                }) = &stages[0]
                {
                    assert!(matches!(object.as_ref(), Expression::Identifier(n) if n == "cache"));
                    assert_eq!(method, "set");
                } else {
                    panic!(
                        "expected MethodCall in pipeline stage, got: {:?}",
                        stages[0]
                    );
                }
            } else {
                panic!("expected Pipeline, got: {:?}", expression);
            }
        } else {
            panic!("expected expression statement, got: {:?}", prog[0]);
        }
    }

    #[test]
    fn test_queue_publish_no_schema() {
        let prog = parse_ok("topic.publish \"payments.approved\", { id: 1 }");
        if let Statement::ExpressionStatement { expression, .. } = &prog[0] {
            assert!(matches!(
                expression,
                Expression::TopicPublish { schema: None, .. }
            ));
        } else {
            panic!("expected TopicPublish expression statement");
        }
    }

    #[test]
    fn test_queue_publish_with_schema() {
        let prog = parse_ok("topic.publish \"payments.approved\" as payment_event, { id: 1 }");
        if let Statement::ExpressionStatement { expression, .. } = &prog[0] {
            if let Expression::TopicPublish { schema, topic, .. } = expression {
                assert_eq!(schema.as_deref(), Some("payment_event"));
                assert!(
                    matches!(topic.as_ref(), Expression::StringLiteral(s) if s == "payments.approved")
                );
            } else {
                panic!("expected TopicPublish");
            }
        } else {
            panic!("expected expression statement");
        }
    }

    #[test]
    fn test_on_invalid_keyword_after_on() {
        let err = parse_err("on route \"x\" take m\n    x = 1\n");
        assert!(matches!(err, MarretaError::UnexpectedToken { .. }));
    }

    #[test]
    fn test_on_queue_multiline_body() {
        let src = "on queue \"q\" take m\n    a = 1\n    b = 2\n    c = 3\n";
        let prog = parse_ok(src);
        if let Statement::OnQueue { body, .. } = &prog[0] {
            assert_eq!(body.len(), 3);
        } else {
            panic!("expected OnQueue");
        }
    }

    #[test]
    fn test_nack_inside_on_queue_body() {
        let src = "on queue \"q\" take m\n    nack\n";
        let prog = parse_ok(src);
        if let Statement::OnQueue { body, .. } = &prog[0] {
            assert!(matches!(body[0], Statement::Nack { requeue: false, .. }));
        } else {
            panic!("expected OnQueue with Nack body");
        }
    }

    #[test]
    fn test_nack_requeue_inside_on_queue_body() {
        let src = "on queue \"q\" take m\n    nack requeue\n";
        let prog = parse_ok(src);
        if let Statement::OnQueue { body, .. } = &prog[0] {
            assert!(matches!(body[0], Statement::Nack { requeue: true, .. }));
        } else {
            panic!("expected OnQueue with Nack requeue body");
        }
    }

    #[test]
    fn test_require_else_nack() {
        let src = "on queue \"q\" take m\n    require m.id else nack\n";
        let prog = parse_ok(src);
        if let Statement::OnQueue { body, .. } = &prog[0] {
            assert!(matches!(
                body[0],
                Statement::Nack {
                    requeue: false,
                    condition: Some(_),
                    ..
                }
            ));
        } else {
            panic!("expected OnQueue with guarded Nack");
        }
    }

    #[test]
    fn test_require_else_nack_requeue() {
        let src = "on queue \"q\" take m\n    require m.amount > 0 else nack requeue\n";
        let prog = parse_ok(src);
        if let Statement::OnQueue { body, .. } = &prog[0] {
            assert!(matches!(
                body[0],
                Statement::Nack {
                    requeue: true,
                    condition: Some(_),
                    ..
                }
            ));
        } else {
            panic!("expected OnQueue with guarded Nack requeue");
        }
    }
}
