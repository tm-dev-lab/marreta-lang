/// Schema payload validator for MarretaLang v0.4.0.
///
/// Validates a `Value::Map` against a `SchemaDefinition`, with support for
/// nested schema references (`Reference`) and typed lists (`TypedList`).
/// On violation, returns `MarretaError::HttpResponse` with status 422.
///
/// Error paths are accumulative: `"field 'billing.city' is required"`.
use std::collections::HashMap;

use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, NaiveTime, Utc};
use rust_decimal::Decimal;

use std::sync::{Arc, RwLock};

use crate::ast::SchemaType;
use crate::error::MarretaError;
use crate::route_loader::SchemaDefinition;
use crate::value::{TemporalInterval, TemporalValue, Value, ValueMap};

/// Maximum recursion depth — guards against circular schemas that slip past
/// the startup cycle detection.
const MAX_DEPTH: usize = 20;

/// Validates `payload` against `schema`.
///
/// `schemas` is the full registry, used to resolve `SchemaType::Reference` fields.
/// Pass an empty map if no cross-schema references are expected (e.g. in unit tests
/// with flat schemas).
///
/// Returns `Ok(())` on success, `Err(422)` on the first violation.
pub fn validate_payload(
    payload: &Value,
    schema: &SchemaDefinition,
    schemas: &HashMap<String, SchemaDefinition>,
) -> Result<(), MarretaError> {
    coerce_payload(payload, schema, schemas).map(|_| ())
}

pub fn coerce_payload(
    payload: &Value,
    schema: &SchemaDefinition,
    schemas: &HashMap<String, SchemaDefinition>,
) -> Result<Value, MarretaError> {
    coerce_recursive(payload, schema, schemas, "", 0, &mut Vec::new())
}

fn coerce_recursive(
    payload: &Value,
    schema: &SchemaDefinition,
    schemas: &HashMap<String, SchemaDefinition>,
    path_prefix: &str,
    depth: usize,
    // Schema names currently being validated up the recursion stack (Spec 062). Lets the
    // coercer recognise a reference that closes a cycle: a bidirectional `db:` relation
    // is let through (it is a relation, not an embedded value), while a value-schema
    // cycle is the genuine infinite embed.
    on_path: &mut Vec<String>,
) -> Result<Value, MarretaError> {
    if depth > MAX_DEPTH {
        return Err(error_422(
            "schema validation exceeded maximum nesting depth",
        ));
    }

    let map = match payload {
        Value::Map(m) => m.read().expect("validator: RwLock poisoned"),
        Value::Null => {
            for field in &schema.fields {
                if !field.optional {
                    let path = field_path(path_prefix, &field.name);
                    return Err(error_422(&format!(
                        "field '{}' is required but payload is null or missing",
                        path
                    )));
                }
            }
            return Ok(Value::Null);
        }
        _ => {
            return Err(error_422("payload must be a JSON object"));
        }
    };

    let mut out = map.clone();

    for field in &schema.fields {
        let path = field_path(path_prefix, &field.name);
        match map.get(&field.name) {
            None => {
                if !field.optional {
                    return Err(error_422(&format!("field '{}' is required", path)));
                }
            }
            Some(Value::Null) => {
                if !field.optional {
                    return Err(error_422(&format!(
                        "field '{}' is required (got null)",
                        path
                    )));
                }
                out.insert(field.name.clone(), Value::Null);
            }
            Some(val) => {
                let coerced =
                    coerce_field_type(val, &field.field_type, &path, schemas, depth, on_path)?;
                out.insert(field.name.clone(), coerced);
            }
        }
    }

    Ok(Value::Map(std::sync::Arc::new(std::sync::RwLock::new(out))))
}

fn coerce_field_type(
    val: &Value,
    expected: &SchemaType,
    path: &str,
    schemas: &HashMap<String, SchemaDefinition>,
    depth: usize,
    on_path: &mut Vec<String>,
) -> Result<Value, MarretaError> {
    match expected {
        SchemaType::StringType => expect_direct_type(val, expected, path),
        SchemaType::IntegerType => expect_direct_type(val, expected, path),
        SchemaType::FloatType => expect_direct_type(val, expected, path),
        SchemaType::DecimalType => coerce_decimal(val, path),
        SchemaType::BooleanType => expect_direct_type(val, expected, path),
        SchemaType::ListType => expect_direct_type(val, expected, path),
        SchemaType::MapType => expect_direct_type(val, expected, path),
        SchemaType::InstantType => coerce_instant(val, path),
        SchemaType::DateType => coerce_date(val, path),
        SchemaType::TimeType => coerce_time(val, path),
        SchemaType::DurationType => coerce_duration(val, path),
        SchemaType::IntervalType => coerce_interval(val, path),
        SchemaType::EnumType(values) => coerce_enum(val, values, path),

        // Nested schema reference: `billing: address`
        SchemaType::Reference(schema_name) => {
            // Spec 062: a reference to a persistent (`db:`) schema is a foreign-key
            // relation (Spec 025), not an embedded value — accept it as-is rather than
            // validating recursively. This lets a persistent schema work as an API
            // contract even when it participates in a (bidirectional) relation cycle:
            // the relation edge is let through instead of looping.
            if schemas
                .get(schema_name)
                .is_some_and(|schema| schema.db_table.is_some())
            {
                return Ok(val.clone());
            }
            // A value-schema reference is an embedded value: validate it recursively, but
            // recognise a value cycle (the target is already on the validation stack) as
            // the genuine infinite embed (also rejected at load).
            if on_path.iter().any(|name| name == schema_name) {
                return Err(error_422(&format!(
                    "field '{}' forms an infinite schema reference cycle via '{}'",
                    path, schema_name
                )));
            }
            match schemas.get(schema_name) {
                Some(nested_schema) => {
                    on_path.push(schema_name.clone());
                    let result =
                        coerce_recursive(val, nested_schema, schemas, path, depth + 1, on_path);
                    on_path.pop();
                    result
                }
                None => {
                    // Unknown schema name — treat as a configuration error
                    Err(error_422(&format!(
                        "field '{}' references unknown schema '{}'",
                        path, schema_name
                    )))
                }
            }
        }

        // Typed list: `items: list of order_item` or `tags: list of string`
        SchemaType::TypedList(inner) => {
            let list = match val {
                Value::List(l) => l,
                _ => {
                    return Err(error_422(&format!(
                        "field '{}' expected array, got {}",
                        path,
                        val.type_name()
                    )));
                }
            };
            let mut out = Vec::with_capacity(list.len());
            for (i, elem) in list.iter().enumerate() {
                let elem_path = format!("{}[{}]", path, i);
                out.push(coerce_field_type(
                    elem, inner, &elem_path, schemas, depth, on_path,
                )?);
            }
            Ok(Value::List(out))
        }
    }
}

