use super::*;

impl Interpreter {
    pub(super) fn dispatch_cache(
        &mut self,
        method: &str,
        arguments: &[Argument],
    ) -> Result<Value, MarretaError> {
        let op = format!("cache.{}", method);
        let driver = self.require_cache_driver(&op)?;

        // Separate positional and named args
        let mut positional: Vec<Value> = Vec::new();
        let mut named: Vec<(String, Value)> = Vec::new();
        for arg in arguments {
            match arg {
                Argument::Positional(expr) => positional.push(self.evaluate(expr)?),
                Argument::Named { name, value } => {
                    named.push((name.clone(), self.evaluate(value)?))
                }
            }
        }

        match method {
            "get" => {
                // cache.get(key) [as schema]
                let key = positional.first().ok_or_else(|| {
                    Self::cache_error("cache.get requires a key argument".into(), &op)
                })?;
                let key_str = key.to_string();
                let result = run_async(async { driver.get(&key_str).await })
                    .map_err(|e| Self::cache_error(e.to_string(), &op))?;
                Ok(result.unwrap_or(Value::Null))
            }

            "set" => {
                // cache.set(key, value, [ttl: N], [only_if_absent: true])
                let key = positional.first().ok_or_else(|| {
                    Self::cache_error("cache.set requires a key argument".into(), &op)
                })?;
                let value = positional.get(1).ok_or_else(|| {
                    Self::cache_error("cache.set requires a value argument".into(), &op)
                })?;
                let key_str = key.to_string();
                let ttl = self.resolve_ttl(&positional, &named);
                let only_if_absent = Self::resolve_named_bool(&named, "only_if_absent");
                let val = value.clone();
                let result =
                    run_async(async { driver.set(&key_str, &val, ttl, only_if_absent).await })
                        .map_err(|e| Self::cache_error(e.to_string(), &op))?;
                Ok(result.unwrap_or(Value::Null))
            }

            "delete" => {
                let key = positional.first().ok_or_else(|| {
                    Self::cache_error("cache.delete requires a key argument".into(), &op)
                })?;
                let key_str = key.to_string();
                let existed = run_async(async { driver.delete(&key_str).await })
                    .map_err(|e| Self::cache_error(e.to_string(), &op))?;
                Ok(Value::Boolean(existed))
            }

            "exists" => {
                let key = positional.first().ok_or_else(|| {
                    Self::cache_error("cache.exists requires a key argument".into(), &op)
                })?;
                let key_str = key.to_string();
                let exists = run_async(async { driver.exists(&key_str).await })
                    .map_err(|e| Self::cache_error(e.to_string(), &op))?;
                Ok(Value::Boolean(exists))
            }

            "ttl" => {
                let key = positional.first().ok_or_else(|| {
                    Self::cache_error("cache.ttl requires a key argument".into(), &op)
                })?;
                let key_str = key.to_string();
                let ttl = run_async(async { driver.ttl(&key_str).await })
                    .map_err(|e| Self::cache_error(e.to_string(), &op))?;
                Ok(ttl
                    .map(|d| Value::Integer(d.as_secs() as i64))
                    .unwrap_or(Value::Null))
            }

            "expire" => {
                let key = positional.first().ok_or_else(|| {
                    Self::cache_error("cache.expire requires a key argument".into(), &op)
                })?;
                let key_str = key.to_string();
                let ttl = self.resolve_explicit_ttl(&named).ok_or_else(|| {
                    Self::cache_error("cache.expire requires ttl: N named argument".into(), &op)
                })?;
                let ok = run_async(async { driver.expire(&key_str, ttl).await })
                    .map_err(|e| Self::cache_error(e.to_string(), &op))?;
                Ok(Value::Boolean(ok))
            }

            "incr" => {
                let key = positional.first().ok_or_else(|| {
                    Self::cache_error("cache.incr requires a key argument".into(), &op)
                })?;
                let key_str = key.to_string();
                let by = Self::resolve_named_i64(&named, "by", 1);
                let ttl = self.resolve_explicit_ttl(&named); // no default_ttl for counters
                let new_val = run_async(async { driver.incr(&key_str, by, ttl).await })
                    .map_err(|e| Self::cache_error(e.to_string(), &op))?;
                Ok(Value::Integer(new_val))
            }

            "decr" => {
                let key = positional.first().ok_or_else(|| {
                    Self::cache_error("cache.decr requires a key argument".into(), &op)
                })?;
                let key_str = key.to_string();
                let by = Self::resolve_named_i64(&named, "by", 1);
                let ttl = self.resolve_explicit_ttl(&named);
                let new_val = run_async(async { driver.decr(&key_str, by, ttl).await })
                    .map_err(|e| Self::cache_error(e.to_string(), &op))?;
                Ok(Value::Integer(new_val))
            }

            "get_many" => {
                let keys_val = positional.first().ok_or_else(|| {
                    Self::cache_error("cache.get_many requires a list of keys".into(), &op)
                })?;
                let keys = match keys_val {
                    Value::List(items) => items.iter().map(|v| v.to_string()).collect::<Vec<_>>(),
                    _ => {
                        return Err(Self::cache_error(
                            "cache.get_many argument must be a list".into(),
                            &op,
                        ));
                    }
                };
                let result = run_async(async { driver.get_many(&keys).await })
                    .map_err(|e| Self::cache_error(e.to_string(), &op))?;
                let mut map = ValueMap::new();
                for (k, v) in result {
                    map.insert(k, v.unwrap_or(Value::Null));
                }
                Ok(Value::Map(Arc::new(std::sync::RwLock::new(map))))
            }

            "set_many" => {
                let entries_val = positional.first().ok_or_else(|| {
                    Self::cache_error("cache.set_many requires a map argument".into(), &op)
                })?;
                let entries: HashMap<String, Value> = match entries_val {
                    Value::Map(m) => m.read().unwrap().clone().into_iter().collect(),
                    _ => {
                        return Err(Self::cache_error(
                            "cache.set_many argument must be a map".into(),
                            &op,
                        ));
                    }
                };
                let ttl = self.resolve_ttl(&positional, &named);
                run_async(async { driver.set_many(&entries, ttl).await })
                    .map_err(|e| Self::cache_error(e.to_string(), &op))?;
                Ok(Value::Null)
            }

            _ => Err(Self::cache_error(
                format!("unknown cache operation '{}'", method),
                &op,
            )),
        }
    }

