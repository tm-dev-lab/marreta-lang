# 039 — Feature Flags

> Status: Approved
> Type: Language namespace / runtime config
> Scope: Static boolean feature flags read from project configuration

---

## 1. Purpose

This spec introduces a small `feature` namespace:

```marreta
if feature.enabled("inventory_api")
    reply 200, { enabled: true }
else
    fail 404, { error: "not_found" }
```

Feature flags let applications keep small runtime switches in configuration
without inventing ad-hoc `env.MY_FLAG == "true"` checks throughout the codebase.

This is intentionally not a remote flag platform. It is a local, deterministic
runtime feature built on the existing `marreta.env` / process env precedence.

---

## 2. Design Principles

1. **Configuration, not control plane.**
   Flags are loaded from local runtime configuration. No remote SDK, polling,
   percentage rollout, user targeting, or dashboard is introduced.

2. **Boolean only.**
   A flag answers one question: enabled or disabled.

3. **Fail closed.**
   Missing flags are disabled by default.

4. **Stable during process lifetime.**
   Flags are read at startup with the rest of the project environment. They do
   not hot-reload while the process is running.

5. **No hidden behavior.**
   Calling code decides what to do when a flag is enabled or disabled.

---

## 3. User-Facing API

### 3.1 `feature.enabled(name)`

```marreta
feature.enabled("inventory_api")
```

Returns:

- `true` when the named feature is enabled
- `false` when the named feature is disabled or missing

Example:

```marreta
route POST "/inventory/reserve" take payload as ReserveRequest
    if feature.enabled("inventory_v2")
        reply 201, reserve_stock_v2(payload)
    else
        reply 201, reserve_stock(payload)
```

### 3.2 Name Type

`feature.enabled` requires a string:

```marreta
feature.enabled("new_checkout")  # ok
feature.enabled(123)             # runtime error
```

The flag name should use lowercase snake case:

```text
new_checkout
inventory_api
low_stock_alert
```

The runtime should reject invalid names with a clear error.

Allowed name regex:

```text
^[a-z][a-z0-9]*(_[a-z0-9]+)*$
```

Rationale:

- maps deterministically to environment variable names
- avoids case-sensitive ambiguity
- avoids punctuation that differs across shells and deployment systems
- avoids leading, trailing, or repeated underscores so the mapping between flag
  names and env var names stays unambiguous

---

## 4. Configuration

Flags are configured through environment variables using this convention:

```text
MARRETA_FEATURE_<UPPER_SNAKE_NAME>
```

Examples:

```env
MARRETA_FEATURE_INVENTORY_API=true
MARRETA_FEATURE_LOW_STOCK_ALERT=false
MARRETA_FEATURE_NEW_CHECKOUT=1
```

Mapping:

| Code | Environment variable |
| --- | --- |
| `feature.enabled("inventory_api")` | `MARRETA_FEATURE_INVENTORY_API` |
| `feature.enabled("low_stock_alert")` | `MARRETA_FEATURE_LOW_STOCK_ALERT` |
| `feature.enabled("new_checkout")` | `MARRETA_FEATURE_NEW_CHECKOUT` |

The existing config precedence applies:

1. Process environment
2. `marreta.env`
3. Missing value

Process environment overrides `marreta.env`, matching Spec 020.

### 4.1 Accepted Values

Enabled values:

```text
true
1
yes
on
enabled
```

Disabled values:

```text
false
0
no
off
disabled
```

Parsing is case-insensitive and trims surrounding whitespace.

Missing value:

```text
false
```

Invalid configured value is a startup/config error, not `false`.

Example invalid value:

```env
MARRETA_FEATURE_INVENTORY_API=maybe
```

Rationale: silently treating a typo as disabled would make rollout failures hard
to diagnose.

Empty string values are invalid configuration:

```env
MARRETA_FEATURE_INVENTORY_API=
```

