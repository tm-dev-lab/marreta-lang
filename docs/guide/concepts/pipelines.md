---
title: "Pipelines"
category: concepts
slug: "concepts/pipelines"
summary: "How the >> operator threads a value through a sequence of steps, so data flows left to right instead of nesting calls."
---

# Pipelines

A pipeline threads a value through a sequence of steps with the `>>` operator. Each step
takes the result of the one before it, so the code reads as the flow of the data, left to
right and top to bottom, instead of nested calls you have to read inside out.

```ruby
recent = db.orders >> where(active: true) >> order_by("id desc") >> fetch
```

The same value enters on the left, each `>>` hands the result to the next step, and the
last step produces the final value. Without the pipeline, that query would nest as
`fetch(order_by(where(db.orders, ...), ...))`, which reads backwards from how it runs.

## Where pipelines show up

The operator is the same everywhere; only the steps change.

- **Database and document queries** build up with steps like `where`, `order_by`, `limit`,
  and a terminal like `fetch` or `count`.
- **Collection transforms** reshape a list with `map`, `keep`, `skip`, and `reduce`.

```ruby
users = db.users >> fetch
names = users >> map user
    skip if not user.active
    keep user.name
```

- **Outbound calls** pipe a body into an HTTP request:

```ruby
created = payload >> http_client.post("https://api.example.com/orders")
```

- **Your own tasks** can be pipeline steps too. A task shared from another file is reached
  through its [file namespace](namespaces.md), which is the file name. Given a file
  `tasks/text.marreta` that exports a task:

```ruby
export task shout(word) => word.upper() + "!"
```

  another file pipes a value straight into it as `text.shout`:

```ruby
loud = params.word >> text.shout
```

## Sequential by nature

A pipeline is ordered: every step waits for the one before it, because each depends on the
previous result. When you have independent work that does not form a chain, like several
outbound calls that do not feed each other, reach for [broadcast](broadcast.md) instead,
which sends one value to several branches at once.
