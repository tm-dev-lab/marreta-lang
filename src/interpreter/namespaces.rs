use super::*;

impl Interpreter {
    pub(super) fn current_timezone(&self) -> Tz {
        std::env::var("MARRETA_TIMEZONE")
            .ok()
            .and_then(|name| name.parse::<Tz>().ok())
            .unwrap_or(chrono_tz::UTC)
    }

    fn base64_url_safe_flag(
        &self,
        named_args: &[(String, Value)],
        method: &str,
    ) -> Result<bool, MarretaError> {
        let mut url_safe = false;
        for (name, value) in named_args {
            if name != "url_safe" {
                return Err(MarretaError::TypeError {
                    message: format!(
                        "base64.{method}() does not accept named argument '{}'",
                        name
                    ),
                    line: self.current_line,
                    column: self.current_column,
                });
            }
            url_safe = value.is_truthy();
        }
        Ok(url_safe)
    }

    fn base64_decode_bytes(&self, text: &str, url_safe: bool) -> Result<Vec<u8>, MarretaError> {
        let decoded = if url_safe {
            BASE64_URL_SAFE
                .decode(text)
                .or_else(|_| BASE64_URL_SAFE_NO_PAD.decode(text))
        } else {
            BASE64_STANDARD
                .decode(text)
                .or_else(|_| BASE64_STANDARD_NO_PAD.decode(text))
        };

        decoded.map_err(|err| MarretaError::RuntimeError {
            message: format!("base64.decode() invalid Base64: {}", err),
            line: self.current_line,
            column: self.current_column,
        })
    }

    pub(super) fn configured_log_level(&self) -> LogLevel {
        match std::env::var("MARRETA_LOG_LEVEL")
            .ok()
            .map(|value| value.to_ascii_lowercase())
            .as_deref()
        {
            Some("debug") => LogLevel::Debug,
            Some("warn") => LogLevel::Warn,
            Some("error") => LogLevel::Error,
            Some("info") | None | Some(_) => LogLevel::Info,
        }
    }