Missing means the environment variable is not set at all. Empty means it is set
to an invalid value.

### 4.2 Prefix Ownership

`MARRETA_FEATURE_*` is reserved for application-level feature flags exposed
through `feature.enabled`.

Runtime-internal toggles use other prefixes and are not reachable through this
API:

```env
MARRETA_REQUEST_LOG=true
MARRETA_TRACE_CONTEXT=true
```

This separation is part of the public contract. Application flags and runtime
toggles must not share the same namespace.

---

## 5. Runtime Semantics

### 5.1 Startup Snapshot

Feature flags are parsed once during startup and stored in the project runtime.

All request handlers, tasks, consumers, and scenario tests see the same flag
snapshot for that process.

Changing `marreta.env` or process env after startup has no effect until the
process restarts.

### 5.2 Availability

`feature.enabled` is available anywhere regular language expressions can run:

- routes
- tasks
- queue/topic consumers
- startup code
- scenario tests

Example in a task:

```marreta
task reserve_stock(payload)
    if feature.enabled("strict_inventory")
        reserve_strict(payload)
    else
        reserve_lenient(payload)
```

Example in a consumer:

```marreta
on queue "incoming_shipment" take event as ShipmentEvent
    if feature.enabled("shipment_processing")
        process_shipment(event)
    else
        log.warn({ event: "shipment_processing_disabled", shipment_id: event.shipment_id })
```

### 5.3 No Side Effects

`feature.enabled` is pure:

- no IO
- no network
- no mutation
- no logging
- no metrics emission

It returns a boolean based on the startup snapshot.

---

## 6. Interaction With Existing Features

### 6.1 `env`

Users can already write:

```marreta
if env.MARRETA_FEATURE_INVENTORY_API == "true"
    ...
```

This spec does not remove that capability.

`feature.enabled` is the recommended API because it centralizes:

- naming convention
- boolean parsing
- missing-value behavior
- invalid-value validation

### 6.2 Project Config

Feature flags use the same config loading model as infrastructure settings:

- `marreta.env` in the project root
- process env override
- available in `serve`, `doctor`, `test`, and other project-aware commands

No new config file is introduced.

### 6.3 Doctor

`marreta doctor` should report invalid feature flag values.

Example:

```text
Config:
  ERROR MARRETA_FEATURE_INVENTORY_API has invalid boolean value "maybe"
```

Doctor does not need to list every valid flag by default in the first version.

Doctor should surface the same invalid configuration that would make `serve`
fail at startup. This lets CI catch bad flag values before deployment without
making doctor the only enforcement point.

### 6.4 Scenario Tests

Scenario tests should execute with the same feature snapshot as the test
runtime.

No new scenario syntax is required in this draft.

Tests can use project env/process env to set flags:

```bash
MARRETA_FEATURE_INVENTORY_API=true marreta test
```

Future specs may add scenario-local feature overrides if repeated tests prove
that env-based setup is too coarse.

### 6.5 Runtime Logs

Calling `feature.enabled` does not emit logs.

Application code may log decisions explicitly:

```marreta
if feature.enabled("inventory_v2")
    log.info({ event: "inventory_v2_path" })
```

---

## 7. Examples

### 7.1 Kill Switch

```env
MARRETA_FEATURE_RESERVATION_WRITES=false
```

```marreta
route POST "/inventory/reserve" take payload as ReserveRequest
    require feature.enabled("reservation_writes") else fail 503, {
        error: "temporarily_disabled"
    }

    reply 201, reserve_stock(payload)
```

### 7.2 Migration Between Two Code Paths

```marreta
task calculate_price(order)
    if feature.enabled("new_pricing")
        calculate_price_v2(order)
    else
        calculate_price_v1(order)
```

### 7.3 Consumer Disablement

```marreta
on queue "incoming_shipment" take event as ShipmentEvent
    require feature.enabled("shipment_processing") else nack requeue

    process_shipment(event)
```

