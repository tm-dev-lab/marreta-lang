use std::{borrow::Cow, fmt};

/// Semantic error categories for use in `error.code` and HTTP responses.
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorCode {
    RaiseError,
    DbError,
    UniqueViolation,
    QueueError,
    CacheError,
    HttpClientError,
    TypeError,
    ReferenceError,
    ArityError,
    ArithmeticError,
    IoError,
    ConfigError,
    InfrastructureError,
    RuntimeError,
    InvalidIdentifier,
    UnknownColumn,
}

impl ErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RaiseError => "raise_error",
            Self::DbError => "db_error",
            Self::UniqueViolation => "unique_violation",
            Self::QueueError => "queue_error",
            Self::CacheError => "cache_error",
            Self::HttpClientError => "http_client_error",
            Self::TypeError => "type_error",
            Self::ReferenceError => "reference_error",
            Self::ArityError => "arity_error",
            Self::ArithmeticError => "arithmetic_error",
            Self::IoError => "io_error",
            Self::ConfigError => "config_error",
            Self::InfrastructureError => "infrastructure_error",
            Self::RuntimeError => "runtime_error",
            Self::InvalidIdentifier => "invalid_identifier",
            Self::UnknownColumn => "unknown_column",
        }
    }
}

/// All error types produced by the MarretaLang engine.
#[derive(Debug, Clone, PartialEq)]
pub enum MarretaError {
    // --- Lexer Errors ---
    UnexpectedCharacter {
        char: char,
        line: usize,
        column: usize,
    },
    UnterminatedString {
        line: usize,
        column: usize,
    },
    InvalidIndentation {
        line: usize,
        expected: usize,
        got: usize,
    },
    UnexpectedIndentation {
        line: usize,
    },
    InvalidNumber {
        lexeme: String,
        line: usize,
        column: usize,
    },

    // --- Parser Errors ---
    UnexpectedToken {
        expected: String,
        got_lexeme: String,
        line: usize,
        column: usize,
    },
    UnexpectedEndOfInput {
        expected: String,
    },
    /// Spec 068: a reserved word (an infrastructure namespace, the `env` accessor, a type
    /// token, or a structural keyword) was used in a binder/declaration position, where only a
    /// fresh identifier is allowed. Reserved words are still free in every name position
    /// (after `.`, a map key, a schema field, a named arg, a `select` column).
    ReservedWord {
        word: String,
        line: usize,
        column: usize,
    },

    // --- Interpreter Errors ---
    UndefinedVariable {
        name: String,
        line: usize,
        column: usize,
    },
    UndefinedTask {
        name: String,
        line: usize,
        column: usize,
    },
    TypeError {
        message: String,
        line: usize,
        column: usize,
    },
    DivisionByZero {
        line: usize,
        column: usize,
    },
    WrongArity {
        task_name: String,
        expected: usize,
        got: usize,
        line: usize,
        column: usize,
    },
    NotCallable {
        name: String,
        line: usize,
        column: usize,
    },
    RuntimeError {
        message: String,
        line: usize,
        column: usize,
    },
    PropertyNotFound {
        object_type: String,
        property: String,
        line: usize,
        column: usize,
    },

    // --- HTTP Errors (used by require/reject) ---
    HttpError {
        status_code: i64,
        message: String,
    },

    // --- HTTP Control Flow (reply/fail — not real errors, used to terminate route execution) ---
    /// Emitted by `reply` and `fail` to terminate route execution and return an HTTP response.
    /// Body and content_type are stored as Strings to allow `PartialEq` + `Clone`.
    HttpResponse {
        status_code: u16,
        /// Pre-serialized body string (JSON, HTML, or plain text)
        body: String,
        /// MIME type, e.g. `"application/json"`, `"text/html"`, `"text/plain"`
        content_type: String,
        /// Extra response headers, e.g. `[("Location", "https://...")]` for redirects
        extra_headers: Vec<(String, String)>,
        /// `false` = reply (success), `true` = fail (error)
        is_error: bool,
    },

    // --- Route Validation ---
    /// Two routes have conflicting URL patterns (would cause axum panic at registration).
    RouteConflict {
        verb: String,
        path_a: String,
        path_b: String,
        line: usize,
        column: usize,
    },

    // --- Multi-file Errors (v0.3.2) ---
    /// Two files export a symbol with the same name.
    ExportConflict {
        name: String,
        file_a: String,
        file_b: String,
    },

    /// Project requires a newer Marreta runtime than the one running it (Spec 063).
    IncompatibleRuntime {
        /// The minimum version the project declares (`requires_marreta`).
        required: String,
        /// The version of the runtime actually running.
        actual: String,
    },

    // --- Schema Errors (v0.4.0) ---
    /// Schema reference graph contains a cycle — server cannot start.
    CircularSchemaReference {
        /// The cycle path, e.g. `"address → user_payload → address"`.
        cycle: String,
    },
    /// Schema declaration violates a language-level contract.
    InvalidSchemaDefinition {
        schema_name: String,
        message: String,
    },
    /// Persistent schema references a contract-only schema, which has no relational mapping.
    InvalidPersistentSchemaReference {
        schema_name: String,
        field_name: String,
        target_schema: String,
    },
    /// Persistent schema metadata is internally inconsistent for relational mapping.
    InvalidPersistentSchemaDefinition {
        schema_name: String,
        message: String,
    },

    // --- Domain Errors (v0.6.0) ---
    /// `raise MSG` — developer-signalled domain error; uncaught → HTTP 500
    RaiseError {
        message: String,
    },

    /// Database operation failure — wraps driver errors with Marreta-friendly messages
    DbError {
        message: String,
        operation: String,
    },

    /// A write violated a unique index/constraint. Raised identically by the relational and the
    /// document providers, and surfaced as HTTP 409 Conflict (Spec 067).
    UniqueConstraintViolation {
        message: String,
        operation: String,
    },

    /// Spec 076: a `db` identifier (an `order_by` clause, a `select` column, or a filter column)
    /// failed the runtime guard. `code` is `InvalidIdentifier` (illegal form) or `UnknownColumn`
    /// (valid form, not a known column). `message` is developer-controlled and never contains SQL,
    /// so it is safe to surface to the client as a 400.
    DbIdentifierError {
        code: ErrorCode,
        message: String,
    },

