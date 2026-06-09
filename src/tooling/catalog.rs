use serde_json::{Value as JsonValue, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogKind {
    Keyword,
    Namespace,
    Function,
    Method,
}

impl CatalogKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keyword => "keyword",
            Self::Namespace => "namespace",
            Self::Function => "function",
            Self::Method => "method",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub name: &'static str,
    pub kind: CatalogKind,
    pub signature: &'static str,
    pub insert_text: &'static str,
    pub summary: &'static str,
    pub example: &'static str,
    pub warnings: &'static [&'static str],
}

impl CatalogEntry {
    pub fn to_json(&self) -> JsonValue {
        json!({
            "name": self.name,
            "kind": self.kind.as_str(),
            "signature": self.signature,
            "insert_text": self.insert_text,
            "summary": self.summary,
            "examples": if self.example.is_empty() {
                Vec::<&str>::new()
            } else {
                vec![self.example]
            },
            "warnings": self.warnings,
        })
    }

    pub fn completion_label(&self) -> String {
        if let Some((_, method)) = self.name.split_once('.') {
            method.to_string()
        } else {
            self.name.to_string()
        }
    }
}

pub fn catalog_json() -> JsonValue {
    json!({
        "version": 1,
        "entries": catalog()
            .iter()
            .map(CatalogEntry::to_json)
            .collect::<Vec<_>>(),
    })
}

pub fn catalog() -> &'static [CatalogEntry] {
    CATALOG
}

pub fn find_entry(name: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|entry| entry.name == name)
}

pub fn entries_for_namespace(namespace: &str) -> Vec<&'static CatalogEntry> {
    let prefix = format!("{namespace}.");
    CATALOG
        .iter()
        .filter(|entry| entry.name.starts_with(&prefix))
        .collect()
}

const EMPTY: &[&str] = &[];