fn expect_direct_type(
    val: &Value,
    expected: &SchemaType,
    path: &str,
) -> Result<Value, MarretaError> {
    let matches = matches!(
        (val, expected),
        (Value::String(_), SchemaType::StringType)
            | (Value::Integer(_), SchemaType::IntegerType)
            | (Value::Float(_), SchemaType::FloatType)
            | (Value::Boolean(_), SchemaType::BooleanType)
            | (Value::List(_), SchemaType::ListType)
            | (Value::Map(_), SchemaType::MapType)
    );

    if matches {
        Ok(val.clone())
    } else {
        Err(error_422(&format!(
            "field '{}' expected {}, got {}",
            path,
            expected,
            val.type_name()
        )))
    }
}

fn coerce_enum(val: &Value, values: &[String], path: &str) -> Result<Value, MarretaError> {
    match val {
        Value::String(s) if values.iter().any(|value| value == s) => Ok(val.clone()),
        Value::String(s) => Err(error_422(&format!(
            "field '{}' expected one of [{}], got '{}'",
            path,
            values.join(", "),
            s
        ))),
        _ => Err(error_422(&format!(
            "field '{}' expected enum string, got {}",
            path,
            val.type_name()
        ))),
    }
}

fn coerce_decimal(val: &Value, path: &str) -> Result<Value, MarretaError> {
    match val {
        Value::Decimal(_) => Ok(val.clone()),
        Value::Integer(n) => Ok(Value::Decimal(Decimal::from(*n))),
        Value::String(s) if !s.contains('e') && !s.contains('E') => s
            .parse::<Decimal>()
            .map(Value::Decimal)
            .map_err(|_| error_422(&format!("field '{}' expected decimal string", path))),
        Value::String(_) => Err(error_422(&format!(
            "field '{}' expected plain decimal string, got scientific notation",
            path
        ))),
        _ => Err(error_422(&format!(
            "field '{}' expected decimal, got {}",
            path,
            val.type_name()
        ))),
    }
}

fn coerce_instant(val: &Value, path: &str) -> Result<Value, MarretaError> {
    match val {
        Value::Instant(_) => Ok(val.clone()),
        Value::String(s) => DateTime::parse_from_rfc3339(s)
            .map(|dt| Value::Instant(dt.with_timezone(&Utc)))
            .map_err(|_| error_422(&format!("field '{}' expected instant RFC3339 string", path))),
        _ => Err(error_422(&format!(
            "field '{}' expected instant, got {}",
            path,
            val.type_name()
        ))),
    }
}

fn coerce_date(val: &Value, path: &str) -> Result<Value, MarretaError> {
    match val {
        Value::Date(_) => Ok(val.clone()),
        Value::String(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(Value::Date)
            .map_err(|_| error_422(&format!("field '{}' expected date YYYY-MM-DD string", path))),
        _ => Err(error_422(&format!(
            "field '{}' expected date, got {}",
            path,
            val.type_name()
        ))),
    }
}

fn coerce_time(val: &Value, path: &str) -> Result<Value, MarretaError> {
    match val {
        Value::Time(_) => Ok(val.clone()),
        Value::String(s) => NaiveTime::parse_from_str(s, "%H:%M:%S")
            .map(Value::Time)
            .map_err(|_| error_422(&format!("field '{}' expected time HH:MM:SS string", path))),
        _ => Err(error_422(&format!(
            "field '{}' expected time, got {}",
            path,
            val.type_name()
        ))),
    }
}

fn coerce_duration(val: &Value, path: &str) -> Result<Value, MarretaError> {
    match val {
        Value::Duration(_) => Ok(val.clone()),
        Value::Integer(ms) => Ok(Value::Duration(ChronoDuration::milliseconds(*ms))),
        Value::Float(ms) => Ok(Value::Duration(ChronoDuration::milliseconds(*ms as i64))),
        Value::String(s) => parse_duration_string(s).map(Value::Duration).map_err(|_| {
            error_422(&format!(
                "field '{}' expected duration string like PT3600S",
                path
            ))
        }),
        _ => Err(error_422(&format!(
            "field '{}' expected duration, got {}",
            path,
            val.type_name()
        ))),
    }
}

fn coerce_interval(val: &Value, path: &str) -> Result<Value, MarretaError> {
    match val {
        Value::Interval(_) => Ok(val.clone()),
        Value::Map(map) => {
            let guard = map.read().unwrap();
            let start = guard
                .get("start")
                .ok_or_else(|| error_422(&format!("field '{}.start' is required", path)))?;
            let end = guard
                .get("end")
                .ok_or_else(|| error_422(&format!("field '{}.end' is required", path)))?;

            let start = coerce_temporal_component(start, &format!("{}.start", path))?;
            let end = coerce_temporal_component(end, &format!("{}.end", path))?;
            ensure_same_temporal_component_kind(&start, &end, path)?;
            Ok(Value::Interval(TemporalInterval { start, end }))
        }
        _ => Err(error_422(&format!(
            "field '{}' expected interval object, got {}",
            path,
            val.type_name()
        ))),
    }
}

fn coerce_temporal_component(val: &Value, path: &str) -> Result<TemporalValue, MarretaError> {
    match val {
        Value::Instant(dt) => Ok(TemporalValue::Instant(*dt)),
        Value::Date(date) => Ok(TemporalValue::Date(*date)),
        Value::Time(time) => Ok(TemporalValue::Time(*time)),
        Value::String(s) => {
            if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
                return Ok(TemporalValue::Instant(dt.with_timezone(&Utc)));
            }
            if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                return Ok(TemporalValue::Date(date));
            }
            if let Ok(time) = NaiveTime::parse_from_str(s, "%H:%M:%S") {
                return Ok(TemporalValue::Time(time));
            }
            Err(error_422(&format!(
                "field '{}' expected temporal string",
                path
            )))
        }
        _ => Err(error_422(&format!(
            "field '{}' expected temporal value, got {}",
            path,
            val.type_name()
        ))),
    }
}

