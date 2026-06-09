use std::collections::BTreeMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use serde_json::json;

static ENABLED: AtomicBool = AtomicBool::new(false);
static REGISTRY: OnceLock<Arc<RuntimeProfileRegistry>> = OnceLock::new();

const BUCKETS_US: [u64; 18] = [
    10,
    25,
    50,
    75,
    100,
    150,
    200,
    300,
    500,
    750,
    1_000,
    1_500,
    2_000,
    3_000,
    5_000,
    10_000,
    25_000,
    u64::MAX,
];

#[derive(Debug, Clone, Copy)]
pub enum ProfilePhase {
    HttpTotal,
    HandlerTotal,
    RouteClone,
    AuthEval,
    EnvSetup,
    RequestBinding,
    SchemaCoercion,
    AstExecute,
    Interpolation,
    JsonSerialize,
    ResponseBuild,
    TotalExecuteRoute,
}

impl ProfilePhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::HttpTotal => "http_total",
            Self::HandlerTotal => "handler_total",
            Self::RouteClone => "route_clone",
            Self::AuthEval => "auth_eval",
            Self::EnvSetup => "env_setup",
            Self::RequestBinding => "request_binding",
            Self::SchemaCoercion => "schema_coercion",
            Self::AstExecute => "ast_execute",
            Self::Interpolation => "interpolation",
            Self::JsonSerialize => "json_serialize",
            Self::ResponseBuild => "response_build",
            Self::TotalExecuteRoute => "total_execute_route",
        }
    }
}

pub struct PhaseTimer {
    route: Arc<RouteProfile>,
    phase: ProfilePhase,
    started_at: Instant,
}

impl Drop for PhaseTimer {
    fn drop(&mut self) {
        self.route
            .record_phase(self.phase, self.started_at.elapsed());
    }
}

#[derive(Default)]
pub struct RuntimeProfileRegistry {
    routes: RwLock<BTreeMap<String, Arc<RouteProfile>>>,
}

impl RuntimeProfileRegistry {
    pub fn route(&self, key: &str) -> Arc<RouteProfile> {
        if let Some(route) = self.routes.read().unwrap().get(key).cloned() {
            return route;
        }

        let mut routes = self.routes.write().unwrap();
        routes
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(RouteProfile::new(key.to_string())))
            .clone()
    }

    pub fn emit_json(&self) {
        let routes = self.routes.read().unwrap();
        let route_summaries: Vec<_> = routes.values().map(|route| route.summary_json()).collect();
        let payload = json!({
            "kind": "marreta.runtime_profile",
            "mode": "hot_path",
            "routes": route_summaries,
        });

        if let Ok(line) = serde_json::to_string(&payload) {
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(stderr, "{line}");
        }
    }
}

pub struct RouteProfile {
    route: String,
    requests: AtomicU64,
    http_total: PhaseStats,
    handler_total: PhaseStats,
    route_clone: PhaseStats,
    auth_eval: PhaseStats,
    env_setup: PhaseStats,
    request_binding: PhaseStats,
    schema_coercion: PhaseStats,
    ast_execute: PhaseStats,
    interpolation: PhaseStats,
    json_serialize: PhaseStats,
    response_build: PhaseStats,
    total_execute_route: PhaseStats,
}

impl RouteProfile {
    fn new(route: String) -> Self {
        Self {
            route,
            requests: AtomicU64::new(0),
            http_total: PhaseStats::new(),
            handler_total: PhaseStats::new(),
            route_clone: PhaseStats::new(),
            auth_eval: PhaseStats::new(),
            env_setup: PhaseStats::new(),
            request_binding: PhaseStats::new(),
            schema_coercion: PhaseStats::new(),
            ast_execute: PhaseStats::new(),
            interpolation: PhaseStats::new(),
            json_serialize: PhaseStats::new(),
            response_build: PhaseStats::new(),
            total_execute_route: PhaseStats::new(),
        }
    }