    // =========================================================================
    // HTTP Client dispatch
    // =========================================================================

    pub(super) fn http_client_error(msg: String, op: &str) -> MarretaError {
        MarretaError::HttpClientError {
            message: msg,
            operation: op.to_string(),
        }
    }

    fn require_http_client_driver(&self, op: &str) -> Result<Arc<dyn HttpClient>, MarretaError> {
        self.http_client_driver
            .clone()
            .ok_or_else(|| MarretaError::HttpClientError {
                message: "http_client.* called but no HTTP client is configured".into(),
                operation: op.to_string(),
            })
    }

    /// Dispatches `http_client.verb(url, [body], [headers:], [query:], [timeout:])`.
    pub(super) fn dispatch_http_client(
        &mut self,
        method: &str,
        arguments: &[Argument],
    ) -> Result<Value, MarretaError> {
        self.dispatch_http_client_with_pipeline(method, arguments, None)
    }

    /// Core http_client dispatch — handles both direct calls and pipeline injection.
    /// When `pipeline_input` is Some, it's injected as request body (POST/PUT/PATCH)
    /// or query params (GET/DELETE).
    pub(super) fn dispatch_http_client_with_pipeline(
        &mut self,
        method: &str,
        arguments: &[Argument],
        pipeline_input: Option<&Value>,
    ) -> Result<Value, MarretaError> {
        let op = format!("http_client.{}", method);

        let http_method = match method {
            "get" => HttpMethod::Get,
            "post" => HttpMethod::Post,
            "put" => HttpMethod::Put,
            "patch" => HttpMethod::Patch,
            "delete" => HttpMethod::Delete,
            _ => {
                return Err(Self::http_client_error(
                    format!(
                        "unknown http_client operation '{}'. Valid: get, post, put, patch, delete",
                        method
                    ),
                    &op,
                ));
            }
        };

        let driver = self.require_http_client_driver(&op)?;

        // Separate positional and named args
        let mut positional: Vec<Value> = Vec::new();
        let mut named: Vec<(String, Value)> = Vec::new();
        for arg in arguments {
            match arg {
                Argument::Positional(expr) => positional.push(self.evaluate(expr)?),
                Argument::Named { name, value } => {
                    named.push((name.clone(), self.evaluate(value)?))
                }
            }
        }

        // First positional arg = URL (required)
        let url = match positional.first() {
            Some(Value::String(s)) => s.clone(),
            Some(other) => {
                return Err(Self::http_client_error(
                    format!(
                        "http_client.{} first argument must be a URL string, got {}",
                        method,
                        other.type_name()
                    ),
                    &op,
                ));
            }
            None => {
                return Err(Self::http_client_error(
                    format!("http_client.{} requires a URL argument", method),
                    &op,
                ));
            }
        };

        // Second positional arg = body (optional, for POST/PUT/PATCH)
        let explicit_body = positional.get(1).cloned();

        // Determine request body: pipeline_input > explicit body > None
        let body = match (&http_method, pipeline_input, &explicit_body) {
            // For POST/PUT/PATCH: pipeline input is the body
            (HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch, Some(input), _) => {
                Some(input.clone())
            }
            // Explicit body arg
            (HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch, None, Some(b)) => {
                Some(b.clone())
            }
            // GET/DELETE don't have a body
            _ => None,
        };

        // Extract query params: from named `query:` arg, or pipeline input for GET/DELETE
        let mut query_map: HashMap<String, String> = HashMap::new();
        // Named `query:` param
        for (name, val) in &named {
            if name == "query"
                && let Value::Map(m) = val
            {
                for (k, v) in m.read().unwrap().iter() {
                    query_map.insert(k.clone(), v.to_string());
                }
            }
        }
        // Pipeline input as query params for GET/DELETE
        if matches!(http_method, HttpMethod::Get | HttpMethod::Delete)
            && let Some(input) = pipeline_input
        {
            match input {
                Value::Map(m) => {
                    for (k, v) in m.read().unwrap().iter() {
                        query_map.insert(k.clone(), v.to_string());
                    }
                }
                _ => {
                    return Err(Self::http_client_error(
                        format!(
                            "pipeline input to http_client.{} must be a Map (query params), got {}",
                            method,
                            input.type_name()
                        ),
                        &op,
                    ));
                }
            }
        }

        // Extract headers from named `headers:` arg
        let mut headers_map: HashMap<String, String> = HashMap::new();
        for (name, val) in &named {
            if name == "headers"
                && let Value::Map(m) = val
            {
                for (k, v) in m.read().unwrap().iter() {
                    headers_map.insert(k.to_lowercase(), v.to_string());
                }
            }
        }
        if let Some(trace_context) = &self.trace_context {
            let child = trace_context.outbound_child();
            headers_map
                .entry("traceparent".to_string())
                .or_insert_with(|| child.traceparent());
            if let Some(tracestate) = child.tracestate {
                headers_map
                    .entry("tracestate".to_string())
                    .or_insert(tracestate);
            }
        }

        // Extract timeout from named `timeout:` arg (milliseconds)
        let timeout = named
            .iter()
            .find(|(name, _)| name == "timeout")
            .and_then(|(_, val)| val.as_integer())
            .filter(|ms| *ms > 0)
            .map(|ms| std::time::Duration::from_millis(ms as u64));

        // Build and execute request
        let request = HttpRequest {
            method: http_method,
            url: url.clone(),
            body,
            headers: headers_map,
            query: query_map,
            timeout,
        };

        let response = run_async(async move { driver.execute(request).await })
            .map_err(|e| Self::http_client_error(e.to_string(), &op))?;

        // Convert HttpResponse to Value::Map { status, body, headers }
        Self::http_response_to_value(response)
    }

