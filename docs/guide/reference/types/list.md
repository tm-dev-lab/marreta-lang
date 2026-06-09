---
title: "list"
category: types
slug: "reference/types/list"
summary: "Ordered collections of values and the methods available on them."
---

# list

A `list` is an ordered collection of values. As a schema field it is written
`tags: list` for any values, or `tags: list of string` for a typed list. A literal
uses brackets, as in `[1, 2, 3]`.

```ruby
items = [3, 1, 2]
sorted = items.sort()
has_two = items.includes(2)
```

## Methods

| Name | Signature | Summary |
|---|---|---|
| `list.length` | `length()` | Returns the number of items. |
| `list.empty?` | `empty?()` | Returns whether the list is empty. |
| `list.first` | `first()` | Returns the first item, or null. |
| `list.last` | `last()` | Returns the last item, or null. |
| `list.push` | `push(value)` | Appends a value and returns the list. |
| `list.includes` | `includes(value)` | Returns whether the list includes a value. |
| `list.slice` | `slice(start, end)` | Returns a sub-list. |
| `list.sort` | `sort()` | Returns a sorted list. |
| `list.reverse` | `reverse()` | Returns a reversed list. |
| `list.unique` | `unique()` | Returns the distinct values. |
| `list.join` | `join(separator)` | Joins the values into a string. |
| `list.flatten` | `flatten()` | Flattens one level of nesting. |
| `list.zip` | `zip(other)` | Pairs two lists of the same length. |
| `list.sum` | `sum()` | Sums a numeric list. |
| `list.mean` | `mean()` | Returns the mean, or null when empty. |
| `list.median` | `median()` | Returns the median, or null when empty. |
| `list.std_dev` | `std_dev()` | Returns the population standard deviation. |
