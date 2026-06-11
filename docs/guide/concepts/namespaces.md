---
title: "Namespaces"
category: concepts
slug: "concepts/namespaces"
summary: "What namespaces are in Marreta, the native ones built into the language and the file namespaces you create by exporting tasks."
---

# Namespaces

A namespace groups related operations under a name you reach with dot syntax. You call
`db.users.find(id)`, `time.now()`, or `cache.get("key")`. The name before the dot is the
namespace, and it tells you where the behavior comes from. Marreta has two kinds of
namespace: the native ones it ships with, and the file namespaces you create.

## Native namespaces

Native namespaces are built into the language and the runtime. They cover data,
messaging, integration, and utilities, and you use them without any import or wiring. The
ones backed by infrastructure (the database, cache, and messaging) read their connection
details from `marreta.env`, so the namespace stays the same while the provider behind it
is configuration.

```ruby
route GET "/users/:id"
    user = db.users.find(params.id)
    require user else fail 404, "user not found"
    reply 200, user
```

There is no `import db` and no client to construct. The full list, one page each, is in
the [Namespaces reference](../reference/namespaces.md).

A native namespace name is reserved, so a variable cannot shadow it. This is what keeps a
configured provider from silently vanishing inside a scope that happened to reuse its
name. See the [reserved words](../reference/keywords.md) model for the full rule.

## File namespaces

You can also create your own namespaces. When a `.marreta` file exports tasks, the file
itself behaves as a namespace for any other file that uses them: the namespace is the file
name, and you call its tasks as `filename.task`. This is how shared logic travels between
files, with the origin visible at the call site and still no imports.

Take a file `tasks/text.marreta`:

```ruby
# A private helper, file-local. It is never reachable as text.decorate, but an
# exported task in the same file can call it.
task decorate(word) => "<" + word + ">"

export task shout(word) => word.upper() + "!"
export task wrap(word) => decorate(word)
```

The file name (`text`) is the namespace. From another file, its exported tasks are reached
through it, including as a pipeline stage:

```ruby
route GET "/text/:word"
    reply 200, {
        shout: text.shout(params.word),
        wrapped: text.wrap(params.word),
        piped: params.word >> text.shout
    }
```

A few rules follow from this:

- Only `export` tasks are reachable from other files. A task without `export`, like
  `decorate` above, stays private to its file.
- A cross-file call must be namespaced. A bare `shout(word)` from another file does not
  resolve; it has to be `text.shout(word)`.
- A file namespace cannot shadow a native namespace, so a file named `db.marreta` that
  exports tasks is a load error. The same is true for the reserved `app` name.
- Tasks and schemas defined in `app.marreta` are global, so they need no namespace.

For the built-in side and the per-namespace reference, see
[Namespaces](../reference/namespaces.md). The `export` keyword that makes a task shareable
is covered in [Keywords](../reference/keywords.md).
