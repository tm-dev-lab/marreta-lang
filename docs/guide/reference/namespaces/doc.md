---
title: "doc"
category: namespaces
slug: "reference/namespaces/doc"
summary: "Store and read schemaless documents in a collection through the document provider, with no migration."
---

# doc

The `doc` namespace stores and reads documents in a collection through the configured
document provider. Each collection is addressed by name, as `doc.<collection>`, and
needs no declaration: the first `save` creates it.

## When to use

Use `doc` for flexible, nested data that you want to persist without defining a table
or running a migration. It is the simplest way to store data in Marreta. When you
need typed columns, relations, and versioned schema changes, use [`db`](db.md)
instead.

[Save and read your first data](../../tutorials/save-and-read-data.md) is a short
tutorial built on `doc`.

## Operations

`save` stores a document and returns it with a generated `_id`:

```ruby
note = doc.notes.save({ title: "First", body: "hello" })
found = doc.notes.find(note._id)
```

| Operation | Signature | Summary |
|---|---|---|
| save | `doc.<collection>.save(map)` | Stores a document and returns it, including the generated `_id`. |
| find | `doc.<collection>.find(id)` | Returns the document with that `_id`, or null. |
| find_all | `doc.<collection>.find_all()` | Returns every document in the collection. |
| update | `doc.<collection>.update(id, map)` | Updates the document by `_id` and returns it. |
| delete | `doc.<collection>.delete(id)` | Deletes the document by `_id`. |

For queries beyond an id lookup, open a pipeline with `>>`. A filter takes a string
field name and a comparison:

```ruby
recent = doc.orders >> where("status" == "open") >> order("created_at", "desc") >> fetch_all
```

| Step | Form | Summary |
|---|---|---|
| where | `where("field" == value)` (also `!=`, `<`, `<=`, `>`, `>=`) | Filters by a field. |
| like / in | `like("field", "pattern")` / `in("field", list)` | Pattern and set filters. |
| order | `order("field", "asc")` | Sorts the results. |
| limit / offset | `limit(n)` / `offset(n)` | Pages the results. |
| pick | `pick(["field", ...])` | Projects each document to the named fields. |
| group_by + aggregate | `group_by("field") >> sum("field", as: "name")` | Groups and aggregates (`sum`, `avg`, `min`, `max`, `count`). |
| fetch_all (terminal) | `>> fetch_all` | Returns the matching documents as a list. |
| fetch_one (terminal) | `>> fetch_one` | Returns the first document, or null. |
| count / exists (terminal) | `>> count` / `>> exists` | Number of matches, or whether any match. |
| update / upsert / delete (terminal) | `>> update(map)` / `>> upsert(map)` / `>> delete` | Writes across every match. |

## Raw aggregation pipeline

When the `>>` query builder cannot express what you need, drop to
`doc.pipeline(collection, stages)`. It runs a raw aggregation pipeline, where each
stage is a single-key map and the keys are plain identifiers, with no `$`:

```ruby
result = doc.pipeline("orders", [
    { match: { status: "paid" } },
    { sort: { amount: -1 } },
    { limit: 2 }
])
```

Like `db.native_query`, it is an escape hatch. Reach for it only when the query
pipeline and the per-document methods fall short.

## Notes

- The document provider must be configured and reachable before `marreta serve`. Run
  `marreta doctor` to check the connection.
- Documents are keyed by `_id`, generated on `save`, not by an `id` column.
- Unlike `db`, there is no migration. A collection and its fields exist as soon as
  you write to them.
- A `doc` filter names the field as a string with a comparison, as in
  `where("city" == value)`. A `db` filter uses `where(city: value)`.
