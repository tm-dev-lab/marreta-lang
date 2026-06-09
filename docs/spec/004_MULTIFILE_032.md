# MarretaLang — Multi-file Support (v0.3.2)

> Status: Delivered.

> **Meta:** Allow MarretaLang projects to be organized across multiple `.marreta` files, eliminating the monolith of a single file. The `export` keyword controls what is visible across files; anything not exported remains private to the file in which it was declared. `app.marreta` is the implicit entrypoint and everything declared in it is already global — without the need for `export`.

---

## 1. Syntax Design

### 1.1 The `export` keyword

`export` is a visibility modifier that can precede any top-level declaration: variables, tasks, and schemas. Without `export`, the symbol is **file-private** — invisible to other files in the project.

**Exporting a variable:**
```marreta
# db/config.marreta
export set db_host = "localhost"
export set db_port = 5432
set internal_pool_size = 10   # private — only visible within db/config.marreta
```

**Exporting a schema:**
```marreta
# models/user.marreta
export schema user_payload
    name: string
    age: integer
    email?: string
    is_active: boolean

export schema user_response
    id: integer
    name: string
```

**Exporting a task:**
```marreta
# tasks/auth.marreta
export task validate_token token
    if token == ""
        reply 401, { error: "missing token" }
    end
    set claims = jwt.decode(token)
    return claims
end

task hash_password raw   # private — internal utility of this file
    return crypto.bcrypt(raw)
end
```

### 1.2 `app.marreta` as entrypoint

`app.marreta` is the root file of the project. Everything declared in it is **implicitly global** — `export` is unnecessary. It is the place where global configuration variables, main routes, and the "glue" between modules live.

```marreta
# app.marreta — entrypoint, everything here is global automatically

set app_name = "My API"
set version  = "0.3.2"

route GET "/health"
    reply 200, { status: "ok", app: app_name, version: version }
end

route POST "/users" take payload as user_payload
    call validate_token(request.headers.authorization)
    db.users.save(payload)
    reply 201, { ok: true }
end
```

---

## 2. Example Project Structure

A real project with authentication, models, and separate routes:

```
my-api/
├── app.marreta              # entrypoint — main routes, global vars
├── models/
│   ├── user.marreta         # export schema user_payload, user_response
│   └── product.marreta      # export schema product_payload
├── tasks/
│   ├── auth.marreta         # export task validate_token
│   └── mailer.marreta       # export task send_welcome_email
└── db/
    └── config.marreta       # export set db_host, db_port
```

The boot command remains simple:

```bash
marreta serve app.marreta
```

The engine automatically scans the entire directory tree from the directory where `app.marreta` is located, collecting all `.marreta` files found. The developer does not need to declare imports — `export` is the only visibility control primitive required.

---

## 3. Implementation Under The Hood (Rust Engine)

### Phase 1: Lexer & AST

**`token.rs`**: Add the `Export` token mapped to the `export` keyword.

```rust
// token.rs
Export,   // new variant in TokenKind
```

The lexer recognizes `export` as a reserved keyword in the same way it does `route`, `task`, `schema`, and `set`.

**`ast.rs`**: New variant `Statement::Export` that **encapsulates** any other exportable statement. The wrapper approach keeps the AST clean — the internal parser of the exported statement does not need to know that it was prefixed by `export`.

```rust
// ast.rs
pub enum Statement {
    // ... existing variants ...
    Export(Box<Statement>),   // wraps Set | Task | Schema
}
```

The parser, upon encountering the `Export` token, consumes it and calls the regular parser for the next statement, wrapping the result in `Statement::Export(Box::new(inner_stmt))`.

> **Invariant:** Only `Statement::Set`, `Statement::Task`, and `Statement::Schema` are valid inside an `Export`. The parser emits an error at parse time if another statement is preceded by `export` (e.g., `export route` — invalid; routes are always global by nature).

---

### Phase 2: Multi-file Loader

Create (or expand) the `file_loader.rs` module.

**Step 1 — File Scanning:**

At the initialization of `marreta serve app.marreta`, the loader:

1. Determines the root directory from the provided `app.marreta` path.
2. Traverses the directory tree recursively (`walkdir` or manual implementation with `std::fs::read_dir`).
3. Collects all `.marreta` files found, excluding `app.marreta` itself from the list (it is handled separately as the entrypoint).

```rust
// file_loader.rs — pseudo-code
pub fn collect_marreta_files(root_dir: &Path) -> Vec<PathBuf> {
    // Recursively traverse, filter *.marreta, exclude app.marreta
}
```

**Step 2 — Two-Pass Loading:**

The loading order follows a mandatory two-pass protocol to avoid circular dependencies and references to symbols not yet loaded:

**Pass 1 — Export Collection (Global Scope Population):**

- All `.marreta` files (except `app.marreta`) are parsed.
- The loader scans the AST of each file for `Statement::Export(inner)`.
- Exported symbols (`Set`, `Task`, `Schema`) are inserted into the **shared global scope** (`GlobalScope`).
- No route is executed. No task code is invoked. Only definition collection.

