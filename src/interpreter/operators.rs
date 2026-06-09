use super::*;

impl Interpreter {
    pub(super) fn apply_binary_op(
        &self,
        op: &BinaryOperator,
        left: &Value,
        right: &Value,
    ) -> Result<Value, MarretaError> {
        match op {
            // --- Arithmetic ---
            BinaryOperator::Add => self.op_add(left, right),
            BinaryOperator::Subtract => self.op_arith(left, right, |a, b| a - b, |a, b| a - b, "-"),
            BinaryOperator::Multiply => self.op_arith(left, right, |a, b| a * b, |a, b| a * b, "*"),
            BinaryOperator::Divide => self.op_divide(left, right),
            BinaryOperator::Modulo => self.op_modulo(left, right),

            // --- Comparison ---
            BinaryOperator::Equal => Ok(Value::Boolean(left == right)),
            BinaryOperator::NotEqual => Ok(Value::Boolean(left != right)),
            BinaryOperator::Greater => self.op_compare(left, right, |o| o.is_gt()),
            BinaryOperator::Less => self.op_compare(left, right, |o| o.is_lt()),
            BinaryOperator::GreaterEqual => self.op_compare(left, right, |o| o.is_ge()),
            BinaryOperator::LessEqual => self.op_compare(left, right, |o| o.is_le()),
            BinaryOperator::In => self.op_in(left, right),

            // And/Or handled in evaluate() for short-circuit
            BinaryOperator::And | BinaryOperator::Or => unreachable!(),
        }
    }

