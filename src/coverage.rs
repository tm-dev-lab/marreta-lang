//! Shared test-coverage consolidation.
//!
//! Both `marreta test --coverage` (run-based: routes covered by scenarios that
//! passed) and `marreta doctor` (static: routes that have a declared scenario)
//! compute the same headline numbers. Centralizing the counting here keeps the
//! two views consistent and avoids duplicating the route-key formatting.

use std::collections::BTreeSet;

use crate::ast::HttpVerb;

/// Canonical "VERB /path" key used to compare scenarios against declared routes.
pub fn route_key(verb: &HttpVerb, path: &str) -> String {
    format!("{verb} {path}")
}

/// Consolidated coverage counts, rendered by both the test runner and doctor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageSummary {
    pub routes_total: usize,
    pub routes_with_scenario: usize,
    pub scenarios_total: usize,
    pub files_total: usize,
    pub unmatched_scenarios: usize,
}

impl CoverageSummary {
    /// Declared routes that no scenario in the covered set resolves to.
    pub fn routes_without_scenario(&self) -> usize {
        self.routes_total.saturating_sub(self.routes_with_scenario)
    }

    /// Share of declared routes that have a scenario, as a percentage. This is a
    /// presence figure ("routes with a scenario"), not a pass/fail coverage
    /// figure. A project with no routes is reported as 100.0.
    pub fn routes_with_scenario_pct(&self) -> f64 {
        if self.routes_total == 0 {
            100.0
        } else {
            (self.routes_with_scenario as f64 / self.routes_total as f64) * 100.0
        }
    }
}

/// Build a summary from the set of all declared route keys, the set of covered
/// route keys, and the scenario/file/unmatched totals the caller already knows.
///
/// `covered` is intersected with `all_routes` so a stale key can never inflate
/// the presence count.
pub fn summarize(
    all_routes: &BTreeSet<String>,
    covered: &BTreeSet<String>,
    scenarios_total: usize,
    files_total: usize,
    unmatched_scenarios: usize,
) -> CoverageSummary {
    let routes_with_scenario = covered
        .iter()
        .filter(|key| all_routes.contains(*key))
        .count();
    CoverageSummary {
        routes_total: all_routes.len(),
        routes_with_scenario,
        scenarios_total,
        files_total,
        unmatched_scenarios,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn route_key_formats_verb_and_path() {
        assert_eq!(
            route_key(&HttpVerb::Get, "/accounts/:id"),
            "GET /accounts/:id"
        );
        assert_eq!(route_key(&HttpVerb::Post, "/orders"), "POST /orders");
    }

    #[test]
    fn summarize_counts_presence_and_complement() {
        let all = set(&["GET /a", "POST /b", "GET /c"]);
        let covered = set(&["GET /a", "POST /b"]);
        let summary = summarize(&all, &covered, 4, 2, 1);
        assert_eq!(summary.routes_total, 3);
        assert_eq!(summary.routes_with_scenario, 2);
        assert_eq!(summary.routes_without_scenario(), 1);
        assert_eq!(summary.scenarios_total, 4);
        assert_eq!(summary.files_total, 2);
        assert_eq!(summary.unmatched_scenarios, 1);
    }

    #[test]
    fn summarize_ignores_covered_keys_not_in_routes() {
        let all = set(&["GET /a"]);
        let covered = set(&["GET /a", "GET /ghost"]);
        let summary = summarize(&all, &covered, 2, 1, 0);
        assert_eq!(summary.routes_total, 1);
        assert_eq!(summary.routes_with_scenario, 1);
        assert_eq!(summary.routes_without_scenario(), 0);
    }

    #[test]
    fn summarize_handles_no_routes() {
        let summary = summarize(&set(&[]), &set(&[]), 0, 0, 0);
        assert_eq!(summary.routes_total, 0);
        assert_eq!(summary.routes_with_scenario, 0);
        assert_eq!(summary.routes_without_scenario(), 0);
    }

    #[test]
    fn routes_with_scenario_pct_is_presence_share() {
        let some = summarize(
            &set(&["GET /a", "POST /b", "GET /c", "GET /d"]),
            &set(&["GET /a"]),
            1,
            1,
            0,
        );
        assert_eq!(some.routes_with_scenario_pct(), 25.0);
        let none = summarize(&set(&[]), &set(&[]), 0, 0, 0);
        assert_eq!(none.routes_with_scenario_pct(), 100.0);
    }
}