**Pass 2 — Route Execution:**

- `app.marreta` is parsed and executed completely.
  - All its declarations are inserted into the global scope (implicit behavior of the entrypoint).
  - Its routes are registered in the `RouteRegistry`.
- The other `.marreta` files have their `Statement::Route` registered in the `RouteRegistry`.
  - Routes in non-entrypoint files can reference any symbol from the global scope (populated in Pass 1).

```
[Pass 1]  models/user.marreta    → export schema user_payload   → GlobalScope
          tasks/auth.marreta     → export task validate_token   → GlobalScope
          db/config.marreta      → export set db_host           → GlobalScope

[Pass 2]  app.marreta            → set app_name, routes         → GlobalScope + RouteRegistry
          (other files)          → routes                       → RouteRegistry
```

---

### Phase 3: Scope Isolation

The scope model now has two well-defined layers:

**`GlobalScope`** (`Arc<RwLock<HashMap<String, Value>>>`):
- Contains all symbols exported via `export` from any file in the project.
- Contains everything declared in `app.marreta` (without the need for `export`).
- Shared across all route executions at runtime.
- Safe concurrent access via `RwLock` — multiple simultaneous readers, exclusive writes.

**`FileScope`** (local per file, only during Phase 1):
- Contains symbols declared without `export` in a non-entrypoint file.
- Discarded after the file is loaded. Not accessible at runtime by other routes.
- Ensures that internal names of different files do not collide with each other.

**Runtime name resolution rule:**

When a route executes and references an identifier, the interpreter looks up the order:
1. Local scope of the route execution (variables declared with `set` within the route itself).
2. `GlobalScope`.
3. If not found: `MarretaError::NameNotFound` with a clear message indicating the identifier.

> **Guaranteed collision-free:** If two different files declare `set helper = ...` without `export`, each one lives in its short-lived `FileScope` and never reaches `GlobalScope`. A collision is only possible between two `export set` of the same name — in this case, the loader emits a startup-time error: `"export conflict: 'helper' already exported by tasks/auth.marreta, redeclared in tasks/mailer.marreta"`.

---

## 4. Acceptance Criteria

1. **(Lexer & Parser)** The `Export` token is recognized and `Statement::Export(Box<Statement>)` is correctly built for exported `set`, `task`, and `schema`. Attempting `export route` results in a descriptive parse error.

2. **(Scanning)** `marreta serve app.marreta` finds and processes all `.marreta` inside the directory automatically, without any manual imports in the code.

3. **(Pass 1 — Exports)** Symbols marked with `export` in auxiliary files become globally available. A route in `app.marreta` can call `validate_token` defined in `tasks/auth.marreta` without any import declaration.

4. **(Pass 2 — Entrypoint)** Everything in `app.marreta` is global without `export`. Variables, tasks, and schemas declared in the entrypoint are accessible by routes from any other file.

5. **(Isolation)** A variable declared without `export` in `db/config.marreta` is **not** accessible from a route in `app.marreta`. The interpreter emits `MarretaError::UndefinedVariable` instead of an incorrect value from another file.

6. **(Export Conflict)** If two files export a symbol with the same name, the startup fails with an error message pointing out the two conflicting files — the server does not boot in an ambiguous state.

7. **(Multi-file Routes)** Routes defined outside `app.marreta` (in auxiliary files) are registered in the `RouteRegistry` and respond normally to Axum, as if they were in the entrypoint.

8. **(E2E Example — Simple E-commerce)** A working example project is created under `examples/ecommerce/` demonstrating a real multi-file application:

   ```
   examples/ecommerce/
   ├── app.marreta              # entrypoint: project_name, project_version, health route
   ├── schemas/
   │   └── payloads.marreta     # export schema product_payload, order_payload
   ├── tasks/
   │   └── pricing.marreta      # export task apply_discount, calculate_total
   └── routes/
       ├── products.marreta     # GET /products, POST /products
       └── orders.marreta       # POST /orders
   ```

   The example must be functional with `marreta serve examples/ecommerce/app.marreta` and serve:
   - At least 2 route files, 1 task file, 1 schema file
   - Schema validation in at least one route
   - A task called from a route
   - Swagger UI at `/docs` with routes grouped by file name (Products, Orders)

   **Purpose:** Acts as a living showcase of everything built through v0.3.2 — multi-file loading, schema validation, task reuse, OpenAPI generation — using the language as a real developer would.

---

## 5. Status

| # | Criterion | Status |
|---|---|---|
| 1 | `export` token, `Statement::Export`, `export route` → parse error | COMPLETE |
| 2 | Auto-scan of `.marreta` files from entrypoint directory | COMPLETE |
| 3 | Exported symbols globally available across files | COMPLETE |
| 4 | Entrypoint (`app.marreta`) implicitly global | COMPLETE |
| 5 | File-private symbols not accessible from other files | COMPLETE |
| 6 | Export name conflict → startup error with file names | COMPLETE |
| 7 | Routes from auxiliary files registered and served by Axum | COMPLETE |
| 8 | E-commerce example functional under `examples/ecommerce/` | COMPLETE |
