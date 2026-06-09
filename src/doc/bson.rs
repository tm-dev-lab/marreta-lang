use std::sync::{Arc, RwLock};

use crate::value::{Value, ValueMap};
use mongodb::bson::{Bson, Document, decimal128::Decimal128, oid::ObjectId};

/// Convert a MarretaLang `Value` to a `bson::Bson`.
/// Used for writing properties to MongoDB.
pub fn value_to_bson(val: &Value) -> Bson {
    match val {
        Value::Null => Bson::Null,
        Value::Boolean(b) => Bson::Boolean(*b),
        Value::Integer(i) => Bson::Int64(*i),
        Value::Float(f) => Bson::Double(*f),
        Value::Decimal(d) => d
            .to_string()
            .parse::<Decimal128>()
            .map(Bson::Decimal128)
            .unwrap_or(Bson::Null),
        Value::String(s) => Bson::String(s.clone()),
        Value::Instant(dt) => {
            Bson::DateTime(mongodb::bson::DateTime::from_millis(dt.timestamp_millis()))
        }
        Value::Date(date) => Bson::String(date.format("%Y-%m-%d").to_string()),
        Value::Time(time) => Bson::String(time.format("%H:%M:%S").to_string()),
        Value::Duration(duration) => Bson::Int64(duration.num_milliseconds()),
        Value::Interval(interval) => {
            let mut doc = Document::new();
            doc.insert(
                "start",
                Bson::String(
                    crate::value::value_to_json(&Value::Interval(interval.clone()))
                        .get("start")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                ),
            );
            doc.insert(
                "end",
                Bson::String(
                    crate::value::value_to_json(&Value::Interval(interval.clone()))
                        .get("end")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                ),
            );
            Bson::Document(doc)
        }
        Value::List(l) => {
            let mut arr = Vec::with_capacity(l.len());
            for item in l {
                arr.push(value_to_bson(item));
            }
            Bson::Array(arr)
        }
        Value::Map(m) => {
            let mut doc = Document::new();
            let guard = m.read().unwrap();
            for (k, v) in guard.iter() {
                // If the key is _id, and the value is a string,
                // try to convert it to ObjectId (Smart Cast)
                if (k == "_id" || k == "id")
                    && let Value::String(s) = v
                    && let Ok(oid) = ObjectId::parse_str(s)
                {
                    doc.insert(k.clone(), Bson::ObjectId(oid));
                    continue;
                }
                doc.insert(k.clone(), value_to_bson(v));
            }
            Bson::Document(doc)
        }
        // Functions, builders, and namespaces cannot be serialized. Fallback to Null.
        _ => Bson::Null,
    }
}

/// Convert a `bson::Bson` to a MarretaLang `Value`.
/// Used for reading properties from MongoDB.
pub fn bson_to_value(b: &Bson) -> Value {
    match b {
        Bson::Double(f) => Value::Float(*f),
        Bson::Decimal128(d) => d
            .to_string()
            .parse::<rust_decimal::Decimal>()
            .map(Value::Decimal)
            .unwrap_or(Value::Null),
        Bson::String(s) => Value::String(s.clone()),
        Bson::Array(arr) => Value::List(arr.iter().map(bson_to_value).collect()),
        Bson::Document(doc) => {
            let mut map = ValueMap::new();
            for (k, v) in doc {
                map.insert(k.clone(), bson_to_value(v));
            }
            Value::Map(Arc::new(RwLock::new(map)))
        }
        Bson::Boolean(b) => Value::Boolean(*b),
        Bson::Null => Value::Null,
        Bson::Int32(i) => Value::Integer(*i as i64),
        Bson::Int64(i) => Value::Integer(*i),
        Bson::ObjectId(oid) => Value::String(oid.to_hex()),
        Bson::DateTime(dt) => Value::Instant(dt.to_system_time().into()),
        Bson::Symbol(s) => Value::String(s.clone()),
        Bson::RegularExpression(regex) => Value::String(regex.pattern.clone()),
        _ => Value::Null,
    }
}

#[cfg(test)]
#[allow(clippy::approx_constant)] // sample float literals (3.14…), not the PI constant
mod tests {
    use super::*;
    use crate::value::{TemporalInterval, TemporalValue};
    use chrono::{Duration as ChronoDuration, NaiveDate, NaiveTime, TimeZone, Utc};

