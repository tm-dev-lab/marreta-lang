/// AI-agent knowledge assets (AGENTS.md primer + llms pointers).
pub mod agents;
/// Abstract syntax tree node definitions.
pub mod ast;
/// Authentication and authorization provider configuration/runtime support.
pub mod auth;
/// Server configuration loaded from `marreta.env` and environment variables.
pub mod config;
/// Shared test-coverage consolidation for `marreta test --coverage` and `marreta doctor`.
pub mod coverage;
/// Database abstraction layer — driver trait, query builder, Postgres impl.
pub mod db;
/// Document database abstraction layer — DocDriver trait, DocQueryState, MongoDB impl.
pub mod doc;
/// Variable scope management for the interpreter.
pub mod environment;
/// Error types used throughout the MarretaLang engine.
pub mod error;
/// Static boolean feature flags backed by MARRETA_FEATURE_* runtime config.
pub mod feature_flags;
/// Multi-file project loader — file scanning and two-pass loading (v0.3.2).
pub mod file_loader;
/// Source formatter support for `marreta fmt`.
pub mod formatter;
/// Tree-walking interpreter that executes the AST.
pub mod interpreter;
/// Lexer (tokenizer) for MarretaLang source code.
pub mod lexer;
/// Source lint support for `marreta lint`.
pub mod lint;
/// Relational migration modeling and planning.
pub mod migrations;
/// OpenAPI 3.0 spec builder — generates JSON from RouteRegistry at startup.
pub mod openapi;
/// Parser that transforms tokens into an AST.
pub mod parser;
/// Persistent schema helpers — relational schema extraction and validation.
pub mod persistent_schema;
/// Queue abstraction layer — QueueDriver trait, RabbitMQ impl, engine (v0.8).
pub mod queue;
/// Response serializer — filters reply values against a schema, stripping undeclared fields (v0.3.3).
pub mod response_serializer;
/// HTTP route registry and conflict detection.
pub mod route_loader;
/// Runtime hot-path profiling helpers.
pub mod runtime_profile;
/// Relation-aware schema-reference cycle detection (shared by loader + lint, Spec 062).
pub mod schema_cycle;
/// HTTP server (axum) — route registration and request handling.
pub mod server;
/// Token types and keyword lookup.
pub mod token;
/// Editor tooling support for catalog, completions, hover, and symbols.
pub mod tooling;
/// Runtime W3C Trace Context parsing and propagation helpers.
pub mod trace_context;
/// Schema payload validator — enforces field types and required fields (HTTP 422).
pub mod validator;
/// Runtime value representation.
pub mod value;
/// Runtime/CLI version metadata.
pub mod version;

pub mod cache;
/// Project doctor command support — intent discovery and validation (v0.14b).
pub mod doctor;
/// HTTP client abstraction layer — HttpClient trait, ReqwestDriver, engine (v0.10).
pub mod http_client;
/// Project scaffold generation for `marreta init`.
pub mod init;
/// API scenario testing support for `marreta test`.
pub mod scenario_tests;
