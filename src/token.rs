/// All possible token types in MarretaLang.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // --- Literals ---
    Integer(i64),
    Float(f64),
    StringLiteral(String),
    True,
    False,
    Null,

    // --- Identifiers ---
    Identifier(String),

    // --- Arithmetic Operators ---
    Plus,    // +
    Minus,   // -
    Star,    // *
    Slash,   // /
    Percent, // %

    // --- Comparison Operators ---
    Equal,        // ==
    NotEqual,     // !=
    Greater,      // >
    Less,         // <
    GreaterEqual, // >=
    LessEqual,    // <=

    // --- Logical Operators (words) ---
    And, // and
    Or,  // or
    Not, // not

    // --- Assignment ---
    Assign, // =

    // --- Pipeline Operators ---
    Pipeline,  // >>
    Broadcast, // *>>

    // --- Arrows ---
    Arrow,    // ->
    FatArrow, // =>

    // --- Delimiters ---
    LeftParen,    // (
    RightParen,   // )
    LeftBracket,  // [
    RightBracket, // ]
    LeftBrace,    // {
    RightBrace,   // }
    Comma,        // ,
    Dot,          // .
    Colon,        // :

    // --- Reserved Words — Core ---
    Task,
    Match,
    Fallback,
    Map,
    Reduce,
    Keep,
    Skip,
    Require,
    Reject,
    While,
    If,
    Else,

    // --- Reserved Words — HTTP (reserved, not implemented in v0.1) ---
    Route,
    Take,
    Reply,
    Fail,
    Listen,

    // --- HTTP Verbs (reserved, not implemented in v0.1) ---
    Get,
    Post,
    Put,
    Patch,
    Delete,

    // --- Reserved Words — Infra (reserved, not implemented in v0.1) ---
    Db,
    Queue,
    Cache,
    Fs,
    Json,
    Base64,
    Uuid,
    Log,
    Time,
    Math,
    HttpClient,

    // --- Reserved Words — Schema & AutoDoc (v0.3.1) ---
    Schema,
    As,

    // --- Reserved Words — Multi-file (v0.3.2) ---
    Export,

    // --- Reserved Words — Advanced Schemas (v0.4.0) ---
    Of,

    // --- Reserved Words — DB (v0.5.0) ---
    Transaction,

    // --- Reserved Words — Error Handling (v0.6.0) ---
    Raise,
    Rescue,

    // --- Reserved Words — Queue (v0.8) ---
    On,
    Topic,
    Nack,
    Requeue,

    // --- Schema Type Keywords ---
    TypeString,
    TypeInteger,
    TypeFloat,
    TypeBoolean,
    TypeInstant,
    TypeDate,
    TypeDuration,
    TypeInterval,
    TypeList,
    TypeMap,

    // --- Control ---
    Newline,
    Indent,
    Dedent,
    Eof,
}

/// A single token produced by the Lexer.
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
    pub lexeme: String,
}

impl Token {
    /// Creates a new token with the given kind, position, and lexeme.
    pub fn new(kind: TokenKind, line: usize, column: usize, lexeme: impl Into<String>) -> Self {
        Self {
            kind,
            line,
            column,
            lexeme: lexeme.into(),
        }
    }

    /// Creates an EOF token at the given position.
    pub fn eof(line: usize, column: usize) -> Self {
        Self::new(TokenKind::Eof, line, column, "")
    }
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?} '{}' at {}:{}",
            self.kind, self.lexeme, self.line, self.column
        )
    }
}