    pub fn request_started(&self) {
        self.requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn timer(self: &Arc<Self>, phase: ProfilePhase) -> PhaseTimer {
        PhaseTimer {
            route: Arc::clone(self),
            phase,
            started_at: Instant::now(),
        }
    }

    fn record_phase(&self, phase: ProfilePhase, duration: Duration) {
        self.phase(phase).record(duration);
    }

    fn phase(&self, phase: ProfilePhase) -> &PhaseStats {
        match phase {
            ProfilePhase::HttpTotal => &self.http_total,
            ProfilePhase::HandlerTotal => &self.handler_total,
            ProfilePhase::RouteClone => &self.route_clone,
            ProfilePhase::AuthEval => &self.auth_eval,
            ProfilePhase::EnvSetup => &self.env_setup,
            ProfilePhase::RequestBinding => &self.request_binding,
            ProfilePhase::SchemaCoercion => &self.schema_coercion,
            ProfilePhase::AstExecute => &self.ast_execute,
            ProfilePhase::Interpolation => &self.interpolation,
            ProfilePhase::JsonSerialize => &self.json_serialize,
            ProfilePhase::ResponseBuild => &self.response_build,
            ProfilePhase::TotalExecuteRoute => &self.total_execute_route,
        }
    }

    fn summary_json(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        obj.insert("route".into(), json!(self.route));
        obj.insert(
            "requests".into(),
            json!(self.requests.load(Ordering::Relaxed)),
        );

        for phase in [
            ProfilePhase::HttpTotal,
            ProfilePhase::HandlerTotal,
            ProfilePhase::RouteClone,
            ProfilePhase::AuthEval,
            ProfilePhase::EnvSetup,
            ProfilePhase::RequestBinding,
            ProfilePhase::SchemaCoercion,
            ProfilePhase::AstExecute,
            ProfilePhase::Interpolation,
            ProfilePhase::JsonSerialize,
            ProfilePhase::ResponseBuild,
            ProfilePhase::TotalExecuteRoute,
        ] {
            obj.insert(phase.as_str().into(), self.phase(phase).summary_json());
        }

        serde_json::Value::Object(obj)
    }
}

pub struct PhaseStats {
    count: AtomicU64,
    total_us: AtomicU64,
    max_us: AtomicU64,
    buckets: Vec<AtomicU64>,
}

impl PhaseStats {
    fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            total_us: AtomicU64::new(0),
            max_us: AtomicU64::new(0),
            buckets: BUCKETS_US.iter().map(|_| AtomicU64::new(0)).collect(),
        }
    }

    fn record(&self, duration: Duration) {
        let micros = duration.as_micros().min(u64::MAX as u128) as u64;
        self.count.fetch_add(1, Ordering::Relaxed);
        self.total_us.fetch_add(micros, Ordering::Relaxed);
        self.update_max(micros);

        let bucket = BUCKETS_US
            .iter()
            .position(|upper| micros <= *upper)
            .unwrap_or(BUCKETS_US.len() - 1);
        self.buckets[bucket].fetch_add(1, Ordering::Relaxed);
    }

    fn update_max(&self, micros: u64) {
        let mut current = self.max_us.load(Ordering::Relaxed);
        while micros > current {
            match self.max_us.compare_exchange_weak(
                current,
                micros,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(previous) => current = previous,
            }
        }
    }

    fn summary_json(&self) -> serde_json::Value {
        let count = self.count.load(Ordering::Relaxed);
        let total = self.total_us.load(Ordering::Relaxed);
        let avg = if count == 0 {
            0.0
        } else {
            total as f64 / count as f64
        };

        json!({
            "count": count,
            "avg_us": avg,
            "max_us": self.max_us.load(Ordering::Relaxed),
            "p50_us": self.percentile(count, 50),
            "p90_us": self.percentile(count, 90),
            "p95_us": self.percentile(count, 95),
            "p99_us": self.percentile(count, 99),
        })
    }

    fn percentile(&self, count: u64, percentile: u64) -> u64 {
        if count == 0 {
            return 0;
        }

        let target = ((count * percentile).saturating_add(99) / 100).max(1);
        let mut seen = 0;
        for (bucket, upper) in self.buckets.iter().zip(BUCKETS_US.iter()) {
            seen += bucket.load(Ordering::Relaxed);
            if seen >= target {
                return *upper;
            }
        }
        *BUCKETS_US.last().unwrap_or(&0)
    }
}

pub fn init_from_env() -> Option<Arc<RuntimeProfileRegistry>> {
    let enabled = std::env::var("MARRETA_RUNTIME_PROFILE")
        .map(|value| value.trim().eq_ignore_ascii_case("hot_path"))
        .unwrap_or(false);
    ENABLED.store(enabled, Ordering::Relaxed);

    if !enabled {
        return None;
    }

    Some(
        REGISTRY
            .get_or_init(|| Arc::new(RuntimeProfileRegistry::default()))
            .clone(),
    )
}

pub fn timer(route: Option<&Arc<RouteProfile>>, phase: ProfilePhase) -> Option<PhaseTimer> {
    if !ENABLED.load(Ordering::Relaxed) {
        return None;
    }

    route.map(|route| route.timer(phase))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_stats_reports_percentile_buckets() {
        let stats = PhaseStats::new();
        stats.record(Duration::from_micros(7));
        stats.record(Duration::from_micros(72));
        stats.record(Duration::from_micros(640));

        let summary = stats.summary_json();
        assert_eq!(summary["count"], 3);
        assert_eq!(summary["max_us"], 640);
        assert_eq!(summary["p50_us"], 75);
        assert_eq!(summary["p95_us"], 750);
    }

    #[test]
    fn registry_reuses_route_profile_by_key() {
        let registry = RuntimeProfileRegistry::default();
        let first = registry.route("GET /items");
        let second = registry.route("GET /items");

        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn disabled_timer_does_not_create_timer() {
        ENABLED.store(false, Ordering::Relaxed);
        let route = Arc::new(RouteProfile::new("GET /health".into()));

        assert!(timer(Some(&route), ProfilePhase::AstExecute).is_none());
    }
}
