use std::collections::HashMap;
use std::sync::Arc;

use crate::value::Value;

/// Manages variable scopes during execution.
///
/// Uses a stack of HashMaps to implement lexical scoping.
/// New scopes are pushed when entering tasks, map blocks, etc.
/// Variable lookup walks from the innermost scope outward.
#[derive(Debug, Clone)]
pub struct Environment {
    /// Shared, read-only definitions (the global/module scope after project
    /// load). Cloning an `Environment` only bumps this `Arc` instead of
    /// deep-copying every global definition. It is read concurrently by request
    /// threads, so it must never be mutated once frozen.
    base: Arc<HashMap<String, Value>>,
    /// Request/block-local scopes (innermost last). All writes target these; the
    /// base is never written.
    scopes: Vec<HashMap<String, Value>>,
}

impl Environment {
    /// Creates a new environment with an empty global scope.
    pub fn new() -> Self {
        Self {
            base: Arc::new(HashMap::new()),
            scopes: vec![HashMap::new()],
        }
    }

    /// Pushes a new child scope onto the stack.
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Pops the current scope. Panics if only the global scope remains.
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// Sets a variable in the current (innermost) scope.
    pub fn set(&mut self, name: String, value: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, value);
        }
    }

    /// Looks up a variable by walking from the innermost scope to the global scope.
    /// Returns `None` if the variable is not defined in any scope.
    pub fn get(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Some(value.clone());
            }
        }
        self.base.get(name).cloned()
    }

    /// Returns true if the variable exists in any scope.
    pub fn has(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .rev()
            .any(|scope| scope.contains_key(name))
            || self.base.contains_key(name)
    }

    /// Updates an existing variable in the nearest scope where it is defined.
    /// If not found in any scope, sets it in the current scope.
    pub fn update(&mut self, name: String, value: Value) {
        for scope in self.scopes.iter_mut().rev() {
            if let std::collections::hash_map::Entry::Occupied(mut e) = scope.entry(name.clone()) {
                e.insert(value);
                return;
            }
        }
        self.set(name, value);
    }

    /// Returns the current scope depth (1 = global only).
    pub fn depth(&self) -> usize {
        self.scopes.len()
    }

    /// Returns all variable names visible from the current scope (for REPL `.vars`).
    pub fn all_variables(&self) -> Vec<(String, Value)> {
        let mut seen = HashMap::new();
        // Walk from innermost to outermost, then the shared base; first
        // occurrence wins (locals shadow the base).
        for scope in self.scopes.iter().rev() {
            for (name, value) in scope {
                seen.entry(name.clone()).or_insert_with(|| value.clone());
            }
        }
        for (name, value) in self.base.iter() {
            seen.entry(name.clone()).or_insert_with(|| value.clone());
        }
        let mut vars: Vec<(String, Value)> = seen.into_iter().collect();
        vars.sort_by(|a, b| a.0.cmp(&b.0));
        vars
    }

    /// Returns only the **local** scope variables (excluding the shared base).
    /// Used to propagate a caller's local definitions (e.g. route-local tasks)
    /// into a called task without re-cloning every global definition, which the
    /// base already provides. Inner scopes shadow outer ones.
    pub fn local_variables(&self) -> Vec<(String, Value)> {
        let mut seen = HashMap::new();
        for scope in self.scopes.iter().rev() {
            for (name, value) in scope {
                seen.entry(name.clone()).or_insert_with(|| value.clone());
            }
        }
        seen.into_iter().collect()
    }

    /// Returns all task names visible from the current scope (for REPL `.tasks`).
    pub fn all_tasks(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .all_variables()
            .into_iter()
            .filter(|(_, v)| matches!(v, Value::Task { .. }))
            .map(|(name, _)| name)
            .collect();
        names.sort();
        names
    }

    /// Moves all current definitions into the shared, read-only base and leaves a
    /// single empty local scope. After this, cloning the environment (per request,
    /// per task call, per broadcast branch) only bumps the base `Arc` instead of
    /// deep-copying every definition. Call once after project load and after all
    /// startup-time injection, before serving requests. The base must not be
    /// mutated afterwards (writes go to local scopes and shadow the base).
    pub fn freeze(&mut self) {
        let mut base = (*self.base).clone();
        for scope in self.scopes.drain(..) {
            // outer-to-inner order: inner scopes shadow outer ones
            base.extend(scope);
        }
        self.base = Arc::new(base);
        self.scopes = vec![HashMap::new()];
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Expression, ParamDef, TaskBody};

    #[test]
    fn test_set_and_get() {
        let mut env = Environment::new();
        env.set("x".into(), Value::Integer(42));
        assert_eq!(env.get("x"), Some(Value::Integer(42)));
    }

    #[test]
    fn test_get_undefined_returns_none() {
        let env = Environment::new();
        assert_eq!(env.get("missing"), None);
    }

    #[test]
    fn test_has() {
        let mut env = Environment::new();
        assert!(!env.has("x"));
        env.set("x".into(), Value::Null);
        assert!(env.has("x"));
    }

    #[test]
    fn test_reassignment() {
        let mut env = Environment::new();
        env.set("x".into(), Value::Integer(1));
        env.set("x".into(), Value::Integer(2));
        assert_eq!(env.get("x"), Some(Value::Integer(2)));
    }

    #[test]
    fn test_child_scope_sees_parent() {
        let mut env = Environment::new();
        env.set("x".into(), Value::Integer(10));
        env.push_scope();
        assert_eq!(env.get("x"), Some(Value::Integer(10)));
    }

    #[test]
    fn test_child_scope_shadowing() {
        let mut env = Environment::new();
        env.set("x".into(), Value::Integer(10));
        env.push_scope();
        env.set("x".into(), Value::Integer(20));
        assert_eq!(env.get("x"), Some(Value::Integer(20)));
        env.pop_scope();
        assert_eq!(env.get("x"), Some(Value::Integer(10)));
    }

    #[test]
    fn test_child_scope_local_variable() {
        let mut env = Environment::new();
        env.push_scope();
        env.set("local".into(), Value::String("hi".into()));
        assert_eq!(env.get("local"), Some(Value::String("hi".into())));
        env.pop_scope();
        assert_eq!(env.get("local"), None);
    }

    #[test]
    fn test_nested_scopes() {
        let mut env = Environment::new();
        env.set("a".into(), Value::Integer(1));
        env.push_scope();
        env.set("b".into(), Value::Integer(2));
        env.push_scope();
        env.set("c".into(), Value::Integer(3));

        assert_eq!(env.get("a"), Some(Value::Integer(1)));
        assert_eq!(env.get("b"), Some(Value::Integer(2)));
        assert_eq!(env.get("c"), Some(Value::Integer(3)));
        assert_eq!(env.depth(), 3);

        env.pop_scope();
        assert_eq!(env.get("c"), None);
        assert_eq!(env.depth(), 2);

        env.pop_scope();
        assert_eq!(env.get("b"), None);
        assert_eq!(env.depth(), 1);
    }

    #[test]
    fn test_pop_scope_never_removes_global() {
        let mut env = Environment::new();
        env.set("x".into(), Value::Integer(1));
        env.pop_scope(); // should be a no-op
        env.pop_scope(); // still no-op
        assert_eq!(env.depth(), 1);
        assert_eq!(env.get("x"), Some(Value::Integer(1)));
    }

    #[test]
    fn test_update_existing_in_parent() {
        let mut env = Environment::new();
        env.set("x".into(), Value::Integer(1));
        env.push_scope();
        env.update("x".into(), Value::Integer(99));
        // Parent was updated, not shadowed
        assert_eq!(env.get("x"), Some(Value::Integer(99)));
        env.pop_scope();
        assert_eq!(env.get("x"), Some(Value::Integer(99)));
    }

    #[test]
    fn test_update_nonexistent_sets_in_current() {
        let mut env = Environment::new();
        env.push_scope();
        env.update("y".into(), Value::Integer(5));
        assert_eq!(env.get("y"), Some(Value::Integer(5)));
        env.pop_scope();
        assert_eq!(env.get("y"), None);
    }

    #[test]
    fn test_all_variables() {
        let mut env = Environment::new();
        env.set("b".into(), Value::Integer(2));
        env.set("a".into(), Value::Integer(1));
        env.push_scope();
        env.set("a".into(), Value::Integer(10)); // shadow

        let vars = env.all_variables();
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0], ("a".into(), Value::Integer(10))); // shadowed value
        assert_eq!(vars[1], ("b".into(), Value::Integer(2)));
    }

    #[test]
    fn test_all_tasks() {
        let mut env = Environment::new();
        env.set("x".into(), Value::Integer(1));
        env.set(
            "double".into(),
            Value::Task {
                name: "double".into(),
                params: vec![ParamDef {
                    name: "n".into(),
                    schema: None,
                }],
                body: TaskBody::Inline(Expression::Null),
                owner_module: None,
                source_module: None,
                line: 0,
                column: 0,
            },
        );
        env.set(
            "triple".into(),
            Value::Task {
                name: "triple".into(),
                params: vec![ParamDef {
                    name: "n".into(),
                    schema: None,
                }],
                body: TaskBody::Inline(Expression::Null),
                owner_module: None,
                source_module: None,
                line: 0,
                column: 0,
            },
        );

        let tasks = env.all_tasks();
        assert_eq!(tasks, vec!["double", "triple"]);
    }

    #[test]
    fn test_default_trait() {
        let env = Environment::default();
        assert_eq!(env.depth(), 1);
    }
}