fn ensure_same_temporal_component_kind(
    start: &TemporalValue,
    end: &TemporalValue,
    path: &str,
) -> Result<(), MarretaError> {
    let same = matches!(
        (start, end),
        (TemporalValue::Instant(_), TemporalValue::Instant(_))
            | (TemporalValue::Date(_), TemporalValue::Date(_))
            | (TemporalValue::Time(_), TemporalValue::Time(_))
    );
    if same {
        Ok(())
    } else {
        Err(error_422(&format!(
            "field '{}' interval start and end must share the same temporal type",
            path
        )))
    }
}

fn parse_duration_string(input: &str) -> Result<ChronoDuration, ()> {
    let seconds = input
        .strip_prefix("PT")
        .and_then(|s| s.strip_suffix('S'))
        .ok_or(())?
        .parse::<f64>()
        .map_err(|_| ())?;
    Ok(ChronoDuration::milliseconds(
        (seconds * 1000.0).round() as i64
    ))
}

/// Builds a dotted path string from a prefix and a field name.
/// `""` + `"city"` → `"city"`, `"billing"` + `"city"` → `"billing.city"`
fn field_path(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{}.{}", prefix, name)
    }
}

fn error_422(msg: &str) -> MarretaError {
    MarretaError::HttpResponse {
        status_code: 422,
        body: serde_json::json!({ "error": msg }).to_string(),
        content_type: "application/json".into(),
        extra_headers: vec![],
        is_error: true,
    }
}

// ── Spec 077: query/header input schemas ────────────────────────────────────────────────────────

/// Canonical form of a header/field name so a schema field matches a header case-insensitively with
/// `_` and `-` treated as equivalent: `request_id` and `X-Request-Id` both canonicalize to
/// `request_id`.
fn canonical_header_key(name: &str) -> String {
    name.to_ascii_lowercase().replace('-', "_")
}

/// Coerce one raw text value into a declared scalar type. A value that cannot be coerced is a 422.
/// Reuses the existing scalar coercers (decimal/temporal/enum) by wrapping the text as a string.
fn coerce_scalar_string(s: &str, ty: &SchemaType, path: &str) -> Result<Value, MarretaError> {
    match ty {
        SchemaType::StringType => Ok(Value::String(s.to_string())),
        SchemaType::IntegerType => s
            .parse::<i64>()
            .map(Value::Integer)
            .map_err(|_| error_422(&format!("field '{}' must be an integer, got '{}'", path, s))),
        SchemaType::FloatType => s
            .parse::<f64>()
            .map(Value::Float)
            .map_err(|_| error_422(&format!("field '{}' must be a number, got '{}'", path, s))),
        SchemaType::BooleanType => match s {
            "true" => Ok(Value::Boolean(true)),
            "false" => Ok(Value::Boolean(false)),
            _ => Err(error_422(&format!(
                "field '{}' must be true or false, got '{}'",
                path, s
            ))),
        },
        SchemaType::DecimalType => coerce_decimal(&Value::String(s.to_string()), path),
        SchemaType::InstantType => coerce_instant(&Value::String(s.to_string()), path),
        SchemaType::DateType => coerce_date(&Value::String(s.to_string()), path),
        SchemaType::TimeType => coerce_time(&Value::String(s.to_string()), path),
        SchemaType::DurationType => coerce_duration(&Value::String(s.to_string()), path),
        SchemaType::IntervalType => coerce_interval(&Value::String(s.to_string()), path),
        SchemaType::EnumType(values) => coerce_enum(&Value::String(s.to_string()), values, path),
        // Non-scalar types are rejected at load by the flat-only check; this is defensive.
        SchemaType::Reference(_)
        | SchemaType::TypedList(_)
        | SchemaType::ListType
        | SchemaType::MapType => Err(error_422(&format!(
            "field '{}' has a type that cannot be read from query or headers",
            path
        ))),
    }
}

/// Validate and coerce a flat text-input map (query string or headers) against `schema`. Inputs
/// arrive as text, each name carrying all its raw values (query/headers may repeat). Empty values
/// are treated as absent. Only declared fields are bound; a missing required field is a 422. When
/// `header_names` is set, field names match input keys by the case-insensitive `_`/`-` convention.
pub fn coerce_scalar_input(
    inputs: &HashMap<String, Vec<String>>,
    schema: &SchemaDefinition,
    header_names: bool,
) -> Result<Value, MarretaError> {
    let normalized: HashMap<String, &Vec<String>> = if header_names {
        inputs
            .iter()
            .map(|(k, v)| (canonical_header_key(k), v))
            .collect()
    } else {
        inputs.iter().map(|(k, v)| (k.clone(), v)).collect()
    };
    let lookup = |name: &str| -> Option<&Vec<String>> {
        if header_names {
            normalized.get(&canonical_header_key(name)).copied()
        } else {
            normalized.get(name).copied()
        }
    };

    let mut out = ValueMap::new();
    for field in &schema.fields {
        // Empty values are treated as absent (Spec 077).
        let present: Vec<&String> = lookup(&field.name)
            .map(|vals| vals.iter().filter(|s| !s.is_empty()).collect())
            .unwrap_or_default();

        match &field.field_type {
            SchemaType::TypedList(inner) => {
                if present.is_empty() {
                    if !field.optional {
                        return Err(error_422(&format!("field '{}' is required", field.name)));
                    }
                    continue;
                }
                let mut items = Vec::with_capacity(present.len());
                for s in &present {
                    items.push(coerce_scalar_string(s, inner, &field.name)?);
                }
                out.insert(field.name.clone(), Value::List(items));
            }
            SchemaType::ListType => {
                if present.is_empty() {
                    if !field.optional {
                        return Err(error_422(&format!("field '{}' is required", field.name)));
                    }
                    continue;
                }
                let items = present
                    .iter()
                    .map(|s| Value::String((*s).clone()))
                    .collect();
                out.insert(field.name.clone(), Value::List(items));
            }
            scalar => match present.first() {
                None => {
                    if !field.optional {
                        return Err(error_422(&format!("field '{}' is required", field.name)));
                    }
                }
                Some(s) => {
                    out.insert(
                        field.name.clone(),
                        coerce_scalar_string(s, scalar, &field.name)?,
                    );
                }
            },
        }
    }
    Ok(Value::Map(Arc::new(RwLock::new(out))))
}

