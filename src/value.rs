use std::fmt;
use std::sync::{Arc, RwLock};

use chrono::{
    DateTime, Duration as ChronoDuration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime,
    SecondsFormat, TimeZone, Utc,
};
use chrono_tz::Tz;
use indexmap::IndexMap;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};

use crate::ast::{ParamDef, TaskBody};
use crate::db::driver::QueryState;
use crate::doc::query::DocQueryState;
use crate::error::MarretaError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationCardinality {
    One,
    Many,
}

#[derive(Debug, Clone)]
pub struct RelationHandle {
    pub query: QueryState,
    pub cardinality: RelationCardinality,
    pub null_short_circuit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemporalValue {
    Instant(DateTime<Utc>),
    Date(NaiveDate),
    Time(NaiveTime),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporalInterval {
    pub start: TemporalValue,
    pub end: TemporalValue,
}

pub type ValueMap = IndexMap<String, Value>;

fn format_instant(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn format_date(date: &NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn format_time(time: &NaiveTime) -> String {
    time.format("%H:%M:%S").to_string()
}

fn format_duration(duration: &ChronoDuration) -> String {
    let total_millis = duration.num_milliseconds();
    if total_millis % 1000 == 0 {
        format!("PT{}S", duration.num_seconds())
    } else {
        let secs = total_millis as f64 / 1000.0;
        format!("PT{}S", secs)
    }
}

fn temporal_value_to_string(value: &TemporalValue) -> String {
    match value {
        TemporalValue::Instant(dt) => format_instant(dt),
        TemporalValue::Date(date) => format_date(date),
        TemporalValue::Time(time) => format_time(time),
    }
}

fn current_timezone() -> Tz {
    std::env::var("MARRETA_TIMEZONE")
        .ok()
        .and_then(|name| name.parse::<Tz>().ok())
        .unwrap_or(chrono_tz::UTC)
}

fn resolve_local_datetime(
    tz: Tz,
    naive: NaiveDateTime,
    prefer_latest: bool,
) -> Result<DateTime<Utc>, MarretaError> {
    let resolved = match tz.from_local_datetime(&naive) {
        LocalResult::Single(dt) => Some(dt),
        LocalResult::Ambiguous(earliest, latest) => {
            Some(if prefer_latest { latest } else { earliest })
        }
        LocalResult::None => None,
    }
    .ok_or_else(|| MarretaError::TypeError {
        message: format!(
            "could not resolve local datetime '{}' in timezone '{}'",
            naive, tz
        ),
        line: 0,
        column: 0,
    })?;

    Ok(resolved.with_timezone(&Utc))
}

/// Runtime value representation for MarretaLang.
#[derive(Debug, Clone)]
pub enum Value {
    Integer(i64),
    Float(f64),
    Decimal(Decimal),
    String(String),
    Boolean(bool),
    Instant(DateTime<Utc>),
    Date(NaiveDate),
    Time(NaiveTime),
    Duration(ChronoDuration),
    Interval(TemporalInterval),
    Null,
    List(Vec<Value>),
    Map(Arc<RwLock<ValueMap>>),
    RelationalRecord {
        schema_name: String,
        fields: Arc<RwLock<ValueMap>>,
    },
    Task {
        name: String,
        params: Vec<ParamDef>,
        body: TaskBody,
        owner_module: Option<String>,
        source_module: Option<String>,
        line: usize,
        column: usize,
    },
    /// Intermediate: `db` namespace reference (before table is selected).
    /// Never escapes to user-visible output — only used during evaluation.
    DbNamespace,
    /// Intermediate: `db.TABLE` reference (before operation is called).
    /// Never escapes to user-visible output — only used during evaluation.
    DbTable(String),
    /// Logical relation handle inferred from a fetched relational record.
    RelationHandle(Box<RelationHandle>),
    /// Lazy query accumulator — grows as pipeline steps are added.
    /// Closed (executed) by a terminal operation (`>> fetch`, `>> count`, etc.).
    QueryBuilder(Box<QueryState>),
    /// Intermediate: `doc` namespace reference (before collection is selected).
    DocNamespace,
    /// Intermediate: `doc.COLLECTION` reference (before operation is called).
    DocCollection(String),
    /// Lazy query accumulator for document databases.
    DocQueryBuilder(Box<DocQueryState>),
    /// Intermediate: `cache` namespace reference (before operation is called).
    /// Never escapes to user-visible output — only used during evaluation.
    CacheNamespace,
    /// Intermediate: `fs` namespace reference (before operation is called).
    /// Never escapes to user-visible output — only used during evaluation.
    FsNamespace,
    /// Intermediate: `json` namespace reference (before operation is called).
    /// Never escapes to user-visible output — only used during evaluation.
    JsonNamespace,
    /// Intermediate: `base64` namespace reference (before operation is called).
    /// Never escapes to user-visible output — only used during evaluation.
    Base64Namespace,
    /// Intermediate: `uuid` namespace reference (before operation is called).
    /// Never escapes to user-visible output — only used during evaluation.
    UuidNamespace,
    /// Intermediate: `feature` namespace reference (before operation is called).
    /// Never escapes to user-visible output — only used during evaluation.
    FeatureNamespace,
    /// Intermediate: `log` namespace reference (before operation is called).
    /// Never escapes to user-visible output — only used during evaluation.
    LogNamespace,
    /// Intermediate: `time` namespace reference (before operation is called).
    /// Never escapes to user-visible output — only used during evaluation.
    TimeNamespace,
    /// Intermediate: `math` namespace reference (before operation is called).
    /// Never escapes to user-visible output — only used during evaluation.
    MathNamespace,
    /// Intermediate: `http_client` namespace reference (before operation is called).
    /// Never escapes to user-visible output — only used during evaluation.
    HttpClientNamespace,
}

impl Value {
    /// Returns whether this value is considered "truthy".
    ///
    /// Falsy: `null`, `false`, `0`, `0.0`, `""`, `[]`, `{}`
    /// Truthy: everything else
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Boolean(b) => *b,
            Value::Integer(n) => *n != 0,
            Value::Float(n) => *n != 0.0,
            Value::Decimal(n) => !n.is_zero(),
            Value::Instant(_)
            | Value::Date(_)
            | Value::Time(_)
            | Value::Duration(_)
            | Value::Interval(_) => true,
            Value::String(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
            Value::Map(m) => !m.read().unwrap().is_empty(),
            Value::RelationalRecord { fields, .. } => !fields.read().unwrap().is_empty(),
            Value::Task { .. } => true,
            Value::DbNamespace
            | Value::DbTable(_)
            | Value::RelationHandle(_)
            | Value::QueryBuilder(_) => false,
            Value::DocNamespace | Value::DocCollection(_) | Value::DocQueryBuilder(_) => false,
            Value::CacheNamespace => false,
            Value::FsNamespace => false,
            Value::JsonNamespace => false,
            Value::Base64Namespace => false,
            Value::UuidNamespace => false,
            Value::FeatureNamespace => false,
            Value::LogNamespace => false,
            Value::TimeNamespace => false,
            Value::MathNamespace => false,
            Value::HttpClientNamespace => false,
        }
    }

    /// Returns the type name as a string (for error messages).
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Integer(_) => "Integer",
            Value::Float(_) => "Float",
            Value::Decimal(_) => "Decimal",
            Value::String(_) => "String",
            Value::Boolean(_) => "Boolean",
            Value::Instant(_) => "Instant",
            Value::Date(_) => "Date",
            Value::Time(_) => "Time",
            Value::Duration(_) => "Duration",
            Value::Interval(_) => "Interval",
            Value::Null => "Null",
            Value::List(_) => "List",
            Value::Map(_) => "Map",
            Value::RelationalRecord { .. } => "RelationalRecord",
            Value::Task { .. } => "Task",
            Value::DbNamespace => "DbNamespace",
            Value::DbTable(_) => "DbTable",
            Value::RelationHandle(_) => "RelationHandle",
            Value::QueryBuilder(_) => "QueryBuilder",
            Value::DocNamespace => "DocNamespace",
            Value::DocCollection(_) => "DocCollection",
            Value::DocQueryBuilder(_) => "DocQueryBuilder",
            Value::CacheNamespace => "CacheNamespace",
            Value::FsNamespace => "FsNamespace",
            Value::JsonNamespace => "JsonNamespace",
            Value::Base64Namespace => "Base64Namespace",
            Value::UuidNamespace => "UuidNamespace",
            Value::FeatureNamespace => "FeatureNamespace",
            Value::LogNamespace => "LogNamespace",
            Value::TimeNamespace => "TimeNamespace",
            Value::MathNamespace => "MathNamespace",
            Value::HttpClientNamespace => "HttpClientNamespace",
        }
    }

    /// Creates a new empty Map value.
    /// Extracts an integer value (coercing Float if whole number).
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Value::Integer(n) => Some(*n),
            Value::Float(f) if f.fract() == 0.0 => Some(*f as i64),
            Value::Decimal(d) => d.to_i64(),
            _ => None,
        }
    }

    pub fn empty_map() -> Self {
        Value::Map(Arc::new(RwLock::new(ValueMap::new())))
    }

    /// Creates a new Map value from key-value pairs.
    pub fn map_from(pairs: Vec<(String, Value)>) -> Self {
        let map: ValueMap = pairs.into_iter().collect();
        Value::Map(Arc::new(RwLock::new(map)))
    }

    // --- Built-in methods ---

    /// Dispatches a built-in method call on this value.
    pub fn call_method(&self, method: &str, args: &[Value]) -> Result<Value, MarretaError> {
        self.call_method_at(method, args, 0, 0)
    }

    /// Dispatches a built-in method call on this value, preserving the source
    /// location of the call site when available.
    pub fn call_method_at(
        &self,
        method: &str,
        args: &[Value],
        line: usize,
        column: usize,
    ) -> Result<Value, MarretaError> {
        if !args.is_empty()
            && matches!(
                method,
                "to_integer" | "to_float" | "to_boolean" | "to_string"
            )
        {
            return Err(MarretaError::TypeError {
                message: format!("{}() takes no arguments", method),
                line,
                column,
            });
        }

        match method {
            "to_integer" => return Ok(Self::convert_to_integer(self)),
            "to_float" => return Ok(Self::convert_to_float(self)),
            "to_boolean" => return Ok(Value::Boolean(self.is_truthy())),
            "to_string" => return Ok(Value::String(self.to_string())),
            _ => {}
        }

        match self {
            Value::String(s) => Self::string_method(s, method, args, line, column),
            Value::List(l) => Self::list_method(l, method, args, line, column),
            Value::Map(m) => Self::map_method(m, method, args, line, column),
            Value::Integer(n) => Self::integer_method(*n, method, args, line, column),
            Value::Boolean(b) => Self::boolean_method(*b, method, args, line, column),
            Value::Float(n) => Self::float_method(*n, method, args, line, column),
            Value::Decimal(n) => Self::decimal_method(*n, method, args, line, column),
            Value::Time(t) => Self::time_method(*t, method, args, line, column),
            _ => Err(MarretaError::PropertyNotFound {
                object_type: self.type_name().into(),
                property: method.into(),
                line,
                column,
            }),
        }
    }

    fn string_method(
        s: &str,
        method: &str,
        args: &[Value],
        line: usize,
        column: usize,
    ) -> Result<Value, MarretaError> {
        match method {
            "length" => Ok(Value::Integer(s.len() as i64)),
            "upper" => Ok(Value::String(s.to_uppercase())),
            "lower" => Ok(Value::String(s.to_lowercase())),
            "trim" => Ok(Value::String(s.trim().to_string())),
            "contains" => {
                let substr = Self::expect_string_arg(args, 0, "contains", line, column)?;
                Ok(Value::Boolean(s.contains(substr.as_str())))
            }
            "split" => {
                let sep = Self::expect_string_arg(args, 0, "split", line, column)?;
                let parts: Vec<Value> = s
                    .split(sep.as_str())
                    .map(|p| Value::String(p.to_string()))
                    .collect();
                Ok(Value::List(parts))
            }
            "replace" => {
                let old = Self::expect_string_arg(args, 0, "replace", line, column)?;
                let new = Self::expect_string_arg(args, 1, "replace", line, column)?;
                Ok(Value::String(s.replace(old.as_str(), new.as_str())))
            }
            "to_string" => Ok(Value::String(s.to_string())),
            "starts_with" => {
                let prefix = Self::expect_string_arg(args, 0, "starts_with", line, column)?;
                Ok(Value::Boolean(s.starts_with(prefix.as_str())))
            }
            "ends_with" => {
                let suffix = Self::expect_string_arg(args, 0, "ends_with", line, column)?;
                Ok(Value::Boolean(s.ends_with(suffix.as_str())))
            }
            "index_of" => {
                let needle = Self::expect_string_arg(args, 0, "index_of", line, column)?;
                Ok(Value::Integer(
                    s.find(needle.as_str()).map(|i| i as i64).unwrap_or(-1),
                ))
            }
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "String".into(),
                property: method.into(),
                line,
                column,
            }),
        }
    }

    fn time_method(
        time: NaiveTime,
        method: &str,
        args: &[Value],
        line: usize,
        column: usize,
    ) -> Result<Value, MarretaError> {
        match method {
            "on" => {
                let date = match args.first() {
                    Some(Value::Date(date)) => *date,
                    Some(other) => {
                        return Err(MarretaError::TypeError {
                            message: format!("time.on() requires Date, got {}", other.type_name()),
                            line,
                            column,
                        });
                    }
                    None => {
                        return Err(MarretaError::TypeError {
                            message: "time.on() requires a date argument".into(),
                            line,
                            column,
                        });
                    }
                };
                let tz = current_timezone();
                let combined = resolve_local_datetime(tz, date.and_time(time), false).map_err(
                    |err| match err {
                        MarretaError::TypeError { message, .. } => MarretaError::TypeError {
                            message,
                            line,
                            column,
                        },
                        other => other,
                    },
                )?;
                Ok(Value::Instant(combined))
            }
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "Time".into(),
                property: method.into(),
                line,
                column,
            }),
        }
    }

    fn list_method(
        l: &[Value],
        method: &str,
        args: &[Value],
        line: usize,
        column: usize,
    ) -> Result<Value, MarretaError> {
        match method {
            "length" => Ok(Value::Integer(l.len() as i64)),
            "first" => Ok(l.first().cloned().unwrap_or(Value::Null)),
            "last" => Ok(l.last().cloned().unwrap_or(Value::Null)),
            "empty?" => Ok(Value::Boolean(l.is_empty())),
            "push" => {
                if args.is_empty() {
                    return Err(MarretaError::TypeError {
                        message: "push requires 1 argument".into(),
                        line,
                        column,
                    });
                }
                let mut new_list = l.to_vec();
                new_list.push(args[0].clone());
                Ok(Value::List(new_list))
            }
            "includes" => {
                if args.is_empty() {
                    return Err(MarretaError::TypeError {
                        message: "includes requires 1 argument".into(),
                        line,
                        column,
                    });
                }
                Ok(Value::Boolean(l.iter().any(|v| v == &args[0])))
            }
            "reverse" => {
                let mut reversed = l.to_vec();
                reversed.reverse();
                Ok(Value::List(reversed))
            }
            "join" => {
                let sep = Self::expect_string_arg(args, 0, "join", line, column)?;
                let parts: Vec<String> = l.iter().map(|v| format!("{}", v)).collect();
                Ok(Value::String(parts.join(&sep)))
            }
            "sort" => {
                fn type_order(v: &Value) -> u8 {
                    match v {
                        Value::Integer(_) => 0,
                        Value::Float(_) => 1,
                        Value::String(_) => 2,
                        Value::Boolean(_) => 3,
                        _ => 4,
                    }
                }
                fn to_f64(v: &Value) -> Option<f64> {
                    match v {
                        Value::Integer(n) => Some(*n as f64),
                        Value::Float(n) => Some(*n),
                        _ => None,
                    }
                }
                let mut sorted = l.to_vec();
                sorted.sort_by(|a, b| {
                    let ta = type_order(a);
                    let tb = type_order(b);
                    if ta != tb {
                        return ta.cmp(&tb);
                    }
                    // Same type class — compare by value
                    match (a, b) {
                        (Value::Integer(_), _) | (Value::Float(_), _) => {
                            let fa = to_f64(a).unwrap_or(0.0);
                            let fb = to_f64(b).unwrap_or(0.0);
                            fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
                        }
                        (Value::String(sa), Value::String(sb)) => sa.cmp(sb),
                        (Value::Boolean(ba), Value::Boolean(bb)) => ba.cmp(bb),
                        _ => std::cmp::Ordering::Equal,
                    }
                });
                Ok(Value::List(sorted))
            }
            "unique" => {
                let mut seen: Vec<Value> = Vec::new();
                let mut result = Vec::new();
                for item in l {
                    if !seen.contains(item) {
                        seen.push(item.clone());
                        result.push(item.clone());
                    }
                }
                Ok(Value::List(result))
            }
            "flatten" => {
                let mut result = Vec::new();
                for item in l {
                    match item {
                        Value::List(inner) => result.extend(inner.iter().cloned()),
                        other => result.push(other.clone()),
                    }
                }
                Ok(Value::List(result))
            }
            "slice" => {
                let from = match args.first() {
                    Some(Value::Integer(n)) => *n as usize,
                    Some(other) => {
                        return Err(MarretaError::TypeError {
                            message: format!(
                                "slice argument 1 must be Integer, got {}",
                                other.type_name()
                            ),
                            line,
                            column,
                        });
                    }
                    None => {
                        return Err(MarretaError::TypeError {
                            message: "slice requires 2 arguments".into(),
                            line,
                            column,
                        });
                    }
                };
                let to = match args.get(1) {
                    Some(Value::Integer(n)) => *n as usize,
                    Some(other) => {
                        return Err(MarretaError::TypeError {
                            message: format!(
                                "slice argument 2 must be Integer, got {}",
                                other.type_name()
                            ),
                            line,
                            column,
                        });
                    }
                    None => {
                        return Err(MarretaError::TypeError {
                            message: "slice requires 2 arguments".into(),
                            line,
                            column,
                        });
                    }
                };
                let len = l.len();
                let from = from.min(len);
                let to = to.min(len);
                let to = to.max(from);
                Ok(Value::List(l[from..to].to_vec()))
            }
            "sum" => {
                if l.is_empty() {
                    return Ok(Value::Integer(0));
                }
                let mut total = 0.0;
                let mut saw_float = false;
                for item in l {
                    match item {
                        Value::Integer(n) => total += *n as f64,
                        Value::Float(n) => {
                            total += *n;
                            saw_float = true;
                        }
                        other => {
                            return Err(MarretaError::TypeError {
                                message: format!(
                                    "sum() requires a numeric list, got {}",
                                    other.type_name()
                                ),
                                line,
                                column,
                            });
                        }
                    }
                }
                if saw_float {
                    Ok(Value::Float(total))
                } else {
                    Ok(Value::Integer(total as i64))
                }
            }
            "mean" | "median" | "std_dev" => {
                if l.is_empty() {
                    return Ok(Value::Null);
                }
                let nums = Self::numeric_list(l, method, line, column)?;
                match method {
                    "mean" => {
                        let mean = nums.iter().sum::<f64>() / nums.len() as f64;
                        Ok(Value::Float(mean))
                    }
                    "median" => {
                        let mut sorted = nums;
                        sorted
                            .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                        let mid = sorted.len() / 2;
                        let median = if sorted.len() % 2 == 0 {
                            (sorted[mid - 1] + sorted[mid]) / 2.0
                        } else {
                            sorted[mid]
                        };
                        Ok(Value::Float(median))
                    }
                    "std_dev" => {
                        let mean = nums.iter().sum::<f64>() / nums.len() as f64;
                        let variance = nums
                            .iter()
                            .map(|n| {
                                let delta = *n - mean;
                                delta * delta
                            })
                            .sum::<f64>()
                            / nums.len() as f64;
                        Ok(Value::Float(variance.sqrt()))
                    }
                    _ => unreachable!(),
                }
            }
            "zip" => match args {
                [Value::List(other)] => {
                    if l.len() != other.len() {
                        return Err(MarretaError::RuntimeError {
                            message: "zip() requires lists of the same length".into(),
                            line,
                            column,
                        });
                    }
                    let zipped = l
                        .iter()
                        .cloned()
                        .zip(other.iter().cloned())
                        .map(|(left, right)| Value::List(vec![left, right]))
                        .collect();
                    Ok(Value::List(zipped))
                }
                [other] => Err(MarretaError::TypeError {
                    message: format!("zip() requires a List argument, got {}", other.type_name()),
                    line,
                    column,
                }),
                _ => Err(MarretaError::TypeError {
                    message: "zip() requires exactly 1 argument".into(),
                    line,
                    column,
                }),
            },
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "List".into(),
                property: method.into(),
                line,
                column,
            }),
        }
    }

    fn map_method(
        m: &Arc<RwLock<ValueMap>>,
        method: &str,
        args: &[Value],
        line: usize,
        column: usize,
    ) -> Result<Value, MarretaError> {
        let map = m.read().unwrap();
        match method {
            "keys" => {
                let keys: Vec<Value> = map.keys().map(|k| Value::String(k.clone())).collect();
                Ok(Value::List(keys))
            }
            "values" => {
                let vals: Vec<Value> = map.values().cloned().collect();
                Ok(Value::List(vals))
            }
            "has" => {
                let key = Self::expect_string_arg(args, 0, "has", line, column)?;
                Ok(Value::Boolean(map.contains_key(&key)))
            }
            "merge" => {
                drop(map); // release borrow
                if args.is_empty() {
                    return Err(MarretaError::TypeError {
                        message: "merge requires 1 argument".into(),
                        line,
                        column,
                    });
                }
                if let Value::Map(other) = &args[0] {
                    let mut merged = m.read().unwrap().clone();
                    for (k, v) in other.read().unwrap().iter() {
                        merged.insert(k.clone(), v.clone());
                    }
                    Ok(Value::Map(Arc::new(RwLock::new(merged))))
                } else {
                    Err(MarretaError::TypeError {
                        message: "merge argument must be a Map".into(),
                        line,
                        column,
                    })
                }
            }
            "delete" => {
                let key = Self::expect_string_arg(args, 0, "delete", line, column)?;
                let new_map: ValueMap = map
                    .iter()
                    .filter(|(k, _)| *k != &key)
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                Ok(Value::Map(Arc::new(RwLock::new(new_map))))
            }
            "size" => Ok(Value::Integer(map.len() as i64)),
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "Map".into(),
                property: method.into(),
                line,
                column,
            }),
        }
    }

    fn integer_method(
        n: i64,
        method: &str,
        args: &[Value],
        line: usize,
        column: usize,
    ) -> Result<Value, MarretaError> {
        match method {
            "abs" => Ok(Value::Integer(n.abs())),
            "min" => match args.first() {
                Some(Value::Integer(other)) => Ok(Value::Integer(n.min(*other))),
                Some(Value::Float(other)) => Ok(Value::Float((n as f64).min(*other))),
                Some(other) => Err(MarretaError::TypeError {
                    message: format!(
                        "min argument must be Integer or Float, got {}",
                        other.type_name()
                    ),
                    line,
                    column,
                }),
                None => Err(MarretaError::TypeError {
                    message: "min requires 1 argument".into(),
                    line,
                    column,
                }),
            },
            "max" => match args.first() {
                Some(Value::Integer(other)) => Ok(Value::Integer(n.max(*other))),
                Some(Value::Float(other)) => Ok(Value::Float((n as f64).max(*other))),
                Some(other) => Err(MarretaError::TypeError {
                    message: format!(
                        "max argument must be Integer or Float, got {}",
                        other.type_name()
                    ),
                    line,
                    column,
                }),
                None => Err(MarretaError::TypeError {
                    message: "max requires 1 argument".into(),
                    line,
                    column,
                }),
            },
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "Integer".into(),
                property: method.into(),
                line,
                column,
            }),
        }
    }

    fn boolean_method(
        _b: bool,
        method: &str,
        _args: &[Value],
        line: usize,
        column: usize,
    ) -> Result<Value, MarretaError> {
        Err(MarretaError::PropertyNotFound {
            object_type: "Boolean".into(),
            property: method.into(),
            line,
            column,
        })
    }

    fn float_method(
        n: f64,
        method: &str,
        args: &[Value],
        line: usize,
        column: usize,
    ) -> Result<Value, MarretaError> {
        match method {
            "abs" => Ok(Value::Float(n.abs())),
            "round" => {
                if args.is_empty() {
                    Ok(Value::Float(n.round()))
                } else {
                    match args.first() {
                        Some(Value::Integer(places)) => {
                            let factor = 10f64.powi(*places as i32);
                            Ok(Value::Float((n * factor).round() / factor))
                        }
                        Some(other) => Err(MarretaError::TypeError {
                            message: format!(
                                "round argument must be Integer, got {}",
                                other.type_name()
                            ),
                            line,
                            column,
                        }),
                        None => Ok(Value::Float(n.round())),
                    }
                }
            }
            "floor" => Ok(Value::Float(n.floor())),
            "ceil" => Ok(Value::Float(n.ceil())),
            "min" => match args.first() {
                Some(Value::Integer(other)) => Ok(Value::Float(n.min(*other as f64))),
                Some(Value::Float(other)) => Ok(Value::Float(n.min(*other))),
                Some(other) => Err(MarretaError::TypeError {
                    message: format!(
                        "min argument must be Integer or Float, got {}",
                        other.type_name()
                    ),
                    line,
                    column,
                }),
                None => Err(MarretaError::TypeError {
                    message: "min requires 1 argument".into(),
                    line,
                    column,
                }),
            },
            "max" => match args.first() {
                Some(Value::Integer(other)) => Ok(Value::Float(n.max(*other as f64))),
                Some(Value::Float(other)) => Ok(Value::Float(n.max(*other))),
                Some(other) => Err(MarretaError::TypeError {
                    message: format!(
                        "max argument must be Integer or Float, got {}",
                        other.type_name()
                    ),
                    line,
                    column,
                }),
                None => Err(MarretaError::TypeError {
                    message: "max requires 1 argument".into(),
                    line,
                    column,
                }),
            },
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "Float".into(),
                property: method.into(),
                line,
                column,
            }),
        }
    }

    fn decimal_method(
        n: Decimal,
        method: &str,
        args: &[Value],
        line: usize,
        column: usize,
    ) -> Result<Value, MarretaError> {
        match method {
            "abs" => Ok(Value::Decimal(n.abs())),
            "floor" => Ok(Value::Decimal(n.floor())),
            "ceil" => Ok(Value::Decimal(n.ceil())),
            "trunc" => Ok(Value::Decimal(n.trunc())),
            "scale" => Ok(Value::Integer(n.scale() as i64)),
            "to_string" => Ok(Value::String(n.to_string())),
            "to_integer" => Ok(Value::Integer(n.trunc().to_i64().unwrap_or(0))),
            "to_float" => Ok(Value::Float(n.to_f64().unwrap_or(0.0))),
            "round" => {
                let places = match args {
                    [] => 0,
                    [Value::Integer(places)] if *places >= 0 => *places as u32,
                    [other] => {
                        return Err(MarretaError::TypeError {
                            message: format!(
                                "round argument must be non-negative Integer, got {}",
                                other.type_name()
                            ),
                            line,
                            column,
                        });
                    }
                    _ => {
                        return Err(MarretaError::TypeError {
                            message: "round requires zero or one argument".into(),
                            line,
                            column,
                        });
                    }
                };
                Ok(Value::Decimal(n.round_dp_with_strategy(
                    places,
                    RoundingStrategy::MidpointNearestEven,
                )))
            }
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "Decimal".into(),
                property: method.into(),
                line,
                column,
            }),
        }
    }

    /// Helper: extract a String argument at the given index.
    fn expect_string_arg(
        args: &[Value],
        index: usize,
        method_name: &str,
        line: usize,
        column: usize,
    ) -> Result<String, MarretaError> {
        match args.get(index) {
            Some(Value::String(s)) => Ok(s.clone()),
            Some(other) => Err(MarretaError::TypeError {
                message: format!(
                    "{} argument {} must be a String, got {}",
                    method_name,
                    index + 1,
                    other.type_name()
                ),
                line,
                column,
            }),
            None => Err(MarretaError::TypeError {
                message: format!(
                    "{} requires at least {} argument(s)",
                    method_name,
                    index + 1
                ),
                line,
                column,
            }),
        }
    }

    fn convert_to_integer(value: &Value) -> Value {
        match value {
            Value::Integer(n) => Value::Integer(*n),
            Value::Float(n) => Value::Integer(*n as i64),
            Value::Decimal(n) => Value::Integer(n.trunc().to_i64().unwrap_or(0)),
            Value::String(s) => Value::Integer(s.trim().parse::<i64>().unwrap_or(0)),
            Value::Boolean(b) => Value::Integer(if *b { 1 } else { 0 }),
            Value::Null => Value::Integer(0),
            _ => Value::Integer(0),
        }
    }

    fn convert_to_float(value: &Value) -> Value {
        match value {
            Value::Integer(n) => Value::Float(*n as f64),
            Value::Float(n) => Value::Float(*n),
            Value::Decimal(n) => Value::Float(n.to_f64().unwrap_or(0.0)),
            Value::String(s) => Value::Float(s.trim().parse::<f64>().unwrap_or(0.0)),
            Value::Boolean(b) => Value::Float(if *b { 1.0 } else { 0.0 }),
            Value::Null => Value::Float(0.0),
            _ => Value::Float(0.0),
        }
    }

    fn numeric_list(
        values: &[Value],
        method_name: &str,
        line: usize,
        column: usize,
    ) -> Result<Vec<f64>, MarretaError> {
        values
            .iter()
            .map(|value| match value {
                Value::Integer(n) => Ok(*n as f64),
                Value::Float(n) => Ok(*n),
                other => Err(MarretaError::TypeError {
                    message: format!(
                        "{}() requires a numeric list, got {}",
                        method_name,
                        other.type_name()
                    ),
                    line,
                    column,
                }),
            })
            .collect()
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Integer(n) => write!(f, "{}", n),
            Value::Float(n) => {
                if n.fract() == 0.0 {
                    write!(f, "{:.1}", n)
                } else {
                    write!(f, "{}", n)
                }
            }
            Value::Decimal(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "{}", s),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Instant(dt) => write!(f, "{}", format_instant(dt)),
            Value::Date(date) => write!(f, "{}", format_date(date)),
            Value::Time(time) => write!(f, "{}", format_time(time)),
            Value::Duration(duration) => write!(f, "{}", format_duration(duration)),
            Value::Interval(interval) => write!(
                f,
                "{{start: {}, end: {}}}",
                temporal_value_to_string(&interval.start),
                temporal_value_to_string(&interval.end)
            ),
            Value::Null => write!(f, "null"),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    // Strings inside collections are quoted
                    if matches!(item, Value::String(_)) {
                        write!(f, "\"{}\"", item)?;
                    } else {
                        write!(f, "{}", item)?;
                    }
                }
                write!(f, "]")
            }
            Value::Map(map) => {
                let map = map.read().unwrap();
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    if matches!(v, Value::String(_)) {
                        write!(f, "{}: \"{}\"", k, v)?;
                    } else {
                        write!(f, "{}: {}", k, v)?;
                    }
                }
                write!(f, "}}")
            }
            Value::RelationalRecord { fields, .. } => {
                let map = fields.read().unwrap();
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    if matches!(v, Value::String(_)) {
                        write!(f, "{}: \"{}\"", k, v)?;
                    } else {
                        write!(f, "{}: {}", k, v)?;
                    }
                }
                write!(f, "}}")
            }
            Value::Task { name, params, .. } => {
                let param_names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
                write!(f, "<task {}({})>", name, param_names.join(", "))
            }
            Value::DbNamespace => write!(f, "<db>"),
            Value::DbTable(t) => write!(f, "<db.{}>", t),
            Value::RelationHandle(handle) => match handle.cardinality {
                RelationCardinality::One => write!(f, "<relation:{}>", handle.query.table),
                RelationCardinality::Many => write!(f, "<relation_many:{}>", handle.query.table),
            },
            Value::QueryBuilder(q) => write!(f, "<query:{}>", q.table),
            Value::DocNamespace => write!(f, "<doc>"),
            Value::CacheNamespace => write!(f, "<cache>"),
            Value::FsNamespace => write!(f, "<fs>"),
            Value::JsonNamespace => write!(f, "<json>"),
            Value::Base64Namespace => write!(f, "<base64>"),
            Value::UuidNamespace => write!(f, "<uuid>"),
            Value::FeatureNamespace => write!(f, "<feature>"),
            Value::LogNamespace => write!(f, "<log>"),
            Value::TimeNamespace => write!(f, "<time>"),
            Value::MathNamespace => write!(f, "<math>"),
            Value::HttpClientNamespace => write!(f, "<http_client>"),
            Value::DocCollection(t) => write!(f, "<doc.{}>", t),
            Value::DocQueryBuilder(q) => write!(f, "<doc_query:{}>", q.collection),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Decimal(a), Value::Decimal(b)) => a == b,
            (Value::Integer(a), Value::Decimal(b)) => Decimal::from(*a) == *b,
            (Value::Decimal(a), Value::Integer(b)) => *a == Decimal::from(*b),
            (Value::Integer(a), Value::Float(b)) => (*a as f64) == *b,
            (Value::Float(a), Value::Integer(b)) => *a == (*b as f64),
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Instant(a), Value::Instant(b)) => a == b,
            (Value::Date(a), Value::Date(b)) => a == b,
            (Value::Time(a), Value::Time(b)) => a == b,
            (Value::Duration(a), Value::Duration(b)) => a == b,
            (Value::Interval(a), Value::Interval(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => *a.read().unwrap() == *b.read().unwrap(),
            (
                Value::RelationalRecord {
                    schema_name: a_name,
                    fields: a_fields,
                },
                Value::RelationalRecord {
                    schema_name: b_name,
                    fields: b_fields,
                },
            ) => a_name == b_name && *a_fields.read().unwrap() == *b_fields.read().unwrap(),
            (Value::DbNamespace, Value::DbNamespace) => true,
            (Value::DbTable(a), Value::DbTable(b)) => a == b,
            (Value::RelationHandle(a), Value::RelationHandle(b)) => {
                a.cardinality == b.cardinality
                    && a.null_short_circuit == b.null_short_circuit
                    && a.query.table == b.query.table
                    && a.query.select_cols == b.query.select_cols
                    && a.query.order_by == b.query.order_by
                    && a.query.limit == b.query.limit
                    && a.query.offset == b.query.offset
            }
            (Value::DocNamespace, Value::DocNamespace) => true,
            (Value::CacheNamespace, Value::CacheNamespace) => true,
            (Value::FsNamespace, Value::FsNamespace) => true,
            (Value::JsonNamespace, Value::JsonNamespace) => true,
            (Value::Base64Namespace, Value::Base64Namespace) => true,
            (Value::UuidNamespace, Value::UuidNamespace) => true,
            (Value::FeatureNamespace, Value::FeatureNamespace) => true,
            (Value::LogNamespace, Value::LogNamespace) => true,
            (Value::TimeNamespace, Value::TimeNamespace) => true,
            (Value::MathNamespace, Value::MathNamespace) => true,
            (Value::HttpClientNamespace, Value::HttpClientNamespace) => true,
            (Value::DocCollection(a), Value::DocCollection(b)) => a == b,
            _ => false,
        }
    }
}