    fn log_level_for_method(&self, method: &str) -> Result<LogLevel, MarretaError> {
        match method {
            "debug" => Ok(LogLevel::Debug),
            "info" => Ok(LogLevel::Info),
            "warn" => Ok(LogLevel::Warn),
            "error" => Ok(LogLevel::Error),
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "LogNamespace".into(),
                property: method.into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    pub(super) fn should_emit_log_level(&self, level: LogLevel) -> bool {
        level >= self.configured_log_level()
    }

    pub(super) fn build_log_event(
        &self,
        level: LogLevel,
        value: &Value,
    ) -> Result<serde_json::Value, MarretaError> {
        let mut event = serde_json::Map::new();
        event.insert(
            "timestamp".into(),
            serde_json::Value::String(Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)),
        );
        event.insert("kind".into(), serde_json::Value::String("app_log".into()));
        event.insert(
            "level".into(),
            serde_json::Value::String(level.as_str().to_string()),
        );
        if let Some(trace_context) = &self.trace_context {
            event.insert(
                "trace_id".into(),
                serde_json::Value::String(trace_context.trace_id.clone()),
            );
            event.insert(
                "span_id".into(),
                serde_json::Value::String(trace_context.span_id.clone()),
            );
        }

        let data = value_to_json_strict(value).map_err(|message| MarretaError::TypeError {
            message: format!("log.{}() {}", level.as_str(), message),
            line: self.current_line,
            column: self.current_column,
        })?;
        event.insert("data".into(), data);

        Ok(serde_json::Value::Object(event))
    }

    fn emit_log_value(&self, level: LogLevel, value: &Value) -> Result<(), MarretaError> {
        if !self.should_emit_log_level(level) {
            return Ok(());
        }

        let line = serde_json::to_string(&self.build_log_event(level, value)?).map_err(|err| {
            MarretaError::RuntimeError {
                message: format!("log.{}() failed: {}", level.as_str(), err),
                line: self.current_line,
                column: self.current_column,
            }
        })?;

        let mut stdout = std::io::stdout().lock();
        stdout
            .write_all(line.as_bytes())
            .and_then(|_| stdout.write_all(b"\n"))
            .map_err(|err| MarretaError::RuntimeError {
                message: format!("log.{}() failed: {}", level.as_str(), err),
                line: self.current_line,
                column: self.current_column,
            })
    }

    pub(super) fn dispatch_log(
        &mut self,
        method: &str,
        arguments: &[Argument],
    ) -> Result<Value, MarretaError> {
        self.dispatch_log_with_input(method, arguments, None)
    }

    pub(super) fn dispatch_log_with_input(
        &mut self,
        method: &str,
        arguments: &[Argument],
        injected_input: Option<&Value>,
    ) -> Result<Value, MarretaError> {
        let level = self.log_level_for_method(method)?;
        let (positional, named) = self.collect_named_and_positional(arguments, injected_input)?;
        Self::ensure_no_named_args(
            &named,
            &format!("log.{method}"),
            self.current_line,
            self.current_column,
        )?;

        if positional.len() != 1 {
            return Err(MarretaError::WrongArity {
                task_name: format!("log.{method}"),
                expected: 1,
                got: positional.len(),
                line: self.current_line,
                column: self.current_column,
            });
        }

        let value = positional.first().expect("log value missing").clone();
        self.emit_log_value(level, &value)?;
        Ok(value)
    }

    fn fs_type_error(&self, message: impl Into<String>) -> MarretaError {
        MarretaError::TypeError {
            message: message.into(),
            line: self.current_line,
            column: self.current_column,
        }
    }

    fn fs_runtime_error(&self, path: &str, err: std::io::Error) -> MarretaError {
        use std::io::ErrorKind;

        match err.kind() {
            ErrorKind::NotFound => MarretaError::FileNotFound {
                path: path.to_string(),
            },
            _ => MarretaError::IoError {
                message: format!("fs operation failed for '{}': {}", path, err),
            },
        }
    }

    fn fs_string_arg(
        &self,
        args: &[Value],
        index: usize,
        method: &str,
    ) -> Result<String, MarretaError> {
        match args.get(index) {
            Some(Value::String(value)) => Ok(value.clone()),
            Some(other) => Err(self.fs_type_error(format!(
                "fs.{}() argument {} must be String, got {}",
                method,
                index + 1,
                other.type_name()
            ))),
            None => Err(self.fs_type_error(format!(
                "fs.{}() missing required argument {}",
                method,
                index + 1
            ))),
        }
    }

    pub(super) fn dispatch_fs(
        &mut self,
        method: &str,
        arguments: &[Argument],
    ) -> Result<Value, MarretaError> {
        self.dispatch_fs_with_input(method, arguments, None)
    }

    pub(super) fn dispatch_fs_with_input(
        &mut self,
        method: &str,
        arguments: &[Argument],
        injected_input: Option<&Value>,
    ) -> Result<Value, MarretaError> {
        let (positional, named) = self.collect_named_and_positional(arguments, injected_input)?;
        Self::ensure_no_named_args(
            &named,
            &format!("fs.{method}"),
            self.current_line,
            self.current_column,
        )?;

        match method {
            "read" => {
                let path = self.fs_string_arg(&positional, 0, "read")?;
                if positional.len() != 1 {
                    return Err(MarretaError::WrongArity {
                        task_name: "fs.read".into(),
                        expected: 1,
                        got: positional.len(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
                let content = std::fs::read_to_string(&path)
                    .map_err(|err| self.fs_runtime_error(&path, err))?;
                Ok(Value::String(content))
            }
            "write" => {
                let (path, content) = if injected_input.is_some() {
                    if positional.len() != 2 {
                        return Err(MarretaError::WrongArity {
                            task_name: "fs.write".into(),
                            expected: 2,
                            got: positional.len(),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                    (
                        self.fs_string_arg(&positional, 1, "write")?,
                        self.fs_string_arg(&positional, 0, "write")?,
                    )
                } else {
                    if positional.len() != 2 {
                        return Err(MarretaError::WrongArity {
                            task_name: "fs.write".into(),
                            expected: 2,
                            got: positional.len(),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                    (
                        self.fs_string_arg(&positional, 0, "write")?,
                        self.fs_string_arg(&positional, 1, "write")?,
                    )
                };
                std::fs::write(&path, content.as_bytes())
                    .map_err(|err| self.fs_runtime_error(&path, err))?;
                Ok(Value::String(content))
            }
            "append" => {
                let (path, content) = if injected_input.is_some() {
                    if positional.len() != 2 {
                        return Err(MarretaError::WrongArity {
                            task_name: "fs.append".into(),
                            expected: 2,
                            got: positional.len(),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                    (
                        self.fs_string_arg(&positional, 1, "append")?,
                        self.fs_string_arg(&positional, 0, "append")?,
                    )
                } else {
                    if positional.len() != 2 {
                        return Err(MarretaError::WrongArity {
                            task_name: "fs.append".into(),
                            expected: 2,
                            got: positional.len(),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                    (
                        self.fs_string_arg(&positional, 0, "append")?,
                        self.fs_string_arg(&positional, 1, "append")?,
                    )
                };

                let mut file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .map_err(|err| self.fs_runtime_error(&path, err))?;
                file.write_all(content.as_bytes())
                    .map_err(|err| self.fs_runtime_error(&path, err))?;
                Ok(Value::String(content))
            }
            "exists" => {
                let path = self.fs_string_arg(&positional, 0, "exists")?;
                if positional.len() != 1 {
                    return Err(MarretaError::WrongArity {
                        task_name: "fs.exists".into(),
                        expected: 1,
                        got: positional.len(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
                Ok(Value::Boolean(std::path::Path::new(&path).exists()))
            }
            "delete" => {
                let path = self.fs_string_arg(&positional, 0, "delete")?;
                if positional.len() != 1 {
                    return Err(MarretaError::WrongArity {
                        task_name: "fs.delete".into(),
                        expected: 1,
                        got: positional.len(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
                match std::fs::remove_file(&path) {
                    Ok(()) => Ok(Value::Boolean(true)),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        Ok(Value::Boolean(false))
                    }
                    Err(err) => Err(self.fs_runtime_error(&path, err)),
                }
            }
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "FsNamespace".into(),
                property: method.into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    pub(super) fn local_date_time_to_utc(
        &self,
        naive: NaiveDateTime,
        prefer_latest: bool,
    ) -> Result<chrono::DateTime<Utc>, MarretaError> {
        let tz = self.current_timezone();
        match tz.from_local_datetime(&naive) {
            LocalResult::Single(dt) => Ok(dt.with_timezone(&Utc)),
            LocalResult::Ambiguous(earliest, latest) => Ok(if prefer_latest {
                latest.with_timezone(&Utc)
            } else {
                earliest.with_timezone(&Utc)
            }),
            LocalResult::None => Err(MarretaError::TypeError {
                message: format!(
                    "could not resolve local datetime '{}' in timezone '{}'",
                    naive, tz
                ),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    pub(super) fn dispatch_json(
        &mut self,
        method: &str,
        arguments: &[Argument],
    ) -> Result<Value, MarretaError> {
        self.dispatch_json_with_input(method, arguments, None)
    }

    pub(super) fn dispatch_json_with_input(
        &mut self,
        method: &str,
        arguments: &[Argument],
        injected_input: Option<&Value>,
    ) -> Result<Value, MarretaError> {
        let (positional, named) = self.collect_named_and_positional(arguments, injected_input)?;
        Self::ensure_no_named_args(
            &named,
            &format!("json.{method}"),
            self.current_line,
            self.current_column,
        )?;

        match method {
            "parse" => {
                if positional.len() != 1 {
                    return Err(MarretaError::WrongArity {
                        task_name: "json.parse".into(),
                        expected: 1,
                        got: positional.len(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
                let text = match positional.first() {
                    Some(Value::String(s)) => s,
                    Some(other) => {
                        return Err(MarretaError::TypeError {
                            message: format!(
                                "json.parse() argument 1 must be String, got {}",
                                other.type_name()
                            ),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                    None => unreachable!(),
                };
                let parsed = serde_json::from_str::<serde_json::Value>(text).map_err(|err| {
                    MarretaError::RuntimeError {
                        message: format!("json.parse() invalid JSON: {}", err),
                        line: self.current_line,
                        column: self.current_column,
                    }
                })?;
                Ok(json_to_value(&parsed))
            }
            "stringify" | "pretty" => {
                if positional.len() != 1 {
                    return Err(MarretaError::WrongArity {
                        task_name: format!("json.{method}"),
                        expected: 1,
                        got: positional.len(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }

                let json = value_to_json_strict(positional.first().expect("json value missing"))
                    .map_err(|message| MarretaError::TypeError {
                        message: format!("json.{method}() {}", message),
                        line: self.current_line,
                        column: self.current_column,
                    })?;

                let text = if method == "pretty" {
                    serde_json::to_string_pretty(&json).map_err(|err| {
                        MarretaError::RuntimeError {
                            message: format!("json.pretty() failed: {}", err),
                            line: self.current_line,
                            column: self.current_column,
                        }
                    })?
                } else {
                    serde_json::to_string(&json).map_err(|err| MarretaError::RuntimeError {
                        message: format!("json.stringify() failed: {}", err),
                        line: self.current_line,
                        column: self.current_column,
                    })?
                };

                Ok(Value::String(text))
            }
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "JsonNamespace".into(),
                property: method.into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    pub(super) fn dispatch_base64(
        &mut self,
        method: &str,
        arguments: &[Argument],
    ) -> Result<Value, MarretaError> {
        self.dispatch_base64_with_input(method, arguments, None)
    }

    pub(super) fn dispatch_base64_with_input(
        &mut self,
        method: &str,
        arguments: &[Argument],
        injected_input: Option<&Value>,
    ) -> Result<Value, MarretaError> {
        let (positional, named) = self.collect_named_and_positional(arguments, injected_input)?;
        let url_safe = self.base64_url_safe_flag(&named, method)?;

        match method {
            "encode" => {
                if positional.len() != 1 {
                    return Err(MarretaError::WrongArity {
                        task_name: "base64.encode".into(),
                        expected: 1,
                        got: positional.len(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
                let text = Self::expect_string_value(
                    &positional,
                    0,
                    "base64.encode",
                    self.current_line,
                    self.current_column,
                )?;
                let encoded = if url_safe {
                    BASE64_URL_SAFE.encode(text.as_bytes())
                } else {
                    BASE64_STANDARD.encode(text.as_bytes())
                };
                Ok(Value::String(encoded))
            }
            "decode" => {
                if positional.len() != 1 {
                    return Err(MarretaError::WrongArity {
                        task_name: "base64.decode".into(),
                        expected: 1,
                        got: positional.len(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
                let text = Self::expect_string_value(
                    &positional,
                    0,
                    "base64.decode",
                    self.current_line,
                    self.current_column,
                )?;
                let bytes = self.base64_decode_bytes(&text, url_safe)?;
                let decoded =
                    String::from_utf8(bytes).map_err(|err| MarretaError::RuntimeError {
                        message: format!(
                            "base64.decode() decoded bytes are not valid UTF-8: {}",
                            err
                        ),
                        line: self.current_line,
                        column: self.current_column,
                    })?;
                Ok(Value::String(decoded))
            }
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "Base64Namespace".into(),
                property: method.into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    pub(super) fn dispatch_uuid(
        &mut self,
        method: &str,
        arguments: &[Argument],
    ) -> Result<Value, MarretaError> {
        let args = self.evaluate_args(arguments)?;
        if !args.is_empty() {
            return Err(MarretaError::WrongArity {
                task_name: format!("uuid.{method}"),
                expected: 0,
                got: args.len(),
                line: self.current_line,
                column: self.current_column,
            });
        }

        match method {
            "v4" => Ok(Value::String(Uuid::new_v4().to_string())),
            "v7" => Ok(Value::String(Uuid::now_v7().to_string())),
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "UuidNamespace".into(),
                property: method.into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    pub(super) fn dispatch_feature(
        &mut self,
        method: &str,
        arguments: &[Argument],
    ) -> Result<Value, MarretaError> {
        let args = self.evaluate_args(arguments)?;

        match method {
            "enabled" => {
                if args.len() != 1 {
                    return Err(MarretaError::WrongArity {
                        task_name: "feature.enabled".into(),
                        expected: 1,
                        got: args.len(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }

                let name = match &args[0] {
                    Value::String(name) => name,
                    other => {
                        return Err(MarretaError::TypeError {
                            message: format!(
                                "feature.enabled() argument 1 must be String, got {}",
                                other.type_name()
                            ),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                };

                if !is_valid_feature_name(name) {
                    return Err(MarretaError::RuntimeError {
                        message: format!(
                            "invalid feature flag name '{}': {}",
                            name, FEATURE_NAME_HELP
                        ),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }

                let enabled = self
                    .project_runtime
                    .as_ref()
                    .map(|runtime| runtime.feature_flags.enabled(name))
                    .unwrap_or_else(|| self.feature_flags.enabled(name));
                Ok(Value::Boolean(enabled))
            }
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "FeatureNamespace".into(),
                property: method.into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    pub(super) fn dispatch_math(
        &mut self,
        method: &str,
        arguments: &[Argument],
    ) -> Result<Value, MarretaError> {
        self.dispatch_math_with_input(method, arguments, None)
    }

    pub(super) fn dispatch_math_with_input(
        &mut self,
        method: &str,
        arguments: &[Argument],
        injected_input: Option<&Value>,
    ) -> Result<Value, MarretaError> {
        let (positional, named) = self.collect_named_and_positional(arguments, injected_input)?;
        match method {
            "abs" => {
                Self::ensure_no_named_args(
                    &named,
                    "math.abs",
                    self.current_line,
                    self.current_column,
                )?;
                let value = positional.first().ok_or_else(|| MarretaError::TypeError {
                    message: "math.abs() requires one numeric argument".into(),
                    line: self.current_line,
                    column: self.current_column,
                })?;
                match value {
                    Value::Integer(n) => Ok(Value::Integer(n.abs())),
                    Value::Float(n) => Ok(Value::Float(n.abs())),
                    other => Err(MarretaError::TypeError {
                        message: format!(
                            "math.abs() requires Integer or Float, got {}",
                            other.type_name()
                        ),
                        line: self.current_line,
                        column: self.current_column,
                    }),
                }
            }
            "floor" => {
                Self::ensure_no_named_args(
                    &named,
                    "math.floor",
                    self.current_line,
                    self.current_column,
                )?;
                let number = self.math_number_arg(&positional, "math.floor")?;
                Ok(Value::Integer(number.floor() as i64))
            }
            "ceil" => {
                Self::ensure_no_named_args(
                    &named,
                    "math.ceil",
                    self.current_line,
                    self.current_column,
                )?;
                let number = self.math_number_arg(&positional, "math.ceil")?;
                Ok(Value::Integer(number.ceil() as i64))
            }
            "round" => self.dispatch_math_round(&positional, &named),
            "min" => {
                Self::ensure_no_named_args(
                    &named,
                    "math.min",
                    self.current_line,
                    self.current_column,
                )?;
                let (left, right) = self.math_two_number_args(&positional, "math.min")?;
                Ok(self.math_min_max_result(left, right, |a, b| a.min(b)))
            }
            "max" => {
                Self::ensure_no_named_args(
                    &named,
                    "math.max",
                    self.current_line,
                    self.current_column,
                )?;
                let (left, right) = self.math_two_number_args(&positional, "math.max")?;
                Ok(self.math_min_max_result(left, right, |a, b| a.max(b)))
            }
            "clamp" => self.dispatch_math_clamp(&positional, &named),
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "MathNamespace".into(),
                property: method.into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    pub(super) fn dispatch_time(
        &mut self,
        method: &str,
        arguments: &[Argument],
    ) -> Result<Value, MarretaError> {
        let args = self.evaluate_args(arguments)?;
        match method {
            "now" => {
                if !args.is_empty() {
                    return Err(MarretaError::TypeError {
                        message: "time.now() takes no arguments".into(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
                Ok(Value::Instant(Utc::now()))
            }
            "today" => {
                if !args.is_empty() {
                    return Err(MarretaError::TypeError {
                        message: "time.today() takes no arguments".into(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
                let tz = self.current_timezone();
                Ok(Value::Date(Utc::now().with_timezone(&tz).date_naive()))
            }
            "parse" => {
                let input = Self::expect_string_value(
                    &args,
                    0,
                    "time.parse",
                    self.current_line,
                    self.current_column,
                )?;
                self.parse_temporal_string(&input)
            }
            "date" => {
                let input = Self::expect_string_value(
                    &args,
                    0,
                    "time.date",
                    self.current_line,
                    self.current_column,
                )?;
                let date = NaiveDate::parse_from_str(&input, "%Y-%m-%d").map_err(|_| {
                    MarretaError::TypeError {
                        message: format!("time.date() requires YYYY-MM-DD, got '{}'", input),
                        line: self.current_line,
                        column: self.current_column,
                    }
                })?;
                Ok(Value::Date(date))
            }
            "at" => {
                let input = Self::expect_string_value(
                    &args,
                    0,
                    "time.at",
                    self.current_line,
                    self.current_column,
                )?;
                let time = NaiveTime::parse_from_str(&input, "%H:%M:%S").map_err(|_| {
                    MarretaError::TypeError {
                        message: format!("time.at() requires HH:MM:SS, got '{}'", input),
                        line: self.current_line,
                        column: self.current_column,
                    }
                })?;
                Ok(Value::Time(time))
            }
            "instant" => {
                let input = Self::expect_string_value(
                    &args,
                    0,
                    "time.instant",
                    self.current_line,
                    self.current_column,
                )?;
                self.parse_instant_string(&input)
            }
            "days" => self.duration_from_unit(&args, "time.days", 86_400_000),
            "hours" => self.duration_from_unit(&args, "time.hours", 3_600_000),
            "minutes" => self.duration_from_unit(&args, "time.minutes", 60_000),
            "seconds" => self.duration_from_unit(&args, "time.seconds", 1_000),
            "interval" => {
                let start = args.first().ok_or_else(|| MarretaError::TypeError {
                    message: "time.interval() requires start and end".into(),
                    line: self.current_line,
                    column: self.current_column,
                })?;
                let end = args.get(1).ok_or_else(|| MarretaError::TypeError {
                    message: "time.interval() requires start and end".into(),
                    line: self.current_line,
                    column: self.current_column,
                })?;
                let start = self.temporal_value_from_runtime_value(start)?;
                let end = self.temporal_value_from_runtime_value(end)?;
                self.ensure_same_temporal_kind(&start, &end)?;
                Ok(Value::Interval(TemporalInterval { start, end }))
            }
            "contains" => {
                let interval = match args.first() {
                    Some(Value::Interval(interval)) => interval,
                    Some(other) => {
                        return Err(MarretaError::TypeError {
                            message: format!(
                                "time.contains() requires Interval as first argument, got {}",
                                other.type_name()
                            ),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                    None => {
                        return Err(MarretaError::TypeError {
                            message: "time.contains() requires interval and value".into(),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                };
                let value = args.get(1).ok_or_else(|| MarretaError::TypeError {
                    message: "time.contains() requires interval and value".into(),
                    line: self.current_line,
                    column: self.current_column,
                })?;
                let value = self.temporal_value_from_runtime_value(value)?;
                self.ensure_same_temporal_kind(&interval.start, &value)?;
                Ok(Value::Boolean(self.interval_contains(interval, &value)?))
            }
            "overlaps" => {
                let left = match args.first() {
                    Some(Value::Interval(interval)) => interval,
                    Some(other) => {
                        return Err(MarretaError::TypeError {
                            message: format!(
                                "time.overlaps() requires Interval as first argument, got {}",
                                other.type_name()
                            ),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                    None => {
                        return Err(MarretaError::TypeError {
                            message: "time.overlaps() requires two intervals".into(),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                };
                let right = match args.get(1) {
                    Some(Value::Interval(interval)) => interval,
                    Some(other) => {
                        return Err(MarretaError::TypeError {
                            message: format!(
                                "time.overlaps() requires Interval as second argument, got {}",
                                other.type_name()
                            ),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                    None => {
                        return Err(MarretaError::TypeError {
                            message: "time.overlaps() requires two intervals".into(),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                };
                self.ensure_same_temporal_kind(&left.start, &right.start)?;
                Ok(Value::Boolean(self.intervals_overlap(left, right)?))
            }
            "format" => {
                let value = args.first().ok_or_else(|| MarretaError::TypeError {
                    message: "time.format() requires value and mask".into(),
                    line: self.current_line,
                    column: self.current_column,
                })?;
                let mask = Self::expect_string_value(
                    &args,
                    1,
                    "time.format",
                    self.current_line,
                    self.current_column,
                )?;
                Ok(Value::String(self.format_temporal_value(value, &mask)?))
            }
            "from_unix" => {
                let secs = Self::expect_numeric_i64_value(
                    &args,
                    0,
                    "time.from_unix",
                    self.current_line,
                    self.current_column,
                )?;
                let dt = chrono::DateTime::<Utc>::from_timestamp(secs, 0).ok_or_else(|| {
                    MarretaError::TypeError {
                        message: format!(
                            "time.from_unix() received invalid epoch seconds '{}'",
                            secs
                        ),
                        line: self.current_line,
                        column: self.current_column,
                    }
                })?;
                Ok(Value::Instant(dt))
            }
            "unix" => match args.first() {
                Some(Value::Instant(dt)) => Ok(Value::Integer(dt.timestamp())),
                Some(other) => Err(MarretaError::TypeError {
                    message: format!("time.unix() requires Instant, got {}", other.type_name()),
                    line: self.current_line,
                    column: self.current_column,
                }),
                None => Err(MarretaError::TypeError {
                    message: "time.unix() requires an instant".into(),
                    line: self.current_line,
                    column: self.current_column,
                }),
            },
            _ => Err(MarretaError::PropertyNotFound {
                object_type: "TimeNamespace".into(),
                property: method.into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn dispatch_math_round(
        &mut self,
        positional: &[Value],
        named: &[(String, Value)],
    ) -> Result<Value, MarretaError> {
        let mut places: Option<i64> = None;

        for (name, evaluated) in named {
            match name.as_str() {
                "places" => {
                    let parsed = evaluated
                        .as_integer()
                        .ok_or_else(|| MarretaError::TypeError {
                            message: format!(
                                "math.round() places: must be Integer, got {}",
                                evaluated.type_name()
                            ),
                            line: self.current_line,
                            column: self.current_column,
                        })?;
                    if parsed < 0 {
                        return Err(MarretaError::RuntimeError {
                            message: "math.round() places: must be zero or greater".into(),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                    places = Some(parsed);
                }
                _ => {
                    return Err(MarretaError::TypeError {
                        message: format!("math.round() does not accept named argument '{}'", name),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
            }
        }

        if positional.len() != 1 {
            return Err(MarretaError::WrongArity {
                task_name: "math.round".into(),
                expected: 1,
                got: positional.len(),
                line: self.current_line,
                column: self.current_column,
            });
        }

        let value = &positional[0];
        let number = match value {
            Value::Integer(n) => *n as f64,
            Value::Float(n) => *n,
            other => {
                return Err(MarretaError::TypeError {
                    message: format!(
                        "math.round() requires Integer or Float, got {}",
                        other.type_name()
                    ),
                    line: self.current_line,
                    column: self.current_column,
                });
            }
        };

        if let Some(places) = places {
            let scale = 10_f64.powi(places as i32);
            return Ok(Value::Float((number * scale).round() / scale));
        }

        Ok(Value::Integer(number.round() as i64))
    }

    fn dispatch_math_clamp(
        &mut self,
        positional: &[Value],
        named: &[(String, Value)],
    ) -> Result<Value, MarretaError> {
        let mut min = None;
        let mut max = None;

        for (name, value) in named {
            match name.as_str() {
                "min" => min = Some(value.clone()),
                "max" => max = Some(value.clone()),
                _ => {
                    return Err(MarretaError::TypeError {
                        message: format!("math.clamp() does not accept named argument '{}'", name),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
            }
        }

        if positional.len() != 1 {
            return Err(MarretaError::WrongArity {
                task_name: "math.clamp".into(),
                expected: 1,
                got: positional.len(),
                line: self.current_line,
                column: self.current_column,
            });
        }

        let min = min.ok_or_else(|| MarretaError::TypeError {
            message: "math.clamp() requires min: and max:".into(),
            line: self.current_line,
            column: self.current_column,
        })?;
        let max = max.ok_or_else(|| MarretaError::TypeError {
            message: "math.clamp() requires min: and max:".into(),
            line: self.current_line,
            column: self.current_column,
        })?;

        let value = self.math_numeric_value(&positional[0], "math.clamp")?;
        let min_value = self.math_numeric_value(&min, "math.clamp")?;
        let max_value = self.math_numeric_value(&max, "math.clamp")?;

        if min_value.value > max_value.value {
            return Err(MarretaError::RuntimeError {
                message: "math.clamp() requires min <= max".into(),
                line: self.current_line,
                column: self.current_column,
            });
        }

        let clamped = value.value.max(min_value.value).min(max_value.value);
        let is_float = value.is_float || min_value.is_float || max_value.is_float;

        Ok(if is_float {
            Value::Float(clamped)
        } else {
            Value::Integer(clamped as i64)
        })
    }

    fn collect_named_and_positional(
        &mut self,
        arguments: &[Argument],
        injected_input: Option<&Value>,
    ) -> Result<(Vec<Value>, Vec<(String, Value)>), MarretaError> {
        let mut positional = Vec::new();
        let mut named = Vec::new();

        if let Some(input) = injected_input {
            positional.push(input.clone());
        }

        for arg in arguments {
            match arg {
                Argument::Positional(expr) => positional.push(self.evaluate(expr)?),
                Argument::Named { name, value } => {
                    named.push((name.clone(), self.evaluate(value)?));
                }
            }
        }

        Ok((positional, named))
    }

    fn ensure_no_named_args(
        named: &[(String, Value)],
        method: &str,
        line: usize,
        column: usize,
    ) -> Result<(), MarretaError> {
        if let Some((name, _)) = named.first() {
            return Err(MarretaError::TypeError {
                message: format!("{method}() does not accept named argument '{}'", name),
                line,
                column,
            });
        }
        Ok(())
    }

    fn math_number_arg(&self, args: &[Value], name: &str) -> Result<f64, MarretaError> {
        if args.len() != 1 {
            return Err(MarretaError::WrongArity {
                task_name: name.into(),
                expected: 1,
                got: args.len(),
                line: self.current_line,
                column: self.current_column,
            });
        }
        let value = args.first().ok_or_else(|| MarretaError::TypeError {
            message: format!("{name}() requires one numeric argument"),
            line: self.current_line,
            column: self.current_column,
        })?;

        match value {
            Value::Integer(n) => Ok(*n as f64),
            Value::Float(n) => Ok(*n),
            other => Err(MarretaError::TypeError {
                message: format!(
                    "{name}() requires Integer or Float, got {}",
                    other.type_name()
                ),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn math_two_number_args(
        &self,
        args: &[Value],
        name: &str,
    ) -> Result<(MathNumericValue, MathNumericValue), MarretaError> {
        if args.len() != 2 {
            return Err(MarretaError::WrongArity {
                task_name: name.into(),
                expected: 2,
                got: args.len(),
                line: self.current_line,
                column: self.current_column,
            });
        }
        let left = args.first().ok_or_else(|| MarretaError::TypeError {
            message: format!("{name}() requires two numeric arguments"),
            line: self.current_line,
            column: self.current_column,
        })?;
        let right = args.get(1).ok_or_else(|| MarretaError::TypeError {
            message: format!("{name}() requires two numeric arguments"),
            line: self.current_line,
            column: self.current_column,
        })?;
        Ok((
            self.math_numeric_value(left, name)?,
            self.math_numeric_value(right, name)?,
        ))
    }

    fn math_numeric_value(
        &self,
        value: &Value,
        name: &str,
    ) -> Result<MathNumericValue, MarretaError> {
        match value {
            Value::Integer(n) => Ok(MathNumericValue {
                value: *n as f64,
                is_float: false,
            }),
            Value::Float(n) => Ok(MathNumericValue {
                value: *n,
                is_float: true,
            }),
            other => Err(MarretaError::TypeError {
                message: format!(
                    "{name}() requires Integer or Float, got {}",
                    other.type_name()
                ),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn math_min_max_result(
        &self,
        left: MathNumericValue,
        right: MathNumericValue,
        op: fn(f64, f64) -> f64,
    ) -> Value {
        let result = op(left.value, right.value);
        if left.is_float || right.is_float {
            Value::Float(result)
        } else {
            Value::Integer(result as i64)
        }
    }

    fn parse_temporal_string(&self, input: &str) -> Result<Value, MarretaError> {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(input) {
            return Ok(Value::Instant(dt.with_timezone(&Utc)));
        }
        if let Ok(date) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
            return Ok(Value::Date(date));
        }
        if let Ok(time) = NaiveTime::parse_from_str(input, "%H:%M:%S") {
            return Ok(Value::Time(time));
        }
        Err(MarretaError::TypeError {
            message: format!("time.parse() could not parse '{}'", input),
            line: self.current_line,
            column: self.current_column,
        })
    }

    fn parse_instant_string(&self, input: &str) -> Result<Value, MarretaError> {
        let dt =
            chrono::DateTime::parse_from_rfc3339(input).map_err(|_| MarretaError::TypeError {
                message: format!("time.instant() requires RFC3339 timestamp, got '{}'", input),
                line: self.current_line,
                column: self.current_column,
            })?;
        Ok(Value::Instant(dt.with_timezone(&Utc)))
    }

    fn duration_from_unit(
        &self,
        args: &[Value],
        name: &str,
        millis_per_unit: i64,
    ) -> Result<Value, MarretaError> {
        let amount = match args.first() {
            Some(Value::Integer(n)) => *n as f64,
            Some(Value::Float(n)) => *n,
            Some(other) => {
                return Err(MarretaError::TypeError {
                    message: format!(
                        "{}() requires numeric amount, got {}",
                        name,
                        other.type_name()
                    ),
                    line: self.current_line,
                    column: self.current_column,
                });
            }
            None => {
                return Err(MarretaError::TypeError {
                    message: format!("{}() requires a numeric amount", name),
                    line: self.current_line,
                    column: self.current_column,
                });
            }
        };
        Ok(Value::Duration(ChronoDuration::milliseconds(
            (amount * millis_per_unit as f64).round() as i64,
        )))
    }

    fn ensure_same_temporal_kind(
        &self,
        left: &TemporalValue,
        right: &TemporalValue,
    ) -> Result<(), MarretaError> {
        let same = matches!(
            (left, right),
            (TemporalValue::Instant(_), TemporalValue::Instant(_))
                | (TemporalValue::Date(_), TemporalValue::Date(_))
                | (TemporalValue::Time(_), TemporalValue::Time(_))
        );
        if same {
            Ok(())
        } else {
            Err(MarretaError::TypeError {
                message: "temporal values must share the same type".into(),
                line: self.current_line,
                column: self.current_column,
            })
        }
    }

    fn interval_contains(
        &self,
        interval: &TemporalInterval,
        value: &TemporalValue,
    ) -> Result<bool, MarretaError> {
        Ok(match (&interval.start, &interval.end, value) {
            (
                TemporalValue::Instant(start),
                TemporalValue::Instant(end),
                TemporalValue::Instant(value),
            ) => value >= start && value <= end,
            (TemporalValue::Date(start), TemporalValue::Date(end), TemporalValue::Date(value)) => {
                value >= start && value <= end
            }
            (TemporalValue::Time(start), TemporalValue::Time(end), TemporalValue::Time(value)) => {
                value >= start && value <= end
            }
            _ => {
                return Err(MarretaError::TypeError {
                    message:
                        "time.contains() requires interval and value of the same temporal type"
                            .into(),
                    line: self.current_line,
                    column: self.current_column,
                });
            }
        })
    }

    fn intervals_overlap(
        &self,
        left: &TemporalInterval,
        right: &TemporalInterval,
    ) -> Result<bool, MarretaError> {
        Ok(match (&left.start, &left.end, &right.start, &right.end) {
            (
                TemporalValue::Instant(a_start),
                TemporalValue::Instant(a_end),
                TemporalValue::Instant(b_start),
                TemporalValue::Instant(b_end),
            ) => a_start <= b_end && b_start <= a_end,
            (
                TemporalValue::Date(a_start),
                TemporalValue::Date(a_end),
                TemporalValue::Date(b_start),
                TemporalValue::Date(b_end),
            ) => a_start <= b_end && b_start <= a_end,
            (
                TemporalValue::Time(a_start),
                TemporalValue::Time(a_end),
                TemporalValue::Time(b_start),
                TemporalValue::Time(b_end),
            ) => a_start <= b_end && b_start <= a_end,
            _ => {
                return Err(MarretaError::TypeError {
                    message: "time.overlaps() requires intervals with matching temporal types"
                        .into(),
                    line: self.current_line,
                    column: self.current_column,
                });
            }
        })
    }

    fn format_temporal_value(&self, value: &Value, mask: &str) -> Result<String, MarretaError> {
        let chrono_mask = mask
            .replace("yyyy", "%Y")
            .replace("MM", "%m")
            .replace("dd", "%d")
            .replace("HH", "%H")
            .replace("mm", "%M")
            .replace("ss", "%S");

        match value {
            Value::Instant(dt) => Ok(dt.format(&chrono_mask).to_string()),
            Value::Date(date) => Ok(date.format(&chrono_mask).to_string()),
            Value::Time(time) => Ok(time.format(&chrono_mask).to_string()),
            other => Err(MarretaError::TypeError {
                message: format!(
                    "time.format() requires instant, date, or time, got {}",
                    other.type_name()
                ),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    pub(super) fn resolve_named_i64(
        named_args: &[(String, Value)],
        key: &str,
        default: i64,
    ) -> i64 {
        for (n, v) in named_args {
            if n == key
                && let Some(i) = v.as_integer()
            {
                return i;
            }
        }
        default
    }
}
