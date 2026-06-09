# 045 — Editor Tooling and LSP

> Status: Delivered
> Type: Developer tooling / editor integration
> Scope: Provide official editor intelligence through CLI-backed tooling and VS Code/LSP integration

---

## 1. Purpose

Marreta is a new language. Developers will not have broad internet material,
Stack Overflow history, or AI models trained on large Marreta codebases.

That changes the bar for tooling. The editor must become part of the learning
surface:

- autocomplete should reveal namespaces and operations;
- hover should explain language constructs and built-ins;
- diagnostics should point to actionable fixes;
- formatting should be one command away;
- project symbols should be discoverable without reading all files.

This spec introduces a CLI-backed tooling contract and an official VS Code/LSP
integration that consumes it.

The key design decision: language intelligence lives in the Marreta CLI/core,
not duplicated inside a TypeScript extension.

First cut scope:

- ship editor intelligence through CLI commands;
- ship a VS Code extension that invokes those commands;
- do not implement a long-running `marreta lsp` server yet.

The extension may use VS Code's language-feature APIs directly. A future
`marreta lsp` must reuse the same Rust catalog/parser/linter modules instead of
creating a second implementation.

---

## 2. Design Principles

1. **CLI/core is the source of truth.**
   Parser, symbol resolution, built-in catalogs, diagnostics, and docs live in
   Rust. Editor clients consume JSON.

2. **VS Code is a thin client.**
   The extension adapts Marreta tooling responses into editor APIs. It should
   not implement a second parser or hardcode language semantics.

3. **Unsaved buffers must work.**
   Editor features must support stdin overlays so autocomplete/lint/hover work
   before the file is saved.

4. **Tooling teaches the language.**
   Hover documentation is not a nice-to-have. For a new language, it replaces
   the missing ecosystem memory developers usually rely on.

5. **Start static, grow contextual.**
   The first cut should provide high-confidence completions and hovers from an
   official catalog. Deep type inference can come later.

6. **Project-root behavior matches other Marreta commands.**
   Editor tooling runs with the nearest directory containing `app.marreta` as
   the project root. Files outside a Marreta project may still use explicit
   file/stdin commands in degraded single-file mode, but project symbols are not
   available there.

7. **No editor-only semantics.**
   If VS Code shows a completion, hover, diagnostic, or symbol, that information
   must be produced by Marreta core or by static metadata generated from Marreta
   core.

---

## 3. CLI Tooling Surface

All editor-facing commands live under:

```bash
marreta tooling ...
```

All machine-readable commands support:

```bash
--format json
--stdin --file <path>
```

Positions are 1-based for both `line` and `column`, matching Marreta diagnostics.
Editor clients with 0-based APIs must convert before invoking the CLI.

When invoked by VS Code, the extension should:

1. find the nearest ancestor directory containing `app.marreta`;
2. run the command with that directory as `cwd`;
3. pass the edited buffer through `--stdin --file <project-relative-path>`;
4. fall back to single-file mode only when no project root exists.

### 3.1 Completions

```bash
marreta tooling completions \
  --file routes/greetings.marreta \
  --line 12 \
  --column 10 \
  --format json
```

Editor mode:

```bash
marreta tooling completions \
  --stdin \
  --file routes/greetings.marreta \
  --line 12 \
  --column 10 \
  --format json
```

Response shape:

```json
[
  {
    "label": "cache.get",
    "kind": "function",
    "detail": "cache.get(key)",
    "documentation": "Returns the cached value for key, or null when missing.",
    "insert_text": "get(${1:key})",
    "source": "builtin",
    "sort_text": "020_cache_get"
  }
]
```

Completion item fields:

| Field | Required | Meaning |
|---|---|---|
| `label` | yes | Text shown in the completion list |
| `kind` | yes | `keyword`, `namespace`, `function`, `method`, `task`, `schema`, `route` |
| `detail` | yes | Short signature or declaration summary |
| `documentation` | yes | Short markdown-safe explanation |
| `insert_text` | yes | Snippet-capable insertion text |
| `source` | yes | `builtin`, `project`, or `keyword` |
| `sort_text` | no | Stable ordering key for editor presentation |

The CLI should return an empty list for unknown or low-confidence contexts, not
guess aggressively.

### 3.2 Hover

```bash
marreta tooling hover \
  --stdin \
  --file routes/greetings.marreta \
  --line 12 \
  --column 8 \
  --format json
```

Response shape:

```json
{
  "contents": [
    {
      "kind": "markdown",
      "value": "### cache.get(key)\nReturns the cached value for `key`, or `null` when missing."
    }
  ],
  "range": {
    "start": { "line": 12, "column": 5 },
    "end": { "line": 12, "column": 14 }
  }
}
```

Hover should work for:

- reserved words: `route`, `task`, `schema`, `reply`, `fail`, `require`,
  `transaction`, `on queue`, `on topic`;
- namespaces: `db`, `doc`, `cache`, `queue`, `topic`, `http_client`, `time`,
  `math`, `uuid`, `feature`, `json`, `base64`, `fs`, `log`;
- namespace operations: `cache.get`, `http_client.post`, `uuid.v7`;
- built-in functions: `type`, `len`, `range`, `decimal`;
- methods by type: string/list/map/integer/float/decimal/time values;
- project symbols: tasks and schemas.

This is the Marreta equivalent of JavaDoc-style hover: short, local,
immediately actionable documentation where the developer is already working.

If the symbol under the cursor cannot be resolved confidently, the CLI returns
`null` rather than generic documentation.

### 3.3 Symbols

```bash
marreta tooling symbols --format json
```

Response shape:

```json
[
  {
    "name": "greet",
    "kind": "task",
    "file": "tasks/greetings.marreta",
    "line": 4,
    "column": 13,
    "detail": "task greet(name)"
  },
  {
    "name": "GreetingResponse",
    "kind": "schema",
    "file": "schemas/greetings.marreta",
    "line": 1,
    "column": 8,
    "detail": "schema GreetingResponse"
  }
]
```

Symbols support:

- VS Code outline;
- workspace symbol search;
- autocomplete of project task/schema names.

First cut symbol kinds:

- route;
- task;
- schema;
- consumer (`on queue`, `on topic`);
- scenario test.

### 3.4 Diagnostics

Diagnostics are provided by `marreta lint --format json`, not a separate
tooling command.

VS Code should call:

```bash
marreta lint --stdin --file routes/greetings.marreta --format json
```

### 3.5 Formatting

Formatting is provided by `marreta fmt`, not a separate tooling command.

VS Code should call:

```bash
marreta fmt --stdin --file routes/greetings.marreta
```

### 3.6 Built-In Catalog

The built-in catalog is exposed as a tooling command so editor integrations do
not need to duplicate Marreta's namespace, operation, method, keyword, or
snippet metadata.

```bash
marreta tooling catalog --format json
```

Response shape:

```json
{
  "version": 1,
  "entries": [
    {
      "name": "cache.get",
      "kind": "function",
      "signature": "cache.get(key)",
      "insert_text": "get(${1:key})",
      "summary": "Returns the cached value for key, or null when missing.",
      "examples": ["value = cache.get(\"greeting\")"],
      "warnings": ["Returns null when the key does not exist."]
    }
  ]
}
```

The catalog command is read-only and deterministic. It does not inspect project
files. Project-specific symbols come from `marreta tooling symbols`.

---

## 4. Built-In Catalog

The CLI must expose an official catalog of built-ins. This catalog is used by:

- completions;
- hover;
- future docs generation;
- future website/API reference;
- tests that keep editor docs aligned with runtime behavior.

The catalog should live in Rust near the runtime dispatch definitions, not in
the VS Code extension.

Each catalog entry contains:

| Field | Meaning |
|---|---|
| `name` | canonical name, for example `cache.get` |
| `kind` | namespace/function/method/keyword |
| `signature` | human-readable Marreta signature |
| `insert_text` | snippet insertion text |
| `summary` | one-line explanation |
| `examples` | one or more short Marreta snippets |
| `warnings` | optional caveats, for example "returns null when missing" |

The catalog is part of the public developer experience. Runtime dispatch names
and catalog names must be tested together so editor docs do not drift.

The `version` field in `marreta tooling catalog --format json` is the schema
version of the catalog response, not the Marreta runtime version.

Hover documentation for built-ins is authored manually in the Rust catalog. It
is not generated from runtime code. Alignment with runtime behavior is enforced
through tests that compare catalog names against dispatch names and through
examples that execute where practical.

### 4.1 Namespace Completion Examples

```marreta
uuid.
# v4()
# v7()

feature.
# enabled(name)

cache.
# get(key)
# set(key, value)
# delete(key)
# exists(key)
# incr(key)
# decr(key)
# get_many(keys)
# set_many(values)

http_client.
# get(url, ...)
# post(url, payload, ...)
# put(url, payload, ...)
# patch(url, payload, ...)
# delete(url, ...)
```