/// Whether a schema is flat enough to bind to query/headers (Spec 077): only scalar fields and
/// `list of <scalar>` (and the untyped `list`). A nested object (schema reference), a
/// `list of <Schema>`, or a `map` is not flat. Returns the first offending field name. Used at load
/// (the binding-site error) and by the lint.
pub fn first_non_flat_field(schema: &SchemaDefinition) -> Option<&str> {
    fn is_scalar(ty: &SchemaType) -> bool {
        matches!(
            ty,
            SchemaType::StringType
                | SchemaType::IntegerType
                | SchemaType::FloatType
                | SchemaType::DecimalType
                | SchemaType::BooleanType
                | SchemaType::InstantType
                | SchemaType::DateType
                | SchemaType::TimeType
                | SchemaType::DurationType
                | SchemaType::IntervalType
                | SchemaType::EnumType(_)
        )
    }
    schema
        .fields
        .iter()
        .find(|field| match &field.field_type {
            SchemaType::ListType => false,
            t if is_scalar(t) => false,
            SchemaType::TypedList(inner) => !is_scalar(inner),
            _ => true, // Reference, MapType, nested
        })
        .map(|field| field.name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{SchemaField, SchemaType};
    use crate::route_loader::SchemaDefinition;
    use crate::value::{Value, ValueMap};
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};

    fn schema(fields: Vec<SchemaField>) -> SchemaDefinition {
        SchemaDefinition {
            db_table: None,
            fields,
        }
    }

    fn field(name: &str, t: SchemaType, optional: bool) -> SchemaField {
        SchemaField {
            name: name.into(),
            field_type: t,
            optional,
        }
    }

    fn map_value(pairs: Vec<(&str, Value)>) -> Value {
        let m: ValueMap = pairs.into_iter().map(|(k, v)| (k.into(), v)).collect();
        Value::Map(Arc::new(RwLock::new(m)))
    }

    fn no_schemas() -> HashMap<String, SchemaDefinition> {
        HashMap::new()
    }

    // --- Spec 077: query/header input coercion + flat check ---

    fn inputs(pairs: Vec<(&str, Vec<&str>)>) -> HashMap<String, Vec<String>> {
        pairs
            .into_iter()
            .map(|(k, vs)| (k.to_string(), vs.into_iter().map(String::from).collect()))
            .collect()
    }

    fn get_field(v: &Value, name: &str) -> Option<Value> {
        if let Value::Map(m) = v {
            m.read().unwrap().get(name).cloned()
        } else {
            None
        }
    }

    #[test]
    fn test_query_coerces_scalars() {
        let s = schema(vec![
            field("term", SchemaType::StringType, false),
            field("limit", SchemaType::IntegerType, true),
            field("active", SchemaType::BooleanType, true),
        ]);
        let v = coerce_scalar_input(
            &inputs(vec![
                ("term", vec!["hi"]),
                ("limit", vec!["20"]),
                ("active", vec!["true"]),
            ]),
            &s,
            false,
        )
        .unwrap();
        assert_eq!(get_field(&v, "term"), Some(Value::String("hi".into())));
        assert_eq!(get_field(&v, "limit"), Some(Value::Integer(20)));
        assert_eq!(get_field(&v, "active"), Some(Value::Boolean(true)));
    }

    #[test]
    fn test_query_bad_integer_is_422() {
        let s = schema(vec![field("limit", SchemaType::IntegerType, false)]);
        assert!(coerce_scalar_input(&inputs(vec![("limit", vec!["abc"])]), &s, false).is_err());
    }

    #[test]
    fn test_query_boolean_only_true_false() {
        let s = schema(vec![field("active", SchemaType::BooleanType, false)]);
        assert!(coerce_scalar_input(&inputs(vec![("active", vec!["1"])]), &s, false).is_err());
        assert!(coerce_scalar_input(&inputs(vec![("active", vec!["true"])]), &s, false).is_ok());
    }

    #[test]
    fn test_query_required_missing_is_422() {
        let s = schema(vec![field("term", SchemaType::StringType, false)]);
        assert!(coerce_scalar_input(&inputs(vec![]), &s, false).is_err());
    }

    #[test]
    fn test_query_empty_value_is_absent() {
        // Optional + empty -> absent (not bound).
        let s = schema(vec![field("term", SchemaType::StringType, true)]);
        let v = coerce_scalar_input(&inputs(vec![("term", vec![""])]), &s, false).unwrap();
        assert_eq!(get_field(&v, "term"), None);
        // Required + empty -> 422 (empty is absent).
        let s2 = schema(vec![field("term", SchemaType::StringType, false)]);
        assert!(coerce_scalar_input(&inputs(vec![("term", vec![""])]), &s2, false).is_err());
    }

    #[test]
    fn test_query_list_from_repeated_key() {
        let s = schema(vec![field(
            "tags",
            SchemaType::TypedList(Box::new(SchemaType::StringType)),
            true,
        )]);
        let v = coerce_scalar_input(&inputs(vec![("tags", vec!["a", "b"])]), &s, false).unwrap();
        assert_eq!(
            get_field(&v, "tags"),
            Some(Value::List(vec![
                Value::String("a".into()),
                Value::String("b".into()),
            ]))
        );
    }

    #[test]
    fn test_header_name_mapping_convention() {
        // Convention: case-insensitive, `_` matches `-`. So `x_request_id` matches `X-Request-Id`,
        // and `request_id` matches `Request-Id`.
        let s = schema(vec![field("x_request_id", SchemaType::StringType, false)]);
        let v =
            coerce_scalar_input(&inputs(vec![("X-Request-Id", vec!["abc"])]), &s, true).unwrap();
        assert_eq!(
            get_field(&v, "x_request_id"),
            Some(Value::String("abc".into()))
        );

        let s2 = schema(vec![field("request_id", SchemaType::StringType, false)]);
        let v2 = coerce_scalar_input(&inputs(vec![("Request-Id", vec!["z"])]), &s2, true).unwrap();
        assert_eq!(
            get_field(&v2, "request_id"),
            Some(Value::String("z".into()))
        );
    }

    #[test]
    fn test_first_non_flat_field() {
        let flat = schema(vec![
            field("a", SchemaType::StringType, false),
            field(
                "b",
                SchemaType::TypedList(Box::new(SchemaType::IntegerType)),
                true,
            ),
        ]);
        assert_eq!(first_non_flat_field(&flat), None);

        let nested = schema(vec![field(
            "addr",
            SchemaType::Reference("Address".into()),
            false,
        )]);
        assert_eq!(first_non_flat_field(&nested), Some("addr"));

        let list_of_schema = schema(vec![field(
            "items",
            SchemaType::TypedList(Box::new(SchemaType::Reference("Item".into()))),
            false,
        )]);
        assert_eq!(first_non_flat_field(&list_of_schema), Some("items"));
    }

    // --- Existing flat validation tests (unchanged behaviour) ---

    #[test]
    fn test_valid_payload_all_required() {
        let s = schema(vec![
            field("name", SchemaType::StringType, false),
            field("age", SchemaType::IntegerType, false),
        ]);
        let payload = map_value(vec![
            ("name", Value::String("Ana".into())),
            ("age", Value::Integer(30)),
        ]);
        assert!(validate_payload(&payload, &s, &no_schemas()).is_ok());
    }

    #[test]
    fn test_missing_required_field_returns_422() {
        let s = schema(vec![
            field("name", SchemaType::StringType, false),
            field("age", SchemaType::IntegerType, false),
        ]);
        let payload = map_value(vec![("name", Value::String("Ana".into()))]);
        let err = validate_payload(&payload, &s, &no_schemas()).unwrap_err();
        match err {
            MarretaError::HttpResponse {
                status_code, body, ..
            } => {
                assert_eq!(status_code, 422);
                assert!(body.contains("age"));
                assert!(body.contains("required"));
            }
            _ => panic!("expected HttpResponse"),
        }
    }

    #[test]
    fn test_wrong_type_returns_422() {
        let s = schema(vec![field("age", SchemaType::IntegerType, false)]);
        let payload = map_value(vec![("age", Value::String("thirty".into()))]);
        let err = validate_payload(&payload, &s, &no_schemas()).unwrap_err();
        match err {
            MarretaError::HttpResponse {
                status_code, body, ..
            } => {
                assert_eq!(status_code, 422);
                assert!(body.contains("age"));
                assert!(body.contains("integer"));
                assert!(body.contains("String"));
            }
            _ => panic!("expected HttpResponse"),
        }
    }

    #[test]
    fn test_enum_accepts_declared_string_value() {
        let s = schema(vec![field(
            "status",
            SchemaType::EnumType(vec!["pending".into(), "paid".into()]),
            false,
        )]);
        let payload = map_value(vec![("status", Value::String("paid".into()))]);
        assert!(validate_payload(&payload, &s, &no_schemas()).is_ok());
    }

    #[test]
    fn test_enum_rejects_unknown_string_value() {
        let s = schema(vec![field(
            "status",
            SchemaType::EnumType(vec!["pending".into(), "paid".into()]),
            false,
        )]);
        let payload = map_value(vec![("status", Value::String("failed".into()))]);
        let err = validate_payload(&payload, &s, &no_schemas()).unwrap_err();
        assert!(matches!(
            err,
            MarretaError::HttpResponse {
                status_code: 422,
                ..
            }
        ));
    }

    #[test]
    fn test_decimal_accepts_string_and_integer_but_rejects_float() {
        let s = schema(vec![field("amount", SchemaType::DecimalType, false)]);
        let from_string = coerce_payload(
            &map_value(vec![("amount", Value::String("19.90".into()))]),
            &s,
            &no_schemas(),
        )
        .unwrap();
        if let Value::Map(map) = from_string {
            assert!(matches!(
                map.read().unwrap().get("amount"),
                Some(Value::Decimal(_))
            ));
        } else {
            panic!("expected Map");
        }

        assert!(
            validate_payload(
                &map_value(vec![("amount", Value::Integer(20))]),
                &s,
                &no_schemas()
            )
            .is_ok()
        );
        assert!(
            validate_payload(
                &map_value(vec![("amount", Value::Float(19.90))]),
                &s,
                &no_schemas()
            )
            .is_err()
        );
        assert!(
            validate_payload(
                &map_value(vec![("amount", Value::String("1e3".into()))]),
                &s,
                &no_schemas()
            )
            .is_err()
        );
    }

    #[test]
    fn test_optional_field_absent_is_ok() {
        let s = schema(vec![
            field("name", SchemaType::StringType, false),
            field("email", SchemaType::StringType, true),
        ]);
        let payload = map_value(vec![("name", Value::String("Ana".into()))]);
        assert!(validate_payload(&payload, &s, &no_schemas()).is_ok());
    }

    #[test]
    fn test_optional_field_null_is_ok() {
        let s = schema(vec![field("email", SchemaType::StringType, true)]);
        let payload = map_value(vec![("email", Value::Null)]);
        assert!(validate_payload(&payload, &s, &no_schemas()).is_ok());
    }

    #[test]
    fn test_required_field_null_returns_422() {
        let s = schema(vec![field("name", SchemaType::StringType, false)]);
        let payload = map_value(vec![("name", Value::Null)]);
        let err = validate_payload(&payload, &s, &no_schemas()).unwrap_err();
        assert!(matches!(
            err,
            MarretaError::HttpResponse {
                status_code: 422,
                ..
            }
        ));
    }

    #[test]
    fn test_null_payload_with_required_fields_returns_422() {
        let s = schema(vec![field("name", SchemaType::StringType, false)]);
        let err = validate_payload(&Value::Null, &s, &no_schemas()).unwrap_err();
        assert!(matches!(
            err,
            MarretaError::HttpResponse {
                status_code: 422,
                ..
            }
        ));
    }

    #[test]
    fn test_null_payload_all_optional_is_ok() {
        let s = schema(vec![field("email", SchemaType::StringType, true)]);
        assert!(validate_payload(&Value::Null, &s, &no_schemas()).is_ok());
    }

    #[test]
    fn test_non_map_payload_returns_422() {
        let s = schema(vec![field("name", SchemaType::StringType, false)]);
        let err =
            validate_payload(&Value::String("notanobject".into()), &s, &no_schemas()).unwrap_err();
        assert!(matches!(
            err,
            MarretaError::HttpResponse {
                status_code: 422,
                ..
            }
        ));
    }

    #[test]
    fn test_all_schema_types_match() {
        let s = schema(vec![
            field("s", SchemaType::StringType, false),
            field("i", SchemaType::IntegerType, false),
            field("f", SchemaType::FloatType, false),
            field("b", SchemaType::BooleanType, false),
        ]);
        let payload = map_value(vec![
            ("s", Value::String("hello".into())),
            ("i", Value::Integer(1)),
            ("f", Value::Float(1.5)),
            ("b", Value::Boolean(true)),
        ]);
        assert!(validate_payload(&payload, &s, &no_schemas()).is_ok());
    }

    #[test]
    fn test_extra_fields_in_payload_are_ignored() {
        let s = schema(vec![field("name", SchemaType::StringType, false)]);
        let payload = map_value(vec![
            ("name", Value::String("Ana".into())),
            ("unexpected", Value::Integer(99)),
        ]);
        assert!(validate_payload(&payload, &s, &no_schemas()).is_ok());
    }

    #[test]
    fn test_error_body_is_valid_json_with_error_key() {
        let s = schema(vec![field("age", SchemaType::IntegerType, false)]);
        let payload = map_value(vec![("age", Value::Boolean(true))]);
        let err = validate_payload(&payload, &s, &no_schemas()).unwrap_err();
        if let MarretaError::HttpResponse { body, .. } = err {
            let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
            assert!(parsed.get("error").is_some());
        }
    }

    // --- v0.4.0: Nested schema reference tests ---

    #[test]
    fn test_nested_schema_reference_valid() {
        let address_schema = schema(vec![
            field("street", SchemaType::StringType, false),
            field("city", SchemaType::StringType, false),
        ]);
        let user_schema = schema(vec![
            field("name", SchemaType::StringType, false),
            field("billing", SchemaType::Reference("address".into()), false),
        ]);
        let mut schemas = HashMap::new();
        schemas.insert("address".into(), address_schema);

        let payload = map_value(vec![
            ("name", Value::String("Ana".into())),
            (
                "billing",
                map_value(vec![
                    ("street", Value::String("Rua das Flores, 10".into())),
                    ("city", Value::String("São Paulo".into())),
                ]),
            ),
        ]);
        assert!(validate_payload(&payload, &user_schema, &schemas).is_ok());
    }

    #[test]
    fn test_nested_schema_missing_required_field_shows_dotted_path() {
        let address_schema = schema(vec![
            field("street", SchemaType::StringType, false),
            field("city", SchemaType::StringType, false),
        ]);
        let user_schema = schema(vec![
            field("name", SchemaType::StringType, false),
            field("billing", SchemaType::Reference("address".into()), false),
        ]);
        let mut schemas = HashMap::new();
        schemas.insert("address".into(), address_schema);

        // billing.city is missing
        let payload = map_value(vec![
            ("name", Value::String("Ana".into())),
            (
                "billing",
                map_value(vec![("street", Value::String("Rua das Flores, 10".into()))]),
            ),
        ]);
        let err = validate_payload(&payload, &user_schema, &schemas).unwrap_err();
        if let MarretaError::HttpResponse { body, .. } = err {
            assert!(
                body.contains("billing.city"),
                "expected path billing.city in: {}",
                body
            );
        } else {
            panic!("expected HttpResponse");
        }
    }

    #[test]
    fn test_nested_schema_wrong_type_shows_dotted_path() {
        let address_schema = schema(vec![field("city", SchemaType::StringType, false)]);
        let user_schema = schema(vec![
            field("name", SchemaType::StringType, false),
            field("billing", SchemaType::Reference("address".into()), false),
        ]);
        let mut schemas = HashMap::new();
        schemas.insert("address".into(), address_schema);

        let payload = map_value(vec![
            ("name", Value::String("Ana".into())),
            ("billing", map_value(vec![("city", Value::Integer(123))])),
        ]);
        let err = validate_payload(&payload, &user_schema, &schemas).unwrap_err();
        if let MarretaError::HttpResponse { body, .. } = err {
            assert!(
                body.contains("billing.city"),
                "expected path billing.city in: {}",
                body
            );
        } else {
            panic!("expected HttpResponse");
        }
    }

    #[test]
    fn test_unknown_schema_reference_returns_422() {
        let user_schema = schema(vec![field(
            "billing",
            SchemaType::Reference("nonexistent".into()),
            false,
        )]);
        let payload = map_value(vec![(
            "billing",
            map_value(vec![("street", Value::String("x".into()))]),
        )]);
        let err = validate_payload(&payload, &user_schema, &no_schemas()).unwrap_err();
        if let MarretaError::HttpResponse { body, .. } = err {
            assert!(body.contains("nonexistent"));
        } else {
            panic!("expected HttpResponse");
        }
    }

    // --- v0.4.0: TypedList tests ---

    #[test]
    fn test_typed_list_of_strings_valid() {
        let s = schema(vec![field(
            "tags",
            SchemaType::TypedList(Box::new(SchemaType::StringType)),
            false,
        )]);
        let payload = map_value(vec![(
            "tags",
            Value::List(vec![
                Value::String("rust".into()),
                Value::String("api".into()),
            ]),
        )]);
        assert!(validate_payload(&payload, &s, &no_schemas()).is_ok());
    }

    #[test]
    fn test_typed_list_wrong_element_type_returns_422() {
        let s = schema(vec![field(
            "tags",
            SchemaType::TypedList(Box::new(SchemaType::StringType)),
            false,
        )]);
        let payload = map_value(vec![(
            "tags",
            Value::List(vec![
                Value::String("rust".into()),
                Value::Integer(42), // wrong type
            ]),
        )]);
        let err = validate_payload(&payload, &s, &no_schemas()).unwrap_err();
        if let MarretaError::HttpResponse { body, .. } = err {
            assert!(
                body.contains("tags[1]"),
                "expected path tags[1] in: {}",
                body
            );
        } else {
            panic!("expected HttpResponse");
        }
    }

    #[test]
    fn test_typed_list_not_array_returns_422() {
        let s = schema(vec![field(
            "tags",
            SchemaType::TypedList(Box::new(SchemaType::StringType)),
            false,
        )]);
        let payload = map_value(vec![("tags", Value::String("notalist".into()))]);
        let err = validate_payload(&payload, &s, &no_schemas()).unwrap_err();
        if let MarretaError::HttpResponse { body, .. } = err {
            assert!(body.contains("tags"));
            assert!(body.contains("array"));
        } else {
            panic!("expected HttpResponse");
        }
    }

    #[test]
    fn test_typed_list_of_schema_valid() {
        let item_schema = schema(vec![
            field("product_id", SchemaType::IntegerType, false),
            field("quantity", SchemaType::IntegerType, false),
        ]);
        let order_schema = schema(vec![
            field("client_id", SchemaType::IntegerType, false),
            field(
                "items",
                SchemaType::TypedList(Box::new(SchemaType::Reference("order_item".into()))),
                false,
            ),
        ]);
        let mut schemas = HashMap::new();
        schemas.insert("order_item".into(), item_schema);

        let payload = map_value(vec![
            ("client_id", Value::Integer(1)),
            (
                "items",
                Value::List(vec![
                    map_value(vec![
                        ("product_id", Value::Integer(10)),
                        ("quantity", Value::Integer(2)),
                    ]),
                    map_value(vec![
                        ("product_id", Value::Integer(11)),
                        ("quantity", Value::Integer(1)),
                    ]),
                ]),
            ),
        ]);
        assert!(validate_payload(&payload, &order_schema, &schemas).is_ok());
    }

    #[test]
    fn test_typed_list_of_schema_invalid_element_shows_indexed_path() {
        let item_schema = schema(vec![
            field("product_id", SchemaType::IntegerType, false),
            field("quantity", SchemaType::IntegerType, false),
        ]);
        let order_schema = schema(vec![field(
            "items",
            SchemaType::TypedList(Box::new(SchemaType::Reference("order_item".into()))),
            false,
        )]);
        let mut schemas = HashMap::new();
        schemas.insert("order_item".into(), item_schema);

        // Second item is missing quantity
        let payload = map_value(vec![(
            "items",
            Value::List(vec![
                map_value(vec![
                    ("product_id", Value::Integer(10)),
                    ("quantity", Value::Integer(2)),
                ]),
                map_value(vec![("product_id", Value::Integer(11))]), // missing quantity
            ]),
        )]);
        let err = validate_payload(&payload, &order_schema, &schemas).unwrap_err();
        if let MarretaError::HttpResponse { body, .. } = err {
            assert!(
                body.contains("items[1].quantity"),
                "expected path items[1].quantity in: {}",
                body
            );
        } else {
            panic!("expected HttpResponse");
        }
    }

    #[test]
    fn test_coerce_payload_turns_temporal_strings_into_native_values() {
        let s = schema(vec![
            field("created_at", SchemaType::InstantType, false),
            field("billing_date", SchemaType::DateType, false),
            field("opens_at", SchemaType::TimeType, false),
            field("sla", SchemaType::DurationType, false),
        ]);
        let payload = map_value(vec![
            ("created_at", Value::String("2026-04-27T13:10:45Z".into())),
            ("billing_date", Value::String("2026-04-27".into())),
            ("opens_at", Value::String("09:30:00".into())),
            ("sla", Value::String("PT5400S".into())),
        ]);

        let coerced = coerce_payload(&payload, &s, &no_schemas()).unwrap();
        let map = match coerced {
            Value::Map(map) => map,
            other => panic!("expected map, got {:?}", other),
        };
        let guard = map.read().unwrap();

        assert!(matches!(guard.get("created_at"), Some(Value::Instant(_))));
        assert!(matches!(guard.get("billing_date"), Some(Value::Date(_))));
        assert!(matches!(guard.get("opens_at"), Some(Value::Time(_))));
        assert!(matches!(guard.get("sla"), Some(Value::Duration(_))));
    }

    #[test]
    fn test_coerce_payload_turns_interval_map_into_native_interval() {
        let s = schema(vec![field(
            "business_window",
            SchemaType::IntervalType,
            false,
        )]);
        let payload = map_value(vec![(
            "business_window",
            map_value(vec![
                ("start", Value::String("2026-04-27".into())),
                ("end", Value::String("2026-04-30".into())),
            ]),
        )]);

        let coerced = coerce_payload(&payload, &s, &no_schemas()).unwrap();
        let map = match coerced {
            Value::Map(map) => map,
            other => panic!("expected map, got {:?}", other),
        };
        let guard = map.read().unwrap();

        match guard.get("business_window") {
            Some(Value::Interval(interval)) => {
                assert!(matches!(interval.start, TemporalValue::Date(_)));
                assert!(matches!(interval.end, TemporalValue::Date(_)));
            }
            other => panic!("expected native interval, got {:?}", other),
        }
    }

    #[test]
    fn test_coerce_payload_rejects_mixed_interval_component_types() {
        let s = schema(vec![field(
            "business_window",
            SchemaType::IntervalType,
            false,
        )]);
        let payload = map_value(vec![(
            "business_window",
            map_value(vec![
                ("start", Value::String("2026-04-27".into())),
                ("end", Value::String("2026-04-27T13:10:45Z".into())),
            ]),
        )]);

        let err = coerce_payload(&payload, &s, &no_schemas()).unwrap_err();
        match err {
            MarretaError::HttpResponse {
                status_code, body, ..
            } => {
                assert_eq!(status_code, 422);
                assert!(body.contains("must share the same temporal type"));
            }
            other => panic!("expected 422 response, got {:?}", other),
        }
    }

    #[test]
    fn test_coerce_payload_preserves_nested_temporal_values_inside_typed_lists() {
        let event_schema = schema(vec![
            field("when", SchemaType::InstantType, false),
            field("duration", SchemaType::DurationType, false),
        ]);
        let root_schema = schema(vec![field(
            "events",
            SchemaType::TypedList(Box::new(SchemaType::Reference("event".into()))),
            false,
        )]);
        let mut schemas = HashMap::new();
        schemas.insert("event".into(), event_schema);

        let payload = map_value(vec![(
            "events",
            Value::List(vec![map_value(vec![
                ("when", Value::String("2026-04-27T13:10:45Z".into())),
                ("duration", Value::String("PT3600S".into())),
            ])]),
        )]);

        let coerced = coerce_payload(&payload, &root_schema, &schemas).unwrap();
        let map = match coerced {
            Value::Map(map) => map,
            other => panic!("expected map, got {:?}", other),
        };
        let guard = map.read().unwrap();
        let events = match guard.get("events") {
            Some(Value::List(events)) => events,
            other => panic!("expected events list, got {:?}", other),
        };

        let event = match &events[0] {
            Value::Map(map) => map,
            other => panic!("expected event map, got {:?}", other),
        };
        let event_guard = event.read().unwrap();
        assert!(matches!(event_guard.get("when"), Some(Value::Instant(_))));
        assert!(matches!(
            event_guard.get("duration"),
            Some(Value::Duration(_))
        ));
    }

    #[test]
    fn test_field_path_helper() {
        assert_eq!(field_path("", "name"), "name");
        assert_eq!(field_path("billing", "city"), "billing.city");
        assert_eq!(
            field_path("order.billing", "zipcode"),
            "order.billing.zipcode"
        );
    }

    // --- Spec 062: cycle-aware, relation-aware coercion ---

    fn persistent_schema(table: &str, fields: Vec<SchemaField>) -> SchemaDefinition {
        SchemaDefinition {
            db_table: Some(table.into()),
            fields,
        }
    }

    fn registry(pairs: Vec<(&str, SchemaDefinition)>) -> HashMap<String, SchemaDefinition> {
        pairs.into_iter().map(|(k, v)| (k.into(), v)).collect()
    }

    #[test]
    fn persistent_bidirectional_relation_validates_as_contract() {
        // `take payload as User` where User <-> Order is a bidirectional db relation.
        // Validation must terminate (the back-reference Order.user -> User is a db
        // relation and is let through), so a persistent schema works as an API contract.
        let schemas = registry(vec![
            (
                "User",
                persistent_schema(
                    "users",
                    vec![
                        field("id", SchemaType::IntegerType, false),
                        field("name", SchemaType::StringType, false),
                        field(
                            "orders",
                            SchemaType::TypedList(Box::new(SchemaType::Reference("Order".into()))),
                            true,
                        ),
                    ],
                ),
            ),
            (
                "Order",
                persistent_schema(
                    "orders",
                    vec![
                        field("id", SchemaType::IntegerType, false),
                        field("user", SchemaType::Reference("User".into()), true),
                    ],
                ),
            ),
        ]);
        let payload = map_value(vec![
            ("id", Value::Integer(1)),
            ("name", Value::String("Ana".into())),
            (
                "orders",
                Value::List(vec![map_value(vec![
                    ("id", Value::Integer(7)),
                    // The back-reference to User is a relation — accepted as-is.
                    ("user", Value::Integer(1)),
                ])]),
            ),
        ]);
        let result = validate_payload(&payload, &schemas["User"], &schemas);
        assert!(
            result.is_ok(),
            "persistent bidirectional relation must validate without looping, got {result:?}"
        );
    }

    #[test]
    fn value_schema_cycle_is_an_infinite_loop_error() {
        // A value-schema cycle (A -> B -> A) is a genuine infinite embed; the coercer
        // recognises it and errors (the load-time check rejects it earlier, this is the
        // backstop).
        let schemas = registry(vec![
            (
                "A",
                schema(vec![field("b", SchemaType::Reference("B".into()), false)]),
            ),
            (
                "B",
                schema(vec![field("a", SchemaType::Reference("A".into()), false)]),
            ),
        ]);
        // Nested deep enough to revisit a schema on the validation stack (the cycle is
        // detected at the reference, before the inner value is inspected).
        let payload = map_value(vec![(
            "b",
            map_value(vec![("a", map_value(vec![("b", map_value(vec![]))]))]),
        )]);
        let err = validate_payload(&payload, &schemas["A"], &schemas).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("infinite schema reference cycle") || msg.contains("cycle"),
            "expected an infinite-cycle error, got {msg}"
        );
    }
}