/// Looks up a word to determine if it is a reserved keyword.
/// Returns `None` if the word is a regular identifier.
pub fn keyword_lookup(word: &str) -> Option<TokenKind> {
    match word {
        // Core
        "task" => Some(TokenKind::Task),
        "match" => Some(TokenKind::Match),
        "fallback" => Some(TokenKind::Fallback),
        "map" => Some(TokenKind::Map),
        "reduce" => Some(TokenKind::Reduce),
        "keep" => Some(TokenKind::Keep),
        "skip" => Some(TokenKind::Skip),
        "require" => Some(TokenKind::Require),
        "reject" => Some(TokenKind::Reject),
        "while" => Some(TokenKind::While),
        "if" => Some(TokenKind::If),
        "else" => Some(TokenKind::Else),

        // Logical
        "and" => Some(TokenKind::And),
        "or" => Some(TokenKind::Or),
        "not" => Some(TokenKind::Not),

        // Literals
        "true" => Some(TokenKind::True),
        "false" => Some(TokenKind::False),
        "null" => Some(TokenKind::Null),

        // HTTP
        "route" => Some(TokenKind::Route),
        "take" => Some(TokenKind::Take),
        "reply" => Some(TokenKind::Reply),
        "fail" => Some(TokenKind::Fail),
        "listen" => Some(TokenKind::Listen),

        // Infra
        "db" => Some(TokenKind::Db),
        "queue" => Some(TokenKind::Queue),
        "cache" => Some(TokenKind::Cache),
        "fs" => Some(TokenKind::Fs),
        "json" => Some(TokenKind::Json),
        "base64" => Some(TokenKind::Base64),
        "uuid" => Some(TokenKind::Uuid),
        "log" => Some(TokenKind::Log),
        "time" => Some(TokenKind::Time),
        "math" => Some(TokenKind::Math),
        "http_client" => Some(TokenKind::HttpClient),

        // HTTP Verbs
        "GET" => Some(TokenKind::Get),
        "POST" => Some(TokenKind::Post),
        "PUT" => Some(TokenKind::Put),
        "PATCH" => Some(TokenKind::Patch),
        "DELETE" => Some(TokenKind::Delete),

        // Schema & AutoDoc
        "schema" => Some(TokenKind::Schema),
        "as" => Some(TokenKind::As),

        // Multi-file
        "export" => Some(TokenKind::Export),

        // Advanced Schemas (v0.4.0)
        "of" => Some(TokenKind::Of),
        "string" => Some(TokenKind::TypeString),
        "integer" => Some(TokenKind::TypeInteger),
        "float" => Some(TokenKind::TypeFloat),
        "boolean" => Some(TokenKind::TypeBoolean),
        "instant" => Some(TokenKind::TypeInstant),
        "date" => Some(TokenKind::TypeDate),
        "duration" => Some(TokenKind::TypeDuration),
        "interval" => Some(TokenKind::TypeInterval),
        // Note: "list" and "map" reuse TokenKind::TypeList/TypeMap but are NOT registered
        // as keywords here — "map" is already TokenKind::Map (pipeline keyword) and "list"
        // is not a reserved word. The schema parser handles them via Identifier fallback.

        // DB (v0.5.0)
        "transaction" => Some(TokenKind::Transaction),

        // Error Handling (v0.6.0)
        "raise" => Some(TokenKind::Raise),
        "rescue" => Some(TokenKind::Rescue),

        // Queue (v0.8)
        "on" => Some(TokenKind::On),
        "topic" => Some(TokenKind::Topic),
        "nack" => Some(TokenKind::Nack),
        "requeue" => Some(TokenKind::Requeue),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_lookup_core() {
        assert_eq!(keyword_lookup("task"), Some(TokenKind::Task));
        assert_eq!(keyword_lookup("match"), Some(TokenKind::Match));
        assert_eq!(keyword_lookup("fallback"), Some(TokenKind::Fallback));
        assert_eq!(keyword_lookup("map"), Some(TokenKind::Map));
        assert_eq!(keyword_lookup("reduce"), Some(TokenKind::Reduce));
        assert_eq!(keyword_lookup("keep"), Some(TokenKind::Keep));
        assert_eq!(keyword_lookup("require"), Some(TokenKind::Require));
        assert_eq!(keyword_lookup("reject"), Some(TokenKind::Reject));
        assert_eq!(keyword_lookup("while"), Some(TokenKind::While));
        assert_eq!(keyword_lookup("if"), Some(TokenKind::If));
        assert_eq!(keyword_lookup("else"), Some(TokenKind::Else));
    }

    #[test]
    fn test_keyword_lookup_logical() {
        assert_eq!(keyword_lookup("and"), Some(TokenKind::And));
        assert_eq!(keyword_lookup("or"), Some(TokenKind::Or));
        assert_eq!(keyword_lookup("not"), Some(TokenKind::Not));
    }

    #[test]
    fn test_keyword_lookup_literals() {
        assert_eq!(keyword_lookup("true"), Some(TokenKind::True));
        assert_eq!(keyword_lookup("false"), Some(TokenKind::False));
        assert_eq!(keyword_lookup("null"), Some(TokenKind::Null));
    }

    #[test]
    fn test_keyword_lookup_http() {
        assert_eq!(keyword_lookup("route"), Some(TokenKind::Route));
        assert_eq!(keyword_lookup("take"), Some(TokenKind::Take));
        assert_eq!(keyword_lookup("reply"), Some(TokenKind::Reply));
        assert_eq!(keyword_lookup("fail"), Some(TokenKind::Fail));
        assert_eq!(keyword_lookup("listen"), Some(TokenKind::Listen));
    }

    #[test]
    fn test_keyword_lookup_http_verbs() {
        assert_eq!(keyword_lookup("GET"), Some(TokenKind::Get));
        assert_eq!(keyword_lookup("POST"), Some(TokenKind::Post));
        assert_eq!(keyword_lookup("PUT"), Some(TokenKind::Put));
        assert_eq!(keyword_lookup("PATCH"), Some(TokenKind::Patch));
        assert_eq!(keyword_lookup("DELETE"), Some(TokenKind::Delete));
    }

    #[test]
    fn test_keyword_lookup_infra() {
        assert_eq!(keyword_lookup("db"), Some(TokenKind::Db));
        assert_eq!(keyword_lookup("queue"), Some(TokenKind::Queue));
        assert_eq!(keyword_lookup("cache"), Some(TokenKind::Cache));
        assert_eq!(keyword_lookup("fs"), Some(TokenKind::Fs));
        assert_eq!(keyword_lookup("json"), Some(TokenKind::Json));
        assert_eq!(keyword_lookup("base64"), Some(TokenKind::Base64));
        assert_eq!(keyword_lookup("uuid"), Some(TokenKind::Uuid));
        assert_eq!(keyword_lookup("log"), Some(TokenKind::Log));
        assert_eq!(keyword_lookup("time"), Some(TokenKind::Time));
        assert_eq!(keyword_lookup("math"), Some(TokenKind::Math));
        assert_eq!(keyword_lookup("http_client"), Some(TokenKind::HttpClient));
    }

    #[test]
    fn test_keyword_lookup_scenarios() {
        assert_eq!(keyword_lookup("scenario"), None);
        assert_eq!(keyword_lookup("given"), None);
        assert_eq!(keyword_lookup("when"), None);
        assert_eq!(keyword_lookup("then"), None);
        assert_eq!(keyword_lookup("returns"), None);
    }

    #[test]
    fn test_keyword_lookup_schema() {
        assert_eq!(keyword_lookup("schema"), Some(TokenKind::Schema));
        assert_eq!(keyword_lookup("as"), Some(TokenKind::As));
        assert_eq!(keyword_lookup("string"), Some(TokenKind::TypeString));
        assert_eq!(keyword_lookup("integer"), Some(TokenKind::TypeInteger));
        assert_eq!(keyword_lookup("float"), Some(TokenKind::TypeFloat));
        assert_eq!(keyword_lookup("boolean"), Some(TokenKind::TypeBoolean));
        assert_eq!(keyword_lookup("time"), Some(TokenKind::Time));
        assert_eq!(keyword_lookup("instant"), Some(TokenKind::TypeInstant));
        assert_eq!(keyword_lookup("date"), Some(TokenKind::TypeDate));
        assert_eq!(keyword_lookup("duration"), Some(TokenKind::TypeDuration));
        assert_eq!(keyword_lookup("interval"), Some(TokenKind::TypeInterval));
        // "list" and "map" are NOT schema type keywords (would conflict with pipeline `map`)
        assert_eq!(keyword_lookup("list"), None);
    }

    #[test]
    fn test_keyword_lookup_export() {
        assert_eq!(keyword_lookup("export"), Some(TokenKind::Export));
        assert_eq!(keyword_lookup("Export"), None); // case-sensitive
    }

    #[test]
    fn test_keyword_lookup_queue() {
        assert_eq!(keyword_lookup("on"), Some(TokenKind::On));
        assert_eq!(keyword_lookup("topic"), Some(TokenKind::Topic));
        assert_eq!(keyword_lookup("nack"), Some(TokenKind::Nack));
        assert_eq!(keyword_lookup("requeue"), Some(TokenKind::Requeue));
        // queue is already in infra group — verify still present
        assert_eq!(keyword_lookup("queue"), Some(TokenKind::Queue));
        // case-sensitive
        assert_eq!(keyword_lookup("On"), None);
        assert_eq!(keyword_lookup("NACK"), None);
    }

    #[test]
    fn test_keyword_lookup_identifiers_return_none() {
        assert_eq!(keyword_lookup("foo"), None);
        assert_eq!(keyword_lookup("myVar"), None);
        assert_eq!(keyword_lookup("payload"), None);
        assert_eq!(keyword_lookup("Task"), None); // case-sensitive
        assert_eq!(keyword_lookup("TRUE"), None); // case-sensitive
    }

    #[test]
    fn test_token_new() {
        let tok = Token::new(TokenKind::Integer(42), 1, 5, "42");
        assert_eq!(tok.kind, TokenKind::Integer(42));
        assert_eq!(tok.line, 1);
        assert_eq!(tok.column, 5);
        assert_eq!(tok.lexeme, "42");
    }

    #[test]
    fn test_token_eof() {
        let tok = Token::eof(10, 1);
        assert_eq!(tok.kind, TokenKind::Eof);
        assert_eq!(tok.line, 10);
    }

    #[test]
    fn test_token_display() {
        let tok = Token::new(TokenKind::Plus, 3, 7, "+");
        let display = format!("{}", tok);
        assert!(display.contains("Plus"));
        assert!(display.contains("3:7"));
    }
}
