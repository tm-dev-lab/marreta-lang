/// Response serializer for MarretaLang v0.3.3.
///
/// Filters a `Value::Map` produced by a route against a `SchemaDefinition`,
/// returning a new map that contains only the fields declared in the schema.
///
/// Rules:
/// - Field declared in schema **and** present in value → included as-is.
/// - Field declared in schema but **absent** in value → `Value::Null` if required,
///   omitted entirely if optional.
/// - Field present in value but **not** declared in schema → stripped.
use crate::route_loader::SchemaDefinition;
use crate::value::{Value, ValueMap};

/// Serializes `value` against `schema`, returning a filtered `Value::Map`.
///
/// If `value` is not a `Value::Map`, returns `value` unchanged — the server
/// will serialize it as-is and skip filtering.
pub fn serialize(value: Value, schema: &SchemaDefinition) -> Value {
    let map = match &value {
        Value::Map(m) => m.read().unwrap().clone(),
        _ => return value,
    };

    let mut pairs: Vec<(String, Value)> = Vec::new();

    for field in &schema.fields {
        match map.get(&field.name) {
            Some(v) => {
                pairs.push((field.name.clone(), v.clone()));
            }
            None => {
                if !field.optional {
                    // Required field missing from the map → serialize as null
                    pairs.push((field.name.clone(), Value::Null));
                }
                // Optional field missing → omit entirely
            }
        }
    }

    let out: ValueMap = pairs.into_iter().collect();
    Value::Map(std::sync::Arc::new(std::sync::RwLock::new(out)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{SchemaField, SchemaType};
    use crate::route_loader::SchemaDefinition;

    fn make_schema(fields: &[(&str, SchemaType, bool)]) -> SchemaDefinition {
        SchemaDefinition {
            db_table: None,
            fields: fields
                .iter()
                .map(|(name, t, optional)| SchemaField {
                    name: name.to_string(),
                    field_type: t.clone(),
                    optional: *optional,
                })
                .collect(),
        }
    }

    fn make_map(pairs: &[(&str, Value)]) -> Value {
        let m: ValueMap = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        Value::Map(std::sync::Arc::new(std::sync::RwLock::new(m)))
    }

    fn get(value: &Value, key: &str) -> Option<Value> {
        if let Value::Map(m) = value {
            m.read().unwrap().get(key).cloned()
        } else {
            None
        }
    }

    fn has_key(value: &Value, key: &str) -> bool {
        if let Value::Map(m) = value {
            m.read().unwrap().contains_key(key)
        } else {
            false
        }
    }

    fn map_len(value: &Value) -> usize {
        if let Value::Map(m) = value {
            m.read().unwrap().len()
        } else {
            0
        }
    }

    #[test]
    fn test_declared_fields_are_included() {
        let schema = make_schema(&[
            ("name", SchemaType::StringType, false),
            ("total", SchemaType::FloatType, false),
        ]);
        let input = make_map(&[
            ("name", Value::String("Widget".into())),
            ("total", Value::Float(9.99)),
        ]);
        let out = serialize(input, &schema);
        assert_eq!(get(&out, "name"), Some(Value::String("Widget".into())));
        assert_eq!(get(&out, "total"), Some(Value::Float(9.99)));
    }

    #[test]
    fn test_undeclared_fields_are_stripped() {
        let schema = make_schema(&[("name", SchemaType::StringType, false)]);
        let input = make_map(&[
            ("name", Value::String("Widget".into())),
            ("internal_id", Value::Integer(42)),
            ("debug_flag", Value::Boolean(true)),
        ]);
        let out = serialize(input, &schema);
        assert_eq!(map_len(&out), 1);
        assert!(!has_key(&out, "internal_id"));
        assert!(!has_key(&out, "debug_flag"));
    }

    #[test]
    fn test_required_field_absent_becomes_null() {
        let schema = make_schema(&[
            ("name", SchemaType::StringType, false),
            ("total", SchemaType::FloatType, false),
        ]);
        let input = make_map(&[("name", Value::String("Widget".into()))]);
        let out = serialize(input, &schema);
        assert_eq!(get(&out, "total"), Some(Value::Null));
    }

    #[test]
    fn test_optional_field_absent_is_omitted() {
        let schema = make_schema(&[
            ("name", SchemaType::StringType, false),
            ("coupon", SchemaType::StringType, true), // optional
        ]);
        let input = make_map(&[("name", Value::String("Widget".into()))]);
        let out = serialize(input, &schema);
        assert!(
            !has_key(&out, "coupon"),
            "optional absent field must be omitted"
        );
    }

    #[test]
    fn test_optional_field_present_is_included() {
        let schema = make_schema(&[
            ("name", SchemaType::StringType, false),
            ("coupon", SchemaType::StringType, true),
        ]);
        let input = make_map(&[
            ("name", Value::String("Widget".into())),
            ("coupon", Value::String("SAVE10".into())),
        ]);
        let out = serialize(input, &schema);
        assert_eq!(get(&out, "coupon"), Some(Value::String("SAVE10".into())));
    }

    #[test]
    fn test_non_map_value_passes_through_unchanged() {
        let schema = make_schema(&[("x", SchemaType::IntegerType, false)]);
        let input = Value::String("plain text".into());
        let out = serialize(input, &schema);
        assert!(matches!(out, Value::String(_)));
    }

    #[test]
    fn test_empty_schema_strips_all_fields() {
        let schema = make_schema(&[]);
        let input = make_map(&[("name", Value::String("Widget".into()))]);
        let out = serialize(input, &schema);
        assert_eq!(map_len(&out), 0);
    }

    #[test]
    fn test_field_order_follows_schema_declaration() {
        // HashMap doesn't guarantee order, but we verify all declared fields are present
        let schema = make_schema(&[
            ("c", SchemaType::IntegerType, false),
            ("a", SchemaType::IntegerType, false),
            ("b", SchemaType::IntegerType, false),
        ]);
        let input = make_map(&[
            ("a", Value::Integer(1)),
            ("b", Value::Integer(2)),
            ("c", Value::Integer(3)),
        ]);
        let out = serialize(input, &schema);
        assert_eq!(map_len(&out), 3);
        assert_eq!(get(&out, "a"), Some(Value::Integer(1)));
        assert_eq!(get(&out, "b"), Some(Value::Integer(2)));
        assert_eq!(get(&out, "c"), Some(Value::Integer(3)));
    }
}