    // --- Queue (v0.8) ---
    /// `nack` / `nack requeue` — signals the queue runtime to reject a message.
    /// Not a user-visible error; caught by the consumer loop.
    NackSignal {
        requeue: bool,
    },

    /// Queue operation failure — connection, publish, or consume error
    QueueError {
        message: String,
        operation: String,
    },

    // --- Cache (v0.9) ---
    /// Cache operation failure — connection, timeout, serialization, or operation error
    CacheError {
        message: String,
        operation: String,
    },

    // --- HTTP Client (v0.10) ---
    /// HTTP client operation failure — connection, timeout, TLS, or invalid URL
    HttpClientError {
        message: String,
        operation: String,
    },

    // --- I/O Errors ---
    FileNotFound {
        path: String,
    },
    IoError {
        message: String,
    },
}

/// Spec 068: the human-readable role of a reserved word, used to explain why it cannot be a
/// name in a binder position. Namespaces and the `env` accessor get a specific phrase; every
/// other reserved word (structural keywords, type tokens) falls back to `None`.
fn reserved_word_role(word: &str) -> Option<&'static str> {
    match word {
        "doc" => Some("the document database namespace"),
        "feature" => Some("the feature-flag namespace"),
        "env" => Some("the environment accessor"),
        "db" => Some("the database namespace"),
        "queue" => Some("the queue namespace"),
        "topic" => Some("the topic namespace"),
        "cache" => Some("the cache namespace"),
        "fs" => Some("the filesystem namespace"),
        "json" => Some("the JSON namespace"),
        "base64" => Some("the base64 namespace"),
        "uuid" => Some("the UUID namespace"),
        "log" => Some("the log namespace"),
        "time" => Some("the time namespace"),
        "math" => Some("the math namespace"),
        "http_client" => Some("the HTTP client namespace"),
        _ => None,
    }
}

/// Spec 068: the dedicated reserved-word message, e.g.
/// `'doc' is a reserved word (the document database namespace); rename the variable.`
fn reserved_word_message(word: &str) -> String {
    match reserved_word_role(word) {
        Some(role) => format!(
            "'{}' is a reserved word ({}); rename the variable.",
            word, role
        ),
        None => format!("'{}' is a reserved word; rename the variable.", word),
    }
}

impl fmt::Display for MarretaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Lexer
            Self::UnexpectedCharacter { char, line, column } => {
                write!(
                    f,
                    "Error at {}:{}: unexpected character '{}'",
                    line, column, char
                )
            }
            Self::UnterminatedString { line, column } => {
                write!(
                    f,
                    "Error at {}:{}: unterminated string literal",
                    line, column
                )
            }
            Self::InvalidIndentation {
                line,
                expected,
                got,
            } => {
                write!(
                    f,
                    "Error at line {}: invalid indentation (expected {} spaces, got {})",
                    line, expected, got
                )
            }
            Self::UnexpectedIndentation { line } => {
                write!(
                    f,
                    "Error at line {}: invalid indentation (this line is indented deeper than expected)",
                    line
                )
            }
            Self::InvalidNumber {
                lexeme,
                line,
                column,
            } => {
                write!(
                    f,
                    "Error at {}:{}: invalid number '{}'",
                    line, column, lexeme
                )
            }

            // Parser
            Self::UnexpectedToken {
                expected,
                got_lexeme,
                line,
                column,
            } => {
                write!(
                    f,
                    "Error at {}:{}: expected {}, got '{}'",
                    line, column, expected, got_lexeme
                )
            }
            Self::UnexpectedEndOfInput { expected } => {
                write!(f, "Error: unexpected end of input, expected {}", expected)
            }
            Self::ReservedWord { word, line, column } => {
                write!(
                    f,
                    "Error at {}:{}: {}",
                    line,
                    column,
                    reserved_word_message(word)
                )
            }

            // Interpreter
            Self::UndefinedVariable { name, line, column } => {
                write!(
                    f,
                    "Error at {}:{}: variable '{}' is not defined",
                    line, column, name
                )
            }
            Self::UndefinedTask { name, line, column } => {
                write!(
                    f,
                    "Error at {}:{}: task '{}' is not defined",
                    line, column, name
                )
            }
            Self::TypeError {
                message,
                line,
                column,
            } => {
                write!(f, "Error at {}:{}: {}", line, column, message)
            }
            Self::DivisionByZero { line, column } => {
                write!(f, "Error at {}:{}: division by zero", line, column)
            }
            Self::WrongArity {
                task_name,
                expected,
                got,
                line,
                column,
            } => {
                write!(
                    f,
                    "Error at {}:{}: task '{}' expects {} argument(s), got {}",
                    line, column, task_name, expected, got
                )
            }
            Self::NotCallable { name, line, column } => {
                write!(
                    f,
                    "Error at {}:{}: '{}' is not callable",
                    line, column, name
                )
            }
            Self::RuntimeError {
                message,
                line,
                column,
            } => {
                write!(f, "Error at {}:{}: {}", line, column, message)
            }
            Self::PropertyNotFound {
                object_type,
                property,
                line,
                column,
            } => {
                write!(
                    f,
                    "Error at {}:{}: property '{}' not found on {}",
                    line, column, property, object_type
                )
            }

            // HTTP
            Self::HttpError {
                status_code,
                message,
            } => {
                write!(f, "HTTP {}: {}", status_code, message)
            }
            Self::HttpResponse {
                status_code,
                content_type,
                is_error,
                ..
            } => {
                if *is_error {
                    write!(f, "HTTP error {}", status_code)
                } else {
                    write!(f, "HTTP response {} ({})", status_code, content_type)
                }
            }
            Self::RouteConflict {
                verb,
                path_a,
                path_b,
                line,
                column,
            } => {
                write!(
                    f,
                    "Error at {}:{}: route conflict — {} \"{}\" and {} \"{}\" match the same URL pattern.\n  Use distinct path segments or different HTTP verbs.",
                    line, column, verb, path_a, verb, path_b
                )
            }

            Self::ExportConflict {
                name,
                file_a,
                file_b,
            } => {
                write!(
                    f,
                    "Export conflict: '{}' is already exported by '{}', redeclared in '{}'",
                    name, file_a, file_b
                )
            }

            Self::IncompatibleRuntime { required, actual } => {
                write!(
                    f,
                    "this project requires marreta runtime {}, but you are running {}",
                    required, actual
                )
            }
            Self::CircularSchemaReference { cycle } => {
                write!(
                    f,
                    "Circular schema reference detected: {}. The server cannot start.",
                    cycle
                )
            }
            Self::InvalidSchemaDefinition {
                schema_name,
                message,
            } => {
                write!(f, "Invalid schema '{}': {}", schema_name, message)
            }
            Self::InvalidPersistentSchemaReference {
                schema_name,
                field_name,
                target_schema,
            } => {
                write!(
                    f,
                    "Persistent schema '{}' field '{}' references non-persistent schema '{}'",
                    schema_name, field_name, target_schema
                )
            }
            Self::InvalidPersistentSchemaDefinition {
                schema_name,
                message,
            } => {
                write!(
                    f,
                    "Invalid persistent schema '{}': {}",
                    schema_name, message
                )
            }

            // Domain errors
            Self::RaiseError { message } => {
                write!(f, "raise: {}", message)
            }
            Self::DbError { message, .. } => {
                write!(f, "database error: {}", message)
            }
            Self::UniqueConstraintViolation { message, .. } => {
                write!(f, "unique constraint violation: {}", message)
            }
            Self::DbIdentifierError { message, .. } => {
                write!(f, "{}", message)
            }
            Self::NackSignal { requeue } => {
                write!(f, "nack(requeue={})", requeue)
            }
            Self::QueueError { message, .. } => {
                write!(f, "queue error: {}", message)
            }
            Self::CacheError { message, .. } => {
                write!(f, "cache error: {}", message)
            }
            Self::HttpClientError { message, .. } => {
                write!(f, "http client error: {}", message)
            }

            // I/O
            Self::FileNotFound { path } => {
                write!(f, "Error: file not found '{}'", path)
            }
            Self::IoError { message } => {
                write!(f, "Error: {}", message)
            }
        }
    }
}