// =============================================================================
// JSON conversion helpers (used by HTTP runtime)
// =============================================================================

/// Converts a MarretaLang `Value` to a `serde_json::Value` for HTTP responses.
pub fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Integer(n) => serde_json::json!(n),
        Value::Float(n) => serde_json::json!(n),
        Value::Decimal(n) => serde_json::json!(n.to_string()),
        Value::String(s) => serde_json::json!(s),
        Value::Boolean(b) => serde_json::json!(b),
        Value::Instant(dt) => serde_json::json!(format_instant(dt)),
        Value::Date(date) => serde_json::json!(format_date(date)),
        Value::Time(time) => serde_json::json!(format_time(time)),
        Value::Duration(duration) => serde_json::json!(format_duration(duration)),
        Value::Interval(interval) => serde_json::json!({
            "start": temporal_value_to_string(&interval.start),
            "end": temporal_value_to_string(&interval.end),
        }),
        Value::Null => serde_json::Value::Null,
        Value::List(items) => serde_json::Value::Array(items.iter().map(value_to_json).collect()),
        Value::Map(m) => {
            let obj: serde_json::Map<String, serde_json::Value> = m
                .read()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), value_to_json(v)))
                .collect();
            serde_json::Value::Object(obj)
        }
        Value::RelationalRecord { fields, .. } => {
            let obj: serde_json::Map<String, serde_json::Value> = fields
                .read()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), value_to_json(v)))
                .collect();
            serde_json::Value::Object(obj)
        }
        Value::Task { name, .. } => serde_json::json!({ "task": name }),
        Value::DbNamespace
        | Value::DbTable(_)
        | Value::RelationHandle(_)
        | Value::QueryBuilder(_) => serde_json::Value::Null,
        Value::DocNamespace | Value::DocCollection(_) | Value::DocQueryBuilder(_) => {
            serde_json::Value::Null
        }
        Value::CacheNamespace => serde_json::Value::Null,
        Value::FsNamespace => serde_json::Value::Null,
        Value::JsonNamespace => serde_json::Value::Null,
        Value::Base64Namespace => serde_json::Value::Null,
        Value::UuidNamespace => serde_json::Value::Null,
        Value::FeatureNamespace => serde_json::Value::Null,
        Value::LogNamespace => serde_json::Value::Null,
        Value::TimeNamespace => serde_json::Value::Null,
        Value::MathNamespace => serde_json::Value::Null,
        Value::HttpClientNamespace => serde_json::Value::Null,
    }
}

