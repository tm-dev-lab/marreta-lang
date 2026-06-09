# 029 — Filesystem Namespace

> Status: Delivered
> Type: Delivered language feature
> Scope: Small native filesystem helper surface for backend/API use cases

---

## 1. Purpose

This spec introduces a small native `fs` namespace for MarretaLang.

The purpose of `fs` is not to turn the language into a general-purpose shell,
automation, or operating-system scripting environment.

The purpose is narrower:

- support the small set of file interactions that appear naturally in backend
  and API code
- keep those interactions readable and explicit
- avoid pushing developers into awkward external scripts or provider-specific
  glue for common local-file tasks

Examples of legitimate use cases:

- load a local template or static text fragment
- persist a generated report or export file
- write local audit/debug artifacts
- read a small local fixture or seed input
- check whether a local file already exists before generating it

---

## 2. Why `fs` Is Sensitive

Unlike `time` or `math`, filesystem access can easily blur the identity of the
language if it grows too far.

If the surface is too broad, MarretaLang stops feeling like a backend/API DSL
and starts becoming a host automation language.

That is not the goal.

Because of that, `fs` must follow stricter boundaries than other namespaces:

1. it must stay small
2. it must remain text-first
3. it must avoid shell-like breadth
4. it must not introduce platform-heavy semantics
5. it must not become a deployment or ops abstraction

---

## 3. Design Principles

The `fs` namespace must follow these rules:

1. It should cover only common backend/API file interactions.
2. It should prefer explicitness over convenience magic.
3. It should accept plain string paths.
4. It should default to text operations, not binary/stream APIs.
5. It should fail clearly when the path is invalid, missing, or inaccessible.
6. It should not expose low-level OS controls in the first cut.
7. It should be UTF-8-only in the first cut.

---

## 4. Proposed Surface

The initial surface should be:

```marreta
fs.read(path)
fs.write(path, value)
fs.append(path, value)
fs.exists(path)
fs.delete(path)
```

This is the current proposed first-cut surface.

All paths are plain strings.

---

## 5. Function Purposes

## 5.1 `fs.read(path)`

Purpose:

- load a local file as text without external glue code

Example:

```marreta
terms = fs.read("templates/terms.txt")
```

## 5.2 `fs.write(path, value)`

Purpose:

- write text content to a file in one direct operation

Example:

```marreta
fs.write("out/report.json", payload.to_string())
```

## 5.3 `fs.append(path, value)`

Purpose:

- append text content to an existing file without reading and rewriting it

Example:

```marreta
fs.append("logs/audit.log", "created order #{order.id}\n")
```

## 5.4 `fs.exists(path)`

Purpose:

- check for local file existence before a read or write decision

Example:

```marreta
already_exported = fs.exists("out/export.csv")
```

## 5.5 `fs.delete(path)`

Purpose:

- remove a local file explicitly when the app owns its lifecycle

Example:

```marreta
deleted = fs.delete("tmp/preview.txt")
```

---

## 6. Semantics

## 6.1 Input types

`fs` functions should accept:

- `string` paths

Write-oriented functions should accept:

- `string`

The first implementation should stay text-first and string-strict for writes.

If a caller wants to persist a non-string value, conversion must be explicit in
user code.

## 6.2 Return types

Return types should be predictable:

- `fs.read(path)` -> `string`
- `fs.write(path, value)` -> written `string`
- `fs.append(path, value)` -> appended `string`
- `fs.exists(path)` -> `boolean`
- `fs.delete(path)` -> `boolean`

`fs.write` and `fs.append` should follow pass-through semantics.

That means:

- they perform the file side effect
- they return exactly the same written string value
- they can be used naturally inside pipelines without breaking the flow

The namespace should remain pipeline-compatible in general.

That means any `fs.*` function may appear in a pipeline as long as its
signature matches the incoming value naturally.

## 6.3 Text semantics

The first cut should treat file contents as text.

That means:

- no binary payload type
- no stream type
- no chunked readers
- no special path object

The first cut should also be UTF-8-only.

That means:

- `fs.read(path)` reads text as UTF-8
- `fs.write(path, value)` writes text as UTF-8
- `fs.append(path, value)` appends text as UTF-8
- invalid UTF-8 on read must fail explicitly
- other encodings are out of scope for now

`fs.read(path)` must preserve the file contents exactly as stored after UTF-8
decoding.

That means:

- no trimming
- no newline normalization
- no whitespace cleanup
- no invisible content rewriting

If binary support is ever added later, it should be a deliberate separate spec.

## 6.4 Path semantics

The first cut should use simple string paths exactly as written by the caller.

The language should not introduce:

- path joining DSL
- glob patterns
- recursive traversal
- implicit directory creation by default

If a parent directory does not exist, `fs.write` and `fs.append` should fail
explicitly rather than guessing intent.

The first cut should not create missing directories implicitly.

---

## 7. Error Behavior