const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        name: "route",
        kind: CatalogKind::Keyword,
        signature: "route VERB \"/path\"",
        insert_text: "route ${1:GET} \"${2:/path}\"\n    ${3:reply 200, { ok: true }}",
        summary: "Declares an HTTP route.",
        example: "route GET \"/greetings\"\n    reply 200, { message: \"Hello\" }",
        warnings: EMPTY,
    },
    CatalogEntry {
        name: "task",
        kind: CatalogKind::Keyword,
        signature: "task name(args)",
        insert_text: "task ${1:name}(${2:arg})\n    ${3:arg}",
        summary: "Declares a reusable Marreta task.",
        example: "task greet(name)\n    \"Hello \" + name",
        warnings: EMPTY,
    },
    CatalogEntry {
        name: "schema",
        kind: CatalogKind::Keyword,
        signature: "schema Name",
        insert_text: "schema ${1:Name}\n    ${2:field}: ${3:string}",
        summary: "Declares a validation and persistence shape.",
        example: "schema Greeting\n    message: string",
        warnings: EMPTY,
    },
    CatalogEntry {
        name: "reply",
        kind: CatalogKind::Keyword,
        signature: "reply STATUS, body",
        insert_text: "reply ${1:200}, ${2:{ ok: true }}",
        summary: "Terminates the current route with an HTTP response.",
        example: "reply 200, { ok: true }",
        warnings: EMPTY,
    },
    CatalogEntry {
        name: "fail",
        kind: CatalogKind::Keyword,
        signature: "fail STATUS, body",
        insert_text: "fail ${1:400}, ${2:{ error: \"bad_request\" }}",
        summary: "Terminates the current route with an HTTP error response.",
        example: "fail 404, { error: \"not_found\" }",
        warnings: EMPTY,
    },
    CatalogEntry {
        name: "require",
        kind: CatalogKind::Keyword,
        signature: "require condition else fail STATUS, body",
        insert_text: "require ${1:condition} else fail ${2:400}, ${3:{ error: \"bad_request\" }}",
        summary: "Guards execution and fails when the condition is false.",
        example: "require payload.name else fail 400, { error: \"missing_name\" }",
        warnings: EMPTY,
    },
    CatalogEntry {
        name: "db",
        kind: CatalogKind::Namespace,
        signature: "db.TABLE.operation(...)",
        insert_text: "db",
        summary: "Relational database namespace.",
        example: "item = db.items.find(1)",
        warnings: &["Requires DB configuration before marreta serve."],
    },
    CatalogEntry {
        name: "doc",
        kind: CatalogKind::Namespace,
        signature: "doc.COLLECTION.operation(...)",
        insert_text: "doc",
        summary: "Document database namespace.",
        example: "doc.events.save({ kind: \"greeting\" })",
        warnings: &["Requires DocDB configuration before marreta serve."],
    },
    CatalogEntry {
        name: "cache",
        kind: CatalogKind::Namespace,
        signature: "cache.operation(...)",
        insert_text: "cache",
        summary: "Cache namespace backed by the configured cache provider.",
        example: "cache.set(\"greeting\", \"Hello\")",
        warnings: &["Requires cache configuration before marreta serve."],
    },
    CatalogEntry {
        name: "queue",
        kind: CatalogKind::Namespace,
        signature: "queue.push \"queue.name\", payload",
        insert_text: "queue",
        summary: "Point-to-point queue producer namespace.",
        example: "queue.push \"greetings.created\", { message: \"Hello\" }",
        warnings: &["Requires queue configuration before marreta serve."],
    },
    CatalogEntry {
        name: "topic",
        kind: CatalogKind::Namespace,
        signature: "topic.publish \"topic.name\", payload",
        insert_text: "topic",
        summary: "Exact-topic publish namespace.",
        example: "topic.publish \"greetings.created\", { message: \"Hello\" }",
        warnings: &["Requires queue configuration before marreta serve."],
    },
    CatalogEntry {
        name: "http_client",
        kind: CatalogKind::Namespace,
        signature: "http_client.verb(url, ...)",
        insert_text: "http_client",
        summary: "Outbound HTTP client namespace.",
        example: "response = http_client.get(\"https://example.test\")",
        warnings: EMPTY,
    },
    CatalogEntry {
        name: "time",
        kind: CatalogKind::Namespace,
        signature: "time.operation(...)",
        insert_text: "time",
        summary: "Date, time, duration, and interval helpers.",
        example: "created_at = time.now()",
        warnings: EMPTY,
    },
    CatalogEntry {
        name: "math",
        kind: CatalogKind::Namespace,
        signature: "math.operation(...)",
        insert_text: "math",
        summary: "Numeric helper namespace.",
        example: "rounded = math.round(12.345, places: 2)",
        warnings: EMPTY,
    },
    CatalogEntry {
        name: "uuid",
        kind: CatalogKind::Namespace,
        signature: "uuid.v4() | uuid.v7()",
        insert_text: "uuid",
        summary: "UUID generation namespace.",
        example: "id = uuid.v7()",
        warnings: EMPTY,
    },
    CatalogEntry {
        name: "feature",
        kind: CatalogKind::Namespace,
        signature: "feature.enabled(name)",
        insert_text: "feature",
        summary: "Static feature flag namespace.",
        example: "if feature.enabled(\"inventory_api\")\n    reply 200, { enabled: true }",
        warnings: &["Missing flags return false."],
    },
    CatalogEntry {
        name: "json",
        kind: CatalogKind::Namespace,
        signature: "json.operation(...)",
        insert_text: "json",
        summary: "JSON parse and serialization namespace.",
        example: "data = json.parse(raw)",
        warnings: EMPTY,
    },
    CatalogEntry {
        name: "base64",
        kind: CatalogKind::Namespace,
        signature: "base64.operation(...)",
        insert_text: "base64",
        summary: "Base64 encode/decode namespace.",
        example: "token = base64.encode(\"client:secret\")",
        warnings: EMPTY,
    },
    CatalogEntry {
        name: "fs",
        kind: CatalogKind::Namespace,
        signature: "fs.operation(...)",
        insert_text: "fs",
        summary: "Local filesystem namespace.",
        example: "content = fs.read(\"./data.txt\")",
        warnings: &["Filesystem operations affect the local runtime environment."],
    },
    CatalogEntry {
        name: "log",
        kind: CatalogKind::Namespace,
        signature: "log.level(message)",
        insert_text: "log",
        summary: "Structured runtime logging namespace.",
        example: "log.info(\"greeting created\")",
        warnings: EMPTY,
    },
    op(
        "cache.get",
        "cache.get(key)",
        "get(${1:key})",
        "Returns cached value or null when missing.",
        "value = cache.get(\"greeting\")",
    ),
    op(
        "cache.set",
        "cache.set(key, value, ttl: N, only_if_absent: true)",
        "set(${1:key}, ${2:value})",
        "Stores a value in cache.",
        "cache.set(\"greeting\", \"Hello\")",
    ),
    op(
        "cache.delete",
        "cache.delete(key)",
        "delete(${1:key})",
        "Deletes a cache key and returns whether it existed.",
        "cache.delete(\"greeting\")",
    ),
    op(
        "cache.exists",
        "cache.exists(key)",
        "exists(${1:key})",
        "Returns whether a cache key exists.",
        "cache.exists(\"greeting\")",
    ),
    op(
        "cache.ttl",
        "cache.ttl(key)",
        "ttl(${1:key})",
        "Returns remaining TTL seconds or null.",
        "cache.ttl(\"greeting\")",
    ),
    op(
        "cache.expire",
        "cache.expire(key, ttl: N)",
        "expire(${1:key}, ttl: ${2:60})",
        "Updates a cache key TTL.",
        "cache.expire(\"greeting\", ttl: 60)",
    ),
    op(
        "cache.incr",
        "cache.incr(key, by: N)",
        "incr(${1:key})",
        "Increments an integer counter.",
        "cache.incr(\"counter\", by: 1)",
    ),
    op(
        "cache.decr",
        "cache.decr(key, by: N)",
        "decr(${1:key})",
        "Decrements an integer counter.",
        "cache.decr(\"counter\", by: 1)",
    ),
    op(
        "cache.get_many",
        "cache.get_many(keys)",
        "get_many(${1:keys})",
        "Reads multiple cache keys.",
        "values = cache.get_many([\"a\", \"b\"])",
    ),
    op(
        "cache.set_many",
        "cache.set_many(values)",
        "set_many(${1:values})",
        "Writes multiple cache entries.",
        "cache.set_many({ a: 1, b: 2 })",
    ),
    op(
        "uuid.v4",
        "uuid.v4()",
        "v4()",
        "Generates a random UUID v4 string.",
        "id = uuid.v4()",
    ),
    op(
        "uuid.v7",
        "uuid.v7()",
        "v7()",
        "Generates a time-ordered UUID v7 string.",
        "id = uuid.v7()",
    ),
    op(
        "feature.enabled",
        "feature.enabled(name)",
        "enabled(${1:name})",
        "Returns true when a static feature flag is enabled.",
        "feature.enabled(\"inventory_api\")",
    ),
    op(
        "json.parse",
        "json.parse(text)",
        "parse(${1:text})",
        "Parses JSON text into a Marreta value.",
        "data = json.parse(raw)",
    ),
    op(
        "json.stringify",
        "json.stringify(value)",
        "stringify(${1:value})",
        "Serializes a Marreta value to compact JSON.",
        "text = json.stringify(data)",
    ),
    op(
        "json.pretty",
        "json.pretty(value)",
        "pretty(${1:value})",
        "Serializes a Marreta value to pretty JSON.",
        "text = json.pretty(data)",
    ),
    op(
        "base64.encode",
        "base64.encode(text, url_safe: true)",
        "encode(${1:text})",
        "Encodes text as base64.",
        "token = base64.encode(\"client:secret\")",
    ),
    op(
        "base64.decode",
        "base64.decode(text, url_safe: true)",
        "decode(${1:text})",
        "Decodes base64 text.",
        "plain = base64.decode(token)",
    ),
    op(
        "fs.read",
        "fs.read(path)",
        "read(${1:path})",
        "Reads a UTF-8 file.",
        "content = fs.read(\"./data.txt\")",
    ),
    op(
        "fs.write",
        "fs.write(path, content)",
        "write(${1:path}, ${2:content})",
        "Writes a UTF-8 file and returns the content.",
        "fs.write(\"./data.txt\", content)",
    ),
    op(
        "fs.append",
        "fs.append(path, content)",
        "append(${1:path}, ${2:content})",
        "Appends UTF-8 content to a file.",
        "fs.append(\"./data.txt\", line)",
    ),
    op(
        "fs.exists",
        "fs.exists(path)",
        "exists(${1:path})",
        "Returns whether a file exists.",
        "fs.exists(\"./data.txt\")",
    ),
    op(
        "fs.delete",
        "fs.delete(path)",
        "delete(${1:path})",
        "Deletes a file and returns whether it existed.",
        "fs.delete(\"./data.txt\")",
    ),
    op(
        "http_client.get",
        "http_client.get(url, headers:, query:, timeout:)",
        "get(${1:url})",
        "Sends an outbound GET request.",
        "response = http_client.get(\"https://example.test\")",
    ),
    op(
        "http_client.post",
        "http_client.post(url, payload, headers:, query:, timeout:)",
        "post(${1:url}, ${2:payload})",
        "Sends an outbound POST request.",
        "response = http_client.post(url, payload)",
    ),
    op(
        "http_client.put",
        "http_client.put(url, payload, headers:, query:, timeout:)",
        "put(${1:url}, ${2:payload})",
        "Sends an outbound PUT request.",
        "response = http_client.put(url, payload)",
    ),
    op(
        "http_client.patch",
        "http_client.patch(url, payload, headers:, query:, timeout:)",
        "patch(${1:url}, ${2:payload})",
        "Sends an outbound PATCH request.",
        "response = http_client.patch(url, payload)",
    ),
    op(
        "http_client.delete",
        "http_client.delete(url, headers:, query:, timeout:)",
        "delete(${1:url})",
        "Sends an outbound DELETE request.",
        "response = http_client.delete(url)",
    ),
    op(
        "math.abs",
        "math.abs(value)",
        "abs(${1:value})",
        "Returns the absolute numeric value.",
        "value = math.abs(-10)",
    ),
    op(
        "math.floor",
        "math.floor(value)",
        "floor(${1:value})",
        "Rounds down to an integer.",
        "value = math.floor(10.9)",
    ),
    op(
        "math.ceil",
        "math.ceil(value)",
        "ceil(${1:value})",
        "Rounds up to an integer.",
        "value = math.ceil(10.1)",
    ),
    op(
        "math.round",
        "math.round(value, places: N)",
        "round(${1:value})",
        "Rounds a number.",
        "value = math.round(10.125, places: 2)",
    ),
    op(
        "math.min",
        "math.min(left, right)",
        "min(${1:left}, ${2:right})",
        "Returns the smaller number.",
        "value = math.min(1, 2)",
    ),
    op(
        "math.max",
        "math.max(left, right)",
        "max(${1:left}, ${2:right})",
        "Returns the larger number.",
        "value = math.max(1, 2)",
    ),
    op(
        "math.clamp",
        "math.clamp(value, min: N, max: N)",
        "clamp(${1:value}, min: ${2:min}, max: ${3:max})",
        "Constrains a number to a range.",
        "value = math.clamp(score, min: 0, max: 100)",
    ),
    op(
        "time.now",
        "time.now()",
        "now()",
        "Returns the current instant.",
        "created_at = time.now()",
    ),
    op(
        "time.today",
        "time.today()",
        "today()",
        "Returns the current local date.",
        "billing_date = time.today()",
    ),
    op(
        "time.parse",
        "time.parse(text)",
        "parse(${1:text})",
        "Parses a temporal string.",
        "value = time.parse(\"2026-05-19\")",
    ),
    op(
        "time.date",
        "time.date(text)",
        "date(${1:text})",
        "Parses YYYY-MM-DD into a date.",
        "date = time.date(\"2026-05-19\")",
    ),
    op(
        "time.at",
        "time.at(text)",
        "at(${1:text})",
        "Parses HH:MM:SS into a time.",
        "opens_at = time.at(\"09:30:00\")",
    ),
    op(
        "time.instant",
        "time.instant(text)",
        "instant(${1:text})",
        "Parses an ISO instant.",
        "created_at = time.instant(\"2026-05-19T12:00:00Z\")",
    ),
    op(
        "time.days",
        "time.days(value)",
        "days(${1:value})",
        "Creates a duration in days.",
        "ttl = time.days(1)",
    ),
    op(
        "time.hours",
        "time.hours(value)",
        "hours(${1:value})",
        "Creates a duration in hours.",
        "ttl = time.hours(2)",
    ),
    op(
        "time.minutes",
        "time.minutes(value)",
        "minutes(${1:value})",
        "Creates a duration in minutes.",
        "ttl = time.minutes(30)",
    ),
    op(
        "time.seconds",
        "time.seconds(value)",
        "seconds(${1:value})",
        "Creates a duration in seconds.",
        "ttl = time.seconds(30)",
    ),
    op(
        "time.interval",
        "time.interval(start, end)",
        "interval(${1:start}, ${2:end})",
        "Creates a temporal interval.",
        "window = time.interval(start, end)",
    ),
    op(
        "time.contains",
        "time.contains(interval, value)",
        "contains(${1:interval}, ${2:value})",
        "Checks whether an interval contains a value.",
        "ok = time.contains(window, today)",
    ),
    op(
        "time.overlaps",
        "time.overlaps(left, right)",
        "overlaps(${1:left}, ${2:right})",
        "Checks whether two intervals overlap.",
        "ok = time.overlaps(a, b)",
    ),
    op(
        "time.format",
        "time.format(value, mask)",
        "format(${1:value}, ${2:mask})",
        "Formats a temporal value.",
        "text = time.format(today, \"%Y-%m-%d\")",
    ),
    op(
        "time.from_unix",
        "time.from_unix(seconds)",
        "from_unix(${1:seconds})",
        "Converts epoch seconds to an instant.",
        "instant = time.from_unix(1700000000)",
    ),
    op(
        "time.unix",
        "time.unix(instant)",
        "unix(${1:instant})",
        "Converts an instant to epoch seconds.",
        "seconds = time.unix(time.now())",
    ),
    op(
        "log.info",
        "log.info(message)",
        "info(${1:message})",
        "Emits an info log event.",
        "log.info(\"started\")",
    ),
    op(
        "log.warn",
        "log.warn(message)",
        "warn(${1:message})",
        "Emits a warning log event.",
        "log.warn(\"slow request\")",
    ),
    op(
        "log.error",
        "log.error(message)",
        "error(${1:message})",
        "Emits an error log event.",
        "log.error(\"failed\")",
    ),
    op(
        "queue.push",
        "queue.push \"queue.name\", payload",
        "push \"${1:queue.name}\", ${2:payload}",
        "Publishes a point-to-point queue message.",
        "queue.push \"greetings.created\", { message: \"Hello\" }",
    ),
    op(
        "topic.publish",
        "topic.publish \"topic.name\", payload",
        "publish \"${1:topic.name}\", ${2:payload}",
        "Publishes an exact-topic message.",
        "topic.publish \"greetings.created\", { message: \"Hello\" }",
    ),
    method(
        "string.upper",
        "upper()",
        "upper()",
        "Converts a string to uppercase.",
        "name.upper()",
    ),
    method(
        "string.lower",
        "lower()",
        "lower()",
        "Converts a string to lowercase.",
        "name.lower()",
    ),
    method(
        "string.trim",
        "trim()",
        "trim()",
        "Trims surrounding whitespace.",
        "name.trim()",
    ),
    method(
        "string.contains",
        "contains(value)",
        "contains(${1:value})",
        "Checks whether a string contains text.",
        "name.contains(\"x\")",
    ),
    method(
        "string.starts_with",
        "starts_with(value)",
        "starts_with(${1:value})",
        "Checks whether a string starts with text.",
        "name.starts_with(\"A\")",
    ),
    method(
        "string.ends_with",
        "ends_with(value)",
        "ends_with(${1:value})",
        "Checks whether a string ends with text.",
        "name.ends_with(\"z\")",
    ),
    method(
        "string.replace",
        "replace(from, to)",
        "replace(${1:from}, ${2:to})",
        "Replaces text inside a string.",
        "name.replace(\"a\", \"b\")",
    ),
    method(
        "string.split",
        "split(separator)",
        "split(${1:separator})",
        "Splits a string into a list.",
        "csv.split(\",\")",
    ),
    method(
        "string.index_of",
        "index_of(value)",
        "index_of(${1:value})",
        "Returns the index of text inside a string, or -1 when missing.",
        "name.index_of(\"ell\")",
    ),
    method(
        "string.length",
        "length()",
        "length()",
        "Returns string length.",
        "name.length()",
    ),
    method(
        "list.length",
        "length()",
        "length()",
        "Returns list length.",
        "items.length()",
    ),
    method(
        "list.first",
        "first()",
        "first()",
        "Returns the first list item or null.",
        "items.first()",
    ),
    method(
        "list.last",
        "last()",
        "last()",
        "Returns the last list item or null.",
        "items.last()",
    ),
    method(
        "list.empty?",
        "empty?()",
        "empty?()",
        "Returns whether the list is empty.",
        "items.empty?()",
    ),
    method(
        "list.push",
        "push(value)",
        "push(${1:value})",
        "Appends a value and returns the list.",
        "items.push(value)",
    ),
    method(
        "list.includes",
        "includes(value)",
        "includes(${1:value})",
        "Checks whether a list includes a value.",
        "items.includes(value)",
    ),
    method(
        "list.reverse",
        "reverse()",
        "reverse()",
        "Returns a reversed list.",
        "items.reverse()",
    ),
    method(
        "list.sort",
        "sort()",
        "sort()",
        "Returns a sorted list.",
        "items.sort()",
    ),
    method(
        "list.unique",
        "unique()",
        "unique()",
        "Returns unique list values.",
        "items.unique()",
    ),
    method(
        "list.join",
        "join(separator)",
        "join(${1:separator})",
        "Joins list values into a string.",
        "items.join(\",\")",
    ),
    method(
        "list.flatten",
        "flatten()",
        "flatten()",
        "Flattens one list level.",
        "items.flatten()",
    ),
    method(
        "list.slice",
        "slice(start, end)",
        "slice(${1:start}, ${2:end})",
        "Returns a list slice.",
        "items.slice(0, 2)",
    ),
    method(
        "list.sum",
        "sum()",
        "sum()",
        "Sums a numeric list.",
        "items.sum()",
    ),
    method(
        "list.mean",
        "mean()",
        "mean()",
        "Returns the mean of a numeric list, or null for an empty list.",
        "items.mean()",
    ),
    method(
        "list.median",
        "median()",
        "median()",
        "Returns the median of a numeric list, or null for an empty list.",
        "items.median()",
    ),
    method(
        "list.std_dev",
        "std_dev()",
        "std_dev()",
        "Returns the population standard deviation of a numeric list.",
        "items.std_dev()",
    ),
    method(
        "list.zip",
        "zip(other)",
        "zip(${1:other})",
        "Pairs two lists of the same length.",
        "items.zip(other_items)",
    ),
    method(
        "map.keys",
        "keys()",
        "keys()",
        "Returns map keys.",
        "payload.keys()",
    ),
    method(
        "map.values",
        "values()",
        "values()",
        "Returns map values.",
        "payload.values()",
    ),
    method(
        "map.has",
        "has(key)",
        "has(${1:key})",
        "Checks whether a map has a key.",
        "payload.has(\"name\")",
    ),
    method(
        "map.merge",
        "merge(other)",
        "merge(${1:other})",
        "Returns a new map with entries from another map merged in.",
        "payload.merge({ active: true })",
    ),
    method(
        "map.delete",
        "delete(key)",
        "delete(${1:key})",
        "Deletes a key and returns the map.",
        "payload.delete(\"name\")",
    ),
    method(
        "map.size",
        "size()",
        "size()",
        "Returns map entry count.",
        "payload.size()",
    ),
    method(
        "integer.abs",
        "abs()",
        "abs()",
        "Returns the absolute integer value.",
        "amount.abs()",
    ),
    method(
        "integer.min",
        "min(other)",
        "min(${1:other})",
        "Returns the smaller numeric value.",
        "amount.min(10)",
    ),
    method(
        "integer.max",
        "max(other)",
        "max(${1:other})",
        "Returns the larger numeric value.",
        "amount.max(10)",
    ),
    method(
        "integer.to_string",
        "to_string()",
        "to_string()",
        "Converts an integer to string.",
        "amount.to_string()",
    ),
    method(
        "float.abs",
        "abs()",
        "abs()",
        "Returns the absolute float value.",
        "score.abs()",
    ),
    method(
        "float.round",
        "round(places)",
        "round(${1:places})",
        "Rounds a float.",
        "score.round(2)",
    ),
    method(
        "float.floor",
        "floor()",
        "floor()",
        "Rounds a float down.",
        "score.floor()",
    ),
    method(
        "float.ceil",
        "ceil()",
        "ceil()",
        "Rounds a float up.",
        "score.ceil()",
    ),
    method(
        "float.min",
        "min(other)",
        "min(${1:other})",
        "Returns the smaller numeric value.",
        "score.min(10)",
    ),
    method(
        "float.max",
        "max(other)",
        "max(${1:other})",
        "Returns the larger numeric value.",
        "score.max(10)",
    ),
    method(
        "float.to_string",
        "to_string()",
        "to_string()",
        "Converts a float to string.",
        "score.to_string()",
    ),
    method(
        "decimal.round",
        "round(places)",
        "round(${1:places})",
        "Rounds a decimal using banker's rounding.",
        "total.round(2)",
    ),
    method(
        "decimal.abs",
        "abs()",
        "abs()",
        "Returns the absolute decimal value.",
        "total.abs()",
    ),
    method(
        "decimal.trunc",
        "trunc()",
        "trunc()",
        "Truncates a decimal toward zero.",
        "total.trunc()",
    ),
    method(
        "decimal.floor",
        "floor()",
        "floor()",
        "Rounds a decimal down.",
        "total.floor()",
    ),
    method(
        "decimal.ceil",
        "ceil()",
        "ceil()",
        "Rounds a decimal up.",
        "total.ceil()",
    ),
    method(
        "decimal.scale",
        "scale()",
        "scale()",
        "Returns the decimal scale.",
        "total.scale()",
    ),
    method(
        "decimal.to_integer",
        "to_integer()",
        "to_integer()",
        "Converts a decimal to integer by truncating toward zero.",
        "total.to_integer()",
    ),
    method(
        "decimal.to_float",
        "to_float()",
        "to_float()",
        "Converts a decimal to float.",
        "total.to_float()",
    ),
    method(
        "decimal.to_string",
        "to_string()",
        "to_string()",
        "Converts a decimal to string.",
        "total.to_string()",
    ),
];

