---
title: "fs"
category: namespaces
slug: "reference/namespaces/fs"
summary: "Read and write UTF-8 files on the local filesystem of the running server."
---

# fs

The `fs` namespace reads and writes UTF-8 files on the local filesystem of the server
process. For durable application data, use [`db`](db.md) or [`doc`](doc.md). Reach for
`fs` only for local files such as a template or an export.

## Operations

File operations are fallible, so guard them with `rescue`:

```ruby
contents = fs.read(path) rescue { error: "could not read" }
```

| Name | Signature | Summary |
|---|---|---|
| `fs.read` | `fs.read(path)` | Reads a UTF-8 file. |
| `fs.write` | `fs.write(path, content)` | Writes a UTF-8 file and returns the content. |
| `fs.append` | `fs.append(path, content)` | Appends UTF-8 content to a file. |
| `fs.exists` | `fs.exists(path)` | Returns whether a file exists. |
| `fs.delete` | `fs.delete(path)` | Deletes a file and returns whether it existed. |

## Notes

- These operations touch the server's local disk, which is not shared across
  instances and may be ephemeral. Do not use `fs` as your source of truth.
- `read`, `write`, `append`, and `delete` can fail (missing file, permissions). Wrap
  them in `rescue`.
