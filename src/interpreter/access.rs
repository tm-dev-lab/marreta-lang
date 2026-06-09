use super::*;

impl Interpreter {
    pub(super) fn access_property(
        &self,
        obj: &Value,
        property: &str,
    ) -> Result<Value, MarretaError> {
        match obj {
            Value::Map(map) => {
                let map = map.read().unwrap();
                Ok(map.get(property).cloned().unwrap_or(Value::Null))
            }
            Value::RelationalRecord {
                schema_name,
                fields,
            } => {
                if let Some(value) = fields.read().unwrap().get(property).cloned() {
                    return Ok(value);
                }
                if let Some(handle) =
                    self.relation_handle_for_property(schema_name, fields, property)
                {
                    return Ok(handle);
                }
                Ok(Value::Null)
            }
            Value::Instant(dt) => self.access_instant_property(dt, property),
            Value::Date(date) => self.access_date_property(date, property),
            Value::Time(time) => self.access_time_property(time, property),
            Value::Duration(duration) => self.access_duration_property(*duration, property),
            Value::Interval(interval) => self.access_interval_property(interval, property),
            _ => Err(MarretaError::PropertyNotFound {
                object_type: obj.type_name().into(),
                property: property.into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn access_instant_property(
        &self,
        dt: &chrono::DateTime<Utc>,
        property: &str,
    ) -> Result<Value, MarretaError> {
        let local = dt.with_timezone(&self.current_timezone());
        match property {
            "year" => Ok(Value::Integer(local.year() as i64)),
            "month" => Ok(Value::Integer(local.month() as i64)),
            "day" => Ok(Value::Integer(local.day() as i64)),
            "hour" => Ok(Value::Integer(local.hour() as i64)),
            "minute" => Ok(Value::Integer(local.minute() as i64)),
            "second" => Ok(Value::Integer(local.second() as i64)),
            "weekday" => Ok(Value::Integer(local.weekday().num_days_from_monday() as i64)),
            "unix" => Ok(Value::Integer(dt.timestamp())),
            "date" => Ok(Value::Date(local.date_naive())),
            "time" => Ok(Value::Time(local.time())),
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "Instant".into(),
                property: property.into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn access_date_property(
        &self,
        date: &NaiveDate,
        property: &str,
    ) -> Result<Value, MarretaError> {
        match property {
            "year" => Ok(Value::Integer(date.year() as i64)),
            "month" => Ok(Value::Integer(date.month() as i64)),
            "day" => Ok(Value::Integer(date.day() as i64)),
            "weekday" => Ok(Value::Integer(date.weekday().num_days_from_monday() as i64)),
            "start_of_day" => Ok(Value::Instant(self.local_date_time_to_utc(
                date.and_hms_opt(0, 0, 0).expect("valid midnight"),
                false,
            )?)),
            "end_of_day" => Ok(Value::Instant(
                self.local_date_time_to_utc(
                    date.and_hms_milli_opt(23, 59, 59, 999)
                        .expect("valid end of day"),
                    true,
                )?,
            )),
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "Date".into(),
                property: property.into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn access_time_property(
        &self,
        time: &NaiveTime,
        property: &str,
    ) -> Result<Value, MarretaError> {
        match property {
            "hour" => Ok(Value::Integer(time.hour() as i64)),
            "minute" => Ok(Value::Integer(time.minute() as i64)),
            "second" => Ok(Value::Integer(time.second() as i64)),
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "Time".into(),
                property: property.into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn access_duration_property(
        &self,
        duration: ChronoDuration,
        property: &str,
    ) -> Result<Value, MarretaError> {
        match property {
            "total_days" => Ok(Value::Float(
                duration.num_milliseconds() as f64 / 86_400_000.0,
            )),
            "total_hours" => Ok(Value::Float(
                duration.num_milliseconds() as f64 / 3_600_000.0,
            )),
            "total_minutes" => Ok(Value::Float(duration.num_milliseconds() as f64 / 60_000.0)),
            "total_seconds" => Ok(Value::Float(duration.num_milliseconds() as f64 / 1_000.0)),
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "Duration".into(),
                property: property.into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn access_interval_property(
        &self,
        interval: &TemporalInterval,
        property: &str,
    ) -> Result<Value, MarretaError> {
        match property {
            "start" => Ok(self.temporal_value_to_runtime_value(&interval.start)),
            "end" => Ok(self.temporal_value_to_runtime_value(&interval.end)),
            "duration" => self.interval_duration(interval).map(Value::Duration),
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "Interval".into(),
                property: property.into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn temporal_value_to_runtime_value(&self, value: &TemporalValue) -> Value {
        match value {
            TemporalValue::Instant(dt) => Value::Instant(*dt),
            TemporalValue::Date(date) => Value::Date(*date),
            TemporalValue::Time(time) => Value::Time(*time),
        }
    }

    pub(super) fn temporal_value_from_runtime_value(
        &self,
        value: &Value,
    ) -> Result<TemporalValue, MarretaError> {
        match value {
            Value::Instant(dt) => Ok(TemporalValue::Instant(*dt)),
            Value::Date(date) => Ok(TemporalValue::Date(*date)),
            Value::Time(time) => Ok(TemporalValue::Time(*time)),
            other => Err(MarretaError::TypeError {
                message: format!(
                    "expected temporal value (instant, date, or time), got {}",
                    other.type_name()
                ),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn interval_duration(
        &self,
        interval: &TemporalInterval,
    ) -> Result<ChronoDuration, MarretaError> {
        match (&interval.start, &interval.end) {
            (TemporalValue::Instant(start), TemporalValue::Instant(end)) => Ok(*end - *start),
            (TemporalValue::Date(start), TemporalValue::Date(end)) => Ok(*end - *start),
            (TemporalValue::Time(start), TemporalValue::Time(end)) => Ok(*end - *start),
            _ => Err(MarretaError::TypeError {
                message: "interval start and end must share the same temporal type".into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }
}