### 4.2 Queue and Topic Examples

```marreta
queue.
# push "queue.name", payload

topic.
# publish "topic.name", payload
```

`queue.push` and `topic.publish` have syntax that is not plain method-call
syntax. Completion detail must show the real Marreta syntax.

### 4.3 Type Method Examples

String:

```marreta
"hello".
# upper()
# lower()
# trim()
# contains(value)
# starts_with(value)
# ends_with(value)
# replace(from, to)
# split(separator)
# length()
```

List:

```marreta
[1, 2].
# length()
# first()
# last()
# push(value)
# includes(value)
# reverse()
# sort()
# unique()
# join(separator)
# flatten()
# slice(start, end)
```

Map:

```marreta
{ name: "Ana" }.
# keys()
# has(key)
# delete(key)
# size()
```

Decimal:

```marreta
decimal("19.90").
# round(places)
# trunc()
# floor()
# ceil()
# to_integer()
# to_string()
```

---

## 5. Completion Levels

### 5.1 Level 1 — Static Built-In Completion

Required for first cut.

Completes:

- keywords;
- namespaces;
- namespace operations;
- built-in functions;
- known methods by obvious literal type.

Examples:

```marreta
cache.
uuid.
http_client.
"hello".
[1, 2].
```

Trigger contexts:

- after `.` on a known namespace or obvious literal;
- at top-level declaration positions;
- after route/task block indentation where statements are expected;
- after keywords that expect known language terms.

### 5.2 Level 2 — Project Symbols

Required for first cut.

Completes:

- task names;
- schema names;
- route-local task names;
- exported cross-file tasks;
- schemas in schema annotation positions.

Examples:

```marreta
take payload as 
# GreetingRequest
# OrderCreated

reply 200 as 
# GreetingResponse

result = gre
# greet(...)
```

Project symbol completion is based on parseable source. If the active unsaved
buffer has syntax errors, the tooling should still use successfully parsed
project files and best-effort symbols from the current buffer when safe.

### 5.3 Level 3 — Contextual Best-Effort Completion

Optional for first cut, but the architecture must allow it.

Examples:

```marreta
items = [1, 2, 3]
items.
# list methods

name = "Thiago"
name.
# string methods
```

No deep type inference is required for values from DB/cache/doc/http in the
first cut.

Level 3 must never hide Level 1 or Level 2 results. If type inference is
uncertain, return generic safe completions or no contextual methods.

---

## 6. Hover Documentation

Hover content should be concise and example-oriented.

Example for `reply`:

````markdown
### reply

Terminates the current route with an HTTP response.

```marreta
reply 200, { ok: true }
reply 201 as GreetingResponse, result
reply text 200, "ok"
```
````

Example for `feature.enabled`:

````markdown
### feature.enabled(name)

Returns `true` when `MARRETA_FEATURE_<NAME>` is enabled.
Missing flags return `false`.

```marreta
if feature.enabled("inventory_api")
    reply 200, { enabled: true }
```
````

Example for a project task:

````markdown
### task greet(name)

Defined in `tasks/greetings.marreta:4`.

```marreta
greet("World")
```
````

Hover should not be long-form documentation. It should answer: "What is this,
how do I use it, and what should I be careful about?"

Hover content priority:

1. exact namespace operation or method under cursor;
2. project task/schema under cursor;
3. keyword or reserved construct;
4. namespace itself;
5. no hover.

Hovers for built-ins should include at most:

- title/signature;
- one sentence summary;
- one small example;
- one caveat when needed.

---

## 7. VS Code Integration

The official VS Code extension should provide:

- syntax highlighting;
- snippets for common declarations;
- format document via `marreta fmt --stdin --file`;
- diagnostics via `marreta lint --stdin --file --format json`;
- completion via `marreta tooling completions ...`;
- hover via `marreta tooling hover ...`;
- document/workspace symbols via `marreta tooling symbols ...`;
- configurable path to the `marreta` binary.

The extension should not duplicate parser logic.

The extension is responsible for editor orchestration only:

- converting VS Code 0-based positions into Marreta 1-based positions;
- passing unsaved buffers through stdin;
- mapping Marreta JSON into VS Code completion/hover/diagnostic objects;
- debouncing expensive calls;
- caching catalog-independent results for a short time when safe.

The extension must not:

- parse Marreta syntax beyond trivial trigger detection;
- maintain a hardcoded list of namespaces or methods;
- decide semantic diagnostics by itself.

### 7.1 Binary Resolution

Resolution order:

1. User setting: `marreta.path`
2. `marreta` on PATH
3. Error diagnostic telling the user how to configure the binary path

The extension should expose:

```json
{
  "marreta.path": "marreta",
  "marreta.tooling.debounceMs": 150,
  "marreta.diagnostics.onChange": true
}
```

### 7.2 Performance

The first cut may shell out to the CLI per request.

If latency becomes visible, future work may add a long-running language server:

```bash
marreta lsp
```

That future server must use the same underlying catalog/parser/linter as the
CLI commands.

Expected first-cut behavior:

- completion/hover calls are debounced;
- diagnostics run on save and optionally on change;
- formatting runs only on explicit format or format-on-save;
- symbols refresh on save or file creation/deletion.

Shelling out is acceptable for the first cut only if common editor interactions
feel responsive on the functional test project.

### 7.3 Syntax Highlighting and Snippets

Syntax highlighting can live in the VS Code extension as TextMate grammar
metadata. This is editor presentation, not semantic language intelligence.

Snippets may also live in the VS Code extension for the first cut, but snippets
for built-in operations should be generated from, or tested against, the Rust
catalog when practical.

First-cut split:

- declaration snippets live in the VS Code extension;
- built-in operation snippets come from catalog `insert_text` through
  completion results;
- the extension must not maintain a second built-in operation snippet list.

Core snippets:

- route declarations;
- task declarations;
- schema declarations;
- `reply`;
- `require ... else fail`;
- `on queue`;
- `on topic`;
- scenario tests.

---

## 8. Non-Goals

- No full semantic type inference in the first cut.
- No long-running `marreta lsp` server in the first cut.
- No rename symbol.
- No refactoring engine.
- No code actions in the first cut.
- No debugger integration.
- No editor-specific parser implementation.
- No AI assistant integration in the first cut.

---

## 9. Implementation Notes

Suggested internal modules:

- `tooling/catalog.rs`: built-in namespaces, methods, docs, signatures.
- `tooling/completions.rs`: completion resolver.
- `tooling/hover.rs`: hover resolver.
- `tooling/symbols.rs`: project symbol extraction.
- `cli/tooling.rs`: command dispatch and JSON rendering.
- `tooling/position.rs`: source-position helpers shared by completions/hover.

The catalog should be tested against runtime dispatch names so docs do not
drift from implementation.

Suggested VS Code extension structure:

- `extension.ts`: activation and command registration.
- `client/marretaCli.ts`: binary resolution and process execution.
- `providers/completion.ts`: completion adapter.
- `providers/hover.ts`: hover adapter.
- `providers/diagnostics.ts`: lint adapter.
- `providers/format.ts`: formatter adapter.
- `providers/symbols.ts`: document/workspace symbols adapter.
- `syntaxes/marreta.tmLanguage.json`: TextMate grammar.
- `snippets/marreta.json`: snippets.

The extension should be tested with fixture projects, not by duplicating parser
fixtures in TypeScript.

---

## 10. Test Plan

Rust/unit tests:

- Catalog contains expected namespaces and operations.
- Catalog operation names match runtime dispatch names.
- Completion after `cache.` returns cache methods.
- Completion after `uuid.` returns `v4` and `v7`.
- Completion in `take payload as` returns schemas.
- Hover on keyword returns markdown docs.
- Hover on namespace operation returns signature docs.
- Hover on project task returns file/line and signature.
- Stdin overlay changes completions/hover without writing files.
- Position conversion resolves the intended token at line/column boundaries.
- Unknown completion contexts return an empty list.
- Unknown hover contexts return null.

Functional tests:

- `marreta tooling completions --format json` returns stable JSON.
- `marreta tooling hover --format json` returns stable JSON.
- `marreta tooling symbols --format json` lists project tasks/schemas/routes.
- `marreta tooling catalog --format json` returns built-in metadata without
  reading project files.
- Unsaved buffer via stdin affects diagnostics and completions.
- VS Code fixture test can invoke CLI-backed completion/hover adapters without
  requiring a running Marreta server.
- VS Code fixture verifies the extension does not contain hardcoded built-in
  namespace lists.
- Formatter adapter calls `marreta fmt --stdin --file`.
- Diagnostics adapter maps `marreta lint --format json` into ranges and
  severities.

---

## 11. Open Questions

None for the first cut.