const fn op(
    name: &'static str,
    signature: &'static str,
    insert_text: &'static str,
    summary: &'static str,
    example: &'static str,
) -> CatalogEntry {
    CatalogEntry {
        name,
        kind: CatalogKind::Function,
        signature,
        insert_text,
        summary,
        example,
        warnings: EMPTY,
    }
}

const fn method(
    name: &'static str,
    signature: &'static str,
    insert_text: &'static str,
    summary: &'static str,
    example: &'static str,
) -> CatalogEntry {
    CatalogEntry {
        name,
        kind: CatalogKind::Method,
        signature,
        insert_text,
        summary,
        example,
        warnings: EMPTY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_exposes_core_namespaces() {
        for name in [
            "db",
            "doc",
            "cache",
            "queue",
            "topic",
            "http_client",
            "time",
            "math",
            "uuid",
            "feature",
            "json",
            "base64",
            "fs",
            "log",
        ] {
            assert!(find_entry(name).is_some(), "missing {name}");
        }
    }

    #[test]
    fn catalog_exposes_runtime_operations() {
        for name in [
            "cache.get",
            "cache.set",
            "uuid.v4",
            "uuid.v7",
            "feature.enabled",
            "http_client.get",
            "http_client.post",
            "json.parse",
            "base64.encode",
            "time.now",
            "math.round",
            "list.mean",
            "list.zip",
            "map.merge",
            "string.index_of",
            "decimal.to_float",
        ] {
            assert!(find_entry(name).is_some(), "missing {name}");
        }
    }

    #[test]
    fn catalog_json_is_versioned() {
        let json = catalog_json();
        assert_eq!(json["version"], 1);
        assert!(json["entries"].as_array().unwrap().len() > 20);
    }
}