The namespace should fail clearly for invalid input or inaccessible files.

Examples:

```marreta
fs.read(null)
fs.read("missing.txt")
fs.write("out/report.txt", null)
fs.append("logs/app.log", { a: 1 })
```

Expected outcomes:

- invalid path type -> type error
- missing file on `read` -> runtime/file error
- missing parent directory on `write` or `append` -> runtime/file error
- permission failures -> runtime/file error
- invalid UTF-8 on `read` -> runtime/file error
- deleting a missing file -> `false`

The namespace should not silently create missing directories in the first cut.

`fs.delete(path)` should be idempotent for missing files.

That means:

- missing file -> no runtime error
- missing file -> returns `false`
- actual removal -> returns `true`

---

## 8. What Does Not Belong

The following should stay out of the initial `fs` surface:

- `read_lines`
- directory listing
- recursive traversal
- globbing
- file move/rename
- copy
- mkdir/rmdir
- chmod/chown
- symlink management
- temporary file APIs
- binary streams
- alternate text encodings
- file watchers
- shell execution

These would expand the surface too far and dilute the language.

---

## 9. Examples

## 9.1 Load a local template

```marreta
route GET "/terms"
    body = fs.read("templates/terms.txt")
    reply text 200, body
```

## 9.2 Persist a generated export

```marreta
route POST "/exports/orders" take payload
    content = "#{payload.orders}"
    fs.write("out/orders.json", content)
    reply 201, { stored: true }
```

## 9.3 Append a local audit line

```marreta
task audit_export(order_id)
    fs.append("logs/export.log", "exported #{order_id}\n")
```

## 9.4 Use with `if/else`

```marreta
content = if fs.exists("cache/report.txt")
    fs.read("cache/report.txt")
else
    generated = "fresh report"
    fs.write("cache/report.txt", generated)
```

## 9.5 Use with pipeline

```marreta
payload >> fs.write("out/request.json")
```

This should behave as a tap/pass-through stage:

- the incoming value is written to disk
- the same string value continues through the pipeline

Example:

```marreta
fs.read("templates/notice.txt")
    >> fs.write("out/notice.txt")
    >> queue.publish("notice.saved")
```

```marreta
"templates/welcome.html"
    >> fs.read
    >> cache.set("welcome-template")
```

```marreta
"tmp/report.txt" >> fs.delete
```

## 9.6 Read lines by composition

The first cut should not introduce `fs.read_lines(...)`.

Line-oriented reading should be expressed by composition on top of
`fs.read(...)`.

Example:

```marreta
content = fs.read("data/users.txt")
lines = content.split("\n")
```

If the caller wants to normalize CRLF explicitly, that should also stay in user
code:

```marreta
content = fs.read("data/users.txt")
lines = content.split("\n") >> map line
    keep line.trim()
```

The intended style is:

- use `fs.read(...)` directly when the file content is required
- let `read` fail explicitly if the file is missing or inaccessible
- use `fs.exists(...)` when file presence is itself the domain question, not as
  a mandatory pre-check before every read

---

## 10. Delivery Notes

Delivered on branch `feature/filesystem-029`.

Phase mapping:

- **Phase 1** — spec closure and reserved namespace delivery in
  `5c47e9d` (`docs: add filesystem namespace spec`)
- **Phase 2** — parser/runtime namespace support, UTF-8-only file semantics,
  pipeline support, and unit coverage in `f9a6d5b`
  (`feat: add filesystem namespace`)
- **Phase 3** — VS Code bundle refresh for `fs` in `4c8ff45`
  (`docs: add vscode extension 0.2.9 package`)

Validated with:

- `cargo test --lib test_fs_ -- --nocapture`
- `cargo test --lib`
- `bash examples/functional_tests/test.sh`

---

## 11. Recommendation

Current recommendation:

1. keep `fs` small
2. keep it text-first
3. keep it UTF-8-only in the first cut
4. avoid directory and OS-management APIs
5. prefer explicit failure over hidden side effects
6. require explicit string conversion before writing non-string values
7. keep delete idempotent for missing files
8. preserve file text exactly as read
9. allow `fs.*` in pipelines when signatures match naturally
10. keep line-reading as composition instead of adding `read_lines`

The best first implementation target is:

- `read`
- `write`
- `append`
- `exists`
- `delete`

---

## 12. Implementation Plan

Implementation followed the plan below and is now complete.

### Phase 1

- finalize namespace boundaries and text-first UTF-8-only semantics

### Phase 2

- add parser/runtime namespace support
- add unit tests for all functions and error paths

### Phase 3

- add functional coverage in `examples/functional_tests`
- validate interaction with:
  - routes
  - tasks
  - `if/else`
  - pipelines

---

## 13. Test Plan

Implementation of this spec included:

1. unit tests for each `fs.*` function
2. unit tests for invalid-type and missing-file errors
3. functional tests in `examples/functional_tests`
4. integration checks proving use in routes and task flows