/// Serializes a `Value` directly to its JSON form, byte-for-byte identical to
/// `value_to_json(value).to_string()`, but without building an intermediate
/// `serde_json::Value` tree. The mapping mirrors `value_to_json` exactly; the
/// two are kept in lock-step and guarded by parity tests.
impl serde::Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        match self {
            Value::Integer(n) => serializer.serialize_i64(*n),
            // `value_to_json` builds these via `serde_json::json!`, which maps a
            // non-finite f64 to Null; mirror that here.
            Value::Float(n) => {
                if n.is_finite() {
                    serializer.serialize_f64(*n)
                } else {
                    serializer.serialize_none()
                }
            }
            Value::Decimal(n) => serializer.serialize_str(&n.to_string()),
            Value::String(s) => serializer.serialize_str(s),
            Value::Boolean(b) => serializer.serialize_bool(*b),
            Value::Instant(dt) => serializer.serialize_str(&format_instant(dt)),
            Value::Date(date) => serializer.serialize_str(&format_date(date)),
            Value::Time(time) => serializer.serialize_str(&format_time(time)),
            Value::Duration(duration) => serializer.serialize_str(&format_duration(duration)),
            Value::Interval(interval) => {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("start", &temporal_value_to_string(&interval.start))?;
                map.serialize_entry("end", &temporal_value_to_string(&interval.end))?;
                map.end()
            }
            Value::Null => serializer.serialize_none(),
            Value::List(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Value::Map(m) => serialize_value_map(&m.read().unwrap(), serializer),
            Value::RelationalRecord { fields, .. } => {
                serialize_value_map(&fields.read().unwrap(), serializer)
            }
            Value::Task { name, .. } => {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("task", name)?;
                map.end()
            }
            Value::DbNamespace
            | Value::DbTable(_)
            | Value::RelationHandle(_)
            | Value::QueryBuilder(_)
            | Value::DocNamespace
            | Value::DocCollection(_)
            | Value::DocQueryBuilder(_)
            | Value::CacheNamespace
            | Value::FsNamespace
            | Value::JsonNamespace
            | Value::Base64Namespace
            | Value::UuidNamespace
            | Value::FeatureNamespace
            | Value::LogNamespace
            | Value::TimeNamespace
            | Value::MathNamespace
            | Value::HttpClientNamespace => serializer.serialize_none(),
        }
    }
}