impl MarretaError {
    /// Returns the semantic error code for use in the `error.code` rescue map field.
    pub fn semantic_code(&self) -> String {
        self.error_code().as_str().to_string()
    }

    /// Returns the structured `ErrorCode` for this error.
    pub fn error_code(&self) -> ErrorCode {
        match self {
            Self::RaiseError { .. } => ErrorCode::RaiseError,
            Self::DbError { .. } => ErrorCode::DbError,
            Self::UniqueConstraintViolation { .. } => ErrorCode::UniqueViolation,
            Self::DbIdentifierError { code, .. } => code.clone(),
            Self::NackSignal { .. } | Self::QueueError { .. } => ErrorCode::QueueError,
            Self::CacheError { .. } => ErrorCode::CacheError,
            Self::HttpClientError { .. } => ErrorCode::HttpClientError,
            Self::TypeError { .. } => ErrorCode::TypeError,
            Self::RuntimeError { .. } => ErrorCode::RuntimeError,
            Self::UndefinedVariable { .. }
            | Self::UndefinedTask { .. }
            | Self::PropertyNotFound { .. }
            | Self::NotCallable { .. } => ErrorCode::ReferenceError,
            Self::WrongArity { .. } => ErrorCode::ArityError,
            Self::DivisionByZero { .. } => ErrorCode::ArithmeticError,
            Self::IoError { .. } | Self::FileNotFound { .. } => ErrorCode::IoError,
            Self::RouteConflict { .. }
            | Self::ExportConflict { .. }
            | Self::CircularSchemaReference { .. }
            | Self::IncompatibleRuntime { .. }
            | Self::InvalidPersistentSchemaReference { .. }
            | Self::InvalidSchemaDefinition { .. }
            | Self::InvalidPersistentSchemaDefinition { .. } => ErrorCode::ConfigError,
            _ => ErrorCode::RuntimeError,
        }
    }

    /// Returns the operation name for use in the `error.op` rescue map field.
    pub fn operation_name(&self) -> String {
        match self {
            Self::RaiseError { .. } => "raise".to_string(),
            Self::DbError { operation, .. } => operation.clone(),
            Self::UniqueConstraintViolation { operation, .. } => operation.clone(),
            Self::NackSignal { .. } => "nack".to_string(),
            Self::QueueError { operation, .. } => operation.clone(),
            Self::CacheError { operation, .. } => operation.clone(),
            Self::HttpClientError { operation, .. } => operation.clone(),
            Self::UndefinedVariable { name, .. } => format!("lookup '{}'", name),
            Self::UndefinedTask { name, .. } => format!("call '{}'", name),
            Self::PropertyNotFound { property, .. } => format!("access '.{}'", property),
            Self::WrongArity { task_name, .. } => format!("call '{}'", task_name),
            Self::DivisionByZero { .. } => "arithmetic".to_string(),
            _ => "interpreter".to_string(),
        }
    }

