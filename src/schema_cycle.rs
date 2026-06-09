//! Relation-aware schema-reference cycle detection (Spec 062), shared by the project
//! loader and `marreta lint` so the rule lives in one place.
//!
//! The payload validator (`validator.rs`) recurses into a `SchemaType::Reference` only
//! when the target is a **value** schema; a reference to a **persistent** (`db:`) schema
//! is a foreign-key relation (Spec 025) and is let through without recursing. So the
//! validator can only loop on a cycle whose edges all target value schemas — i.e. a
//! cycle lying **entirely within value schemas**. A cycle that passes through any
//! persistent schema is broken at that relation edge and is safe (and lets a persistent
//! schema work as an API contract even in a relation cycle).
//!
//! Therefore a cycle is **disallowed** iff every node in it is a value (non-`db:`)
//! schema. The search starts from value schemas and traverses only into value schemas
//! (edges into persistent schemas are cut, mirroring the validator), returning the first
//! such cycle as a path `A -> B -> A`, or `None`.

use std::collections::{HashMap, HashSet};

/// Returns the first disallowed (all-value) schema-reference cycle, or `None`. `refs`
/// maps a schema name to the schema names it references; `persistent` is the set of
/// `db:` schema names.
pub fn find_disallowed_cycle(
    refs: &HashMap<String, Vec<String>>,
    persistent: &HashSet<String>,
) -> Option<Vec<String>> {
    // Deterministic iteration so the reported cycle is stable across runs.
    let mut value_starts: Vec<&String> = refs
        .keys()
        .filter(|name| !persistent.contains(*name))
        .collect();
    value_starts.sort();

    for start in value_starts {
        let mut path = Vec::new();
        let mut on_path = HashSet::new();
        let mut visited = HashSet::new();
        if let Some(cycle) = walk(
            start,
            refs,
            persistent,
            &mut path,
            &mut on_path,
            &mut visited,
        ) {
            return Some(cycle);
        }
    }
    None
}

/// DFS through **value** schemas only (edges into persistent schemas are cut, mirroring
/// the validator's let-pass for relations). A reference to a node already on the stack is
/// a back-edge → an all-value cycle.
fn walk(
    node: &str,
    refs: &HashMap<String, Vec<String>>,
    persistent: &HashSet<String>,
    path: &mut Vec<String>,
    on_path: &mut HashSet<String>,
    visited: &mut HashSet<String>,
) -> Option<Vec<String>> {
    path.push(node.to_string());
    on_path.insert(node.to_string());

    let mut neighbors: Vec<&String> = refs
        .get(node)
        .map(|v| v.iter().collect())
        .unwrap_or_default();
    neighbors.sort();

    for next in neighbors {
        // Edge into a persistent schema is a relation the validator lets pass — cut it.
        if persistent.contains(next) {
            continue;
        }
        if on_path.contains(next) {
            let pos = path.iter().position(|n| n == next).unwrap_or(0);
            let mut cycle = path[pos..].to_vec();
            cycle.push(next.clone());
            return Some(cycle);
        }
        if refs.contains_key(next) && !visited.contains(next) {
            if let Some(cycle) = walk(next, refs, persistent, path, on_path, visited) {
                return Some(cycle);
            }
        }
    }

    on_path.remove(node);
    visited.insert(node.to_string());
    path.pop();
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(edges: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
        edges
            .iter()
            .map(|(name, refs)| {
                (
                    name.to_string(),
                    refs.iter().map(|s| s.to_string()).collect(),
                )
            })
            .collect()
    }

    fn persistent(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn value_cycle_is_disallowed() {
        let refs = graph(&[("A", &["B"]), ("B", &["A"])]);
        let cycle = find_disallowed_cycle(&refs, &persistent(&[])).unwrap();
        assert!(cycle.contains(&"A".to_string()) && cycle.contains(&"B".to_string()));
    }

    #[test]
    fn value_self_reference_is_disallowed() {
        let refs = graph(&[("Node", &["Node"])]);
        assert!(find_disallowed_cycle(&refs, &persistent(&[])).is_some());
    }

    #[test]
    fn all_persistent_cycle_is_allowed() {
        let refs = graph(&[("DbUser", &["DbOrder"]), ("DbOrder", &["DbUser"])]);
        assert!(find_disallowed_cycle(&refs, &persistent(&["DbUser", "DbOrder"])).is_none());
    }

    #[test]
    fn allowed_bidirectional_relation_user_order() {
        // `User.orders: list of Order` (User -> Order) and `Order.user: User` (Order ->
        // User), both persistent — Spec 025's canonical bidirectional relation, allowed.
        let refs = graph(&[("User", &["Order"]), ("Order", &["User"])]);
        assert!(find_disallowed_cycle(&refs, &persistent(&["User", "Order"])).is_none());
    }

    #[test]
    fn cycle_through_a_persistent_schema_is_allowed() {
        // Profile (value) <-> DbUser (persistent). The edge Profile -> DbUser is a
        // relation the validator lets pass, so the cycle never loops — allowed. (This
        // supersedes the earlier "reject any value-touching cycle" rule, now that the
        // validator is relation-aware.)
        let refs = graph(&[("Profile", &["DbUser"]), ("DbUser", &["Profile"])]);
        assert!(find_disallowed_cycle(&refs, &persistent(&["DbUser"])).is_none());
    }

    #[test]
    fn value_reaching_a_persistent_cycle_is_allowed() {
        // Profile (value) -> DbUser <-> DbOrder (relational cycle). Validating Profile
        // lets the DbUser relation pass, so it never enters the cycle — allowed.
        let refs = graph(&[
            ("Profile", &["DbUser"]),
            ("DbUser", &["DbOrder"]),
            ("DbOrder", &["DbUser"]),
        ]);
        assert!(find_disallowed_cycle(&refs, &persistent(&["DbUser", "DbOrder"])).is_none());
    }

    #[test]
    fn all_value_cycle_through_a_persistent_branch_is_still_disallowed() {
        // A genuine all-value cycle (A <-> B) coexisting with a persistent reference is
        // still rejected — the value cycle itself loops.
        let refs = graph(&[("A", &["B", "DbX"]), ("B", &["A"]), ("DbX", &[])]);
        let cycle = find_disallowed_cycle(&refs, &persistent(&["DbX"])).unwrap();
        assert!(cycle.contains(&"A".to_string()) && cycle.contains(&"B".to_string()));
    }

    #[test]
    fn persistent_self_reference_is_allowed() {
        let refs = graph(&[("DbCategory", &["DbCategory"])]);
        assert!(find_disallowed_cycle(&refs, &persistent(&["DbCategory"])).is_none());
    }

    #[test]
    fn acyclic_graph_has_no_cycle() {
        let refs = graph(&[("A", &["B"]), ("B", &["C"]), ("C", &[])]);
        assert!(find_disallowed_cycle(&refs, &persistent(&[])).is_none());
    }
}