/// Serializes a `ValueMap` as a JSON object preserving insertion order (the
/// `serde_json` `preserve_order` feature makes `value_to_json` do the same).
fn serialize_value_map<S: serde::Serializer>(
    map: &ValueMap,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut out = serializer.serialize_map(Some(map.len()))?;
    for (key, value) in map {
        out.serialize_entry(key, value)?;
    }
    out.end()
}

/// Serializes a `Value` to a JSON string directly (no intermediate
/// `serde_json::Value`), falling back to the tree-based path on the unexpected
/// chance of a serializer error so output is always identical.
pub fn value_to_json_string(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value_to_json(value).to_string())
}

/// Converts a MarretaLang `Value` to `serde_json::Value` using the strict rules
/// of the explicit `json` namespace.
pub fn value_to_json_strict(value: &Value) -> Result<serde_json::Value, String> {
    match value {
        Value::Integer(n) => Ok(serde_json::json!(n)),
        Value::Float(n) => Ok(serde_json::json!(n)),
        Value::Decimal(n) => Ok(serde_json::json!(n.to_string())),
        Value::String(s) => Ok(serde_json::json!(s)),
        Value::Boolean(b) => Ok(serde_json::json!(b)),
        Value::Instant(dt) => Ok(serde_json::json!(format_instant(dt))),
        Value::Date(date) => Ok(serde_json::json!(format_date(date))),
        Value::Time(time) => Ok(serde_json::json!(format_time(time))),
        Value::Duration(duration) => Ok(serde_json::json!(format_duration(duration))),
        Value::Interval(interval) => Ok(serde_json::json!({
            "start": temporal_value_to_string(&interval.start),
            "end": temporal_value_to_string(&interval.end),
        })),
        Value::Null => Ok(serde_json::Value::Null),
        Value::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(value_to_json_strict(item)?);
            }
            Ok(serde_json::Value::Array(out))
        }
        Value::Map(m) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in m.read().unwrap().iter() {
                obj.insert(k.clone(), value_to_json_strict(v)?);
            }
            Ok(serde_json::Value::Object(obj))
        }
        Value::RelationalRecord { fields, .. } => {
            let mut obj = serde_json::Map::new();
            for (k, v) in fields.read().unwrap().iter() {
                obj.insert(k.clone(), value_to_json_strict(v)?);
            }
            Ok(serde_json::Value::Object(obj))
        }
        other => Err(format!(
            "json values cannot serialize {}",
            other.type_name()
        )),
    }
}

