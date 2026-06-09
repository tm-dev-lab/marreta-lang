# MarretaLang — Core Implementation Plan (v0.1)

> Status: Delivered.

> **Goal:** Implement the MarretaLang core engine in Rust, capable of reading `.marreta` files, transforming them into an AST, and executing via interpreter.
>
> **At the end of this phase**, the engine should be able to execute scripts with: variables, operators, conditionals (`require`/`reject`/`match`/`if` suffix), tasks, pipelines (`>>`/`*>>`/`map`/`keep`), lists, maps, and an interactive REPL.
>
> **Not included in this phase:** HTTP, database, messaging, cache.

---

## Table of Contents

1. [General Architecture](#1-general-architecture)
2. [Phase 1 — Rust Project Scaffold](#2-phase-1--rust-project-scaffold)
3. [Phase 2 — Lexer (Tokenizer)](#3-phase-2--lexer-tokenizer)
4. [Phase 3 — Parser (Syntactic Analysis)](#4-phase-3--parser-syntactic-analysis)
5. [Phase 4 — AST (Abstract Syntax Tree)](#5-phase-4--ast-abstract-syntax-tree)
6. [Phase 5 — Interpreter (Execution Engine)](#6-phase-5--interpreter-execution-engine)
7. [Phase 6 — Type System and Values](#7-phase-6--type-system-and-values)
8. [Phase 7 — Variables and Scope](#8-phase-7--variables-and-scope)
9. [Phase 8 — Operators](#9-phase-8--operators)
10. [Phase 9 — Conditionals](#10-phase-9--conditionals)
11. [Phase 10 — Tasks (Functions)](#11-phase-10--tasks-functions)
12. [Phase 11 — Pipelines](#12-phase-11--pipelines)
13. [Phase 12 — Interactive REPL](#13-phase-12--interactive-repl)
14. [Phase 13 — CLI and File Execution](#14-phase-13--cli-and-file-execution)
15. [Phase 14 — Error Handling](#15-phase-14--error-handling)
16. [Phase 15 — Tests](#16-phase-15--tests)
17. [Final Project Structure](#17-final-project-structure)
18. [Acceptance Criteria](#18-acceptance-criteria)

---

## 1. General Architecture

The engine follows the classic interpreter pipeline, adapted for MarretaLang's specifics:

```
Source Code (.marreta)
        │
        ▼
   ┌─────────┐
   │  LEXER  │  Transforms text into tokens
   └────┬────┘
        │ Vec<Token>
        ▼
   ┌─────────┐
   │ PARSER  │  Transforms tokens into syntax tree
   └────┬────┘
        │ AST (Vec<Statement>)
        ▼
   ┌─────────────┐
   │ INTERPRETER │  Walks the AST and executes instructions
   └─────────────┘
        │
        ▼
    Result / Side Effects
```

### Rust Modules

```
src/
├── main.rs          # Entry point, CLI and REPL
├── lexer.rs         # Tokenizer
├── token.rs         # Token type definitions
├── parser.rs        # Syntactic analysis
├── ast.rs           # AST node definitions
├── interpreter.rs   # Execution engine
├── environment.rs   # Scope and variable management
├── value.rs         # Runtime type system (Value enum)
└── error.rs         # Language error types
```

---

## 2. Phase 1 — Rust Project Scaffold

### 2.1 Create the project

```bash
cargo init marreta --name marreta
```

### 2.2 Initial dependencies (`Cargo.toml`)

```toml
[package]
name = "marreta"
version = "0.1.0"
edition = "2021"
description = "MarretaLang - A DSL for REST APIs"
license = "MIT"

[dependencies]
# No external dependencies in Core v0.1
# The Lexer, Parser, and Interpreter are 100% hand-written

[dev-dependencies]
pretty_assertions = "1"  # For tests with visual diff
```

**Architectural decision:** The core v0.1 does not use parsing libraries (such as `nom`, `pest`, or `chumsky`). The lexer and parser are hand-rolled for full control over error messages and language behavior. This is the industry standard for languages that need high-quality error messages (Rust, Go, and V did the same).

### 2.3 Directory structure

```bash
marreta/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── lexer.rs
│   ├── token.rs
│   ├── parser.rs
│   ├── ast.rs
│   ├── interpreter.rs
│   ├── environment.rs
│   ├── value.rs
│   └── error.rs
├── tests/
│   ├── lexer_tests.rs
│   ├── parser_tests.rs
│   ├── interpreter_tests.rs
│   └── integration_tests.rs
└── examples/
    ├── hello.marreta
    ├── variables.marreta
    ├── conditionals.marreta
    ├── tasks.marreta
    └── pipelines.marreta
```

---

## 3. Phase 2 — Lexer (Tokenizer)

The Lexer is the first stage of the pipeline. It reads the source code character by character and produces a sequence of **Tokens** — atomic units that the Parser will consume.

### 3.1 Token Definitions (`token.rs`)

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    Integer(i64),           // 42
    Float(f64),             // 3.14
    StringLiteral(String),  // "hello #{name}"
    True,                   // true
    False,                  // false
    Null,                   // null

    // Identifiers
    Identifier(String),     // name, user, payload

    // Arithmetic Operators
    Plus,          // +
    Minus,         // -
    Star,          // *
    Slash,         // /
    Percent,       // %

    // Comparison Operators
    Equal,         // ==
    NotEqual,      // !=
    Greater,       // >
    Less,          // <
    GreaterEqual,  // >=
    LessEqual,     // <=

    // Logical Operators (words)
    And,           // and
    Or,            // or
    Not,           // not

    // Assignment Operators
    Assign,        // =

    // Pipeline Operators
    Pipeline,      // >>
    Broadcast,     // *>>

    // Arrows
    Arrow,         // ->
    FatArrow,      // =>

    // Delimiters
    LeftParen,     // (
    RightParen,    // )
    LeftBracket,   // [
    RightBracket,  // ]
    LeftBrace,     // {
    RightBrace,    // }
    Comma,         // ,
    Dot,           // .
    Colon,         // :
    Hash,          // #

    // Reserved Words — Core
    Task,          // task
    Match,         // match
    Fallback,      // fallback
    Map,           // map
    Keep,          // keep
    Require,       // require
    Reject,        // reject
    If,            // if
    Else,          // else

    // Reserved Words — HTTP (reserved but not implemented in v0.1)
    Route,         // route
    Take,          // take
    Reply,         // reply
    Fail,          // fail
    Listen,        // listen

    // HTTP Verbs (reserved but not implemented in v0.1)
    Get,           // GET
    Post,          // POST
    Put,           // PUT
    Patch,         // PATCH
    Delete,        // DELETE

    // Reserved Words — Infra (reserved but not implemented in v0.1)
    Db,            // db
    Queue,         // queue
    Cache,         // cache

    // Control
    Newline,       // \n (significant — defines end of statement)
    Indent,        // Indentation increase
    Dedent,        // Indentation decrease
    Eof,           // End of file
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
    pub lexeme: String,     // The original text that generated this token
}
```

### 3.2 Lexer Logic (`lexer.rs`)

The Lexer needs to handle the following MarretaLang-specific challenges:

#### 3.2.1 Significant indentation

Since MarretaLang uses indentation to define scope (no `{}`), the Lexer needs to track indentation levels and emit `Indent` and `Dedent` tokens.

**Algorithm:**
1. Maintain an **indentation level stack**, starting with `[0]`.
2. At the beginning of each line, count the indentation spaces.
3. If the level **increased**: push the new level and emit an `Indent` token.
4. If the level **decreased**: pop until the matching level is found, emitting a `Dedent` for each removed level.
5. If the level **didn't change**: emit nothing (continues in the same scope).

**Rule:** Only spaces are accepted for indentation (not tabs). 1 level = 4 spaces.

#### 3.2.2 String interpolation

Strings containing `#{}` need to be handled by the Lexer as a sequence of concatenation tokens:

```marreta
"Hello #{name}, you are #{age} years old"
```

The Lexer should emit this as a sequence that the Parser will assemble as string concatenation.

**Simplified strategy for v0.1:** The Lexer emits the entire `StringLiteral` with `#{}` markers preserved. The Interpreter resolves interpolation at runtime.

#### 3.2.3 Significant vs. ignorable newlines

Not every line break is an end of statement. Lines ending with `>>`, `*>>`, `->`, `=>`, or `,` indicate continuation on the next line. The Lexer should **suppress the Newline** in these cases.

**Rule:** If the last non-whitespace token before `\n` is a continuation operator, do not emit `Newline`.

#### 3.2.4 Comments

Lines starting with `#` (optionally preceded by spaces) are ignored by the Lexer. Inline comments (`code # comment`) are also supported — the Lexer ignores everything after `#` until the end of the line.

**Warning:** The `#` inside strings (`"#{var}"`) is NOT a comment. The Lexer needs to be context-aware (inside/outside string).

### 3.3 Reserved Words Table

The Lexer needs a map to distinguish identifiers from reserved words:

```rust
fn keyword_lookup(word: &str) -> Option<TokenKind> {
    match word {
        "task"     => Some(TokenKind::Task),
        "match"    => Some(TokenKind::Match),
        "fallback" => Some(TokenKind::Fallback),
        "map"      => Some(TokenKind::Map),
        "keep"     => Some(TokenKind::Keep),
        "require"  => Some(TokenKind::Require),
        "reject"   => Some(TokenKind::Reject),
        "if"       => Some(TokenKind::If),
        "else"     => Some(TokenKind::Else),
        "and"      => Some(TokenKind::And),
        "or"       => Some(TokenKind::Or),
        "not"      => Some(TokenKind::Not),
        "true"     => Some(TokenKind::True),
        "false"    => Some(TokenKind::False),
        "null"     => Some(TokenKind::Null),
        "route"    => Some(TokenKind::Route),
        "take"     => Some(TokenKind::Take),
        "reply"    => Some(TokenKind::Reply),
        "fail"     => Some(TokenKind::Fail),
        "listen"   => Some(TokenKind::Listen),
        "db"       => Some(TokenKind::Db),
        "queue"    => Some(TokenKind::Queue),
        "cache"    => Some(TokenKind::Cache),
        "GET"      => Some(TokenKind::Get),
        "POST"     => Some(TokenKind::Post),
        "PUT"      => Some(TokenKind::Put),
        "PATCH"    => Some(TokenKind::Patch),
        "DELETE"   => Some(TokenKind::Delete),
        _          => None,
    }
}
```

### 3.4 Deliverables

- [ ] `token.rs` with all `TokenKind` defined
- [ ] `lexer.rs` with `Lexer::new(source: &str)` and `Lexer::tokenize() -> Result<Vec<Token>, MarretaError>`
- [ ] Significant indentation working (Indent/Dedent)
- [ ] Significant newlines with suppression on continuations
- [ ] String interpolation preserved for Interpreter resolution
- [ ] Comments ignored
- [ ] Unit tests for each token type

---

## 4. Phase 3 — Parser (Syntactic Analysis)

The Parser consumes the sequence of Tokens and produces an **AST (Abstract Syntax Tree)**. It is the heart of the language grammar — it defines what is valid code and what is not.

### 4.1 Technique: Pratt Parser (Operator Precedence)

For mathematical and logical expressions, we use a **Pratt Parser** (Top-Down Operator Precedence). This technique correctly resolves operator precedence and associativity without unnecessary recursion.

**Precedence table (lowest to highest):**

| Level | Operators | Associativity |
|---|---|---|
| 1 | `or` | Left |
| 2 | `and` | Left |
| 3 | `not` | Prefix (unary) |
| 4 | `==`, `!=` | Left |
| 5 | `>`, `<`, `>=`, `<=` | Left |
| 6 | `+`, `-` | Left |
| 7 | `*`, `/`, `%` | Left |
| 8 | `-` (unary negation) | Prefix |
| 9 | `.` (property access) | Left |
| 10 | `(` (function call) | Postfix |

### 4.2 MarretaLang Grammar (Pseudo-formal)

```
program        → statement* EOF

statement      → assignment
               | require_stmt
               | reject_stmt
               | match_stmt
               | task_def
               | pipeline_stmt
               | expression_stmt

assignment     → IDENTIFIER "=" expression (("if" expression)?)
               | IDENTIFIER "=" expression

require_stmt   → "require" expression "else" "fail" INTEGER "," STRING

reject_stmt    → "reject" expression "else" "fail" INTEGER "," STRING

match_stmt     → IDENTIFIER "=" "match" expression NEWLINE INDENT
                   (match_arm NEWLINE)*
                 DEDENT

match_arm      → expression "->" expression
               | "fallback" "->" expression

task_def       → "task" IDENTIFIER "(" params ")" "=>" expression
               | "task" IDENTIFIER "(" params ")" NEWLINE INDENT
                   statement*
                 DEDENT

params         → IDENTIFIER ("," IDENTIFIER)*
               | ε

pipeline_stmt  → expression ">>" pipeline_target
               | expression "*>>" NEWLINE INDENT
                   ("->" pipeline_target NEWLINE)*
                 DEDENT

pipeline_target → expression
                | "map" IDENTIFIER NEWLINE INDENT
                    statement*
                    "keep" expression NEWLINE
                  DEDENT
                | pipeline_target ">>" pipeline_target

expression     → literal
               | IDENTIFIER
               | expression operator expression
               | expression "." IDENTIFIER
               | expression "." IDENTIFIER "(" arguments ")"
               | IDENTIFIER "(" arguments ")"
               | "not" expression
               | "-" expression
               | "(" expression ")"
               | "[" (expression ("," expression)*)? "]"
               | "{" (IDENTIFIER ":" expression ("," IDENTIFIER ":" expression)*)? "}"

literal        → INTEGER | FLOAT | STRING | TRUE | FALSE | NULL

arguments      → expression ("," expression)*
               | named_args

named_args     → IDENTIFIER ":" expression ("," IDENTIFIER ":" expression)*
```

### 4.3 Indentation Parsing

The Parser consumes `Indent` and `Dedent` tokens to delimit blocks:

```rust
fn parse_block(&mut self) -> Result<Vec<Statement>, MarretaError> {
    self.expect(TokenKind::Indent)?;
    let mut statements = Vec::new();
    while !self.check(TokenKind::Dedent) && !self.check(TokenKind::Eof) {
        statements.push(self.parse_statement()?);
        self.skip_newlines();
    }
    self.expect(TokenKind::Dedent)?;
    Ok(statements)
}
```

### 4.4 Parsing the `if` Suffix

The conditional suffix (`status = "ok" if active`) is treated as a wrapper on the assignment:

```
When parsing an assignment:
1. Parse IDENTIFIER "=" expression
2. If the next token is `if`:
   - Consume `if`
   - Parse expression (the condition)
   - Return ConditionalAssignment { target, value, condition }
3. Otherwise:
   - Return Assignment { target, value }
```

### 4.5 Deliverables

- [ ] `parser.rs` with `Parser::new(tokens: Vec<Token>)` and `Parser::parse() -> Result<Vec<Statement>, MarretaError>`
- [ ] Pratt Parser for expressions with correct precedence
- [ ] Block parsing by indentation
- [ ] Parsing of `require`, `reject`, `match`, `task`, `map/keep`, `>>`, `*>>`
- [ ] Parsing of `if` suffix
- [ ] Parsing of function calls with named arguments
- [ ] Error messages with line and column
- [ ] Unit tests for each syntactic construct

---

## 5. Phase 4 — AST (Abstract Syntax Tree)

The AST is the in-memory representation of the parsed program. Each node is a strongly-typed Rust `enum`.

### 5.1 Node Definitions (`ast.rs`)

```rust
/// A program is a list of statements
pub type Program = Vec<Statement>;

/// Statements (instructions that don't directly produce a value)
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// name = "value"
    Assignment {
        target: String,
        value: Expression,
    },

    /// name = "value" if condition
    ConditionalAssignment {
        target: String,
        value: Expression,
        condition: Expression,
    },

    /// require EXPR else fail CODE, MSG
    Require {
        condition: Expression,
        error_code: i64,
        error_message: String,
    },

    /// reject EXPR else fail CODE, MSG
    Reject {
        condition: Expression,
        error_code: i64,
        error_message: String,
    },

    /// task name(params) => expr
    TaskDef {
        name: String,
        params: Vec<String>,
        body: TaskBody,
    },

    /// An expression used as a statement (e.g., function call)
    ExpressionStatement {
        expression: Expression,
    },
}

/// Task body
#[derive(Debug, Clone, PartialEq)]
pub enum TaskBody {
    /// task apply_discount(value) => value * 0.90
    Inline(Expression),

    /// task calculate(item)
    ///     base = item.price * 1.15
    ///     base - discount
    Block(Vec<Statement>, Expression),  // statements + final expression (implicit return)
}

/// Expressions (everything that produces a value)
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    // Literals
    Integer(i64),
    Float(f64),
    StringLiteral(String),     // may contain #{} for interpolation
    Boolean(bool),
    Null,
    List(Vec<Expression>),
    MapLiteral(Vec<(String, Expression)>),

    // Identifier
    Identifier(String),

    // Binary operations
    BinaryOp {
        left: Box<Expression>,
        operator: BinaryOperator,
        right: Box<Expression>,
    },

    // Unary operations
    UnaryOp {
        operator: UnaryOperator,
        operand: Box<Expression>,
    },

    // Property access: obj.field
    PropertyAccess {
        object: Box<Expression>,
        property: String,
    },

    // Method call: obj.method(args)
    MethodCall {
        object: Box<Expression>,
        method: String,
        arguments: Vec<Argument>,
    },

    // Function call: func(args)
    FunctionCall {
        name: String,
        arguments: Vec<Argument>,
    },

    // Task call in pipeline: task(task_name)
    TaskCall {
        name: String,
    },

    // Match expression
    Match {
        subject: Box<Expression>,
        arms: Vec<MatchArm>,
    },

    // Pipeline: expr >> expr
    Pipeline {
        input: Box<Expression>,
        stages: Vec<PipelineStage>,
    },

    // Broadcast: expr *>> [destinations]
    Broadcast {
        input: Box<Expression>,
        targets: Vec<Expression>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOperator {
    Add,            // +
    Subtract,       // -
    Multiply,       // *
    Divide,         // /
    Modulo,         // %
    Equal,          // ==
    NotEqual,       // !=
    Greater,        // >
    Less,           // <
    GreaterEqual,   // >=
    LessEqual,      // <=
    And,            // and
    Or,             // or
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOperator {
    Negate,     // -
    Not,        // not
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub value: Expression,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatchPattern {
    Literal(Expression),  // "VIP", 42, true
    Fallback,             // fallback
}

#[derive(Debug, Clone, PartialEq)]
pub enum PipelineStage {
    /// >> db.collection.save
    Expression(Expression),

    /// >> map item
    ///        ...
    ///        keep item
    Map {
        variable: String,
        body: Vec<Statement>,
        keep: Expression,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Argument {
    Positional(Expression),                     // func(value)
    Named { name: String, value: Expression },  // func(limit: 10)
}
```

### 5.2 Deliverables

- [ ] `ast.rs` with all nodes defined above
- [ ] `Display` implementation for AST pretty-printing (debug)
- [ ] Tests for manual construction of valid ASTs

---

## 6. Phase 5 — Interpreter (Execution Engine)

The Interpreter is a **tree-walking interpreter**: it recursively traverses the AST and executes each node.

### 6.1 Main Structure (`interpreter.rs`)

```rust
pub struct Interpreter {
    environment: Environment,   // Scope stack
}

impl Interpreter {
    pub fn new() -> Self { ... }

    pub fn execute(&mut self, program: Program) -> Result<Value, MarretaError> { ... }

    fn execute_statement(&mut self, stmt: &Statement) -> Result<(), MarretaError> { ... }

    fn evaluate_expression(&mut self, expr: &Expression) -> Result<Value, MarretaError> { ... }
}
```

### 6.2 Execution Cycle

For each Statement type in the AST:

| Statement | Interpreter Action |
|---|---|
| `Assignment` | Evaluates the right-hand expression. Binds the result to the name in the current scope. |
| `ConditionalAssignment` | Evaluates the condition. If truthy, evaluates the value and binds. If falsy, does nothing (variable stays `null` or keeps previous value). |
| `Require` | Evaluates the condition. If falsy, returns `MarretaError::HttpError { code, message }`. |
| `Reject` | Evaluates the condition. If truthy, returns `MarretaError::HttpError { code, message }`. |
| `TaskDef` | Registers the task in the scope as a `Value::Task { params, body }`. |
| `ExpressionStatement` | Evaluates the expression and discards the result (side effects only). |

For each Expression type in the AST:

| Expression | Interpreter Action |
|---|---|
| `Integer/Float/String/Boolean/Null` | Returns the corresponding `Value`. |
| `List` | Evaluates each element and returns `Value::List(vec)`. |
| `MapLiteral` | Evaluates each value and returns `Value::Map(hashmap)`. |
| `Identifier` | Looks up the name in the current scope (and parents). Error if not found. |
| `BinaryOp` | Evaluates left and right, applies the operator. |
| `UnaryOp` | Evaluates the operand, applies the operator. |
| `PropertyAccess` | Evaluates the object. If Map, looks up the key. If List, looks up built-in methods. |
| `MethodCall` | Evaluates the object and arguments. Dispatches to built-in or provider method. |
| `FunctionCall` | Looks up the task in scope. Creates new scope with parameters. Executes the body. |
| `Match` | Evaluates the subject. Iterates arms comparing patterns. Returns the value of the first match. |
| `Pipeline` | Evaluates input. For each stage, passes the result as input to the next. |
| `Broadcast` | Evaluates input. For each target, executes with a copy of the input. |

### 6.3 String Interpolation

When the Interpreter encounters a `StringLiteral` containing `#{}`:

1. Scan the string looking for `#{`.
2. Extract the variable name (or simple expression) between `{` and `}`.
3. Evaluate the expression in the current scope.
4. Replace `#{...}` with the result converted to string.
5. Return the final string.

### 6.4 Deliverables

- [ ] `interpreter.rs` with `execute`, `execute_statement`, `evaluate_expression`
- [ ] Support for all AST nodes defined in Phase 4
- [ ] String interpolation
- [ ] Unit tests for each node type

---

## 7. Phase 6 — Type System and Values

### 7.1 The `Value` Enum (`value.rs`)

```rust
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum Value {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Null,
    List(Vec<Value>),
    Map(Rc<RefCell<HashMap<String, Value>>>),  // Rc for sharing
    Task {
        name: String,
        params: Vec<String>,
        body: TaskBody,
    },
}
```

### 7.2 Coercion Rules

The Interpreter needs clear rules for operations between different types:

| Operation | Behavior |
|---|---|
| `Integer + Integer` | Returns `Integer` |
| `Integer + Float` | Promotes Integer to Float, returns `Float` |
| `String + any` | Converts the other to String, returns concatenation |
| `any == any` | Value comparison (deep equality for Maps/Lists) |
| `Boolean and/or Boolean` | Standard boolean logic |
| `non-Boolean` in boolean context | **Falsy:** `null`, `false`, `0`, `""`, `[]` (empty list). **Truthy:** everything else. |

### 7.3 Truthiness (Fundamental for `require`/`reject`)

The definition of "truthy/falsy" is critical for guard functionality:

```rust
impl Value {
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Boolean(b) => *b,
            Value::Integer(n) => *n != 0,
            Value::Float(n) => *n != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
            Value::Map(m) => !m.borrow().is_empty(),
            Value::Task { .. } => true,
        }
    }
}
```

### 7.4 Built-in Methods

Some methods are native to types and don't require tasks:

**String:**
- `.length()` → Integer
- `.upper()` → String
- `.lower()` → String
- `.trim()` → String
- `.contains(substr)` → Boolean
- `.split(separator)` → List
- `.replace(old, new)` → String

**List:**
- `.length()` → Integer
- `.first()` → Value
- `.last()` → Value
- `.empty?()` → Boolean (note: `?` in the name is valid in MarretaLang, Ruby-inspired)
- `.push(value)` → List (returns new list)
- `.includes(value)` → Boolean
- `.reverse()` → List

**Map:**
- `.keys()` → List
- `.values()` → List
- `.has(key)` → Boolean
- `.merge(other_map)` → Map

**Integer/Float:**
- `.abs()` → Integer/Float
- `.to_string()` → String

### 7.5 Deliverables

- [ ] `value.rs` with the complete `Value` enum
- [ ] `is_truthy()` implemented
- [ ] `Display` for `Value` (serialization for output/debug)
- [ ] Coercion rules implemented in the Interpreter
- [ ] All built-in methods listed above
- [ ] Tests for truthiness, coercion, and built-in methods

---

## 8. Phase 7 — Variables and Scope

### 8.1 Execution Environment (`environment.rs`)

The Interpreter needs a nested scope system. Each block (`task`, `map`, `match`) creates a new child scope.

```rust
use std::collections::HashMap;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug)]
pub struct Environment {
    scopes: Vec<HashMap<String, Value>>,  // Scope stack
}

impl Environment {
    /// Creates an environment with the global scope
    pub fn new() -> Self {
        Environment {
            scopes: vec![HashMap::new()],
        }
    }

    /// Pushes a new scope (when entering a task, map, etc.)
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Pops the current scope (when exiting a task, map, etc.)
    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Defines a variable in the current scope
    pub fn set(&mut self, name: String, value: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, value);
        }
    }

    /// Looks up a variable from the current scope to the global one (lexical scoping)
    pub fn get(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Some(value.clone());
            }
        }
        None
    }
}
```

### 8.2 Scope Rules

1. **Variables are mutable** — reassignment is allowed (`x = 1` followed by `x = 2` is valid).
2. **Shadowing is allowed** — a child scope can redefine a parent scope variable without altering it.
3. **Tasks are registered in the global scope** — regardless of where they are defined, tasks are globally accessible (simplification for v0.1).
4. **Undeclared variables cause errors** — accessing `x` without assigning first returns `MarretaError::UndefinedVariable`.

### 8.3 Deliverables

- [ ] `environment.rs` with scope push/pop, variable get/set
- [ ] Lexical scoping (lookup from innermost to outermost scope)
- [ ] Tests for nested scope and shadowing

---

## 9. Phase 8 — Operators

### 9.1 Arithmetic Operators

| Operation | Integer + Integer | Integer + Float | Float + Float | String + any |
|---|---|---|---|---|
| `+` | `Integer` | `Float` | `Float` | Concatenation (`String`) |
| `-` | `Integer` | `Float` | `Float` | Error |
| `*` | `Integer` | `Float` | `Float` | Error |
| `/` | `Integer` (integer division) | `Float` | `Float` | Error |
| `%` | `Integer` | `Float` | `Float` | Error |

**Division by zero:** Returns `MarretaError::DivisionByZero` with line and column.

### 9.2 Comparison Operators

All return `Boolean`:

- `==` and `!=`: Deep comparison (deep equality). Maps and Lists are compared recursively by value.
- `>`, `<`, `>=`, `<=`: Only between numeric values (Integer and Float). Comparing other types generates `MarretaError::TypeError`.

### 9.3 Logical Operators

- `and`: Returns the first falsy value, or the last if all are truthy (short-circuit).
- `or`: Returns the first truthy value, or the last if all are falsy (short-circuit).
- `not`: Returns inverted `Boolean` of the operand's truthiness.

**Note on `or` as fallback:**
```marreta
# This works as "default value" because of short-circuit
limit = query.limit or 10
```
If `query.limit` is `null` (falsy), `or` returns `10`. This pattern is essential for language ergonomics.

### 9.4 Deliverables

- [ ] All arithmetic operators with Integer/Float coercion
- [ ] Comparison operators with deep equality
- [ ] Logical operators with short-circuit
- [ ] `or` as value fallback
- [ ] Typed errors for invalid operations (e.g., `"abc" - 1`)
- [ ] Tests for each type combination

---

## 10. Phase 9 — Conditionals

### 10.1 `require`

```marreta
require payload.items else fail 400, "Cart is empty"
```

**Interpreter implementation:**

```rust
fn execute_require(&mut self, condition: &Expression, code: i64, msg: &str) -> Result<(), MarretaError> {
    let value = self.evaluate_expression(condition)?;
    if !value.is_truthy() {
        return Err(MarretaError::HttpError {
            status_code: code,
            message: msg.to_string(),
        });
    }
    Ok(())
}
```

### 10.2 `reject`

```marreta
reject client.delinquent else fail 402, "Payment pending"
```

**Implementation:** Identical to `require`, but with inverted logic (`if value.is_truthy()` → error).

### 10.3 `if` Suffix

```marreta
fee = 0.0 if client.vip
```

**Interpreter implementation:**

```rust
fn execute_conditional_assignment(&mut self, target: &str, value: &Expression, condition: &Expression) -> Result<(), MarretaError> {
    let cond_result = self.evaluate_expression(condition)?;
    if cond_result.is_truthy() {
        let val = self.evaluate_expression(value)?;
        self.environment.set(target.to_string(), val);
    }
    Ok(())
}
```

### 10.4 `match`

```marreta
fee = match client.type
    "VIP" -> 0.0
    "PREMIUM" -> 5.0
    fallback -> 15.0
```

**Interpreter implementation:**

```rust
fn evaluate_match(&mut self, subject: &Expression, arms: &[MatchArm]) -> Result<Value, MarretaError> {
    let subject_val = self.evaluate_expression(subject)?;
    for arm in arms {
        match &arm.pattern {
            MatchPattern::Literal(expr) => {
                let pattern_val = self.evaluate_expression(expr)?;
                if subject_val == pattern_val {
                    return self.evaluate_expression(&arm.value);
                }
            }
            MatchPattern::Fallback => {
                return self.evaluate_expression(&arm.value);
            }
        }
    }
    Ok(Value::Null)  // No arm matched and there's no fallback
}
```

### 10.5 Deliverables

- [ ] `require` interrupting execution with HTTP error
- [ ] `reject` with inverted logic
- [ ] `if` suffix on assignments
- [ ] `match` with literal pattern matching and `fallback`
- [ ] Tests for each conditional with truthy and falsy values

---

## 11. Phase 10 — Tasks (Functions)

### 11.1 Definition

Tasks are the reusable logic units in MarretaLang. They are defined with `task` and registered in the global scope.

#### Inline task:
```marreta
task apply_discount(value) => value * 0.90
```

#### Task with body:
```marreta
task calculate_tax(item)
    base = item.price * 1.15
    discount = base * 0.05 if item.promotion
    base - discount
```

### 11.2 Implementation

**Definition (in the Interpreter):**

```rust
fn execute_task_def(&mut self, name: &str, params: &[String], body: &TaskBody) -> Result<(), MarretaError> {
    let task_value = Value::Task {
        name: name.to_string(),
        params: params.to_vec(),
        body: body.clone(),
    };
    self.environment.set(name.to_string(), task_value);
    Ok(())
}
```

**Invocation (in the Interpreter):**

```rust
fn call_task(&mut self, name: &str, args: &[Value]) -> Result<Value, MarretaError> {
    let task = self.environment.get(name)
        .ok_or(MarretaError::UndefinedTask { name: name.to_string() })?;

    match task {
        Value::Task { params, body, .. } => {
            // Validate arity
            if args.len() != params.len() {
                return Err(MarretaError::WrongArity {
                    expected: params.len(),
                    got: args.len(),
                });
            }

            // Create new scope
            self.environment.push_scope();

            // Bind arguments to parameters
            for (param, arg) in params.iter().zip(args.iter()) {
                self.environment.set(param.clone(), arg.clone());
            }

            // Execute body
            let result = match &body {
                TaskBody::Inline(expr) => self.evaluate_expression(expr),
                TaskBody::Block(stmts, final_expr) => {
                    for stmt in stmts {
                        self.execute_statement(stmt)?;
                    }
                    self.evaluate_expression(final_expr)
                }
            };

            // Destroy scope
            self.environment.pop_scope();

            result
        }
        _ => Err(MarretaError::NotCallable { name: name.to_string() }),
    }
}
```

### 11.3 Tasks as pipeline arguments

```marreta
payload.items >> task(calculate_tax) >> db.orders.save
```

When the parser sees `task(name)` in a pipeline context, it creates a `TaskCall { name }` node. The Interpreter applies the task to each element of the input list.

### 11.4 Deliverables

- [ ] Inline and body task definitions
- [ ] Task invocation with positional arguments
- [ ] Isolated scope for each task call
- [ ] Implicit return (last expression)
- [ ] `TaskCall` in pipelines (automatic application to lists)
- [ ] Arity error (wrong number of arguments)
- [ ] Tests for tasks with different body types and arguments

---

## 12. Phase 11 — Pipelines

### 12.1 Simple Pipeline (`>>`)

```marreta
payload.items >> task(calculate_tax)
```

**Implementation:**

```rust
fn evaluate_pipeline(&mut self, input: &Expression, stages: &[PipelineStage]) -> Result<Value, MarretaError> {
    let mut current = self.evaluate_expression(input)?;

    for stage in stages {
        current = match stage {
            PipelineStage::Expression(expr) => {
                self.apply_pipeline_stage(&current, expr)?
            }
            PipelineStage::Map { variable, body, keep } => {
                self.evaluate_map_stage(&current, variable, body, keep)?
            }
        };
    }

    Ok(current)
}
```

### 12.2 Implicit Iteration

When the input of a pipeline is a `Value::List`, the engine applies the stage to **each element** automatically:

```rust
fn apply_pipeline_stage(&mut self, input: &Value, stage: &Expression) -> Result<Value, MarretaError> {
    match input {
        Value::List(items) => {
            let mut results = Vec::new();
            for item in items {
                let result = self.apply_single(item, stage)?;
                results.push(result);
            }
            Ok(Value::List(results))
        }
        _ => self.apply_single(input, stage),
    }
}
```

### 12.3 Map/Keep

```marreta
payload.items
    >> map item
        item.total = item.price * 1.15
        keep item
    >> db.orders.save
```

**Implementation:**

```rust
fn evaluate_map_stage(
    &mut self,
    input: &Value,
    variable: &str,
    body: &[Statement],
    keep: &Expression,
) -> Result<Value, MarretaError> {
    let items = match input {
        Value::List(items) => items,
        _ => return Err(MarretaError::TypeError {
            message: "map requires a list input".to_string(),
        }),
    };

    let mut results = Vec::new();
    for item in items {
        self.environment.push_scope();
        self.environment.set(variable.to_string(), item.clone());

        for stmt in body {
            self.execute_statement(stmt)?;
        }

        let kept = self.evaluate_expression(keep)?;
        results.push(kept);

        self.environment.pop_scope();
    }

    Ok(Value::List(results))
}
```

### 12.4 Broadcast (`*>>`)

```marreta
orders *>>
    -> queue.push("payments")
    -> queue.push("invoices")
    -> cache.set("latest")
```

**v0.1 implementation (sequential):**

In Core v0.1, without infra modules, broadcast executes each target sequentially. In v0.2+, with `tokio`, each target will be an async task.

```rust
fn evaluate_broadcast(&mut self, input: &Expression, targets: &[Expression]) -> Result<Value, MarretaError> {
    let value = self.evaluate_expression(input)?;

    for target in targets {
        self.apply_single(&value, target)?;
    }

    Ok(value)  // Broadcast returns the original input
}
```

### 12.5 Deliverables

- [ ] Simple pipeline (`>>`) with stage chaining
- [ ] Implicit iteration over lists
- [ ] `map`/`keep` with isolated scope
- [ ] Broadcast `*>>` (sequential in v0.1)
- [ ] Multi-stage chaining (`>> a >> b >> c`)
- [ ] Tests for pipelines with lists, maps, and tasks

---

## 13. Phase 12 — Interactive REPL

A REPL (Read-Eval-Print Loop) allows the developer to interactively test MarretaLang expressions and tasks.

### 13.1 Behavior

```
$ marreta
MarretaLang v0.1.0
>> x = 42
>> x * 2
84
>> name = "Marreta"
>> "Hello #{name}!"
Hello Marreta!
>> task double(n) => n * 2
>> double(21)
42
>> .exit
```

### 13.2 Implementation

```rust
fn run_repl() {
    println!("MarretaLang v0.1.0");
    let mut interpreter = Interpreter::new();
    let stdin = std::io::stdin();

    loop {
        print!(">> ");
        std::io::stdout().flush().unwrap();

        let mut line = String::new();
        stdin.read_line(&mut line).unwrap();
        let line = line.trim();

        if line == ".exit" || line == ".quit" {
            break;
        }

        match execute_line(&mut interpreter, line) {
            Ok(Some(value)) => println!("{}", value),
            Ok(None) => {}  // Statement with no value (e.g., assignment)
            Err(err) => eprintln!("Error: {}", err),
        }
    }
}
```

### 13.3 REPL Special Commands

| Command | Action |
|---|---|
| `.exit` / `.quit` | Exits the REPL |
| `.vars` | Lists all variables in the current scope |
| `.tasks` | Lists all defined tasks |
| `.clear` | Clears the environment (resets variables and tasks) |
| `.help` | Shows quick help |

### 13.4 Multi-line in the REPL

When the REPL detects that a line ends with a continuation operator (`>>`, `->`, etc.) or when an indentation block is opened (task, map), it enters multi-line mode, displaying `..` as a secondary prompt until the block is closed.

```
>> task square(n)
..     n * n
..
>> square(5)
25
```

### 13.5 Deliverables

- [ ] Basic REPL with read and execute loop
- [ ] Persistent state between lines (variables and tasks survive)
- [ ] Special commands (`.exit`, `.vars`, `.tasks`, `.clear`, `.help`)
- [ ] Multi-line mode for indented blocks
- [ ] Formatted output (values printed in a readable way)

---

## 14. Phase 13 — CLI and File Execution

### 14.1 Command Line Interface

```bash
# Execute a .marreta file
marreta run app.marreta

# Start the REPL
marreta repl

# Show version
marreta --version

# Show help
marreta --help

# Tokenize (debug)
marreta tokenize app.marreta

# Parse and show AST (debug)
marreta parse app.marreta
```

### 14.2 Implementation (`main.rs`)

```rust
fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("run") => {
            let path = args.get(2).expect("Usage: marreta run <file.marreta>");
            run_file(path);
        }
        Some("repl") | None => {
            run_repl();
        }
        Some("tokenize") => {
            let path = args.get(2).expect("Usage: marreta tokenize <file.marreta>");
            debug_tokenize(path);
        }
        Some("parse") => {
            let path = args.get(2).expect("Usage: marreta parse <file.marreta>");
            debug_parse(path);
        }
        Some("--version") => {
            println!("MarretaLang v0.1.0");
        }
        Some("--help") => {
            print_help();
        }
        Some(cmd) => {
            eprintln!("Unknown command: {}", cmd);
            std::process::exit(1);
        }
    }
}
```

### 14.3 Deliverables

- [ ] `main.rs` with command dispatch
- [ ] `marreta run <file>` reads, tokenizes, parses, and executes
- [ ] `marreta repl` starts the REPL
- [ ] `marreta tokenize <file>` prints tokens (debug)
- [ ] `marreta parse <file>` prints AST (debug)
- [ ] `--version` and `--help`

---

## 15. Phase 14 — Error Handling

### 15.1 Error Types (`error.rs`)

```rust
#[derive(Debug)]
pub enum MarretaError {
    // Lexer Errors
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

    // Parser Errors
    UnexpectedToken {
        expected: String,
        got: Token,
    },
    UnexpectedEndOfInput,

    // Interpreter Errors
    UndefinedVariable {
        name: String,
        line: usize,
    },
    UndefinedTask {
        name: String,
        line: usize,
    },
    TypeError {
        message: String,
        line: usize,
    },
    DivisionByZero {
        line: usize,
    },
    WrongArity {
        task_name: String,
        expected: usize,
        got: usize,
        line: usize,
    },
    NotCallable {
        name: String,
        line: usize,
    },
    PropertyNotFound {
        object_type: String,
        property: String,
        line: usize,
    },

    // HTTP Errors (used by require/reject, propagated to the HTTP runtime in v0.2)
    HttpError {
        status_code: i64,
        message: String,
    },

    // I/O Errors
    FileNotFound {
        path: String,
    },
}
```

### 15.2 Error Formatting

Errors should be displayed with context for the developer:

```
Error at checkout.marreta:15:5
  require payload.items else fail 400, "Cart is empty"
  ^^^^^^^
  Variable 'payload' is not defined in this scope.
```

**Implementation:** Each AST node carries `line` and `column` information (propagated from Tokens). The error system uses this information to point to the exact position in the source code.

### 15.3 Deliverables

- [ ] `error.rs` with all error types
- [ ] `Display` implemented for each variant with friendly formatting
- [ ] Position (line/column) in all error messages
- [ ] Tests for each error type

---

## 16. Phase 15 — Tests

### 16.1 Testing Strategy

| Layer | Type | What it tests |
|---|---|---|
| Lexer | Unit | Each token type is generated correctly. Indentation. Comments. |
| Parser | Unit | Each syntactic construct generates the correct AST node. Syntax errors. |
| Interpreter | Unit | Each AST node is executed correctly. Runtime errors. |
| Value | Unit | Truthiness. Coercion. Built-in methods. |
| Environment | Unit | Nested scope. Shadowing. Undefined variable. |
| Integration | End-to-end | Complete `.marreta` files are executed and output is verified. |

### 16.2 Integration Test Examples

Each file in `examples/` serves as an integration test:

**`examples/variables.marreta`:**
```marreta
name = "Marreta"
version = 0.1
active = true
message = "#{name} v#{version} active: #{active}"
```

**Expected result:** No output (only assignments), but the REPL would show the values.

**`examples/conditionals.marreta`:**
```marreta
balance = 150
status = "approved" if balance > 100

fee = match "VIP"
    "VIP" -> 0.0
    "PREMIUM" -> 5.0
    fallback -> 15.0
```

**`examples/tasks.marreta`:**
```marreta
task double(n) => n * 2

task rectangle_area(width, height)
    width * height

result = double(21)
area = rectangle_area(5, 10)
```

**`examples/pipelines.marreta`:**
```marreta
task double(n) => n * 2
task plus_one(n) => n + 1

numbers = [1, 2, 3, 4, 5]

# Simple pipeline with task
doubled = numbers >> task(double)

# Pipeline with map/keep
processed = numbers
    >> map n
        n_doubled = n * 2
        n_final = n_doubled + 10
        keep n_final

# Multi-stage pipeline
result = numbers >> task(double) >> task(plus_one)
```

### 16.3 Deliverables

- [ ] Unit tests for each module (minimum 80% coverage)
- [ ] Integration tests for each file in `examples/`
- [ ] CI configured to run `cargo test` on each push
- [ ] Error tests (verifies that correct errors are emitted for invalid code)

---

## 17. Final Project Structure

```
marreta/
├── Cargo.toml
├── Cargo.lock
├── SPEC.md
├── IMPLEMENTATION_PLAN.md
├── src/
│   ├── main.rs              # CLI entry point + REPL
│   ├── lib.rs               # Public re-exports
│   ├── lexer.rs             # Tokenizer (hand-rolled)
│   ├── token.rs             # TokenKind and Token definitions
│   ├── parser.rs            # Pratt Parser + statement parser
│   ├── ast.rs               # AST nodes (enums)
│   ├── interpreter.rs       # Tree-walking interpreter
│   ├── environment.rs       # Scope and variables
│   ├── value.rs             # Runtime type system
│   └── error.rs             # MarretaError and formatting
├── tests/
│   ├── lexer_tests.rs       # Lexer unit tests
│   ├── parser_tests.rs      # Parser unit tests
│   ├── interpreter_tests.rs # Interpreter unit tests
│   ├── value_tests.rs       # Type and coercion tests
│   └── integration_tests.rs # End-to-end tests with .marreta files
└── examples/
    ├── hello.marreta
    ├── variables.marreta
    ├── conditionals.marreta
    ├── tasks.marreta
    └── pipelines.marreta
```

---

## 18. Acceptance Criteria

The Core v0.1 is **complete** when all items below are met:

### Functional

- [x] The Lexer correctly tokenizes all examples in `examples/`
- [x] The Parser generates valid ASTs for all examples
- [x] The Interpreter executes all examples and produces correct results
- [x] Variables with type inference work (Integer, Float, String, Boolean, Null, List, Map)
- [x] All arithmetic, comparison, and logical operators work with coercion
- [x] `require` and `reject` interrupt execution with correct HTTP error
- [x] `if` suffix works on assignments
- [x] `match` with literal patterns and `fallback` works
- [x] Inline and body tasks work with parameters and implicit return
- [x] Pipeline `>>` works with implicit iteration over lists
- [x] `map`/`keep` works with isolated scope
- [x] Broadcast `*>>` works (sequential)
- [x] String interpolation `#{}` works
- [x] Interactive REPL works with persistent state
- [x] `marreta run <file>` executes `.marreta` files
- [x] Error messages indicate line and column

### Quality

- [x] `cargo test` passes with 0 failures — 353 tests (288 unit + 65 integration)
- [x] `cargo clippy` with no warnings
- [x] `cargo fmt` applied to all code
- [x] Test coverage >= 80% — all modules covered
- [x] No `unwrap()` in production code (only in tests)
- [x] All errors are typed via `MarretaError` (no `panic!`)

### Documentation

- [x] `SPEC.md` updated with any divergences during implementation
- [x] Comments on public Rust functions (doc comments `///`)
- [x] `examples/` with at least 5 scripts demonstrating each Core feature

---

## Recommended Implementation Order

```
1.  Scaffold (Cargo.toml, directory structure)
2.  token.rs (define all TokenKind)
3.  error.rs (define MarretaError)
4.  ast.rs (define all AST nodes)
5.  value.rs (define Value + is_truthy + Display)
6.  environment.rs (scope and variables)
7.  lexer.rs (tokenize: literals → operators → keywords → indentation)
8.  parser.rs (parse: expressions → statements → blocks)
9.  interpreter.rs (execute: literals → operators → variables → conditionals → tasks → pipelines)
10. main.rs (CLI + REPL)
11. Unit tests per module
12. Integration tests with examples/
13. Polish (clippy, fmt, error messages)
```

Each phase should result in a functional and testable commit. The development agent should follow this order to ensure each lower layer is solid before building the next one.