    /// Converts an HttpClientResponse into a MarretaLang Map value.
    fn http_response_to_value(response: HttpClientResponse) -> Result<Value, MarretaError> {
        let mut map = ValueMap::new();
        map.insert("status".to_string(), Value::Integer(response.status as i64));
        map.insert("body".to_string(), response.body);

        let mut headers = ValueMap::new();
        for (k, v) in response.headers {
            headers.insert(k, Value::String(v));
        }
        map.insert(
            "headers".to_string(),
            Value::Map(Arc::new(RwLock::new(headers))),
        );

        Ok(Value::Map(Arc::new(RwLock::new(map))))
    }

    // =========================================================================
    // DB direct call dispatch
    // =========================================================================

    /// Dispatches `db.TABLE.operation(args)` to the active DB driver.
    /// Receives raw AST arguments so named arg keys are preserved (needed for find_all filters).
    /// Runs the async driver call on the current tokio runtime (blocking).
    /// Returns an error if no DB is configured.
    /// `doc.pipeline(collection, list)` — Layer 4 power pipeline.
    /// Translates a list of Marreta stage maps to MQL and executes them.
    pub(super) fn dispatch_doc_pipeline(&mut self, args: &[Value]) -> Result<Value, MarretaError> {
        if args.len() != 2 {
            return Err(MarretaError::TypeError {
                message: format!(
                    "doc.pipeline expects 2 arguments (collection, stages), got {}",
                    args.len()
                ),
                line: self.current_line,
                column: self.current_column,
            });
        }
        let collection = match &args[0] {
            Value::String(s) => s.clone(),
            _ => {
                return Err(MarretaError::TypeError {
                    message: "doc.pipeline first argument must be a string (collection name)"
                        .to_string(),
                    line: self.current_line,
                    column: self.current_column,
                });
            }
        };
        let stages = match &args[1] {
            Value::List(v) => v.clone(),
            _ => {
                return Err(MarretaError::TypeError {
                    message: "doc.pipeline second argument must be a list of stage maps"
                        .to_string(),
                    line: self.current_line,
                    column: self.current_column,
                });
            }
        };

        // Validate all stages upfront — translate_pipeline_stage returns descriptive errors
        // for unknown stage keys regardless of which driver is active (incl. mock in tests).
        for stage in &stages {
            crate::doc::mongodb::translate_pipeline_stage(stage)?;
        }

        let engine = self.require_doc_engine()?;
        let driver = engine.driver.clone();
        let rows = self.block_db(driver.raw_pipeline(&collection, &stages))?;
        Ok(Value::List(
            rows.into_iter()
                .map(crate::doc::mongodb::doc_row_to_value)
                .collect(),
        ))
    }