/// Converts a `serde_json::Value` (from an HTTP request body) to a MarretaLang `Value`.
pub fn json_to_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Boolean(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Integer(i)
            } else {
                Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => Value::List(arr.iter().map(json_to_value).collect()),
        serde_json::Value::Object(obj) => {
            let map: ValueMap = obj
                .iter()
                .map(|(k, v)| (k.clone(), json_to_value(v)))
                .collect();
            Value::Map(Arc::new(RwLock::new(map)))
        }
    }
}

#[cfg(test)]
#[allow(clippy::approx_constant)] // sample float literals (3.14…), not the PI constant
mod tests {
    use super::*;

    // --- Truthiness ---

    #[test]
    fn test_truthy_values() {
        assert!(Value::Integer(1).is_truthy());
        assert!(Value::Integer(-1).is_truthy());
        assert!(Value::Float(0.1).is_truthy());
        assert!(Value::String("hello".into()).is_truthy());
        assert!(Value::Boolean(true).is_truthy());
        assert!(Value::List(vec![Value::Integer(1)]).is_truthy());
    }

    #[test]
    fn test_falsy_values() {
        assert!(!Value::Null.is_truthy());
        assert!(!Value::Boolean(false).is_truthy());
        assert!(!Value::Integer(0).is_truthy());
        assert!(!Value::Float(0.0).is_truthy());
        assert!(!Value::Decimal(Decimal::ZERO).is_truthy());
        assert!(!Value::String("".into()).is_truthy());
        assert!(!Value::List(vec![]).is_truthy());
        assert!(!Value::empty_map().is_truthy());
    }

    #[test]
    fn test_task_is_always_truthy() {
        let task = Value::Task {
            name: "noop".into(),
            params: vec![],
            body: TaskBody::Inline(crate::ast::Expression::Null),
            owner_module: None,
            source_module: None,
            line: 0,
            column: 0,
        };
        assert!(task.is_truthy());
    }

    // --- Type names ---

    #[test]
    fn test_type_names() {
        assert_eq!(Value::Integer(0).type_name(), "Integer");
        assert_eq!(Value::Float(0.0).type_name(), "Float");
        assert_eq!(Value::Decimal(Decimal::ZERO).type_name(), "Decimal");
        assert_eq!(Value::String("".into()).type_name(), "String");
        assert_eq!(Value::Boolean(true).type_name(), "Boolean");
        assert_eq!(Value::Null.type_name(), "Null");
        assert_eq!(Value::List(vec![]).type_name(), "List");
        assert_eq!(Value::empty_map().type_name(), "Map");
    }

    // --- Display ---

    #[test]
    fn test_display_primitives() {
        assert_eq!(format!("{}", Value::Integer(42)), "42");
        assert_eq!(format!("{}", Value::Float(3.14)), "3.14");
        assert_eq!(format!("{}", Value::Float(5.0)), "5.0");
        assert_eq!(
            format!("{}", Value::Decimal("19.90".parse().unwrap())),
            "19.90"
        );
        assert_eq!(format!("{}", Value::String("hello".into())), "hello");
        assert_eq!(format!("{}", Value::Boolean(true)), "true");
        assert_eq!(format!("{}", Value::Null), "null");
    }

    #[test]
    fn test_display_list() {
        let list = Value::List(vec![
            Value::Integer(1),
            Value::String("a".into()),
            Value::Null,
        ]);
        assert_eq!(format!("{}", list), "[1, \"a\", null]");
    }

    #[test]
    fn test_display_empty_list() {
        assert_eq!(format!("{}", Value::List(vec![])), "[]");
    }

    #[test]
    fn test_display_task() {
        let task = Value::Task {
            name: "double".into(),
            params: vec![crate::ast::ParamDef {
                name: "n".into(),
                schema: None,
            }],
            body: TaskBody::Inline(crate::ast::Expression::Null),
            owner_module: None,
            source_module: None,
            line: 0,
            column: 0,
        };
        assert_eq!(format!("{}", task), "<task double(n)>");
    }

    // --- Equality ---

    #[test]
    fn test_equality_same_types() {
        assert_eq!(Value::Integer(42), Value::Integer(42));
        assert_ne!(Value::Integer(1), Value::Integer(2));
        assert_eq!(Value::String("a".into()), Value::String("a".into()));
        assert_eq!(Value::Boolean(true), Value::Boolean(true));
        assert_eq!(Value::Null, Value::Null);
    }

    #[test]
    fn test_equality_int_float_cross() {
        assert_eq!(Value::Integer(5), Value::Float(5.0));
        assert_eq!(Value::Float(3.0), Value::Integer(3));
        assert_ne!(Value::Integer(5), Value::Float(5.1));
    }

    #[test]
    fn test_equality_decimal_cross_type_rules() {
        assert_eq!(
            Value::Decimal("5.00".parse().unwrap()),
            Value::Decimal("5".parse().unwrap())
        );
        assert_eq!(Value::Decimal("5.00".parse().unwrap()), Value::Integer(5));
        assert_eq!(Value::Integer(5), Value::Decimal("5.00".parse().unwrap()));
        assert_ne!(Value::Decimal("5.00".parse().unwrap()), Value::Float(5.0));
    }

    #[test]
    fn test_equality_different_types() {
        assert_ne!(Value::Integer(1), Value::String("1".into()));
        assert_ne!(Value::Boolean(true), Value::Integer(1));
        assert_ne!(Value::Null, Value::Boolean(false));
    }

