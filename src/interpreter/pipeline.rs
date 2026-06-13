use super::*;

impl Interpreter {
    /// Spec 076 (perf follow-up): the known column names for a `db:` table, when a schema declares
    /// it, for the identifier guard's schema layer. An O(1) lookup of the `Arc` index computed once
    /// at load (`ProjectRuntime.db_columns`), so promoting `db.<table>` clones a pointer, not the
    /// set, and does not rebuild the schema model per query. `None` when no schema declares the
    /// table (the syntactic floor still guards it) or no project runtime is available.
    fn db_known_columns(
        &self,
        table: &str,
    ) -> Option<std::sync::Arc<std::collections::HashSet<String>>> {
        self.project_runtime
            .as_ref()?
            .db_columns
            .get(table)
            .cloned()
    }

    pub(super) fn evaluate_pipeline_stage(
        &mut self,
        input: &Value,
        stage: &PipelineStage,
    ) -> Result<Value, MarretaError> {
        // If input is DbTable, promote to QueryBuilder before processing the stage.
        // This handles `db.users >> where(...) >> fetch` — the first `>>` converts
        // `DbTable("users")` into a lazy `QueryBuilder`.
        let input = if let Value::DbTable(table) = input {
            let mut state = crate::db::driver::QueryState::new(table.clone());
            state.known_columns = self.db_known_columns(table);
            Value::QueryBuilder(Box::new(state))
        } else if let Value::DocCollection(coll) = input {
            Value::DocQueryBuilder(Box::new(crate::doc::query::DocQueryState::new(
                coll.clone(),
            )))
        } else {
            input.clone()
        };

        if let Value::RelationHandle(ref relation) = input {
            if let PipelineStage::Expression(expr) = stage {
                return self.apply_relation_pipeline_stage(relation, expr);
            }
            return Err(MarretaError::TypeError {
                message: "cannot use map/keep on a relation handle — did you forget >> fetch?"
                    .to_string(),
                line: self.current_line,
                column: self.current_column,
            });
        }

        // If input is a QueryBuilder, handle DB pipeline steps and terminals.
        if let Value::QueryBuilder(ref q) = input {
            if let PipelineStage::Expression(expr) = stage {
                return self.apply_query_pipeline_stage(q, expr);
            }
            return Err(MarretaError::TypeError {
                message: "cannot use map/keep on a QueryBuilder — did you forget >> fetch or >> fetch_one?".to_string(),
                line: self.current_line,
                column: self.current_column,
            });
        }

        // If input is a DocQueryBuilder, handle MongoDB pipeline steps and terminals.
        if let Value::DocQueryBuilder(ref q) = input {
            if let PipelineStage::Expression(expr) = stage {
                return self.apply_doc_query_pipeline_stage(q, expr);
            }
            return Err(MarretaError::TypeError {
                message: "cannot use map/keep on a DocQueryBuilder — did you forget >> fetch?"
                    .to_string(),
                line: self.current_line,
                column: self.current_column,
            });
        }

        match stage {
            PipelineStage::Expression(expr) => self.apply_pipeline_value(&input, expr),
            PipelineStage::Map { variable, body } => match &input {
                Value::List(items) => {
                    let mut results = Vec::new();
                    for item in items {
                        self.env.push_scope();
                        self.env.set(variable.clone(), item.clone());
                        let mut kept: Option<Value> = None;
                        let mut skipped = false;
                        'map_body: for map_stmt in body {
                            match map_stmt {
                                crate::ast::MapStatement::Statement(stmt) => {
                                    self.execute_statement(stmt)?;
                                }
                                crate::ast::MapStatement::Keep { value, condition } => {
                                    if let Some(cond_expr) = condition {
                                        let cond_val = self.evaluate(cond_expr)?;
                                        if cond_val.is_truthy() {
                                            kept = Some(self.evaluate(value)?);
                                            break 'map_body;
                                        }
                                        // condition false — fall through
                                    } else {
                                        // unconditional keep
                                        kept = Some(self.evaluate(value)?);
                                        break 'map_body;
                                    }
                                }
                                crate::ast::MapStatement::Skip { condition } => {
                                    let cond_val = self.evaluate(condition)?;
                                    if cond_val.is_truthy() {
                                        skipped = true;
                                        break 'map_body;
                                    }
                                    // condition false — fall through
                                }
                            }
                        }
                        self.env.pop_scope();
                        if !skipped && let Some(v) = kept {
                            results.push(v);
                        }
                        // else: no keep fired, implicit skip — element dropped
                    }
                    Ok(Value::List(results))
                }
                _ => Err(MarretaError::TypeError {
                    message: format!("pipeline map requires a List, got {}", input.type_name()),
                    line: self.current_line,
                    column: self.current_column,
                }),
            },
            PipelineStage::Reduce {
                initial,
                accumulator,
                item,
                body,
            } => match &input {
                Value::List(items) => {
                    let mut acc = self.evaluate(initial)?;
                    for current_item in items {
                        self.env.push_scope();
                        self.env.set(accumulator.clone(), acc.clone());
                        self.env.set(item.clone(), current_item.clone());
                        let next = match body {
                            TaskBody::Inline(expr) => self.evaluate(expr)?,
                            TaskBody::Block(stmts, return_expr) => {
                                for stmt in stmts {
                                    self.execute_statement(stmt)?;
                                }
                                self.evaluate(return_expr)?
                            }
                        };
                        self.env.pop_scope();
                        acc = next;
                    }
                    Ok(acc)
                }
                _ => Err(MarretaError::TypeError {
                    message: format!("pipeline reduce requires a List, got {}", input.type_name()),
                    line: self.current_line,
                    column: self.current_column,
                }),
            },
            PipelineStage::Rescue { .. } => {
                // Rescue stages are handled in the railway-oriented pipeline path.
                // If we reach here, no error occurred — pass input through unchanged.
                Ok(input)
            }
        }
    }

    /// Handles a single pipeline stage when the input is a `QueryBuilder`.
    ///
    /// Steps (`where`, `join`, `left_join`, `select`, `order_by`, `limit`, `offset`) accumulate
    /// clauses and return an updated `QueryBuilder`.
    ///
    /// Terminals (`fetch`, `fetch_one`, `count`, `exists`, `update`, `delete`) execute the query
    /// and return a plain `Value` (closing the SQL context).
    fn apply_query_pipeline_stage(
        &mut self,
        q: &crate::db::driver::QueryState,
        expr: &Expression,
    ) -> Result<Value, MarretaError> {
        use crate::db::driver::{JoinClause, JoinKind};

        match expr {
            // ── Accumulating steps ─────────────────────────────────────────────
            Expression::FunctionCall { name, arguments } => match name.as_str() {
                "where" => {
                    let filters = self.parse_where_args(arguments)?;
                    let mut next = q.clone();
                    next.filters.extend(filters);
                    Ok(Value::QueryBuilder(Box::new(next)))
                }

                "join" | "left_join" => {
                    let kind = if name == "join" {
                        JoinKind::Inner
                    } else {
                        JoinKind::Left
                    };
                    // join("table", on: "fk_col")
                    let args = self.evaluate_args(arguments)?;
                    let table = match args.first() {
                        Some(Value::String(s)) => s.clone(),
                        _ => {
                            return Err(MarretaError::TypeError {
                                message: format!(
                                    "{} first argument must be a table name string",
                                    name
                                ),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    // `on:` named arg — extract from raw arguments
                    let on = self
                        .extract_named_string_arg(arguments, "on")?
                        .ok_or_else(|| MarretaError::TypeError {
                            message: format!("{} requires on: \"fk_column\" named argument", name),
                            line: self.current_line,
                            column: self.current_column,
                        })?;
                    let mut next = q.clone();
                    next.joins.push(JoinClause { kind, table, on });
                    Ok(Value::QueryBuilder(Box::new(next)))
                }

                "select" => {
                    let cols = self
                        .evaluate_args(arguments)?
                        .into_iter()
                        .map(|v| match v {
                            Value::String(s) => Ok(s),
                            other => Err(MarretaError::TypeError {
                                message: format!(
                                    "select columns must be strings, got {}",
                                    other.type_name()
                                ),
                                line: self.current_line,
                                column: self.current_column,
                            }),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let mut next = q.clone();
                    next.select_cols = cols;
                    Ok(Value::QueryBuilder(Box::new(next)))
                }

                "order_by" => {
                    let args = self.evaluate_args(arguments)?;
                    let order = match args.first() {
                        Some(Value::String(s)) => s.clone(),
                        _ => return Err(MarretaError::TypeError {
                            message: "order_by requires a string argument (e.g. order_by(\"created_at desc\"))".to_string(),
                            line: self.current_line,
                            column: self.current_column,
                        }),
                    };
                    let mut next = q.clone();
                    next.order_by = Some(order);
                    Ok(Value::QueryBuilder(Box::new(next)))
                }

                "limit" => {
                    let args = self.evaluate_args(arguments)?;
                    let n = match args.first() {
                        Some(Value::Integer(n)) => *n,
                        _ => {
                            return Err(MarretaError::TypeError {
                                message: "limit requires an integer argument".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let mut next = q.clone();
                    next.limit = Some(n);
                    Ok(Value::QueryBuilder(Box::new(next)))
                }

                "offset" => {
                    let args = self.evaluate_args(arguments)?;
                    let n = match args.first() {
                        Some(Value::Integer(n)) => *n,
                        _ => {
                            return Err(MarretaError::TypeError {
                                message: "offset requires an integer argument".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let mut next = q.clone();
                    next.offset = Some(n);
                    Ok(Value::QueryBuilder(Box::new(next)))
                }

                "like" => {
                    let args = self.evaluate_args(arguments)?;
                    let col = match args.first() {
                        Some(Value::String(s)) => s.clone(),
                        _ => return Err(MarretaError::TypeError {
                            message: "like requires a column name string as first argument (e.g. like(\"name\", \"Ana%\"))".to_string(),
                            line: self.current_line,
                            column: self.current_column,
                        }),
                    };
                    let pattern = match args.get(1) {
                        Some(v) => v.clone(),
                        None => {
                            return Err(MarretaError::TypeError {
                                message: "like requires a pattern string as second argument"
                                    .to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let mut next = q.clone();
                    next.filters.push(FilterClause {
                        column: col,
                        op: FilterOp::Like,
                        value: pattern,
                    });
                    Ok(Value::QueryBuilder(Box::new(next)))
                }

                "in" => {
                    let args = self.evaluate_args(arguments)?;
                    let col = match args.first() {
                        Some(Value::String(s)) => s.clone(),
                        _ => return Err(MarretaError::TypeError {
                            message: "in requires a column name string as first argument (e.g. in(\"status\", [\"active\", \"pending\"]))".to_string(),
                            line: self.current_line,
                            column: self.current_column,
                        }),
                    };
                    let list = match args.get(1) {
                        Some(v @ Value::List(_)) => v.clone(),
                        Some(other) => {
                            return Err(MarretaError::TypeError {
                                message: format!(
                                    "in requires a list as second argument, got {}",
                                    other.type_name()
                                ),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                        None => {
                            return Err(MarretaError::TypeError {
                                message: "in requires a list as second argument".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let mut next = q.clone();
                    next.filters.push(FilterClause {
                        column: col,
                        op: FilterOp::In,
                        value: list,
                    });
                    Ok(Value::QueryBuilder(Box::new(next)))
                }

                // ── Terminals ──────────────────────────────────────────────────
                "update" => {
                    let args = self.evaluate_args(arguments)?;
                    let data = match args.first() {
                        Some(v) => value_to_db_row(v, self.current_line, self.current_column)?,
                        None => {
                            return Err(MarretaError::TypeError {
                                message: ">> update({...}) requires a map argument".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let engine = self.require_db_engine()?;
                    let rows_affected = if self.has_active_tx() {
                        let mut tx = self.take_tx();
                        let q = q.clone();
                        let (tx_back, res) = run_async(async move {
                            let r = tx.query_update(&q, data).await;
                            (tx, r)
                        });
                        self.restore_tx(tx_back);
                        res
                    } else {
                        self.block_db(engine.driver.query_update(q, data))
                    }?;
                    Ok(Value::Integer(rows_affected as i64))
                }

                _ => Err(MarretaError::TypeError {
                    message: format!(
                        "unknown query pipeline step '{}' — supported steps: where, like, in, join, left_join, select, order_by, limit, offset; terminals: fetch, fetch_one, count, exists, update, delete",
                        name
                    ),
                    line: self.current_line,
                    column: self.current_column,
                }),
            },

            // Terminals via bare identifier: `>> fetch`, `>> fetch_all`, `>> fetch_one`, `>> count`, `>> exists`, `>> delete`
            Expression::Identifier(name) => {
                let engine = self.require_db_engine()?;
                match name.as_str() {
                    "fetch" | "fetch_all" => {
                        let rows = if self.has_active_tx() {
                            let mut tx = self.take_tx();
                            let q = q.clone();
                            let (tx_back, res) = run_async(async move {
                                let r = tx.query_fetch(&q).await;
                                (tx, r)
                            });
                            self.restore_tx(tx_back);
                            res
                        } else {
                            self.block_db(engine.driver.query_fetch(q))
                        }?;
                        Ok(Value::List(
                            rows.into_iter()
                                .map(|row| self.db_row_to_runtime_value(&q.table, row))
                                .collect(),
                        ))
                    }
                    "fetch_one" => {
                        let row = if self.has_active_tx() {
                            let mut tx = self.take_tx();
                            let q = q.clone();
                            let (tx_back, res) = run_async(async move {
                                let r = tx.query_fetch_one(&q).await;
                                (tx, r)
                            });
                            self.restore_tx(tx_back);
                            res
                        } else {
                            self.block_db(engine.driver.query_fetch_one(q))
                        }?;
                        Ok(row
                            .map(|db_row| self.db_row_to_runtime_value(&q.table, db_row))
                            .unwrap_or(Value::Null))
                    }
                    "count" => {
                        let n = if self.has_active_tx() {
                            let mut tx = self.take_tx();
                            let q = q.clone();
                            let (tx_back, res) = run_async(async move {
                                let r = tx.query_count(&q).await;
                                (tx, r)
                            });
                            self.restore_tx(tx_back);
                            res
                        } else {
                            self.block_db(engine.driver.query_count(q))
                        }?;
                        Ok(Value::Integer(n))
                    }
                    "exists" => {
                        let b = if self.has_active_tx() {
                            let mut tx = self.take_tx();
                            let q = q.clone();
                            let (tx_back, res) = run_async(async move {
                                let r = tx.query_exists(&q).await;
                                (tx, r)
                            });
                            self.restore_tx(tx_back);
                            res
                        } else {
                            self.block_db(engine.driver.query_exists(q))
                        }?;
                        Ok(Value::Boolean(b))
                    }
                    "delete" => {
                        let n = if self.has_active_tx() {
                            let mut tx = self.take_tx();
                            let q = q.clone();
                            let (tx_back, res) = run_async(async move {
                                let r = tx.query_delete(&q).await;
                                (tx, r)
                            });
                            self.restore_tx(tx_back);
                            res
                        } else {
                            self.block_db(engine.driver.query_delete(q))
                        }?;
                        Ok(Value::Integer(n as i64))
                    }
                    _ => Err(MarretaError::TypeError {
                        message: format!(
                            "'{}' is not a valid query terminal — use: fetch, fetch_one, count, exists, update({{...}}), delete",
                            name
                        ),
                        line: self.current_line,
                        column: self.current_column,
                    }),
                }
            }

            other => Err(MarretaError::TypeError {
                message: format!(
                    "expected a query pipeline step or terminal after QueryBuilder, got {:?}",
                    other
                ),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn apply_relation_pipeline_stage(
        &mut self,
        relation: &RelationHandle,
        expr: &Expression,
    ) -> Result<Value, MarretaError> {
        match relation.cardinality {
            RelationCardinality::Many => self.apply_query_pipeline_stage(&relation.query, expr),
            RelationCardinality::One => match expr {
                Expression::Identifier(name) if name == "fetch" => {
                    if relation.null_short_circuit {
                        return Ok(Value::Null);
                    }
                    let engine = self.require_db_engine()?;
                    let row = if self.has_active_tx() {
                        let mut tx = self.take_tx();
                        let q = relation.query.clone();
                        let (tx_back, res) = run_async(async move {
                            let r = tx.query_fetch_one(&q).await;
                            (tx, r)
                        });
                        self.restore_tx(tx_back);
                        res
                    } else {
                        self.block_db(engine.driver.query_fetch_one(&relation.query))
                    }?;
                    Ok(row
                        .map(|db_row| self.db_row_to_runtime_value(&relation.query.table, db_row))
                        .unwrap_or(Value::Null))
                }
                Expression::Identifier(name) if name == "exists" => {
                    if relation.null_short_circuit {
                        return Ok(Value::Boolean(false));
                    }
                    let engine = self.require_db_engine()?;
                    let exists = if self.has_active_tx() {
                        let mut tx = self.take_tx();
                        let q = relation.query.clone();
                        let (tx_back, res) = run_async(async move {
                            let r = tx.query_exists(&q).await;
                            (tx, r)
                        });
                        self.restore_tx(tx_back);
                        res
                    } else {
                        self.block_db(engine.driver.query_exists(&relation.query))
                    }?;
                    Ok(Value::Boolean(exists))
                }
                Expression::Identifier(name) => Err(MarretaError::TypeError {
                    message: format!(
                        "singular relation '{}' only supports >> fetch and >> exists",
                        name
                    ),
                    line: self.current_line,
                    column: self.current_column,
                }),
                Expression::FunctionCall { name, .. } => Err(MarretaError::TypeError {
                    message: format!(
                        "singular relation does not support {}(); use >> fetch or >> exists",
                        name
                    ),
                    line: self.current_line,
                    column: self.current_column,
                }),
                other => Err(MarretaError::TypeError {
                    message: format!(
                        "expected >> fetch or >> exists after singular relation, got {:?}",
                        other
                    ),
                    line: self.current_line,
                    column: self.current_column,
                }),
            },
        }
    }

    fn apply_doc_query_pipeline_stage(
        &mut self,
        q: &crate::doc::query::DocQueryState,
        expr: &Expression,
    ) -> Result<Value, MarretaError> {
        use crate::doc::query::SortDirection;

        match expr {
            Expression::FunctionCall { name, arguments } => match name.as_str() {
                "where" => {
                    let filters = self.parse_doc_where_args(arguments)?;
                    let mut next = q.clone();
                    next.filters.extend(filters);
                    Ok(Value::DocQueryBuilder(Box::new(next)))
                }
                "pick" => {
                    if q.is_aggregate() {
                        return Err(MarretaError::TypeError {
                            message: "pick() cannot be used in aggregation pipelines — use accumulator aliases directly".to_string(),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                    let args = self.evaluate_args(arguments)?;
                    let list = match args.first() {
                        Some(Value::List(l)) => l.clone(),
                        _ => {
                            return Err(MarretaError::TypeError {
                                message: "pick requires a list string field names".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let mut fields = Vec::new();
                    for item in list {
                        if let Value::String(s) = item {
                            fields.push(s);
                        } else {
                            return Err(MarretaError::TypeError {
                                message: "pick fields must be strings".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    }
                    let mut next = q.clone();
                    next.projection = Some(fields);
                    Ok(Value::DocQueryBuilder(Box::new(next)))
                }
                "order" => {
                    let args = self.evaluate_args(arguments)?;
                    let field = match args.first() {
                        Some(Value::String(s)) => s.clone(),
                        _ => return Err(MarretaError::TypeError {
                            message: "order requires a string field name as first argument, e.g. order(\"created_at\", \"desc\")".to_string(),
                            line: self.current_line,
                            column: self.current_column,
                        }),
                    };
                    let dir = match args.get(1) {
                        Some(Value::String(s)) if s.to_lowercase() == "desc" => SortDirection::Desc,
                        Some(Value::String(s)) if s.to_lowercase() == "asc" => SortDirection::Asc,
                        Some(_) => return Err(MarretaError::TypeError {
                            message: "order direction must be \"asc\" or \"desc\"".to_string(),
                            line: self.current_line,
                            column: self.current_column,
                        }),
                        None => return Err(MarretaError::TypeError {
                            message: "order requires a direction as second argument: order(\"field\", \"asc\") or order(\"field\", \"desc\")".to_string(),
                            line: self.current_line,
                            column: self.current_column,
                        }),
                    };
                    let mut next = q.clone();
                    if q.is_aggregate() {
                        next.post_sort = Some((field, dir));
                    } else {
                        next.sort = Some((field, dir));
                    }
                    Ok(Value::DocQueryBuilder(Box::new(next)))
                }
                "limit" => {
                    let args = self.evaluate_args(arguments)?;
                    let n = match args.first() {
                        Some(Value::Integer(n)) => *n,
                        _ => {
                            return Err(MarretaError::TypeError {
                                message: "limit requires an integer argument".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let mut next = q.clone();
                    if q.is_aggregate() {
                        next.post_limit = Some(n);
                    } else {
                        next.limit = Some(n);
                    }
                    Ok(Value::DocQueryBuilder(Box::new(next)))
                }
                "offset" => {
                    let args = self.evaluate_args(arguments)?;
                    let n = match args.first() {
                        Some(Value::Integer(n)) => *n,
                        _ => {
                            return Err(MarretaError::TypeError {
                                message: "offset requires an integer argument".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let mut next = q.clone();
                    next.offset = Some(n);
                    Ok(Value::DocQueryBuilder(Box::new(next)))
                }
                "like" => {
                    let args = self.evaluate_args(arguments)?;
                    let col = match args.first() {
                        Some(Value::String(s)) => s.clone(),
                        _ => {
                            return Err(MarretaError::TypeError {
                                message: "like requires a column name string".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let pattern = match args.get(1) {
                        Some(Value::String(s)) => s.clone(),
                        _ => {
                            return Err(MarretaError::TypeError {
                                message: "like requires a pattern string".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let mut next = q.clone();
                    next.filters
                        .push(crate::doc::query::DocFilter::Like(col, pattern));
                    Ok(Value::DocQueryBuilder(Box::new(next)))
                }
                "in" => {
                    let args = self.evaluate_args(arguments)?;
                    let col = match args.first() {
                        Some(Value::String(s)) => s.clone(),
                        _ => {
                            return Err(MarretaError::TypeError {
                                message: "in requires a column name string".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let list = match args.get(1) {
                        Some(Value::List(l)) => l.clone(),
                        _ => {
                            return Err(MarretaError::TypeError {
                                message: "in requires a list as second argument".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let mut next = q.clone();
                    next.filters
                        .push(crate::doc::query::DocFilter::In(col, list));
                    Ok(Value::DocQueryBuilder(Box::new(next)))
                }
                "group_by" => {
                    use crate::doc::query::DocQueryMode;
                    if !q.accumulators.is_empty() {
                        return Err(MarretaError::TypeError {
                            message: "group_by() must appear before accumulator steps (sum, avg, min, max, count)".to_string(),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                    let args = self.evaluate_args(arguments)?;
                    let field = match args.first() {
                        Some(Value::String(s)) => s.clone(),
                        _ => {
                            return Err(MarretaError::TypeError {
                                message: "group_by requires a string field name".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let mut next = q.clone();
                    next.group_by = Some(field);
                    next.mode = DocQueryMode::Aggregate;
                    Ok(Value::DocQueryBuilder(Box::new(next)))
                }
                "sum" | "avg" | "min" | "max" => {
                    use crate::doc::query::{Accumulator, DocQueryMode};
                    let step_name = name.clone();
                    // Validate not a write terminal context
                    if matches!(
                        q.mode,
                        DocQueryMode::Update(_) | DocQueryMode::Upsert(_) | DocQueryMode::Delete
                    ) {
                        return Err(MarretaError::TypeError {
                            message: "write terminals (update/upsert/delete) cannot follow aggregation steps".to_string(),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                    // Parse: first positional arg = field, named arg `as` = alias
                    let mut field_opt: Option<String> = None;
                    let mut alias_opt: Option<String> = None;
                    for arg in arguments {
                        match arg {
                            crate::ast::Argument::Positional(expr) => {
                                let v = self.evaluate(expr)?;
                                match v {
                                    Value::String(s) => field_opt = Some(s),
                                    _ => {
                                        return Err(MarretaError::TypeError {
                                            message: format!(
                                                "{}() field argument must be a string",
                                                step_name
                                            ),
                                            line: self.current_line,
                                            column: self.current_column,
                                        });
                                    }
                                }
                            }
                            crate::ast::Argument::Named {
                                name: arg_name,
                                value: expr,
                            } if arg_name == "as" => {
                                let v = self.evaluate(expr)?;
                                match v {
                                    Value::String(s) => alias_opt = Some(s),
                                    _ => {
                                        return Err(MarretaError::TypeError {
                                            message: format!(
                                                "{}() 'as:' argument must be a string",
                                                step_name
                                            ),
                                            line: self.current_line,
                                            column: self.current_column,
                                        });
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    let field = match field_opt {
                        Some(f) => f,
                        None => {
                            return Err(MarretaError::TypeError {
                                message: format!(
                                    "{}() requires a field name as first argument, e.g. {}(\"amount\", as: \"total\")",
                                    step_name, step_name
                                ),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let alias = match alias_opt {
                        Some(a) => a,
                        None => {
                            return Err(MarretaError::TypeError {
                                message: format!(
                                    "{}() requires a named 'as:' argument, e.g. {}(\"{}\", as: \"alias\")",
                                    step_name, step_name, field
                                ),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let acc = match step_name.as_str() {
                        "sum" => Accumulator::Sum { field, alias },
                        "avg" => Accumulator::Avg { field, alias },
                        "min" => Accumulator::Min { field, alias },
                        _ => Accumulator::Max { field, alias },
                    };
                    let mut next = q.clone();
                    next.accumulators.push(acc);
                    next.mode = DocQueryMode::Aggregate;
                    Ok(Value::DocQueryBuilder(Box::new(next)))
                }
                "count"
                    if q.is_aggregate()
                        || q.group_by.is_some()
                        || !q.accumulators.is_empty()
                        || {
                            // count as accumulator when any aggregation context hint is present
                            // OR when it has a named `as:` argument (disambiguate from terminal count)
                            arguments.iter().any(|a| matches!(a, crate::ast::Argument::Named { name, .. } if name == "as"))
                        } =>
                {
                    use crate::doc::query::{Accumulator, DocQueryMode};
                    // Reject positional field arg
                    for arg in arguments {
                        if let crate::ast::Argument::Positional(_) = arg {
                            return Err(MarretaError::TypeError {
                                message: "count() does not accept a field argument — use count(as: \"alias\")".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    }
                    let mut alias_opt: Option<String> = None;
                    for arg in arguments {
                        if let crate::ast::Argument::Named {
                            name: arg_name,
                            value: expr,
                        } = arg
                            && arg_name == "as"
                        {
                            let v = self.evaluate(expr)?;
                            if let Value::String(s) = v {
                                alias_opt = Some(s);
                            }
                        }
                    }
                    let alias = match alias_opt {
                        Some(a) => a,
                        None => return Err(MarretaError::TypeError {
                            message:
                                "count() requires a named 'as:' argument, e.g. count(as: \"total\")"
                                    .to_string(),
                            line: self.current_line,
                            column: self.current_column,
                        }),
                    };
                    let mut next = q.clone();
                    next.accumulators.push(Accumulator::Count { alias });
                    next.mode = DocQueryMode::Aggregate;
                    Ok(Value::DocQueryBuilder(Box::new(next)))
                }
                "update" => {
                    if q.is_aggregate() {
                        return Err(MarretaError::TypeError {
                            message: "write terminals (update/upsert/delete) cannot follow aggregation steps".to_string(),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                    let args = self.evaluate_args(arguments)?;
                    let data = match args.first() {
                        Some(v) => crate::doc::mongodb::value_to_doc_row(
                            v,
                            self.current_line,
                            self.current_column,
                        )?,
                        None => {
                            return Err(MarretaError::TypeError {
                                message: ">> update({...}) requires a map argument".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let engine = self.require_doc_engine()?;
                    let n = self.block_db(engine.driver.query_update(q, data))?;
                    Ok(Value::Integer(n))
                }
                "upsert" => {
                    if q.is_aggregate() {
                        return Err(MarretaError::TypeError {
                            message: "write terminals (update/upsert/delete) cannot follow aggregation steps".to_string(),
                            line: self.current_line,
                            column: self.current_column,
                        });
                    }
                    let args = self.evaluate_args(arguments)?;
                    let data = match args.first() {
                        Some(v) => crate::doc::mongodb::value_to_doc_row(
                            v,
                            self.current_line,
                            self.current_column,
                        )?,
                        None => {
                            return Err(MarretaError::TypeError {
                                message: ">> upsert({...}) requires a map argument".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                    };
                    let engine = self.require_doc_engine()?;
                    let n = self.block_db(engine.driver.query_upsert(q, data))?;
                    Ok(Value::Integer(n))
                }
                _ => Err(MarretaError::TypeError {
                    message: format!(
                        "unknown doc pipeline step '{}' — supported: where, pick, order, limit, offset, like, in, group_by, sum, avg, min, max, count; terminals: fetch_all, fetch_one, count, exists, update({{...}}), upsert({{...}}), delete",
                        name
                    ),
                    line: self.current_line,
                    column: self.current_column,
                }),
            },

            Expression::Identifier(name) => {
                let engine = self.require_doc_engine()?;
                match name.as_str() {
                    "fetch_all" | "fetch" => {
                        if q.is_aggregate() {
                            let rows = self.block_db(engine.driver.query_aggregate(q))?;
                            Ok(Value::List(
                                rows.into_iter()
                                    .map(crate::doc::mongodb::doc_row_to_value)
                                    .collect(),
                            ))
                        } else {
                            let rows = self.block_db(engine.driver.query_fetch(q))?;
                            Ok(Value::List(
                                rows.into_iter()
                                    .map(crate::doc::mongodb::doc_row_to_value)
                                    .collect(),
                            ))
                        }
                    }
                    "fetch_one" => {
                        if q.is_aggregate() {
                            let rows = self.block_db(engine.driver.query_aggregate(q))?;
                            Ok(rows
                                .into_iter()
                                .next()
                                .map(crate::doc::mongodb::doc_row_to_value)
                                .unwrap_or(Value::Null))
                        } else {
                            let row = self.block_db(engine.driver.query_fetch_one(q))?;
                            Ok(row
                                .map(crate::doc::mongodb::doc_row_to_value)
                                .unwrap_or(Value::Null))
                        }
                    }
                    "count" => {
                        let n = self.block_db(engine.driver.query_count(q))?;
                        Ok(Value::Integer(n))
                    }
                    "exists" => {
                        let b = self.block_db(engine.driver.query_exists(q))?;
                        Ok(Value::Boolean(b))
                    }
                    "delete" => {
                        if q.is_aggregate() {
                            return Err(MarretaError::TypeError {
                                message: "write terminals (update/upsert/delete) cannot follow aggregation steps".to_string(),
                                line: self.current_line,
                                column: self.current_column,
                            });
                        }
                        let n = self.block_db(engine.driver.query_delete(q))?;
                        Ok(Value::Integer(n))
                    }
                    _ => Err(MarretaError::TypeError {
                        message: format!(
                            "'{}' is not a valid doc query terminal — use: fetch_all, fetch_one, count, exists, delete",
                            name
                        ),
                        line: self.current_line,
                        column: self.current_column,
                    }),
                }
            }
            other => Err(MarretaError::TypeError {
                message: format!(
                    "expected a doc query pipeline step or terminal, got {:?}",
                    other
                ),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }
}