    pub(super) fn dispatch_doc_direct(
        &mut self,
        collection: String,
        method: &str,
        raw_args: &[Argument],
    ) -> Result<Value, MarretaError> {
        let engine = self.require_doc_engine()?;
        let driver = engine.driver.clone();

        match method {
            "save" => {
                let args = self.evaluate_args(raw_args)?;
                let data = args.first().ok_or_else(|| MarretaError::TypeError {
                    message: "doc.COLLECTION.save requires a map argument".to_string(),
                    line: self.current_line,
                    column: self.current_column,
                })?;
                let row = crate::doc::mongodb::value_to_doc_row(
                    data,
                    self.current_line,
                    self.current_column,
                )?;
                let result = self.block_db(driver.save(&collection, row))?;
                Ok(crate::doc::mongodb::doc_row_to_value(result))
            }
            "find" => {
                let args = self.evaluate_args(raw_args)?;
                let id = args.first().ok_or_else(|| MarretaError::TypeError {
                    message: "doc.COLLECTION.find requires an id argument".to_string(),
                    line: self.current_line,
                    column: self.current_column,
                })?;
                let result = self.block_db(driver.find(&collection, id))?;
                Ok(result
                    .map(crate::doc::mongodb::doc_row_to_value)
                    .unwrap_or(Value::Null))
            }
            "find_all" => {
                // In Phase 2 direct calls, find_all() takes no arguments.
                // It relies on subsequent where() or limits via the Pipeline in Phase 3.
                let result = self.block_db(driver.find_all(&collection))?;
                Ok(Value::List(
                    result
                        .into_iter()
                        .map(crate::doc::mongodb::doc_row_to_value)
                        .collect(),
                ))
            }
            "update" | "update_by_id" => {
                let args = self.evaluate_args(raw_args)?;
                let id = args.first().ok_or_else(|| MarretaError::TypeError {
                    message: "doc.COLLECTION.update requires (id, partial_map) arguments"
                        .to_string(),
                    line: self.current_line,
                    column: self.current_column,
                })?;
                let data = args.get(1).ok_or_else(|| MarretaError::TypeError {
                    message: "doc.COLLECTION.update requires a second map argument".to_string(),
                    line: self.current_line,
                    column: self.current_column,
                })?;
                let row = crate::doc::mongodb::value_to_doc_row(
                    data,
                    self.current_line,
                    self.current_column,
                )?;
                let result = self.block_db(driver.update_by_id(&collection, id, row))?;
                Ok(crate::doc::mongodb::doc_row_to_value(result))
            }
            "delete" | "delete_by_id" => {
                let args = self.evaluate_args(raw_args)?;
                let id = args.first().ok_or_else(|| MarretaError::TypeError {
                    message: "doc.COLLECTION.delete requires an id argument".to_string(),
                    line: self.current_line,
                    column: self.current_column,
                })?;
                let deleted = self.block_db(driver.delete_by_id(&collection, id))?;
                Ok(Value::Boolean(deleted))
            }
            // Fallback for missing direct methods
            _ => Err(MarretaError::PropertyNotFound {
                object_type: format!("DocCollection({})", collection),
                property: method.into(),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    /// Dispatches `db.TABLE.operation(args)` to the active DB driver.
    /// Receives raw AST arguments so named arg keys are preserved (needed for find_all filters).
    /// Runs the async driver call on the current tokio runtime (blocking).
    /// Returns an error if no DB is configured.
    pub(super) fn dispatch_db_direct(
        &mut self,
        table: String,
        method: &str,
        raw_args: &[Argument],
    ) -> Result<Value, MarretaError> {
        let engine = self.db_engine.clone().ok_or_else(|| MarretaError::TypeError {
            message: "db.* called but no DB is configured (set MARRETA_DB_PROVIDER, MARRETA_DB_HOST, MARRETA_DB_PORT, MARRETA_DB_NAME, MARRETA_DB_USER, and optional MARRETA_DB_PASSWORD)".to_string(),
            line: self.current_line,
            column: self.current_column,
        })?;

        let driver = engine.driver.clone();

        match method {
            "save" => {
                let args = self.evaluate_args(raw_args)?;
                let data = args.first().ok_or_else(|| MarretaError::TypeError {
                    message: "db.TABLE.save requires a map argument".to_string(),
                    line: self.current_line,
                    column: self.current_column,
                })?;
                let row = value_to_db_row(data, self.current_line, self.current_column)?;
                if self.resolve_schema_name_for_table(&table).is_some() && row.contains_key("id") {
                    return Err(MarretaError::TypeError {
                        message: "db.TABLE.save must not include `id` for db: schemas; id is generated by the database in v1".to_string(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
                let result = if self.has_active_tx() {
                    let mut tx = self.take_tx();
                    let table_name = table.clone();
                    let (tx_back, res) = run_async(async move {
                        let r = tx.save(&table_name, row).await;
                        (tx, r)
                    });
                    self.restore_tx(tx_back);
                    res
                } else {
                    self.block_db(driver.save(&table, row))
                }?;
                Ok(self.db_row_to_runtime_value(&table, result))
            }

            "find" => {
                let args = self.evaluate_args(raw_args)?;
                let id = args.first().ok_or_else(|| MarretaError::TypeError {
                    message: "db.TABLE.find requires an id argument".to_string(),
                    line: self.current_line,
                    column: self.current_column,
                })?;
                let result = if self.has_active_tx() {
                    let mut tx = self.take_tx();
                    let id = id.clone();
                    let table_name = table.clone();
                    let (tx_back, res) = run_async(async move {
                        let r = tx.find(&table_name, &id).await;
                        (tx, r)
                    });
                    self.restore_tx(tx_back);
                    res
                } else {
                    self.block_db(driver.find(&table, id))
                }?;
                Ok(result
                    .map(|row| self.db_row_to_runtime_value(&table, row))
                    .unwrap_or(Value::Null))
            }

            "find_all" => {
                let filters = self.args_to_equality_filters(raw_args)?;
                let rows = if self.has_active_tx() {
                    let mut tx = self.take_tx();
                    let table_name = table.clone();
                    let (tx_back, res) = run_async(async move {
                        let r = tx.find_all(&table_name, filters).await;
                        (tx, r)
                    });
                    self.restore_tx(tx_back);
                    res
                } else {
                    self.block_db(driver.find_all(&table, filters))
                }?;
                Ok(Value::List(
                    rows.into_iter()
                        .map(|row| self.db_row_to_runtime_value(&table, row))
                        .collect(),
                ))
            }

            "update" => {
                let args = self.evaluate_args(raw_args)?;
                let id = args.first().ok_or_else(|| MarretaError::TypeError {
                    message: "db.TABLE.update requires (id, partial_map) arguments".to_string(),
                    line: self.current_line,
                    column: self.current_column,
                })?;
                let data = args.get(1).ok_or_else(|| MarretaError::TypeError {
                    message: "db.TABLE.update requires a second argument (partial map)".to_string(),
                    line: self.current_line,
                    column: self.current_column,
                })?;
                let row = value_to_db_row(data, self.current_line, self.current_column)?;
                let result = if self.has_active_tx() {
                    let mut tx = self.take_tx();
                    let id = id.clone();
                    let table_name = table.clone();
                    let (tx_back, res) = run_async(async move {
                        let r = tx.update_by_id(&table_name, &id, row).await;
                        (tx, r)
                    });
                    self.restore_tx(tx_back);
                    res
                } else {
                    self.block_db(driver.update_by_id(&table, id, row))
                }?;
                Ok(result
                    .map(|row| self.db_row_to_runtime_value(&table, row))
                    .unwrap_or(Value::Null))
            }

            "delete" => {
                let args = self.evaluate_args(raw_args)?;
                let id = args.first().ok_or_else(|| MarretaError::TypeError {
                    message: "db.TABLE.delete requires an id argument".to_string(),
                    line: self.current_line,
                    column: self.current_column,
                })?;
                let deleted = if self.has_active_tx() {
                    let mut tx = self.take_tx();
                    let id = id.clone();
                    let (tx_back, res) = run_async(async move {
                        let r = tx.delete_by_id(&table, &id).await;
                        (tx, r)
                    });
                    self.restore_tx(tx_back);
                    res
                } else {
                    self.block_db(driver.delete_by_id(&table, id))
                }?;
                Ok(Value::Boolean(deleted))
            }

            other => Err(MarretaError::TypeError {
                message: format!(
                    "unknown db operation '{}' on table '{}' — supported: save, find, find_all, update, delete",
                    other, table
                ),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    /// Dispatches `db.native_query(sql, arg1, arg2, …)` to the active DB driver.
    ///
    /// The SQL string uses Postgres positional placeholders (`$1`, `$2`, …) directly.
    /// Extra arguments after the SQL string are bound in order.
    ///
    /// Example:
    ///   `db.native_query("SELECT * FROM users WHERE id = $1", user_id)`
    pub(super) fn dispatch_native_query(
        &mut self,
        raw_args: &[Argument],
    ) -> Result<Value, MarretaError> {
        let engine = self.require_db_engine()?;

        // Extract the raw SQL string from the AST — do NOT evaluate/interpolate it yet,
        // because #{} placeholders must be extracted first and bound as prepared params.
        let sql_template = match raw_args.first() {
            Some(crate::ast::Argument::Positional(crate::ast::Expression::StringLiteral(s))) => {
                s.clone()
            }
            Some(_) => {
                // Fallback: evaluate the first arg and coerce to string (no interpolation safety).
                let args = self.evaluate_args(raw_args)?;
                match args.into_iter().next() {
                    Some(Value::String(s)) => s,
                    Some(other) => {
                        return Err(MarretaError::TypeError {
                            message: format!(
                                "db.native_query first argument must be a string, got {}",
                                other.type_name()
                            ),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                    None => unreachable!(),
                }
            }
            None => {
                return Err(MarretaError::TypeError {
                    message: "db.native_query requires a SQL string as the first argument"
                        .to_string(),
                    line: self.current_line,
                    column: self.current_column,
                });
            }
        };

        // Extract #{expr} placeholders from the SQL template, evaluate each,
        // replace with $1, $2, … and collect values as prepared statement params.
        let (sql, params) = self.extract_native_query_params(&sql_template)?;

        let rows = if self.has_active_tx() {
            let mut tx = self.take_tx();
            let (tx_back, res) = run_async(async move {
                let r = tx.native_query(&sql, params).await;
                (tx, r)
            });
            self.restore_tx(tx_back);
            res
        } else {
            self.block_db(engine.driver.native_query(&sql, params))
        }?;

        Ok(Value::List(rows.into_iter().map(db_row_to_value).collect()))
    }

    /// Parses a SQL template string, extracting `#{}` interpolations as
    /// prepared statement parameters.
    ///
    /// `"SELECT * FROM users WHERE email = #{email} AND active = #{flag}"`
    /// →  sql    = `"SELECT * FROM users WHERE email = $1 AND active = $2"`
    ///    params = `[Value::String("ana@..."), Value::Boolean(true)]`
    fn extract_native_query_params(
        &mut self,
        template: &str,
    ) -> Result<(String, Vec<Value>), MarretaError> {
        if !template.contains("#{") {
            return Ok((template.to_string(), Vec::new()));
        }

        let mut sql = String::new();
        let mut params: Vec<Value> = Vec::new();
        let chars: Vec<char> = template.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if i + 1 < chars.len() && chars[i] == '#' && chars[i + 1] == '{' {
                i += 2; // skip `#{`
                let start = i;
                let mut depth = 1;
                while i < chars.len() && depth > 0 {
                    if chars[i] == '{' {
                        depth += 1;
                    }
                    if chars[i] == '}' {
                        depth -= 1;
                    }
                    if depth > 0 {
                        i += 1;
                    }
                }
                let expr_src: String = chars[start..i].iter().collect();
                i += 1; // skip `}`

                // Parse and evaluate the expression inside #{}
                let val = self.evaluate_source_expression(expr_src.trim())?;
                params.push(val);
                sql.push_str(&format!("${}", params.len()));
            } else {
                sql.push(chars[i]);
                i += 1;
            }
        }

        Ok((sql, params))
    }

    /// Parses a single expression from a source string and evaluates it
    /// in the current interpreter scope. Used by #{} extraction in native_query.
    pub(super) fn evaluate_source_expression(&mut self, src: &str) -> Result<Value, MarretaError> {
        use crate::ast::Statement;
        use crate::lexer::Lexer;
        use crate::parser::Parser;

        // Wrap in an assignment so the public `parse()` entry point can handle it.
        let wrapped = format!("__nq_param = {}", src);
        let tokens = Lexer::new(&wrapped)
            .tokenize()
            .map_err(|e| MarretaError::TypeError {
                message: format!("invalid expression in native_query #{{{}}}: {}", src, e),
                line: self.current_line,
                column: self.current_column,
            })?;
        let program = Parser::new(tokens)
            .parse()
            .map_err(|e| MarretaError::TypeError {
                message: format!("invalid expression in native_query #{{{}}}: {}", src, e),
                line: self.current_line,
                column: self.current_column,
            })?;
        // The program is a single Assignment — extract and evaluate the RHS.
        match program.into_iter().next() {
            Some(Statement::Assignment { value, .. }) => self.evaluate(&value),
            _ => Err(MarretaError::TypeError {
                message: format!("could not parse expression in native_query #{{{}}}", src),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    /// Converts named AST arguments to equality `FilterClause` list.
    /// `find_all(status: "active", role: "admin")` → two Eq filter clauses.
    /// Positional arguments are rejected with a descriptive error.
    fn args_to_equality_filters(
        &mut self,
        raw_args: &[Argument],
    ) -> Result<Vec<FilterClause>, MarretaError> {
        let mut filters = Vec::new();
        for arg in raw_args {
            match arg {
                Argument::Named { name, value } => {
                    let val = self.evaluate(value)?;
                    filters.push(FilterClause {
                        column: name.clone(),
                        op: FilterOp::Eq,
                        value: val,
                    });
                }
                Argument::Positional(_) => {
                    return Err(MarretaError::TypeError {
                        message: "find_all filters must be named arguments (e.g. find_all(status: \"active\"))".to_string(),
                        line: self.current_line,
                        column: self.current_column,
                    });
                }
            }
        }
        Ok(filters)
    }
}