    #[test]
    fn test_value_to_bson_primitives() {
        assert_eq!(value_to_bson(&Value::Null), Bson::Null);
        assert_eq!(value_to_bson(&Value::Boolean(true)), Bson::Boolean(true));
        assert_eq!(value_to_bson(&Value::Integer(42)), Bson::Int64(42));
        assert_eq!(value_to_bson(&Value::Float(3.14)), Bson::Double(3.14));
        assert_eq!(
            value_to_bson(&Value::String("test".into())),
            Bson::String("test".into())
        );
    }

    #[test]
    fn test_bson_to_value_primitives() {
        assert_eq!(bson_to_value(&Bson::Null), Value::Null);
        assert_eq!(bson_to_value(&Bson::Boolean(true)), Value::Boolean(true));
        assert_eq!(bson_to_value(&Bson::Int32(42)), Value::Integer(42));
        assert_eq!(bson_to_value(&Bson::Int64(42)), Value::Integer(42));
        assert_eq!(bson_to_value(&Bson::Double(3.14)), Value::Float(3.14));
        assert_eq!(
            bson_to_value(&Bson::String("test".into())),
            Value::String("test".into())
        );
    }

    #[test]
    fn test_smart_cast_object_id() {
        let hex = "507f1f77bcf86cd799439011";
        let mut row = ValueMap::new();
        row.insert("_id".into(), Value::String(hex.into()));
        row.insert("name".into(), Value::String("Ana".into()));
        let val = Value::Map(Arc::new(RwLock::new(row)));

        let bson_doc = value_to_bson(&val);
        if let Bson::Document(doc) = bson_doc {
            assert!(matches!(doc.get("_id").unwrap(), Bson::ObjectId(_)));
            assert!(matches!(doc.get("name").unwrap(), Bson::String(_)));
        } else {
            panic!("Expected Document");
        }
    }

    // --- value_to_bson: additional type coverage ---

    #[test]
    fn test_value_to_bson_list() {
        let list = Value::List(vec![Value::Integer(1), Value::String("a".into())]);
        let bson = value_to_bson(&list);
        if let Bson::Array(arr) = bson {
            assert_eq!(arr.len(), 2);
            assert_eq!(arr[0], Bson::Int64(1));
            assert_eq!(arr[1], Bson::String("a".into()));
        } else {
            panic!("Expected Array");
        }
    }

    #[test]
    fn test_value_to_bson_map() {
        let mut map = ValueMap::new();
        map.insert("score".into(), Value::Float(9.5));
        map.insert("active".into(), Value::Boolean(true));
        let val = Value::Map(Arc::new(RwLock::new(map)));
        let bson = value_to_bson(&val);
        if let Bson::Document(doc) = bson {
            assert!(matches!(doc.get("score").unwrap(), Bson::Double(_)));
            assert!(matches!(doc.get("active").unwrap(), Bson::Boolean(true)));
        } else {
            panic!("Expected Document");
        }
    }

    #[test]
    fn test_value_to_bson_map_id_non_hex_string() {
        // _id that is NOT a valid ObjectId hex → falls back to plain string
        let mut map = ValueMap::new();
        map.insert("_id".into(), Value::String("not-a-hex-oid".into()));
        let val = Value::Map(Arc::new(RwLock::new(map)));
        let bson = value_to_bson(&val);
        if let Bson::Document(doc) = bson {
            assert!(matches!(doc.get("_id").unwrap(), Bson::String(_)));
        } else {
            panic!("Expected Document");
        }
    }

    #[test]
    fn test_value_to_bson_fallback_null() {
        // DbNamespace (not serializable) should become Null
        let val = Value::DbNamespace;
        assert_eq!(value_to_bson(&val), Bson::Null);
    }

    // --- bson_to_value: additional type coverage ---