    fn op_in(&self, left: &Value, right: &Value) -> Result<Value, MarretaError> {
        match right {
            Value::List(items) => Ok(Value::Boolean(items.iter().any(|item| item == left))),
            Value::String(haystack) => match left {
                Value::String(needle) => Ok(Value::Boolean(haystack.contains(needle))),
                _ => Err(MarretaError::TypeError {
                    message: format!(
                        "left operand of 'in' must be String when right operand is String, got {}",
                        left.type_name()
                    ),
                    line: self.current_line,
                    column: self.current_column,
                }),
            },
            _ => Err(MarretaError::TypeError {
                message: format!(
                    "right operand of 'in' must be List or String, got {}",
                    right.type_name()
                ),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn op_add(&self, left: &Value, right: &Value) -> Result<Value, MarretaError> {
        match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a + b)),
            (Value::Decimal(a), Value::Decimal(b)) => Ok(Value::Decimal(*a + *b)),
            (Value::Decimal(a), Value::Integer(b)) => Ok(Value::Decimal(*a + Decimal::from(*b))),
            (Value::Integer(a), Value::Decimal(b)) => Ok(Value::Decimal(Decimal::from(*a) + *b)),
            (Value::Decimal(_), Value::Float(_)) | (Value::Float(_), Value::Decimal(_)) => {
                Err(MarretaError::TypeError {
                    message: "cannot mix Decimal and Float".into(),
                    line: self.current_line,
                    column: self.current_column,
                })
            }
            (Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
            (Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a + *b as f64)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            // String concatenation
            (Value::String(a), b) => Ok(Value::String(format!("{}{}", a, b))),
            (a, Value::String(b)) => Ok(Value::String(format!("{}{}", a, b))),
            // List concatenation
            (Value::List(a), Value::List(b)) => {
                let mut merged = a.clone();
                merged.extend(b.clone());
                Ok(Value::List(merged))
            }
            _ => Err(MarretaError::TypeError {
                message: format!("cannot add {} and {}", left.type_name(), right.type_name()),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn op_arith(
        &self,
        left: &Value,
        right: &Value,
        int_op: fn(i64, i64) -> i64,
        float_op: fn(f64, f64) -> f64,
        op_name: &str,
    ) -> Result<Value, MarretaError> {
        match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(int_op(*a, *b))),
            (Value::Decimal(a), Value::Decimal(b)) => {
                let result = match op_name {
                    "-" => *a - *b,
                    "*" => *a * *b,
                    _ => unreachable!("unsupported decimal arithmetic op"),
                };
                Ok(Value::Decimal(result))
            }
            (Value::Decimal(a), Value::Integer(b)) => {
                let b = Decimal::from(*b);
                let result = match op_name {
                    "-" => *a - b,
                    "*" => *a * b,
                    _ => unreachable!("unsupported decimal arithmetic op"),
                };
                Ok(Value::Decimal(result))
            }
            (Value::Integer(a), Value::Decimal(b)) => {
                let a = Decimal::from(*a);
                let result = match op_name {
                    "-" => a - *b,
                    "*" => a * *b,
                    _ => unreachable!("unsupported decimal arithmetic op"),
                };
                Ok(Value::Decimal(result))
            }
            (Value::Decimal(_), Value::Float(_)) | (Value::Float(_), Value::Decimal(_)) => {
                Err(MarretaError::TypeError {
                    message: "cannot mix Decimal and Float".into(),
                    line: self.current_line,
                    column: self.current_column,
                })
            }
            (Value::Integer(a), Value::Float(b)) => Ok(Value::Float(float_op(*a as f64, *b))),
            (Value::Float(a), Value::Integer(b)) => Ok(Value::Float(float_op(*a, *b as f64))),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_op(*a, *b))),
            _ => Err(MarretaError::TypeError {
                message: format!(
                    "cannot apply '{}' to {} and {}",
                    op_name,
                    left.type_name(),
                    right.type_name()
                ),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn op_divide(&self, left: &Value, right: &Value) -> Result<Value, MarretaError> {
        match (left, right) {
            (Value::Integer(_), Value::Integer(0)) | (Value::Float(_), Value::Integer(0)) => {
                Err(MarretaError::DivisionByZero {
                    line: self.current_line,
                    column: self.current_column,
                })
            }
            (Value::Decimal(_), Value::Integer(0)) => Err(MarretaError::DivisionByZero {
                line: self.current_line,
                column: self.current_column,
            }),
            (Value::Integer(_), Value::Decimal(b)) | (Value::Decimal(_), Value::Decimal(b))
                if b.is_zero() =>
            {
                Err(MarretaError::DivisionByZero {
                    line: self.current_line,
                    column: self.current_column,
                })
            }
            (Value::Integer(_) | Value::Float(_), Value::Float(b)) if *b == 0.0 => {
                Err(MarretaError::DivisionByZero {
                    line: self.current_line,
                    column: self.current_column,
                })
            }
            (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a / b)),
            (Value::Decimal(a), Value::Decimal(b)) => Ok(Value::Decimal(*a / *b)),
            (Value::Decimal(a), Value::Integer(b)) => Ok(Value::Decimal(*a / Decimal::from(*b))),
            (Value::Integer(a), Value::Decimal(b)) => Ok(Value::Decimal(Decimal::from(*a) / *b)),
            (Value::Decimal(_), Value::Float(_)) | (Value::Float(_), Value::Decimal(_)) => {
                Err(MarretaError::TypeError {
                    message: "cannot mix Decimal and Float".into(),
                    line: self.current_line,
                    column: self.current_column,
                })
            }
            (Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 / b)),
            (Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a / *b as f64)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
            _ => Err(MarretaError::TypeError {
                message: format!(
                    "cannot divide {} by {}",
                    left.type_name(),
                    right.type_name()
                ),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn op_modulo(&self, left: &Value, right: &Value) -> Result<Value, MarretaError> {
        match (left, right) {
            (Value::Integer(_), Value::Integer(0)) => Err(MarretaError::DivisionByZero {
                line: self.current_line,
                column: self.current_column,
            }),
            (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a % b)),
            (Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 % b)),
            (Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a % *b as f64)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a % b)),
            _ => Err(MarretaError::TypeError {
                message: format!(
                    "cannot apply '%' to {} and {}",
                    left.type_name(),
                    right.type_name()
                ),
                line: self.current_line,
                column: self.current_column,
            }),
        }
    }

    fn op_compare<F>(&self, left: &Value, right: &Value, pred: F) -> Result<Value, MarretaError>
    where
        F: Fn(std::cmp::Ordering) -> bool,
    {
        let ord = match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => a.cmp(b),
            (Value::Decimal(a), Value::Decimal(b)) => a.cmp(b),
            (Value::Decimal(a), Value::Integer(b)) => a.cmp(&Decimal::from(*b)),
            (Value::Integer(a), Value::Decimal(b)) => Decimal::from(*a).cmp(b),
            (Value::Decimal(_), Value::Float(_)) | (Value::Float(_), Value::Decimal(_)) => {
                return Err(MarretaError::TypeError {
                    message: "cannot compare Decimal and Float".into(),
                    line: self.current_line,
                    column: self.current_column,
                });
            }
            (Value::Integer(a), Value::Float(b)) => (*a as f64)
                .partial_cmp(b)
                .unwrap_or(std::cmp::Ordering::Equal),
            (Value::Float(a), Value::Integer(b)) => a
                .partial_cmp(&(*b as f64))
                .unwrap_or(std::cmp::Ordering::Equal),
            (Value::Float(a), Value::Float(b)) => {
                a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
            }
            (Value::String(a), Value::String(b)) => a.cmp(b),
            _ => {
                return Err(MarretaError::TypeError {
                    message: format!(
                        "cannot compare {} and {}",
                        left.type_name(),
                        right.type_name()
                    ),
                    line: self.current_line,
                    column: self.current_column,
                });
            }
        };
        Ok(Value::Boolean(pred(ord)))
    }

    // =========================================================================
    // Unary operations
    // =========================================================================

    pub(super) fn apply_unary_op(
        &self,
        op: &UnaryOperator,
        val: &Value,
    ) -> Result<Value, MarretaError> {
        match op {
            UnaryOperator::Negate => match val {
                Value::Integer(n) => Ok(Value::Integer(-n)),
                Value::Float(n) => Ok(Value::Float(-n)),
                Value::Decimal(n) => Ok(Value::Decimal(-*n)),
                _ => Err(MarretaError::TypeError {
                    message: format!("cannot negate {}", val.type_name()),
                    line: self.current_line,
                    column: self.current_column,
                }),
            },
            UnaryOperator::Not => Ok(Value::Boolean(!val.is_truthy())),
        }
    }
}