    /// Returns an optional trace-oriented operation label for uncaught runtime
    /// traces. Only infrastructure/domain operations that add debugging value
    /// should appear as a dedicated innermost frame.
    pub fn trace_operation_label(&self) -> Option<Cow<'_, str>> {
        match self {
            Self::RaiseError { .. } => Some(Cow::Borrowed("raise")),
            Self::DbError { operation, .. } => Some(Cow::Borrowed(operation.as_str())),
            Self::UniqueConstraintViolation { operation, .. } => {
                Some(Cow::Borrowed(operation.as_str()))
            }
            Self::QueueError { operation, .. } => Some(Cow::Borrowed(operation.as_str())),
            Self::CacheError { operation, .. } => Some(Cow::Borrowed(operation.as_str())),
            Self::HttpClientError { operation, .. } => Some(Cow::Borrowed(operation.as_str())),
            _ => None,
        }
    }

    /// Returns a developer-friendly message with no Rust internals exposed.
    /// Each variant carries its full context — this method just extracts it.
    pub fn display_message(&self) -> String {
        match self {
            Self::RaiseError { message } => message.clone(),
            Self::DbError { message, .. } => message.clone(),
            Self::UniqueConstraintViolation { message, .. } => message.clone(),
            Self::DbIdentifierError { message, .. } => message.clone(),
            Self::NackSignal { requeue } => format!("nack(requeue={})", requeue),
            Self::QueueError { message, .. } => message.clone(),
            Self::CacheError { message, .. } => message.clone(),
            Self::HttpClientError { message, .. } => message.clone(),
            Self::TypeError { message, .. } => message.clone(),
            Self::RuntimeError { message, .. } => message.clone(),
            Self::UndefinedVariable { name, .. } => format!("variable '{}' is not defined", name),
            Self::UndefinedTask { name, .. } => format!("task '{}' is not defined", name),
            Self::DivisionByZero { .. } => "division by zero".to_string(),
            Self::WrongArity {
                task_name,
                expected,
                got,
                ..
            } => {
                format!(
                    "task '{}' expects {} argument(s), got {}",
                    task_name, expected, got
                )
            }
            Self::PropertyNotFound {
                property,
                object_type,
                ..
            } => {
                format!("property '{}' not found on {}", property, object_type)
            }
            Self::NotCallable { name, .. } => {
                format!("'{}' is not callable — expected a task", name)
            }
            Self::HttpError { message, .. } => message.clone(),
            Self::FileNotFound { path } => format!("file not found: {}", path),
            Self::IoError { message } => message.clone(),
            Self::RouteConflict { path_a, .. } => format!("route conflict: {}", path_a),
            Self::ExportConflict { name, .. } => {
                format!("export conflict: '{}' defined more than once", name)
            }
            Self::CircularSchemaReference { cycle } => {
                format!("circular schema reference involving '{}'", cycle)
            }
            Self::InvalidSchemaDefinition {
                schema_name,
                message,
            } => {
                format!("invalid schema '{}': {}", schema_name, message)
            }
            Self::InvalidPersistentSchemaReference {
                schema_name,
                field_name,
                target_schema,
            } => {
                format!(
                    "persistent schema '{}' field '{}' references non-persistent schema '{}'",
                    schema_name, field_name, target_schema
                )
            }
            Self::InvalidPersistentSchemaDefinition {
                schema_name,
                message,
            } => {
                format!("invalid persistent schema '{}': {}", schema_name, message)
            }
            Self::UnexpectedToken {
                expected,
                got_lexeme,
                ..
            } => {
                format!("unexpected token '{}', expected {}", got_lexeme, expected)
            }
            Self::UnexpectedCharacter { char, .. } => {
                format!("unexpected character '{}'", char)
            }
            Self::UnterminatedString { .. } => "unterminated string literal".to_string(),
            Self::InvalidIndentation { .. } => "invalid indentation".to_string(),
            Self::UnexpectedIndentation { .. } => "invalid indentation".to_string(),
            Self::InvalidNumber { lexeme, .. } => format!("invalid number literal: {}", lexeme),
            Self::UnexpectedEndOfInput { expected } => {
                format!("unexpected end of input, expected {}", expected)
            }
            Self::ReservedWord { word, .. } => reserved_word_message(word),
            _ => self.to_string(),
        }
    }

    /// Returns the line number if this error carries source location.
    pub fn line(&self) -> Option<usize> {
        match self {
            Self::UnexpectedCharacter { line, .. }
            | Self::UnterminatedString { line, .. }
            | Self::InvalidIndentation { line, .. }
            | Self::UnexpectedIndentation { line }
            | Self::InvalidNumber { line, .. }
            | Self::UnexpectedToken { line, .. }
            | Self::ReservedWord { line, .. }
            | Self::UndefinedVariable { line, .. }
            | Self::UndefinedTask { line, .. }
            | Self::TypeError { line, .. }
            | Self::RuntimeError { line, .. }
            | Self::DivisionByZero { line, .. }
            | Self::WrongArity { line, .. }
            | Self::NotCallable { line, .. }
            | Self::PropertyNotFound { line, .. }
            | Self::RouteConflict { line, .. } => Some(*line),
            _ => None,
        }
    }

    /// Returns the column number if this error carries source location.
    pub fn column(&self) -> Option<usize> {
        match self {
            Self::UnexpectedCharacter { column, .. }
            | Self::UnterminatedString { column, .. }
            | Self::InvalidNumber { column, .. }
            | Self::UnexpectedToken { column, .. }
            | Self::ReservedWord { column, .. }
            | Self::UndefinedVariable { column, .. }
            | Self::UndefinedTask { column, .. }
            | Self::TypeError { column, .. }
            | Self::RuntimeError { column, .. }
            | Self::DivisionByZero { column, .. }
            | Self::WrongArity { column, .. }
            | Self::NotCallable { column, .. }
            | Self::PropertyNotFound { column, .. }
            | Self::RouteConflict { column, .. } => Some(*column),
            _ => None,
        }
    }
}

impl std::error::Error for MarretaError {}