    #[test]
    fn test_bson_to_value_array() {
        let arr = Bson::Array(vec![Bson::Int64(1), Bson::String("x".into())]);
        let v = bson_to_value(&arr);
        if let Value::List(items) = v {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0], Value::Integer(1));
            assert_eq!(items[1], Value::String("x".into()));
        } else {
            panic!("Expected List");
        }
    }

    #[test]
    fn test_bson_to_value_document() {
        let mut doc = mongodb::bson::Document::new();
        doc.insert("count", Bson::Int32(5));
        let v = bson_to_value(&Bson::Document(doc));
        if let Value::Map(m) = v {
            let guard = m.read().unwrap();
            assert_eq!(*guard.get("count").unwrap(), Value::Integer(5));
        } else {
            panic!("Expected Map");
        }
    }

    #[test]
    fn test_bson_to_value_object_id() {
        let oid = ObjectId::parse_str("507f1f77bcf86cd799439011").unwrap();
        let v = bson_to_value(&Bson::ObjectId(oid));
        assert_eq!(v, Value::String("507f1f77bcf86cd799439011".into()));
    }

    #[test]
    fn test_bson_to_value_datetime() {
        let dt = mongodb::bson::DateTime::from_millis(0);
        let v = bson_to_value(&Bson::DateTime(dt));
        assert!(matches!(v, Value::Instant(_)));
    }

    #[test]
    fn test_value_to_bson_temporal_values() {
        let instant = Value::Instant(Utc.with_ymd_and_hms(2026, 4, 27, 13, 10, 45).unwrap());
        let date = Value::Date(NaiveDate::from_ymd_opt(2026, 4, 27).unwrap());
        let time = Value::Time(NaiveTime::from_hms_opt(9, 30, 0).unwrap());
        let duration = Value::Duration(ChronoDuration::minutes(90));
        let interval = Value::Interval(TemporalInterval {
            start: TemporalValue::Date(NaiveDate::from_ymd_opt(2026, 4, 27).unwrap()),
            end: TemporalValue::Date(NaiveDate::from_ymd_opt(2026, 4, 30).unwrap()),
        });

        assert!(matches!(value_to_bson(&instant), Bson::DateTime(_)));
        assert_eq!(value_to_bson(&date), Bson::String("2026-04-27".into()));
        assert_eq!(value_to_bson(&time), Bson::String("09:30:00".into()));
        assert_eq!(value_to_bson(&duration), Bson::Int64(5_400_000));

        match value_to_bson(&interval) {
            Bson::Document(doc) => {
                assert_eq!(doc.get_str("start").unwrap(), "2026-04-27");
                assert_eq!(doc.get_str("end").unwrap(), "2026-04-30");
            }
            other => panic!("expected BSON document for interval, got {:?}", other),
        }
    }

    #[test]
    fn test_bson_to_value_symbol() {
        let v = bson_to_value(&Bson::Symbol("mysym".into()));
        assert_eq!(v, Value::String("mysym".into()));
    }

    #[test]
    fn test_bson_to_value_regex() {
        let regex = mongodb::bson::Regex {
            pattern: "^hello".into(),
            options: "i".into(),
        };
        let v = bson_to_value(&Bson::RegularExpression(regex));
        assert_eq!(v, Value::String("^hello".into()));
    }

    #[test]
    fn test_bson_to_value_undefined_fallback() {
        let v = bson_to_value(&Bson::Undefined);
        assert_eq!(v, Value::Null);
    }

    #[test]
    fn test_bson_to_value_decimal128() {
        let d = "19.90".parse::<mongodb::bson::Decimal128>().unwrap();
        let v = bson_to_value(&Bson::Decimal128(d));
        assert_eq!(v, Value::Decimal("19.90".parse().unwrap()));
    }

    // --- Round-trip tests ---

    #[test]
    fn test_round_trip_string() {
        let original = Value::String("hello".into());
        let b = value_to_bson(&original);
        let back = bson_to_value(&b);
        assert_eq!(original, back);
    }

    #[test]
    fn test_round_trip_integer() {
        let original = Value::Integer(99);
        let b = value_to_bson(&original);
        let back = bson_to_value(&b);
        assert_eq!(original, back);
    }

    #[test]
    fn test_round_trip_float() {
        let original = Value::Float(3.14);
        let b = value_to_bson(&original);
        let back = bson_to_value(&b);
        assert_eq!(original, back);
    }

    #[test]
    fn test_round_trip_decimal() {
        let original = Value::Decimal("1234567890.99".parse().unwrap());
        let b = value_to_bson(&original);
        assert!(matches!(b, Bson::Decimal128(_)));
        let back = bson_to_value(&b);
        assert_eq!(original, back);
    }

    #[test]
    fn test_round_trip_boolean() {
        let original = Value::Boolean(false);
        let b = value_to_bson(&original);
        let back = bson_to_value(&b);
        assert_eq!(original, back);
    }

    #[test]
    fn test_round_trip_null() {
        let original = Value::Null;
        let b = value_to_bson(&original);
        let back = bson_to_value(&b);
        assert_eq!(original, back);
    }
}