    #[test]
    fn test_equality_lists() {
        let a = Value::List(vec![Value::Integer(1), Value::Integer(2)]);
        let b = Value::List(vec![Value::Integer(1), Value::Integer(2)]);
        let c = Value::List(vec![Value::Integer(1), Value::Integer(3)]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_equality_maps() {
        let a = Value::map_from(vec![("x".into(), Value::Integer(1))]);
        let b = Value::map_from(vec![("x".into(), Value::Integer(1))]);
        let c = Value::map_from(vec![("x".into(), Value::Integer(2))]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // --- String methods ---

    #[test]
    fn test_string_length() {
        let s = Value::String("hello".into());
        assert_eq!(s.call_method("length", &[]).unwrap(), Value::Integer(5));
    }

    #[test]
    fn test_string_upper_lower() {
        let s = Value::String("Hello".into());
        assert_eq!(
            s.call_method("upper", &[]).unwrap(),
            Value::String("HELLO".into())
        );
        assert_eq!(
            s.call_method("lower", &[]).unwrap(),
            Value::String("hello".into())
        );
    }

    #[test]
    fn test_string_trim() {
        let s = Value::String("  hi  ".into());
        assert_eq!(
            s.call_method("trim", &[]).unwrap(),
            Value::String("hi".into())
        );
    }

    #[test]
    fn test_string_contains() {
        let s = Value::String("hello world".into());
        assert_eq!(
            s.call_method("contains", &[Value::String("world".into())])
                .unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            s.call_method("contains", &[Value::String("xyz".into())])
                .unwrap(),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_string_split() {
        let s = Value::String("a,b,c".into());
        let result = s
            .call_method("split", &[Value::String(",".into())])
            .unwrap();
        assert_eq!(
            result,
            Value::List(vec![
                Value::String("a".into()),
                Value::String("b".into()),
                Value::String("c".into()),
            ])
        );
    }

    #[test]
    fn test_string_replace() {
        let s = Value::String("hello world".into());
        let result = s
            .call_method(
                "replace",
                &[
                    Value::String("world".into()),
                    Value::String("marreta".into()),
                ],
            )
            .unwrap();
        assert_eq!(result, Value::String("hello marreta".into()));
    }

    #[test]
    fn test_decimal_methods_and_json_serialization() {
        let value = Value::Decimal("19.90".parse().unwrap());

        assert_eq!(
            value.call_method("round", &[Value::Integer(0)]).unwrap(),
            Value::Decimal("20".parse().unwrap())
        );
        assert_eq!(
            Value::Decimal("2.345".parse().unwrap())
                .call_method("round", &[Value::Integer(2)])
                .unwrap(),
            Value::Decimal("2.34".parse().unwrap())
        );
        assert_eq!(
            Value::Decimal("-19.90".parse().unwrap())
                .call_method("trunc", &[])
                .unwrap(),
            Value::Decimal("-19".parse().unwrap())
        );
        assert_eq!(
            Value::Decimal("-19.90".parse().unwrap())
                .call_method("abs", &[])
                .unwrap(),
            Value::Decimal("19.90".parse().unwrap())
        );
        assert_eq!(
            Value::Decimal("19.10".parse().unwrap())
                .call_method("ceil", &[])
                .unwrap(),
            Value::Decimal("20".parse().unwrap())
        );
        assert_eq!(
            value.call_method("floor", &[]).unwrap(),
            Value::Decimal("19".parse().unwrap())
        );
        assert_eq!(value.call_method("scale", &[]).unwrap(), Value::Integer(2));
        assert_eq!(
            value.call_method("to_string", &[]).unwrap(),
            Value::String("19.90".into())
        );
        assert_eq!(
            Value::Decimal("-19.90".parse().unwrap())
                .call_method("to_integer", &[])
                .unwrap(),
            Value::Integer(-19)
        );
        assert_eq!(
            Value::Decimal("1.25".parse().unwrap())
                .call_method("to_float", &[])
                .unwrap(),
            Value::Float(1.25)
        );
        assert_eq!(value_to_json(&value), serde_json::json!("19.90"));
        assert_eq!(
            value_to_json_strict(&value).unwrap(),
            serde_json::json!("19.90")
        );
    }

    #[test]
    fn test_list_sum() {
        let list = Value::List(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        assert_eq!(list.call_method("sum", &[]).unwrap(), Value::Integer(6));
    }

    #[test]
    fn test_list_mean_median_std_dev() {
        let list = Value::List(vec![
            Value::Float(7.5),
            Value::Float(8.0),
            Value::Float(6.5),
            Value::Float(9.0),
            Value::Float(7.0),
        ]);
        assert_eq!(list.call_method("mean", &[]).unwrap(), Value::Float(7.6));
        assert_eq!(list.call_method("median", &[]).unwrap(), Value::Float(7.5));
        let std_dev = list.call_method("std_dev", &[]).unwrap();
        match std_dev {
            Value::Float(v) => assert!((v - 0.8602325267).abs() < 0.0001),
            other => panic!("expected Float, got {:?}", other),
        }
    }

    #[test]
    fn test_list_stats_empty_return_null() {
        let list = Value::List(vec![]);
        assert_eq!(list.call_method("mean", &[]).unwrap(), Value::Null);
        assert_eq!(list.call_method("median", &[]).unwrap(), Value::Null);
        assert_eq!(list.call_method("std_dev", &[]).unwrap(), Value::Null);
    }

    #[test]
    fn test_list_zip() {
        let left = Value::List(vec![
            Value::String("Ana".into()),
            Value::String("Joao".into()),
        ]);
        let zipped = left
            .call_method(
                "zip",
                &[Value::List(vec![Value::Integer(30), Value::Integer(25)])],
            )
            .unwrap();
        assert_eq!(
            zipped,
            Value::List(vec![
                Value::List(vec![Value::String("Ana".into()), Value::Integer(30)]),
                Value::List(vec![Value::String("Joao".into()), Value::Integer(25)]),
            ])
        );
    }

    #[test]
    fn test_list_zip_length_mismatch_is_runtime_error() {
        let left = Value::List(vec![Value::Integer(1)]);
        let err = left
            .call_method(
                "zip",
                &[Value::List(vec![Value::Integer(1), Value::Integer(2)])],
            )
            .unwrap_err();
        assert!(matches!(err, MarretaError::RuntimeError { .. }));
    }

    #[test]
    fn test_scalar_conversions() {
        assert_eq!(
            Value::String("25".into())
                .call_method("to_integer", &[])
                .unwrap(),
            Value::Integer(25)
        );
        assert_eq!(
            Value::String("19.90".into())
                .call_method("to_float", &[])
                .unwrap(),
            Value::Float(19.9)
        );
        assert_eq!(
            Value::Integer(42).call_method("to_string", &[]).unwrap(),
            Value::String("42".into())
        );
        assert_eq!(
            Value::Null.call_method("to_boolean", &[]).unwrap(),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_invalid_numeric_string_conversions_fallback_to_zero() {
        assert_eq!(
            Value::String("abc".into())
                .call_method("to_integer", &[])
                .unwrap(),
            Value::Integer(0)
        );
        assert_eq!(
            Value::String("abc".into())
                .call_method("to_float", &[])
                .unwrap(),
            Value::Float(0.0)
        );
    }

    // --- List methods ---

    #[test]
    fn test_list_length() {
        let l = Value::List(vec![Value::Integer(1), Value::Integer(2)]);
        assert_eq!(l.call_method("length", &[]).unwrap(), Value::Integer(2));
    }

    #[test]
    fn test_list_first_last() {
        let l = Value::List(vec![
            Value::Integer(10),
            Value::Integer(20),
            Value::Integer(30),
        ]);
        assert_eq!(l.call_method("first", &[]).unwrap(), Value::Integer(10));
        assert_eq!(l.call_method("last", &[]).unwrap(), Value::Integer(30));
    }

    #[test]
    fn test_list_first_last_empty() {
        let l = Value::List(vec![]);
        assert_eq!(l.call_method("first", &[]).unwrap(), Value::Null);
        assert_eq!(l.call_method("last", &[]).unwrap(), Value::Null);
    }

    #[test]
    fn test_list_empty_question() {
        assert_eq!(
            Value::List(vec![]).call_method("empty?", &[]).unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            Value::List(vec![Value::Null])
                .call_method("empty?", &[])
                .unwrap(),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_list_push() {
        let l = Value::List(vec![Value::Integer(1)]);
        let result = l.call_method("push", &[Value::Integer(2)]).unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::Integer(1), Value::Integer(2)])
        );
        // Original is unchanged (returns new list)
        assert_eq!(l, Value::List(vec![Value::Integer(1)]));
    }

    #[test]
    fn test_list_includes() {
        let l = Value::List(vec![Value::Integer(1), Value::String("a".into())]);
        assert_eq!(
            l.call_method("includes", &[Value::Integer(1)]).unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            l.call_method("includes", &[Value::Integer(99)]).unwrap(),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_list_reverse() {
        let l = Value::List(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        let result = l.call_method("reverse", &[]).unwrap();
        assert_eq!(
            result,
            Value::List(vec![
                Value::Integer(3),
                Value::Integer(2),
                Value::Integer(1)
            ])
        );
    }

    // --- Map methods ---

    #[test]
    fn test_map_keys_values() {
        let m = Value::map_from(vec![("a".into(), Value::Integer(1))]);
        let keys = m.call_method("keys", &[]).unwrap();
        if let Value::List(k) = keys {
            assert_eq!(k.len(), 1);
            assert_eq!(k[0], Value::String("a".into()));
        } else {
            panic!("expected List");
        }
    }

    #[test]
    fn test_map_has() {
        let m = Value::map_from(vec![("x".into(), Value::Integer(1))]);
        assert_eq!(
            m.call_method("has", &[Value::String("x".into())]).unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            m.call_method("has", &[Value::String("y".into())]).unwrap(),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_map_merge() {
        let a = Value::map_from(vec![("x".into(), Value::Integer(1))]);
        let b = Value::map_from(vec![("y".into(), Value::Integer(2))]);
        let merged = a.call_method("merge", &[b]).unwrap();
        if let Value::Map(m) = merged {
            let m = m.read().unwrap();
            assert_eq!(m.len(), 2);
            assert_eq!(m.get("x"), Some(&Value::Integer(1)));
            assert_eq!(m.get("y"), Some(&Value::Integer(2)));
        } else {
            panic!("expected Map");
        }
    }

    // --- Integer/Float methods ---

    #[test]
    fn test_integer_abs() {
        assert_eq!(
            Value::Integer(-5).call_method("abs", &[]).unwrap(),
            Value::Integer(5)
        );
        assert_eq!(
            Value::Integer(5).call_method("abs", &[]).unwrap(),
            Value::Integer(5)
        );
    }

    #[test]
    fn test_float_abs() {
        assert_eq!(
            Value::Float(-3.14).call_method("abs", &[]).unwrap(),
            Value::Float(3.14)
        );
    }

    #[test]
    fn test_to_string_method() {
        assert_eq!(
            Value::Integer(42).call_method("to_string", &[]).unwrap(),
            Value::String("42".into())
        );
        assert_eq!(
            Value::Float(3.14).call_method("to_string", &[]).unwrap(),
            Value::String("3.14".into())
        );
        assert_eq!(
            Value::String("hi".into())
                .call_method("to_string", &[])
                .unwrap(),
            Value::String("hi".into())
        );
    }

    // --- Error cases ---

    #[test]
    fn test_unknown_method_error() {
        let result = Value::Integer(1).call_method("nonexistent", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_arg_type_error() {
        let s = Value::String("hello".into());
        let result = s.call_method("contains", &[Value::Integer(1)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_arg_error() {
        let s = Value::String("hello".into());
        let result = s.call_method("contains", &[]);
        assert!(result.is_err());
    }

    // --- value_to_json tests ---

    #[test]
    fn test_value_to_json_integer() {
        assert_eq!(value_to_json(&Value::Integer(42)), serde_json::json!(42));
    }

    #[test]
    fn test_value_to_json_float() {
        assert_eq!(value_to_json(&Value::Float(3.14)), serde_json::json!(3.14));
    }

    #[test]
    fn test_value_to_json_string() {
        assert_eq!(
            value_to_json(&Value::String("hello".into())),
            serde_json::json!("hello")
        );
    }

    #[test]
    fn test_value_to_json_boolean_true() {
        assert_eq!(
            value_to_json(&Value::Boolean(true)),
            serde_json::json!(true)
        );
    }

    #[test]
    fn test_value_to_json_boolean_false() {
        assert_eq!(
            value_to_json(&Value::Boolean(false)),
            serde_json::json!(false)
        );
    }

    #[test]
    fn test_value_to_json_null() {
        assert_eq!(value_to_json(&Value::Null), serde_json::Value::Null);
    }

    #[test]
    fn test_value_to_json_list() {
        let list = Value::List(vec![Value::Integer(1), Value::Integer(2)]);
        assert_eq!(value_to_json(&list), serde_json::json!([1, 2]));
    }

    #[test]
    fn test_value_to_json_map() {
        let m = Value::map_from(vec![("x".into(), Value::Integer(10))]);
        let json = value_to_json(&m);
        assert_eq!(json["x"], serde_json::json!(10));
    }

    #[test]
    fn test_value_to_json_nested_list() {
        let inner = Value::List(vec![Value::String("a".into())]);
        let outer = Value::List(vec![inner]);
        let json = value_to_json(&outer);
        assert_eq!(json[0][0], "a");
    }

    #[test]
    fn test_value_to_json_temporal_values() {
        let instant = Value::Instant(Utc.with_ymd_and_hms(2026, 4, 27, 13, 10, 45).unwrap());
        let date = Value::Date(NaiveDate::from_ymd_opt(2026, 4, 27).unwrap());
        let time = Value::Time(NaiveTime::from_hms_opt(9, 30, 0).unwrap());
        let duration = Value::Duration(ChronoDuration::minutes(90));
        let interval = Value::Interval(TemporalInterval {
            start: TemporalValue::Date(NaiveDate::from_ymd_opt(2026, 4, 27).unwrap()),
            end: TemporalValue::Date(NaiveDate::from_ymd_opt(2026, 4, 30).unwrap()),
        });

        assert_eq!(
            value_to_json(&instant),
            serde_json::json!("2026-04-27T13:10:45Z")
        );
        assert_eq!(value_to_json(&date), serde_json::json!("2026-04-27"));
        assert_eq!(value_to_json(&time), serde_json::json!("09:30:00"));
        assert_eq!(value_to_json(&duration), serde_json::json!("PT5400S"));
        assert_eq!(
            value_to_json(&interval),
            serde_json::json!({
                "start": "2026-04-27",
                "end": "2026-04-30"
            })
        );
    }

    #[test]
    fn value_to_json_string_matches_value_to_json_tostring() {
        fn map_of(pairs: &[(&str, Value)]) -> Value {
            let mut m = ValueMap::new();
            for (k, v) in pairs {
                m.insert((*k).to_string(), v.clone());
            }
            Value::Map(Arc::new(RwLock::new(m)))
        }

        let cases = vec![
            Value::Null,
            Value::Boolean(true),
            Value::Boolean(false),
            Value::Integer(0),
            Value::Integer(-42),
            Value::Integer(i64::MIN),
            Value::Float(3.0),
            Value::Float(-0.5),
            Value::Float(1.0e10),
            // non-finite floats must both render as null
            Value::Float(f64::NAN),
            Value::Float(f64::INFINITY),
            Value::Decimal(Decimal::new(199900, 2)),
            Value::String("plain".into()),
            Value::String("quote\"back\\slash\nnew\ttab".into()),
            Value::String("unicode: café ☕ \u{1F600}".into()),
            Value::List(vec![
                Value::Integer(1),
                Value::String("two".into()),
                Value::Null,
            ]),
            // key order must be preserved (insertion order)
            map_of(&[
                ("z", Value::Integer(1)),
                ("a", Value::Integer(2)),
                ("m", Value::String("x".into())),
            ]),
            // nested + mixed
            map_of(&[
                (
                    "list",
                    Value::List(vec![Value::Boolean(false), Value::Float(2.5)]),
                ),
                ("nested", map_of(&[("deep", Value::Null)])),
                ("dec", Value::Decimal(Decimal::new(5, 1))),
            ]),
            Value::Instant(Utc.with_ymd_and_hms(2026, 4, 27, 13, 10, 45).unwrap()),
            Value::Interval(TemporalInterval {
                start: TemporalValue::Date(NaiveDate::from_ymd_opt(2026, 4, 27).unwrap()),
                end: TemporalValue::Date(NaiveDate::from_ymd_opt(2026, 4, 30).unwrap()),
            }),
            Value::DbNamespace,
        ];

        for value in cases {
            assert_eq!(
                value_to_json_string(&value),
                value_to_json(&value).to_string(),
                "mismatch for {value:?}"
            );
        }
    }

    // --- json_to_value tests ---

    #[test]
    fn test_json_to_value_null() {
        assert_eq!(json_to_value(&serde_json::Value::Null), Value::Null);
    }

    #[test]
    fn test_json_to_value_bool() {
        assert_eq!(
            json_to_value(&serde_json::json!(true)),
            Value::Boolean(true)
        );
        assert_eq!(
            json_to_value(&serde_json::json!(false)),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_json_to_value_integer() {
        assert_eq!(json_to_value(&serde_json::json!(42)), Value::Integer(42));
    }

    #[test]
    fn test_json_to_value_float() {
        assert_eq!(json_to_value(&serde_json::json!(3.14)), Value::Float(3.14));
    }

    #[test]
    fn test_json_to_value_string() {
        assert_eq!(
            json_to_value(&serde_json::json!("hello")),
            Value::String("hello".into())
        );
    }

    #[test]
    fn test_json_to_value_array() {
        let json = serde_json::json!([1, 2, 3]);
        let val = json_to_value(&json);
        assert_eq!(
            val,
            Value::List(vec![
                Value::Integer(1),
                Value::Integer(2),
                Value::Integer(3),
            ])
        );
    }

    #[test]
    fn test_json_to_value_object() {
        let json = serde_json::json!({ "name": "alice", "age": 30 });
        let val = json_to_value(&json);
        if let Value::Map(m) = val {
            let m = m.read().unwrap();
            assert_eq!(m.get("name"), Some(&Value::String("alice".into())));
            assert_eq!(m.get("age"), Some(&Value::Integer(30)));
        } else {
            panic!("expected Map");
        }
    }

    #[test]
    fn test_json_roundtrip() {
        // Value → JSON → Value → JSON should produce the same JSON
        let original = Value::map_from(vec![
            ("id".into(), Value::Integer(1)),
            ("active".into(), Value::Boolean(true)),
            ("name".into(), Value::String("test".into())),
        ]);
        let json = value_to_json(&original);
        let restored = json_to_value(&json);
        let json2 = value_to_json(&restored);
        assert_eq!(json, json2);
    }

    // --- type_name for Task/DbNamespace/DbTable/QueryBuilder ---

    #[test]
    fn test_type_names_task_db_variants() {
        use crate::ast::{Expression, TaskBody};
        use crate::db::driver::QueryState;
        let task = Value::Task {
            name: "f".into(),
            params: vec![],
            body: TaskBody::Inline(Expression::Null),
            owner_module: None,
            source_module: None,
            line: 0,
            column: 0,
        };
        assert_eq!(task.type_name(), "Task");
        assert_eq!(Value::DbNamespace.type_name(), "DbNamespace");
        assert_eq!(Value::DbTable("users".into()).type_name(), "DbTable");
        assert_eq!(
            Value::QueryBuilder(Box::new(QueryState::new("users"))).type_name(),
            "QueryBuilder"
        );
    }

    // --- is_truthy for db/task variants ---

    #[test]
    fn test_db_namespace_is_falsy() {
        assert!(!Value::DbNamespace.is_truthy());
    }

    #[test]
    fn test_db_table_is_falsy() {
        assert!(!Value::DbTable("orders".into()).is_truthy());
    }

    #[test]
    fn test_query_builder_is_falsy() {
        use crate::db::driver::QueryState;
        assert!(!Value::QueryBuilder(Box::new(QueryState::new("t"))).is_truthy());
    }

    // --- call_method on non-method types returns PropertyNotFound ---

    #[test]
    fn test_call_method_null_returns_error() {
        let err = Value::Null.call_method("length", &[]).unwrap_err();
        assert!(matches!(err, MarretaError::PropertyNotFound { .. }));
        if let MarretaError::PropertyNotFound {
            object_type,
            property,
            ..
        } = err
        {
            assert_eq!(object_type, "Null");
            assert_eq!(property, "length");
        }
    }

    #[test]
    fn test_call_method_boolean_returns_error() {
        let err = Value::Boolean(true).call_method("upper", &[]).unwrap_err();
        assert!(
            matches!(err, MarretaError::PropertyNotFound { object_type, .. } if object_type == "Boolean")
        );
    }

    #[test]
    fn test_call_method_task_returns_error() {
        use crate::ast::{Expression, TaskBody};
        let task = Value::Task {
            name: "f".into(),
            params: vec![],
            body: TaskBody::Inline(Expression::Null),
            owner_module: None,
            source_module: None,
            line: 0,
            column: 0,
        };
        let err = task.call_method("length", &[]).unwrap_err();
        assert!(
            matches!(err, MarretaError::PropertyNotFound { object_type, .. } if object_type == "Task")
        );
    }

    #[test]
    fn test_call_method_db_namespace_returns_error() {
        let err = Value::DbNamespace.call_method("keys", &[]).unwrap_err();
        assert!(
            matches!(err, MarretaError::PropertyNotFound { object_type, .. } if object_type == "DbNamespace")
        );
    }

    // --- String: starts_with, ends_with, index_of ---

    #[test]
    fn test_string_starts_with() {
        let s = Value::String("hello world".into());
        assert_eq!(
            s.call_method("starts_with", &[Value::String("hello".into())])
                .unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            s.call_method("starts_with", &[Value::String("world".into())])
                .unwrap(),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_string_ends_with() {
        let s = Value::String("hello world".into());
        assert_eq!(
            s.call_method("ends_with", &[Value::String("world".into())])
                .unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            s.call_method("ends_with", &[Value::String("hello".into())])
                .unwrap(),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_string_index_of_found() {
        let s = Value::String("hello".into());
        assert_eq!(
            s.call_method("index_of", &[Value::String("ell".into())])
                .unwrap(),
            Value::Integer(1)
        );
    }

    #[test]
    fn test_string_index_of_not_found() {
        let s = Value::String("hello".into());
        assert_eq!(
            s.call_method("index_of", &[Value::String("xyz".into())])
                .unwrap(),
            Value::Integer(-1)
        );
    }

    #[test]
    fn test_string_to_string_method() {
        let s = Value::String("abc".into());
        assert_eq!(
            s.call_method("to_string", &[]).unwrap(),
            Value::String("abc".into())
        );
    }

    #[test]
    fn test_string_unknown_method_returns_error() {
        let err = Value::String("x".into())
            .call_method("nonexistent", &[])
            .unwrap_err();
        assert!(
            matches!(err, MarretaError::PropertyNotFound { object_type, .. } if object_type == "String")
        );
    }

    #[test]
    fn test_string_starts_with_wrong_arg_type() {
        let err = Value::String("hello".into())
            .call_method("starts_with", &[Value::Integer(1)])
            .unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    // --- List: join, sort, unique, flatten, slice, error branches ---

    #[test]
    fn test_list_join() {
        let l = Value::List(vec![
            Value::String("a".into()),
            Value::String("b".into()),
            Value::String("c".into()),
        ]);
        assert_eq!(
            l.call_method("join", &[Value::String(",".into())]).unwrap(),
            Value::String("a,b,c".into())
        );
    }

    #[test]
    fn test_list_sort_integers() {
        let l = Value::List(vec![
            Value::Integer(3),
            Value::Integer(1),
            Value::Integer(2),
        ]);
        assert_eq!(
            l.call_method("sort", &[]).unwrap(),
            Value::List(vec![
                Value::Integer(1),
                Value::Integer(2),
                Value::Integer(3)
            ])
        );
    }

    #[test]
    fn test_list_sort_strings() {
        let l = Value::List(vec![
            Value::String("banana".into()),
            Value::String("apple".into()),
            Value::String("cherry".into()),
        ]);
        assert_eq!(
            l.call_method("sort", &[]).unwrap(),
            Value::List(vec![
                Value::String("apple".into()),
                Value::String("banana".into()),
                Value::String("cherry".into()),
            ])
        );
    }

    #[test]
    fn test_list_sort_mixed_types() {
        // integers come before strings by type_order
        let l = Value::List(vec![Value::String("a".into()), Value::Integer(1)]);
        let result = l.call_method("sort", &[]).unwrap();
        if let Value::List(items) = result {
            assert!(matches!(items[0], Value::Integer(_)));
            assert!(matches!(items[1], Value::String(_)));
        } else {
            panic!("expected List");
        }
    }

    #[test]
    fn test_list_unique() {
        let l = Value::List(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(1),
            Value::Integer(3),
        ]);
        assert_eq!(
            l.call_method("unique", &[]).unwrap(),
            Value::List(vec![
                Value::Integer(1),
                Value::Integer(2),
                Value::Integer(3)
            ])
        );
    }

    #[test]
    fn test_list_flatten() {
        let l = Value::List(vec![
            Value::List(vec![Value::Integer(1), Value::Integer(2)]),
            Value::Integer(3),
            Value::List(vec![Value::Integer(4)]),
        ]);
        assert_eq!(
            l.call_method("flatten", &[]).unwrap(),
            Value::List(vec![
                Value::Integer(1),
                Value::Integer(2),
                Value::Integer(3),
                Value::Integer(4),
            ])
        );
    }

    #[test]
    fn test_list_slice() {
        let l = Value::List(vec![
            Value::Integer(10),
            Value::Integer(20),
            Value::Integer(30),
            Value::Integer(40),
        ]);
        assert_eq!(
            l.call_method("slice", &[Value::Integer(1), Value::Integer(3)])
                .unwrap(),
            Value::List(vec![Value::Integer(20), Value::Integer(30)])
        );
    }

    #[test]
    fn test_list_slice_out_of_bounds() {
        let l = Value::List(vec![Value::Integer(1), Value::Integer(2)]);
        // beyond length — clamps to len
        assert_eq!(
            l.call_method("slice", &[Value::Integer(0), Value::Integer(100)])
                .unwrap(),
            Value::List(vec![Value::Integer(1), Value::Integer(2)])
        );
    }

    #[test]
    fn test_list_push_no_args_error() {
        let err = Value::List(vec![]).call_method("push", &[]).unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    #[test]
    fn test_list_includes_no_args_error() {
        let err = Value::List(vec![])
            .call_method("includes", &[])
            .unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    #[test]
    fn test_list_slice_wrong_arg_type_error() {
        let err = Value::List(vec![Value::Integer(1)])
            .call_method("slice", &[Value::String("x".into()), Value::Integer(1)])
            .unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    #[test]
    fn test_list_slice_missing_second_arg_error() {
        let err = Value::List(vec![Value::Integer(1)])
            .call_method("slice", &[Value::Integer(0)])
            .unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    #[test]
    fn test_list_slice_missing_first_arg_error() {
        let err = Value::List(vec![Value::Integer(1)])
            .call_method("slice", &[])
            .unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    #[test]
    fn test_list_unknown_method_returns_error() {
        let err = Value::List(vec![])
            .call_method("nonexistent", &[])
            .unwrap_err();
        assert!(
            matches!(err, MarretaError::PropertyNotFound { object_type, .. } if object_type == "List")
        );
    }

    // --- Map: values, delete, size, merge error cases ---

    #[test]
    fn test_map_values() {
        let m = Value::map_from(vec![("a".into(), Value::Integer(42))]);
        let vals = m.call_method("values", &[]).unwrap();
        if let Value::List(v) = vals {
            assert_eq!(v.len(), 1);
            assert_eq!(v[0], Value::Integer(42));
        } else {
            panic!("expected List");
        }
    }

    #[test]
    fn test_map_delete() {
        let m = Value::map_from(vec![
            ("a".into(), Value::Integer(1)),
            ("b".into(), Value::Integer(2)),
        ]);
        let result = m
            .call_method("delete", &[Value::String("a".into())])
            .unwrap();
        if let Value::Map(map) = result {
            let map = map.read().unwrap();
            assert_eq!(map.len(), 1);
            assert!(!map.contains_key("a"));
            assert!(map.contains_key("b"));
        } else {
            panic!("expected Map");
        }
    }

    #[test]
    fn test_map_size() {
        let m = Value::map_from(vec![
            ("x".into(), Value::Integer(1)),
            ("y".into(), Value::Integer(2)),
            ("z".into(), Value::Integer(3)),
        ]);
        assert_eq!(m.call_method("size", &[]).unwrap(), Value::Integer(3));
    }

    #[test]
    fn test_map_merge_no_args_error() {
        let m = Value::map_from(vec![]);
        let err = m.call_method("merge", &[]).unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    #[test]
    fn test_map_merge_non_map_error() {
        let m = Value::map_from(vec![]);
        let err = m.call_method("merge", &[Value::Integer(1)]).unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    #[test]
    fn test_map_unknown_method_returns_error() {
        let m = Value::empty_map();
        let err = m.call_method("nonexistent", &[]).unwrap_err();
        assert!(
            matches!(err, MarretaError::PropertyNotFound { object_type, .. } if object_type == "Map")
        );
    }

    // --- Integer: min, max, to_string, error cases ---

    #[test]
    fn test_integer_to_string_method() {
        assert_eq!(
            Value::Integer(99).call_method("to_string", &[]).unwrap(),
            Value::String("99".into())
        );
    }

    #[test]
    fn test_integer_min_with_integer() {
        assert_eq!(
            Value::Integer(5)
                .call_method("min", &[Value::Integer(3)])
                .unwrap(),
            Value::Integer(3)
        );
        assert_eq!(
            Value::Integer(5)
                .call_method("min", &[Value::Integer(10)])
                .unwrap(),
            Value::Integer(5)
        );
    }

    #[test]
    fn test_integer_min_with_float() {
        assert_eq!(
            Value::Integer(5)
                .call_method("min", &[Value::Float(2.5)])
                .unwrap(),
            Value::Float(2.5)
        );
    }

    #[test]
    fn test_integer_max_with_integer() {
        assert_eq!(
            Value::Integer(5)
                .call_method("max", &[Value::Integer(10)])
                .unwrap(),
            Value::Integer(10)
        );
    }

    #[test]
    fn test_integer_max_with_float() {
        assert_eq!(
            Value::Integer(5)
                .call_method("max", &[Value::Float(7.5)])
                .unwrap(),
            Value::Float(7.5)
        );
    }

    #[test]
    fn test_integer_min_wrong_type_error() {
        let err = Value::Integer(5)
            .call_method("min", &[Value::String("x".into())])
            .unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    #[test]
    fn test_integer_min_missing_arg_error() {
        let err = Value::Integer(5).call_method("min", &[]).unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    #[test]
    fn test_integer_max_wrong_type_error() {
        let err = Value::Integer(5)
            .call_method("max", &[Value::Boolean(true)])
            .unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    #[test]
    fn test_integer_max_missing_arg_error() {
        let err = Value::Integer(5).call_method("max", &[]).unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    #[test]
    fn test_integer_unknown_method_returns_error() {
        let err = Value::Integer(1).call_method("floor", &[]).unwrap_err();
        assert!(
            matches!(err, MarretaError::PropertyNotFound { object_type, .. } if object_type == "Integer")
        );
    }

    // --- Float: round, floor, ceil, min, max, to_string, error cases ---

    #[test]
    fn test_float_round_no_args() {
        assert_eq!(
            Value::Float(3.7).call_method("round", &[]).unwrap(),
            Value::Float(4.0)
        );
        assert_eq!(
            Value::Float(3.2).call_method("round", &[]).unwrap(),
            Value::Float(3.0)
        );
    }

    #[test]
    fn test_float_round_with_places() {
        assert_eq!(
            Value::Float(3.14159)
                .call_method("round", &[Value::Integer(2)])
                .unwrap(),
            Value::Float(3.14)
        );
    }

    #[test]
    fn test_float_round_wrong_arg_type_error() {
        let err = Value::Float(3.14)
            .call_method("round", &[Value::String("x".into())])
            .unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    #[test]
    fn test_float_floor() {
        assert_eq!(
            Value::Float(3.9).call_method("floor", &[]).unwrap(),
            Value::Float(3.0)
        );
    }

    #[test]
    fn test_float_ceil() {
        assert_eq!(
            Value::Float(3.1).call_method("ceil", &[]).unwrap(),
            Value::Float(4.0)
        );
    }

    #[test]
    fn test_float_to_string_method() {
        assert_eq!(
            Value::Float(1.5).call_method("to_string", &[]).unwrap(),
            Value::String("1.5".into())
        );
    }

    #[test]
    fn test_float_min_with_integer() {
        assert_eq!(
            Value::Float(5.0)
                .call_method("min", &[Value::Integer(3)])
                .unwrap(),
            Value::Float(3.0)
        );
    }

    #[test]
    fn test_float_min_with_float() {
        assert_eq!(
            Value::Float(5.0)
                .call_method("min", &[Value::Float(2.5)])
                .unwrap(),
            Value::Float(2.5)
        );
    }

    #[test]
    fn test_float_max_with_integer() {
        assert_eq!(
            Value::Float(5.0)
                .call_method("max", &[Value::Integer(10)])
                .unwrap(),
            Value::Float(10.0)
        );
    }

    #[test]
    fn test_float_max_with_float() {
        assert_eq!(
            Value::Float(5.0)
                .call_method("max", &[Value::Float(7.5)])
                .unwrap(),
            Value::Float(7.5)
        );
    }

    #[test]
    fn test_float_min_wrong_type_error() {
        let err = Value::Float(1.0)
            .call_method("min", &[Value::Boolean(true)])
            .unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    #[test]
    fn test_float_min_missing_arg_error() {
        let err = Value::Float(1.0).call_method("min", &[]).unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    #[test]
    fn test_float_max_wrong_type_error() {
        let err = Value::Float(1.0)
            .call_method("max", &[Value::String("x".into())])
            .unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    #[test]
    fn test_float_max_missing_arg_error() {
        let err = Value::Float(1.0).call_method("max", &[]).unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }

    #[test]
    fn test_float_unknown_method_returns_error() {
        let err = Value::Float(1.0)
            .call_method("nonexistent", &[])
            .unwrap_err();
        assert!(
            matches!(err, MarretaError::PropertyNotFound { object_type, .. } if object_type == "Float")
        );
    }

    // --- Display for db/task/query variants ---

    #[test]
    fn test_display_db_namespace() {
        assert_eq!(format!("{}", Value::DbNamespace), "<db>");
    }

    #[test]
    fn test_display_db_table() {
        assert_eq!(
            format!("{}", Value::DbTable("orders".into())),
            "<db.orders>"
        );
    }

    #[test]
    fn test_display_query_builder() {
        use crate::db::driver::QueryState;
        let q = Value::QueryBuilder(Box::new(QueryState::new("products")));
        assert_eq!(format!("{}", q), "<query:products>");
    }

    // --- value_to_json: Task and DB variants ---

    #[test]
    fn test_value_to_json_task_variant() {
        use crate::ast::{Expression, TaskBody};
        let task = Value::Task {
            name: "my_task".into(),
            params: vec![],
            body: TaskBody::Inline(Expression::Null),
            owner_module: None,
            source_module: None,
            line: 0,
            column: 0,
        };
        let json = value_to_json(&task);
        assert_eq!(json["task"], "my_task");
    }

    #[test]
    fn test_value_to_json_db_namespace_is_null() {
        assert_eq!(value_to_json(&Value::DbNamespace), serde_json::Value::Null);
    }

    #[test]
    fn test_value_to_json_db_table_is_null() {
        assert_eq!(
            value_to_json(&Value::DbTable("t".into())),
            serde_json::Value::Null
        );
    }

    #[test]
    fn test_value_to_json_query_builder_is_null() {
        use crate::db::driver::QueryState;
        let q = Value::QueryBuilder(Box::new(QueryState::new("t")));
        assert_eq!(value_to_json(&q), serde_json::Value::Null);
    }

    // --- PartialEq for db variants ---

    #[test]
    fn test_db_namespace_equals_itself() {
        assert_eq!(Value::DbNamespace, Value::DbNamespace);
    }

    #[test]
    fn test_db_table_equals_same_name() {
        assert_eq!(
            Value::DbTable("users".into()),
            Value::DbTable("users".into())
        );
    }

    #[test]
    fn test_db_table_not_equals_different_name() {
        assert_ne!(
            Value::DbTable("users".into()),
            Value::DbTable("orders".into())
        );
    }

    #[test]
    fn test_db_namespace_not_equal_to_db_table() {
        assert_ne!(Value::DbNamespace, Value::DbTable("x".into()));
    }

    // --- Sort with floats ---

    #[test]
    fn test_list_sort_floats() {
        let l = Value::List(vec![
            Value::Float(2.5),
            Value::Float(1.1),
            Value::Float(3.0),
        ]);
        assert_eq!(
            l.call_method("sort", &[]).unwrap(),
            Value::List(vec![
                Value::Float(1.1),
                Value::Float(2.5),
                Value::Float(3.0)
            ])
        );
    }

    // --- List slice with second arg wrong type ---

    #[test]
    fn test_list_slice_second_arg_wrong_type_error() {
        let err = Value::List(vec![Value::Integer(1), Value::Integer(2)])
            .call_method("slice", &[Value::Integer(0), Value::String("x".into())])
            .unwrap_err();
        assert!(matches!(err, MarretaError::TypeError { .. }));
    }
}