/// Convenience type alias for Results throughout the engine.
pub type MarretaResult<T> = Result<T, MarretaError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unexpected_character_display() {
        let err = MarretaError::UnexpectedCharacter {
            char: '@',
            line: 5,
            column: 12,
        };
        assert_eq!(
            format!("{}", err),
            "Error at 5:12: unexpected character '@'"
        );
    }

    #[test]
    fn test_unterminated_string_display() {
        let err = MarretaError::UnterminatedString { line: 3, column: 1 };
        assert_eq!(
            format!("{}", err),
            "Error at 3:1: unterminated string literal"
        );
    }

    #[test]
    fn test_invalid_indentation_display() {
        let err = MarretaError::InvalidIndentation {
            line: 7,
            expected: 4,
            got: 3,
        };
        assert_eq!(
            format!("{}", err),
            "Error at line 7: invalid indentation (expected 4 spaces, got 3)"
        );
    }

    #[test]
    fn test_invalid_number_display() {
        let err = MarretaError::InvalidNumber {
            lexeme: "3.14.5".into(),
            line: 2,
            column: 8,
        };
        assert_eq!(format!("{}", err), "Error at 2:8: invalid number '3.14.5'");
    }

    #[test]
    fn test_unexpected_token_display() {
        let err = MarretaError::UnexpectedToken {
            expected: "identifier".into(),
            got_lexeme: "+".into(),
            line: 1,
            column: 10,
        };
        assert_eq!(
            format!("{}", err),
            "Error at 1:10: expected identifier, got '+'"
        );
    }

    #[test]
    fn test_unexpected_end_of_input_display() {
        let err = MarretaError::UnexpectedEndOfInput {
            expected: "expression".into(),
        };
        assert_eq!(
            format!("{}", err),
            "Error: unexpected end of input, expected expression"
        );
    }

    #[test]
    fn test_undefined_variable_display() {
        let err = MarretaError::UndefinedVariable {
            name: "x".into(),
            line: 4,
            column: 5,
        };
        assert_eq!(
            format!("{}", err),
            "Error at 4:5: variable 'x' is not defined"
        );
    }

    #[test]
    fn test_undefined_task_display() {
        let err = MarretaError::UndefinedTask {
            name: "foo".into(),
            line: 10,
            column: 1,
        };
        assert_eq!(
            format!("{}", err),
            "Error at 10:1: task 'foo' is not defined"
        );
    }

    #[test]
    fn test_type_error_display() {
        let err = MarretaError::TypeError {
            message: "cannot add String and Integer".into(),
            line: 6,
            column: 3,
        };
        assert_eq!(
            format!("{}", err),
            "Error at 6:3: cannot add String and Integer"
        );
    }

    #[test]
    fn test_division_by_zero_display() {
        let err = MarretaError::DivisionByZero {
            line: 8,
            column: 15,
        };
        assert_eq!(format!("{}", err), "Error at 8:15: division by zero");
    }

    #[test]
    fn test_wrong_arity_display() {
        let err = MarretaError::WrongArity {
            task_name: "double".into(),
            expected: 1,
            got: 3,
            line: 12,
            column: 5,
        };
        assert_eq!(
            format!("{}", err),
            "Error at 12:5: task 'double' expects 1 argument(s), got 3"
        );
    }

    #[test]
    fn test_not_callable_display() {
        let err = MarretaError::NotCallable {
            name: "x".into(),
            line: 2,
            column: 1,
        };
        assert_eq!(format!("{}", err), "Error at 2:1: 'x' is not callable");
    }

    #[test]
    fn test_property_not_found_display() {
        let err = MarretaError::PropertyNotFound {
            object_type: "Integer".into(),
            property: "name".into(),
            line: 9,
            column: 7,
        };
        assert_eq!(
            format!("{}", err),
            "Error at 9:7: property 'name' not found on Integer"
        );
    }

    #[test]
    fn test_http_error_display() {
        let err = MarretaError::HttpError {
            status_code: 404,
            message: "Not found".into(),
        };
        assert_eq!(format!("{}", err), "HTTP 404: Not found");
    }

    #[test]
    fn test_file_not_found_display() {
        let err = MarretaError::FileNotFound {
            path: "app.marreta".into(),
        };
        assert_eq!(format!("{}", err), "Error: file not found 'app.marreta'");
    }

    #[test]
    fn test_io_error_display() {
        let err = MarretaError::IoError {
            message: "permission denied".into(),
        };
        assert_eq!(format!("{}", err), "Error: permission denied");
    }

    #[test]
    fn test_error_is_send_and_sync() {
        fn assert_error<T: std::error::Error>() {}
        assert_error::<MarretaError>();
    }

    #[test]
    fn test_http_response_reply_display() {
        let err = MarretaError::HttpResponse {
            status_code: 200,
            body: r#"{"ok":true}"#.into(),
            content_type: "application/json".into(),
            extra_headers: vec![],
            is_error: false,
        };
        assert_eq!(format!("{}", err), "HTTP response 200 (application/json)");
    }

    #[test]
    fn test_http_response_fail_display() {
        let err = MarretaError::HttpResponse {
            status_code: 404,
            body: r#"{"error":"Not found"}"#.into(),
            content_type: "application/json".into(),
            extra_headers: vec![],
            is_error: true,
        };
        assert_eq!(format!("{}", err), "HTTP error 404");
    }

    #[test]
    fn test_route_conflict_display() {
        let err = MarretaError::RouteConflict {
            verb: "GET".into(),
            path_a: "/users/:id".into(),
            path_b: "/users/:name".into(),
            line: 5,
            column: 1,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("5:1"));
        assert!(msg.contains("GET"));
        assert!(msg.contains("/users/:id"));
        assert!(msg.contains("/users/:name"));
    }

    #[test]
    fn test_export_conflict_display() {
        let err = MarretaError::ExportConflict {
            name: "tax_rate".into(),
            file_a: "tasks/pricing.marreta".into(),
            file_b: "tasks/discount.marreta".into(),
        };
        assert_eq!(
            format!("{}", err),
            "Export conflict: 'tax_rate' is already exported by 'tasks/pricing.marreta', redeclared in 'tasks/discount.marreta'"
        );
    }

    #[test]
    fn test_circular_schema_reference_display() {
        let err = MarretaError::CircularSchemaReference {
            cycle: "address → user → address".into(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("address → user → address"));
        assert!(msg.contains("cannot start"));
    }

    #[test]
    fn test_invalid_persistent_schema_reference_display() {
        let err = MarretaError::InvalidPersistentSchemaReference {
            schema_name: "User".into(),
            field_name: "address".into(),
            target_schema: "AddressPayload".into(),
        };
        assert_eq!(
            format!("{}", err),
            "Persistent schema 'User' field 'address' references non-persistent schema 'AddressPayload'"
        );
    }

    #[test]
    fn test_http_response_body_preserved() {
        let err = MarretaError::HttpResponse {
            status_code: 201,
            body: r#"{"id":42}"#.into(),
            content_type: "application/json".into(),
            extra_headers: vec![],
            is_error: false,
        };
        if let MarretaError::HttpResponse {
            body, status_code, ..
        } = err
        {
            assert_eq!(status_code, 201);
            assert_eq!(body, r#"{"id":42}"#);
        }
    }

    // --- RaiseError / DbError display ---

    #[test]
    fn test_raise_error_display() {
        let err = MarretaError::RaiseError {
            message: "boom".into(),
        };
        assert_eq!(format!("{}", err), "raise: boom");
    }

    #[test]
    fn test_db_error_display() {
        let err = MarretaError::DbError {
            message: "connection refused".into(),
            operation: "db.find".into(),
        };
        assert_eq!(format!("{}", err), "database error: connection refused");
    }

    // --- semantic_code (all variants) ---

    #[test]
    fn test_semantic_code_raise_error() {
        assert_eq!(
            MarretaError::RaiseError {
                message: "x".into()
            }
            .semantic_code(),
            "raise_error"
        );
    }

    #[test]
    fn test_semantic_code_db_error() {
        assert_eq!(
            MarretaError::DbError {
                message: "x".into(),
                operation: "op".into()
            }
            .semantic_code(),
            "db_error"
        );
    }

    #[test]
    fn test_semantic_code_type_error() {
        assert_eq!(
            MarretaError::TypeError {
                message: "x".into(),
                line: 1,
                column: 1
            }
            .semantic_code(),
            "type_error"
        );
    }

    #[test]
    fn test_semantic_code_reference_errors() {
        assert_eq!(
            MarretaError::UndefinedVariable {
                name: "x".into(),
                line: 1,
                column: 1
            }
            .semantic_code(),
            "reference_error"
        );
        assert_eq!(
            MarretaError::UndefinedTask {
                name: "f".into(),
                line: 1,
                column: 1
            }
            .semantic_code(),
            "reference_error"
        );
        assert_eq!(
            MarretaError::PropertyNotFound {
                object_type: "T".into(),
                property: "p".into(),
                line: 1,
                column: 1
            }
            .semantic_code(),
            "reference_error"
        );
        assert_eq!(
            MarretaError::NotCallable {
                name: "x".into(),
                line: 1,
                column: 1
            }
            .semantic_code(),
            "reference_error"
        );
    }

    #[test]
    fn test_semantic_code_arity_error() {
        assert_eq!(
            MarretaError::WrongArity {
                task_name: "f".into(),
                expected: 1,
                got: 2,
                line: 1,
                column: 1
            }
            .semantic_code(),
            "arity_error"
        );
    }

    #[test]
    fn test_semantic_code_arithmetic_error() {
        assert_eq!(
            MarretaError::DivisionByZero { line: 1, column: 1 }.semantic_code(),
            "arithmetic_error"
        );
    }

    #[test]
    fn test_semantic_code_io_errors() {
        assert_eq!(
            MarretaError::IoError {
                message: "x".into()
            }
            .semantic_code(),
            "io_error"
        );
        assert_eq!(
            MarretaError::FileNotFound { path: "f".into() }.semantic_code(),
            "io_error"
        );
    }

    #[test]
    fn test_semantic_code_config_errors() {
        assert_eq!(
            MarretaError::RouteConflict {
                verb: "GET".into(),
                path_a: "/a".into(),
                path_b: "/b".into(),
                line: 1,
                column: 1
            }
            .semantic_code(),
            "config_error"
        );
        assert_eq!(
            MarretaError::ExportConflict {
                name: "x".into(),
                file_a: "a".into(),
                file_b: "b".into()
            }
            .semantic_code(),
            "config_error"
        );
        assert_eq!(
            MarretaError::CircularSchemaReference {
                cycle: "a→b".into()
            }
            .semantic_code(),
            "config_error"
        );
        assert_eq!(
            MarretaError::InvalidPersistentSchemaReference {
                schema_name: "User".into(),
                field_name: "address".into(),
                target_schema: "AddressPayload".into(),
            }
            .semantic_code(),
            "config_error"
        );
    }

    #[test]
    fn test_semantic_code_runtime_error_fallback() {
        // Variants not matched by a specific arm → RuntimeError
        assert_eq!(
            MarretaError::HttpError {
                status_code: 400,
                message: "bad".into()
            }
            .semantic_code(),
            "runtime_error"
        );
        assert_eq!(
            MarretaError::UnexpectedToken {
                expected: "x".into(),
                got_lexeme: "y".into(),
                line: 1,
                column: 1
            }
            .semantic_code(),
            "runtime_error"
        );
        assert_eq!(
            MarretaError::UnexpectedEndOfInput {
                expected: "x".into()
            }
            .semantic_code(),
            "runtime_error"
        );
        assert_eq!(
            MarretaError::UnexpectedCharacter {
                char: '@',
                line: 1,
                column: 1
            }
            .semantic_code(),
            "runtime_error"
        );
        assert_eq!(
            MarretaError::HttpResponse {
                status_code: 200,
                body: "".into(),
                content_type: "application/json".into(),
                extra_headers: vec![],
                is_error: false,
            }
            .semantic_code(),
            "runtime_error"
        );
    }

    // --- operation_name (all variants) ---

    #[test]
    fn test_operation_name_raise_error() {
        assert_eq!(
            MarretaError::RaiseError {
                message: "x".into()
            }
            .operation_name(),
            "raise"
        );
    }

    #[test]
    fn test_operation_name_db_error() {
        assert_eq!(
            MarretaError::DbError {
                message: "x".into(),
                operation: "db.find".into()
            }
            .operation_name(),
            "db.find"
        );
    }

    #[test]
    fn test_operation_name_undefined_variable() {
        assert_eq!(
            MarretaError::UndefinedVariable {
                name: "foo".into(),
                line: 1,
                column: 1
            }
            .operation_name(),
            "lookup 'foo'"
        );
    }

    #[test]
    fn test_operation_name_undefined_task() {
        assert_eq!(
            MarretaError::UndefinedTask {
                name: "bar".into(),
                line: 1,
                column: 1
            }
            .operation_name(),
            "call 'bar'"
        );
    }

    #[test]
    fn test_operation_name_property_not_found() {
        assert_eq!(
            MarretaError::PropertyNotFound {
                object_type: "T".into(),
                property: "name".into(),
                line: 1,
                column: 1
            }
            .operation_name(),
            "access '.name'"
        );
    }

    #[test]
    fn test_operation_name_wrong_arity() {
        assert_eq!(
            MarretaError::WrongArity {
                task_name: "calc".into(),
                expected: 2,
                got: 1,
                line: 1,
                column: 1
            }
            .operation_name(),
            "call 'calc'"
        );
    }

    #[test]
    fn test_operation_name_division_by_zero() {
        assert_eq!(
            MarretaError::DivisionByZero { line: 1, column: 1 }.operation_name(),
            "arithmetic"
        );
    }

    #[test]
    fn test_operation_name_fallback_returns_interpreter() {
        assert_eq!(
            MarretaError::TypeError {
                message: "x".into(),
                line: 1,
                column: 1
            }
            .operation_name(),
            "interpreter"
        );
        assert_eq!(
            MarretaError::HttpError {
                status_code: 400,
                message: "bad".into()
            }
            .operation_name(),
            "interpreter"
        );
        assert_eq!(
            MarretaError::FileNotFound { path: "f".into() }.operation_name(),
            "interpreter"
        );
        assert_eq!(
            MarretaError::RouteConflict {
                verb: "GET".into(),
                path_a: "/a".into(),
                path_b: "/b".into(),
                line: 1,
                column: 1
            }
            .operation_name(),
            "interpreter"
        );
    }

    // --- display_message (all remaining variants) ---

    #[test]
    fn test_display_message_raise_error() {
        assert_eq!(
            MarretaError::RaiseError {
                message: "custom error".into()
            }
            .display_message(),
            "custom error"
        );
    }

    #[test]
    fn test_display_message_db_error() {
        assert_eq!(
            MarretaError::DbError {
                message: "timeout".into(),
                operation: "op".into()
            }
            .display_message(),
            "timeout"
        );
    }

    #[test]
    fn test_display_message_type_error() {
        assert_eq!(
            MarretaError::TypeError {
                message: "bad type".into(),
                line: 1,
                column: 1
            }
            .display_message(),
            "bad type"
        );
    }

    #[test]
    fn test_display_message_undefined_variable() {
        assert_eq!(
            MarretaError::UndefinedVariable {
                name: "x".into(),
                line: 1,
                column: 1
            }
            .display_message(),
            "variable 'x' is not defined"
        );
    }

    #[test]
    fn test_display_message_undefined_task() {
        assert_eq!(
            MarretaError::UndefinedTask {
                name: "foo".into(),
                line: 1,
                column: 1
            }
            .display_message(),
            "task 'foo' is not defined"
        );
    }

    #[test]
    fn test_display_message_division_by_zero() {
        assert_eq!(
            MarretaError::DivisionByZero { line: 1, column: 1 }.display_message(),
            "division by zero"
        );
    }

    #[test]
    fn test_display_message_wrong_arity() {
        assert_eq!(
            MarretaError::WrongArity {
                task_name: "f".into(),
                expected: 2,
                got: 1,
                line: 1,
                column: 1
            }
            .display_message(),
            "task 'f' expects 2 argument(s), got 1"
        );
    }

    #[test]
    fn test_display_message_property_not_found() {
        assert_eq!(
            MarretaError::PropertyNotFound {
                object_type: "String".into(),
                property: "push".into(),
                line: 1,
                column: 1
            }
            .display_message(),
            "property 'push' not found on String"
        );
    }

    #[test]
    fn test_display_message_not_callable() {
        assert_eq!(
            MarretaError::NotCallable {
                name: "x".into(),
                line: 1,
                column: 1
            }
            .display_message(),
            "'x' is not callable — expected a task"
        );
    }

    #[test]
    fn test_display_message_http_error() {
        assert_eq!(
            MarretaError::HttpError {
                status_code: 403,
                message: "Forbidden".into()
            }
            .display_message(),
            "Forbidden"
        );
    }

    #[test]
    fn test_display_message_file_not_found() {
        assert_eq!(
            MarretaError::FileNotFound {
                path: "routes/api.marreta".into()
            }
            .display_message(),
            "file not found: routes/api.marreta"
        );
    }

    #[test]
    fn test_display_message_io_error() {
        assert_eq!(
            MarretaError::IoError {
                message: "permission denied".into()
            }
            .display_message(),
            "permission denied"
        );
    }

    #[test]
    fn test_display_message_route_conflict() {
        let msg = MarretaError::RouteConflict {
            verb: "GET".into(),
            path_a: "/users/:id".into(),
            path_b: "/users/:name".into(),
            line: 5,
            column: 1,
        }
        .display_message();
        assert!(msg.contains("/users/:id"));
    }

    #[test]
    fn test_display_message_export_conflict() {
        let msg = MarretaError::ExportConflict {
            name: "rate".into(),
            file_a: "a.marreta".into(),
            file_b: "b.marreta".into(),
        }
        .display_message();
        assert!(msg.contains("rate"));
    }

    #[test]
    fn test_display_message_circular_schema() {
        let msg = MarretaError::CircularSchemaReference {
            cycle: "a → b → a".into(),
        }
        .display_message();
        assert!(msg.contains("a → b → a"));
    }

    #[test]
    fn test_display_message_invalid_persistent_schema_reference() {
        let msg = MarretaError::InvalidPersistentSchemaReference {
            schema_name: "User".into(),
            field_name: "address".into(),
            target_schema: "AddressPayload".into(),
        }
        .display_message();
        assert!(msg.contains("User"));
        assert!(msg.contains("address"));
        assert!(msg.contains("AddressPayload"));
    }

    #[test]
    fn test_display_message_unexpected_token() {
        assert_eq!(
            MarretaError::UnexpectedToken {
                expected: "identifier".into(),
                got_lexeme: "123".into(),
                line: 1,
                column: 1,
            }
            .display_message(),
            "unexpected token '123', expected identifier"
        );
    }

    #[test]
    fn test_display_message_unexpected_character() {
        assert_eq!(
            MarretaError::UnexpectedCharacter {
                char: '@',
                line: 1,
                column: 1
            }
            .display_message(),
            "unexpected character '@'"
        );
    }

    #[test]
    fn test_display_message_unterminated_string() {
        assert_eq!(
            MarretaError::UnterminatedString { line: 1, column: 1 }.display_message(),
            "unterminated string literal"
        );
    }

    #[test]
    fn test_display_message_invalid_indentation() {
        assert_eq!(
            MarretaError::InvalidIndentation {
                line: 1,
                expected: 4,
                got: 2
            }
            .display_message(),
            "invalid indentation"
        );
    }

    #[test]
    fn test_display_message_invalid_number() {
        assert_eq!(
            MarretaError::InvalidNumber {
                lexeme: "3.14.5".into(),
                line: 1,
                column: 1
            }
            .display_message(),
            "invalid number literal: 3.14.5"
        );
    }

    #[test]
    fn test_display_message_unexpected_end_of_input() {
        assert_eq!(
            MarretaError::UnexpectedEndOfInput {
                expected: "expression".into()
            }
            .display_message(),
            "unexpected end of input, expected expression"
        );
    }

    // --- line() and column() ---

    #[test]
    fn test_line_returns_some_for_located_errors() {
        assert_eq!(
            MarretaError::UnexpectedCharacter {
                char: '@',
                line: 5,
                column: 3
            }
            .line(),
            Some(5)
        );
        assert_eq!(
            MarretaError::UnterminatedString { line: 7, column: 1 }.line(),
            Some(7)
        );
        assert_eq!(
            MarretaError::InvalidIndentation {
                line: 2,
                expected: 4,
                got: 3
            }
            .line(),
            Some(2)
        );
        assert_eq!(
            MarretaError::InvalidNumber {
                lexeme: "x".into(),
                line: 9,
                column: 2
            }
            .line(),
            Some(9)
        );
        assert_eq!(
            MarretaError::UnexpectedToken {
                expected: "x".into(),
                got_lexeme: "y".into(),
                line: 4,
                column: 1
            }
            .line(),
            Some(4)
        );
        assert_eq!(
            MarretaError::UndefinedVariable {
                name: "x".into(),
                line: 6,
                column: 1
            }
            .line(),
            Some(6)
        );
        assert_eq!(
            MarretaError::UndefinedTask {
                name: "f".into(),
                line: 8,
                column: 1
            }
            .line(),
            Some(8)
        );
        assert_eq!(
            MarretaError::TypeError {
                message: "x".into(),
                line: 10,
                column: 1
            }
            .line(),
            Some(10)
        );
        assert_eq!(
            MarretaError::DivisionByZero {
                line: 11,
                column: 3
            }
            .line(),
            Some(11)
        );
        assert_eq!(
            MarretaError::WrongArity {
                task_name: "f".into(),
                expected: 1,
                got: 2,
                line: 12,
                column: 1
            }
            .line(),
            Some(12)
        );
        assert_eq!(
            MarretaError::NotCallable {
                name: "x".into(),
                line: 13,
                column: 1
            }
            .line(),
            Some(13)
        );
        assert_eq!(
            MarretaError::PropertyNotFound {
                object_type: "T".into(),
                property: "p".into(),
                line: 14,
                column: 1
            }
            .line(),
            Some(14)
        );
        assert_eq!(
            MarretaError::RouteConflict {
                verb: "GET".into(),
                path_a: "/a".into(),
                path_b: "/b".into(),
                line: 15,
                column: 1
            }
            .line(),
            Some(15)
        );
    }

    #[test]
    fn test_line_returns_none_for_unlocated_errors() {
        assert_eq!(
            MarretaError::UnexpectedEndOfInput {
                expected: "x".into()
            }
            .line(),
            None
        );
        assert_eq!(
            MarretaError::RaiseError {
                message: "x".into()
            }
            .line(),
            None
        );
        assert_eq!(
            MarretaError::DbError {
                message: "x".into(),
                operation: "op".into()
            }
            .line(),
            None
        );
        assert_eq!(
            MarretaError::HttpError {
                status_code: 400,
                message: "x".into()
            }
            .line(),
            None
        );
        assert_eq!(MarretaError::FileNotFound { path: "f".into() }.line(), None);
        assert_eq!(
            MarretaError::IoError {
                message: "x".into()
            }
            .line(),
            None
        );
        assert_eq!(
            MarretaError::ExportConflict {
                name: "x".into(),
                file_a: "a".into(),
                file_b: "b".into()
            }
            .line(),
            None
        );
        assert_eq!(
            MarretaError::CircularSchemaReference { cycle: "x".into() }.line(),
            None
        );
        assert_eq!(
            MarretaError::HttpResponse {
                status_code: 200,
                body: "".into(),
                content_type: "".into(),
                extra_headers: vec![],
                is_error: false
            }
            .line(),
            None
        );
    }

    #[test]
    fn test_column_returns_some_for_located_errors() {
        assert_eq!(
            MarretaError::UnexpectedCharacter {
                char: '@',
                line: 1,
                column: 5
            }
            .column(),
            Some(5)
        );
        assert_eq!(
            MarretaError::UnterminatedString { line: 1, column: 9 }.column(),
            Some(9)
        );
        assert_eq!(
            MarretaError::InvalidNumber {
                lexeme: "x".into(),
                line: 1,
                column: 3
            }
            .column(),
            Some(3)
        );
        assert_eq!(
            MarretaError::UnexpectedToken {
                expected: "x".into(),
                got_lexeme: "y".into(),
                line: 1,
                column: 7
            }
            .column(),
            Some(7)
        );
        assert_eq!(
            MarretaError::UndefinedVariable {
                name: "x".into(),
                line: 1,
                column: 2
            }
            .column(),
            Some(2)
        );
        assert_eq!(
            MarretaError::DivisionByZero { line: 1, column: 8 }.column(),
            Some(8)
        );
    }

    #[test]
    fn test_column_returns_none_for_unlocated_errors() {
        assert_eq!(
            MarretaError::InvalidIndentation {
                line: 1,
                expected: 4,
                got: 2
            }
            .column(),
            None
        );
        assert_eq!(
            MarretaError::RaiseError {
                message: "x".into()
            }
            .column(),
            None
        );
        assert_eq!(
            MarretaError::DbError {
                message: "x".into(),
                operation: "op".into()
            }
            .column(),
            None
        );
        assert_eq!(
            MarretaError::UnexpectedEndOfInput {
                expected: "x".into()
            }
            .column(),
            None
        );
        assert_eq!(
            MarretaError::ExportConflict {
                name: "x".into(),
                file_a: "a".into(),
                file_b: "b".into()
            }
            .column(),
            None
        );
    }

    #[test]
    fn test_trace_operation_label_only_for_traceworthy_errors() {
        assert_eq!(
            MarretaError::DbError {
                message: "x".into(),
                operation: "db.users.find".into()
            }
            .trace_operation_label(),
            Some("db.users.find".into())
        );
        assert_eq!(
            MarretaError::RaiseError {
                message: "boom".into()
            }
            .trace_operation_label(),
            Some("raise".into())
        );
        assert_eq!(
            MarretaError::TypeError {
                message: "x".into(),
                line: 1,
                column: 1
            }
            .trace_operation_label(),
            None
        );
    }

    // --- ErrorCode::as_str ---

    #[test]
    fn test_error_code_as_str_all_variants() {
        assert_eq!(ErrorCode::RaiseError.as_str(), "raise_error");
        assert_eq!(ErrorCode::DbError.as_str(), "db_error");
        assert_eq!(ErrorCode::TypeError.as_str(), "type_error");
        assert_eq!(ErrorCode::ReferenceError.as_str(), "reference_error");
        assert_eq!(ErrorCode::ArityError.as_str(), "arity_error");
        assert_eq!(ErrorCode::ArithmeticError.as_str(), "arithmetic_error");
        assert_eq!(ErrorCode::IoError.as_str(), "io_error");
        assert_eq!(ErrorCode::ConfigError.as_str(), "config_error");
        assert_eq!(
            ErrorCode::InfrastructureError.as_str(),
            "infrastructure_error"
        );
        assert_eq!(ErrorCode::RuntimeError.as_str(), "runtime_error");
    }
}