### 7.4 Hide Beta Endpoint

```marreta
route GET "/beta/inventory-dashboard"
    require feature.enabled("beta_inventory_dashboard") else fail 404, {
        error: "not_found"
    }

    reply 200, dashboard_data()
```

---

## 8. Non-Goals

This spec does not introduce:

- remote feature flag services
- polling or hot reload
- percentage rollout
- user targeting
- request-context targeting
- per-tenant flag evaluation
- flag variants / string values
- JSON flag payloads
- flag mutation from Marreta code
- scenario-local flag syntax
- a UI or dashboard
- audit history of flag changes

This spec is not a configuration management system. It is a typed accessor over
boolean environment variables.

Any future extension that crosses one of these boundaries would change the
feature class and requires a separate spec with explicit rationale, not a small
extension to this one:

- non-boolean values
- dynamic updates
- remote synchronization
- runtime mutation
- per-request evaluation
- user or tenant targeting

---

## 9. Decisions

1. `feature.enabled(name)` is the only API in this draft.

   Rationale: start with the smallest useful primitive. Additional helpers such
   as `feature.disabled` or `feature.require` would be convenience wrappers.
   They are intentionally omitted because existing Marreta syntax already covers
   them:

   ```marreta
   !feature.enabled("x")
   require feature.enabled("x") else fail 404, { error: "not_found" }
   ```

2. Missing flags are disabled.

   Rationale: fail closed is safer for rollout and kill-switch behavior.

3. Invalid configured flag values are startup/config errors.

   Rationale: typoed flags should fail loudly instead of silently changing
   production behavior.

4. Flags are static for the process lifetime.

   Rationale: hot reload implies watchers, synchronization, and distributed
   consistency questions that are outside the language core.

5. Flags are boolean only.

   Rationale: variants and payloads turn feature flags into remote config. That
   is a different feature class.

6. `MARRETA_FEATURE_*` is reserved for application-level feature flags.

   Rationale: runtime toggles such as `MARRETA_REQUEST_LOG` and
   `MARRETA_TRACE_CONTEXT` already exist and should not be exposed through
   `feature.enabled`. If future runtime-internal flags are needed, they should
   use a separate prefix such as `MARRETA_INTERNAL_*`.

---

## 10. Watch Points

1. **Scenario ergonomics.**
   If tests need many flag combinations, env-based setup may become awkward.
   Future syntax could introduce scenario-local flag overrides.

2. **Doctor visibility.**
   The first version only needs to report invalid values. If operators need a
   full flag inventory, `doctor` can grow a feature flag summary later.

3. **Strict naming.**
   The first version rejects leading, trailing, and repeated underscores. If
   real projects find that too restrictive, revisit with concrete examples
   rather than loosening the mapping pre-emptively.

---

## 11. Implementation Sketch

Config:

- scan merged config/environment for keys with prefix `MARRETA_FEATURE_`
- parse boolean values strictly
- normalize keys to lowercase snake case
- store `HashMap<String, bool>` in runtime config

Interpreter:

- add reserved namespace `feature`
- implement `feature.enabled(name)`
- validate argument count and type
- validate flag name regex
- return `Value::Boolean`

Doctor:

- report invalid `MARRETA_FEATURE_*` values
- no need to list valid flags in v1

Testing:

- unit tests for boolean parsing
- unit tests for name normalization and invalid names
- interpreter tests for enabled, disabled, missing, non-string argument
- config/doctor tests for invalid value
- functional test showing a route gated by a flag
- scenario test showing flag access inside `marreta test`

---

## 12. Success Criteria

- `feature.enabled("name")` returns true/false consistently.
- Missing flags return false.
- Process env overrides `marreta.env`.
- Invalid configured values fail loudly.
- Flags work in routes, tasks, consumers, and scenario tests.
- No remote service, hot reload, rollout engine, or targeting system is added.
