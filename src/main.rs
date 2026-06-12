use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::time::Instant;

use marreta::cache::CacheEngine;
use marreta::config::MarretaConfig;
use marreta::db::DbEngine;
use marreta::doctor::build_doctor_report;
use marreta::error::MarretaError;
use marreta::file_loader;
use marreta::formatter::{
    FormatError, discover_explicit_files, discover_project_files, format_file, format_source,
};
use marreta::http_client::HttpClientEngine;
use marreta::init::{InitOptions, init_project_with_options, parse_services, render_next_steps};
use marreta::lexer::Lexer;
use marreta::lint::{
    LintError, LintFormat, lint_paths, lint_project, lint_project_stdin, lint_stdin,
};
use marreta::migrations::{
    MigrationListEntry, MigrationStatusReport, build_migration_inventory, build_persistent_tables,
    build_schema_from_local_migrations, compare_migration_state, default_migration_name,
    discard_pending_migration, discover_local_migrations, plan_migration, render_postgres_down_sql,
    render_postgres_up_sql, write_migration_files,
};
use marreta::parser::Parser;
use marreta::queue::QueueEngine;
use marreta::scenario_tests::{
    ScenarioDefinition, ScenarioFile, discover_scenario_files, load_scenario_files, run_scenarios,
};
use marreta::server::{ServerConfig, serve};
use marreta::tooling::{catalog, completions, definition, hover, symbols};
use marreta::value::{Value, ValueMap};
use marreta::version::runtime_version_label;

/// Consistent framing (header rule + footer with elapsed time) for one-shot,
/// human-facing commands. CLI presentation only.
mod cli_ux;

/// Extracts a human-readable message from a panic payload without exposing Rust
/// internals. Exposed as a pure helper so Phase G can assert the format.
fn format_panic_message(payload: &dyn std::any::Any) -> String {
    let msg = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(|s| s.as_str()))
        .unwrap_or("unexpected internal error");
    format!(
        "[marreta] Internal error: {}\n  → The engine encountered an unrecoverable condition.\n  → Please report this at github.com/marreta-lang/marreta/issues",
        msg
    )
}

/// True when `fmt` runs in its machine mode (`--stdin`, editor format-on-save),
/// which emits formatted source to stdout and must stay frame-free.
fn fmt_is_machine_mode(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--stdin")
}

/// True when `lint` runs in a machine mode (`--stdin`, or `--format json`), which
/// must stay frame-free.
fn lint_is_machine_mode(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--stdin" || arg == "--format=json")
        || args
            .windows(2)
            .any(|w| w[0] == "--format" && w[1] == "json")
}

fn main() {
    // Register a custom panic hook so Rust panic output never leaks to end users.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("{}", format_panic_message(info.payload()));
    }));

    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("tokenize") => {
            let path = match args.get(2) {
                Some(p) => p,
                None => {
                    exit_with_marreta_cli_error(
                        "invalid tokenize usage",
                        "Usage: marreta tokenize <file.marreta>",
                    );
                }
            };
            debug_tokenize(path);
        }
        Some("parse") => {
            let path = match args.get(2) {
                Some(p) => p,
                None => {
                    exit_with_marreta_cli_error(
                        "invalid parse usage",
                        "Usage: marreta parse <file.marreta>",
                    );
                }
            };
            debug_parse(path);
        }
        Some("serve") => {
            let port_override = parse_port_arg(&args);
            let entrypoint = resolve_project_entrypoint(extract_serve_path_arg(&args));
            run_serve(&entrypoint, port_override);
        }
        Some("doctor") => {
            // Frame opened before arg parsing so a parse failure is also framed.
            cli_ux::begin("doctor");
            let (connect, path) = parse_doctor_args(&args);
            let entrypoint = resolve_project_entrypoint(path);
            run_doctor(&entrypoint, connect);
        }
        Some("init") => {
            cli_ux::begin("init");
            let (path, options) = parse_init_args(&args);
            run_init(path, options);
        }
        Some("fmt") => {
            // `--stdin` is machine mode (editor format-on-save) and stays frame-free.
            if !fmt_is_machine_mode(&args) {
                cli_ux::begin("fmt");
            }
            let fmt_args = parse_fmt_args(&args);
            run_fmt(fmt_args);
        }
        Some("lint") => {
            // `--stdin` and `--format json` are machine modes and stay frame-free.
            if !lint_is_machine_mode(&args) {
                cli_ux::begin("lint");
            }
            let lint_args = parse_lint_args(&args);
            run_lint(lint_args);
        }
        Some("tooling") => {
            run_tooling(&args);
        }
        Some("test") => {
            cli_ux::begin("test");
            let test_args = parse_test_args(&args);
            let entrypoint = resolve_project_entrypoint(None);
            run_test(&entrypoint, test_args);
        }
        Some("migrate") => {
            run_migrate(&args);
        }
        Some("--version" | "-v") => {
            println!("{}", runtime_version_label());
        }
        Some("--help" | "-h") | None => {
            print_help();
        }
        Some(cmd) => {
            exit_with_marreta_cli_error(
                "unknown command",
                format!("'{}' is invalid. Run 'marreta --help' for usage.", cmd),
            );
        }
    }
}

fn print_marreta_startup_error(err: &MarretaError) {
    eprintln!(
        "[marreta] {}: {}",
        err.semantic_code(),
        err.display_message()
    );
    let op = err.operation_name();
    if op != "interpreter" {
        eprintln!("[marreta] op: {}", op);
    }
    match (err.line(), err.column()) {
        (Some(line), Some(column)) if line > 0 && column > 0 => {
            eprintln!("[marreta] at {}:{}", line, column);
        }
        (Some(line), _) if line > 0 => {
            eprintln!("[marreta] at line {}", line);
        }
        _ => {}
    }
}

fn exit_with_marreta_startup_error(err: MarretaError) -> ! {
    print_marreta_startup_error(&err);
    cli_ux::abort();
    process::exit(1);
}

fn exit_with_marreta_runtime_error(context: &str, err: impl std::fmt::Display) -> ! {
    eprintln!("[marreta] runtime_error: {}", context);
    eprintln!("[marreta] detail: {}", err);
    cli_ux::abort();
    process::exit(1);
}

fn exit_with_marreta_cli_error(context: &str, detail: impl std::fmt::Display) -> ! {
    eprintln!("[marreta] cli_error: {}", context);
    eprintln!("[marreta] detail: {}", detail);
    cli_ux::abort();
    process::exit(1);
}

// =============================================================================
// File execution
// =============================================================================

/// Number of tokio worker threads for the HTTP server, sized to the container's
/// actual CPU allocation instead of the host core count.
///
/// This is transparent: the developer configures nothing. By default tokio sizes
/// its worker pool to the host's logical CPUs, which ignores a container CPU
/// limit (`--cpus`, set via the CFS quota) and over-subscribes threads onto a
/// throttled CPU. We detect the cgroup CPU quota and size the pool to it, with a
/// floor of 2 so a CPU-bound handler cannot stall all I/O on a single worker. An
/// explicit `MARRETA_WORKER_THREADS` (or `TOKIO_WORKER_THREADS`) env overrides it.
fn server_worker_threads() -> usize {
    for var in ["MARRETA_WORKER_THREADS", "TOKIO_WORKER_THREADS"] {
        if let Ok(value) = std::env::var(var)
            && let Ok(n) = value.trim().parse::<usize>()
            && n >= 1
        {
            return n;
        }
    }

    let host = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let effective = detect_cpu_quota().unwrap_or(host);
    effective.clamp(2, host.max(2))
}

/// Reads the cgroup CPU quota (v2 `cpu.max`, then v1 `cpu.cfs_quota_us`) and
/// returns the effective whole-CPU count, or `None` when unlimited/unreadable.
fn detect_cpu_quota() -> Option<usize> {
    if let Ok(contents) = std::fs::read_to_string("/sys/fs/cgroup/cpu.max") {
        let mut parts = contents.split_whitespace();
        let quota = parts.next()?;
        if quota == "max" {
            return None;
        }
        let quota: f64 = quota.parse().ok()?;
        let period: f64 = parts
            .next()
            .and_then(|p| p.parse().ok())
            .unwrap_or(100_000.0);
        if quota > 0.0 && period > 0.0 {
            return Some((quota / period).ceil() as usize);
        }
        return None;
    }

    let quota: i64 = std::fs::read_to_string("/sys/fs/cgroup/cpu/cpu.cfs_quota_us")
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let period: i64 = std::fs::read_to_string("/sys/fs/cgroup/cpu/cpu.cfs_period_us")
        .ok()?
        .trim()
        .parse()
        .ok()?;
    if quota > 0 && period > 0 {
        Some(((quota as f64) / (period as f64)).ceil() as usize)
    } else {
        None
    }
}

fn run_serve(entrypoint: &ProjectEntrypoint, port_override: Option<u16>) {
    let startup_started_at = Instant::now();
    let entrypoint = entrypoint.path.as_path();
    let project_root = entrypoint.parent().unwrap_or_else(|| Path::new("."));
    apply_project_env_defaults(project_root);
    let marreta_config = MarretaConfig::load_from_project_root(project_root);
    if let Some(err) = marreta_config.first_feature_flag_config_error() {
        exit_with_marreta_cli_error("invalid feature flag config", err);
    }
    let mut loaded = match file_loader::load_project_with_feature_flags(
        entrypoint,
        marreta_config.feature_flags.clone(),
    ) {
        Ok(r) => r,
        Err(e) => exit_with_marreta_startup_error(e),
    };

    // Inject `env` map — project marreta.env first, then process env overrides.
    let mut env_map: ValueMap = MarretaConfig::project_env_vars(project_root)
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    env_map.extend(std::env::vars().map(|(k, v)| (k, Value::String(v))));
    loaded.runtime.inject_global(
        "env".to_string(),
        Value::Map(Arc::new(std::sync::RwLock::new(env_map))),
    );
    // Freeze the global/module environments into shared read-only bases so each
    // request (and task call / broadcast branch) clones an Arc instead of
    // deep-copying every global definition.
    loaded.runtime.freeze_envs();
    let registry = loaded.registry;
    let runtime = Arc::new(loaded.runtime);
    let doc_index_plan = loaded.doc_index_plan;
    let marreta_config = match port_override {
        Some(p) => marreta_config.with_port(p),
        None => marreta_config,
    };

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(server_worker_threads())
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => exit_with_marreta_runtime_error("failed to start async runtime", e),
    };

    // Announce provider connection progress so a slow or failing connection is visible
    // instead of a silent wait. Only configured providers are mentioned (Spec 065).
    let any_provider = marreta_config.db.is_some()
        || marreta_config.doc.is_some()
        || marreta_config.cache.is_some()
        || (!registry.consumers.is_empty() && marreta_config.queue.is_some());
    if any_provider {
        println!("Connecting to providers...");
    }

    // Initialize DB engine if configured (blocking on the tokio runtime)
    if let Some(db) = marreta_config.db.as_ref() {
        println!("DB connecting ({})...", db.provider_name());
    }
    let db_engine = rt.block_on(async { DbEngine::from_config(&marreta_config).await });

    let db_engine = match db_engine {
        Ok(e) => e,
        Err(err) => exit_with_marreta_startup_error(err),
    };

    if db_engine.is_some() {
        println!(
            "DB connected ({})",
            marreta_config
                .db
                .as_ref()
                .map(|db| db.provider_name())
                .unwrap_or("unknown")
        );
    }

    // Initialize Doc engine if configured
    if let Some(doc) = marreta_config.doc.as_ref() {
        println!("DocDB connecting ({})...", doc.provider_name());
    }
    let doc_engine =
        rt.block_on(async { marreta::doc::DocEngine::from_config(&marreta_config).await });

    let doc_engine = match doc_engine {
        Ok(e) => e,
        Err(err) => exit_with_marreta_startup_error(err),
    };

    if doc_engine.is_some() {
        println!(
            "DocDB connected ({})",
            marreta_config
                .doc
                .as_ref()
                .map(|doc| doc.provider_name())
                .unwrap_or("unknown")
        );
    }

    // Initialize queue driver if consumers are defined
    let queue_driver = if !registry.consumers.is_empty() {
        if let Some(queue) = marreta_config.queue.as_ref() {
            println!("Queue connecting ({})...", queue.provider_name());
        }
        match rt.block_on(QueueEngine::from_config(&marreta_config)) {
            Ok(Some(engine)) => {
                println!(
                    "Queue connected ({})",
                    marreta_config
                        .queue
                        .as_ref()
                        .map(|queue| queue.provider_name())
                        .unwrap_or("unknown")
                );
                Some(engine.driver)
            }
            Ok(None) => None,
            Err(err) => exit_with_marreta_runtime_error("queue initialization error", err),
        }
    } else {
        None
    };

    // Initialize cache driver if cache config is present
    let cache_engine = if marreta_config.cache.is_some() {
        if let Some(cache) = marreta_config.cache.as_ref() {
            println!("Cache connecting ({})...", cache.provider_name());
        }
        match rt.block_on(CacheEngine::from_config(&marreta_config)) {
            Ok(Some(engine)) => {
                println!(
                    "Cache connected ({})",
                    marreta_config
                        .cache
                        .as_ref()
                        .map(|cache| cache.provider_name())
                        .unwrap_or("unknown")
                );
                Some(engine)
            }
            Ok(None) => None,
            Err(err) => exit_with_marreta_runtime_error("cache initialization error", err),
        }
    } else {
        None
    };

    let config = ServerConfig {
        host: marreta_config.host,
        port: marreta_config.port,
        cors_enabled: marreta_config.cors_enabled,
        cors_origin: marreta_config.cors_origin,
        docs_enabled: marreta_config.docs_enabled,
        docs_path: marreta_config.docs_path,
        db_engine,
        doc_engine,
        queue_driver,
        cache_engine,
        http_client_driver: match HttpClientEngine::from_env() {
            Ok(engine) => Some(engine.driver),
            Err(err) => exit_with_marreta_runtime_error("http client initialization error", err),
        },
        request_log_enabled: marreta::server::request_log_enabled_for_serve_from_env(),
        trace_context_enabled: marreta::server::trace_context_enabled_for_serve_from_env(),
        startup_started_at: Some(startup_started_at),
    };

    // Spec 067: ensure the inferred document indexes in the background, concurrent with serving,
    // so a slow online build on a large collection never delays the bind. The app serves
    // immediately; a brand-new query shape runs unindexed (today's behavior) until its build
    // finishes. A failed build is logged, never crashing serve.
    if let Some(engine) = config.doc_engine.as_ref() {
        if !doc_index_plan.is_empty() {
            rt.spawn(ensure_doc_indexes(engine.driver.clone(), doc_index_plan));
        }
    }

    if let Err(e) = rt.block_on(serve(registry, runtime, config)) {
        exit_with_marreta_runtime_error("server error", e);
    }
}

/// Ensure the inferred document indexes (Spec 067) in the background. `createIndex` is idempotent
/// and builds online (it does not lock the collection), so serving is unaffected. A failed build
/// is logged and skipped.
async fn ensure_doc_indexes(
    driver: Arc<dyn marreta::doc::DocDriver>,
    plan: Vec<marreta::doc::index_inference::InferredIndex>,
) {
    for idx in plan {
        println!(
            "ensuring document index {} on {} ...",
            idx.name, idx.collection
        );
        match driver
            .ensure_index(&idx.collection, &idx.keys, false, &idx.name)
            .await
        {
            Ok(()) => println!("document index {} ready", idx.name),
            Err(e) => eprintln!(
                "document index {} build failed (serving continues): {}",
                idx.name, e
            ),
        }
    }
}

fn run_migrate(args: &[String]) {
    cli_ux::begin("migrate");
    let label: &str = match args.get(2).map(|s| s.as_str()) {
        Some("diff") => {
            let entrypoint = resolve_project_entrypoint(args.get(3).map(|s| s.as_str()));
            run_migrate_diff(&entrypoint);
            "diff"
        }
        Some("generate") => {
            let entrypoint = resolve_project_entrypoint(args.get(3).map(|s| s.as_str()));
            run_migrate_generate(&entrypoint);
            "generate"
        }
        Some("status") => {
            let entrypoint = resolve_project_entrypoint(args.get(3).map(|s| s.as_str()));
            run_migrate_status(&entrypoint);
            "status"
        }
        Some("list") => {
            let entrypoint = resolve_project_entrypoint(args.get(3).map(|s| s.as_str()));
            run_migrate_list(&entrypoint);
            "list"
        }
        Some("discard") => {
            let version = match args.get(3) {
                Some(v) => v,
                None => exit_with_marreta_cli_error(
                    "invalid migrate discard usage",
                    "Usage: marreta migrate discard <version> [app.marreta]",
                ),
            };
            let entrypoint = resolve_project_entrypoint(args.get(4).map(|s| s.as_str()));
            run_migrate_discard(version, &entrypoint);
            "discard"
        }
        Some("explain") => {
            run_migrate_explain(args.get(3).map(|s| s.as_str()));
            "explain"
        }
        Some("apply") => {
            let entrypoint = resolve_project_entrypoint(args.get(3).map(|s| s.as_str()));
            run_migrate_apply(&entrypoint);
            "apply"
        }
        Some("rollback") => {
            let entrypoint = resolve_project_entrypoint(args.get(3).map(|s| s.as_str()));
            run_migrate_rollback(&entrypoint);
            "rollback"
        }
        Some(subcommand) => {
            exit_with_marreta_cli_error(
                "unknown migrate subcommand",
                format!("migrate {} is not implemented yet", subcommand),
            );
        }
        None => exit_with_marreta_cli_error(
            "invalid migrate usage",
            "Usage: marreta migrate <diff|generate|status|list|explain|discard|apply|rollback>",
        ),
    };
    cli_ux::end(cli_ux::Outcome::Success, label);
}

fn run_doctor(entrypoint: &ProjectEntrypoint, connect: bool) {
    let entrypoint_path = entrypoint.path.as_path();
    let project_root = entrypoint_path.parent().unwrap_or_else(|| Path::new("."));
    apply_project_env_defaults(project_root);
    let config = MarretaConfig::load_from_project_root(project_root);
    let loaded = match file_loader::load_project_with_feature_flags(
        entrypoint_path,
        config.feature_flags.clone(),
    ) {
        Ok(loaded) => loaded,
        Err(err) => exit_with_marreta_startup_error(err),
    };
    let report = build_doctor_report(entrypoint_path, &loaded, &config, connect);

    for (idx, section) in report.sections.iter().enumerate() {
        println!("{}:", section.title);
        for entry in &section.entries {
            println!("  {:<5} {}", entry.status.as_str(), entry.message);
        }
        if idx + 1 < report.sections.len() {
            println!();
        }
    }

    let (outcome, summary) = if report.has_errors {
        (cli_ux::Outcome::Failure, "issues found")
    } else {
        (cli_ux::Outcome::Success, "all checks passed")
    };
    cli_ux::end(outcome, summary);

    if report.has_errors {
        process::exit(1);
    }
}

fn parse_init_args(args: &[String]) -> (&str, InitOptions) {
    let usage = "Usage: marreta init <project-path> [--with db,cache,doc,queue]";
    let Some(path) = args.get(2).map(String::as_str) else {
        exit_with_marreta_cli_error("invalid init usage", usage);
    };

    let mut services = std::collections::BTreeSet::new();
    let mut index = 3;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--with" {
            let Some(value) = args.get(index + 1) else {
                exit_with_marreta_cli_error(
                    "invalid init usage",
                    "--with requires a comma-separated service list",
                );
            };
            match parse_services(value) {
                Ok(parsed) => services.extend(parsed),
                Err(err) => exit_with_marreta_cli_error("invalid init usage", err),
            }
            index += 2;
        } else if let Some(value) = arg.strip_prefix("--with=") {
            if value.is_empty() {
                exit_with_marreta_cli_error(
                    "invalid init usage",
                    "--with requires a comma-separated service list",
                );
            }
            match parse_services(value) {
                Ok(parsed) => services.extend(parsed),
                Err(err) => exit_with_marreta_cli_error("invalid init usage", err),
            }
            index += 1;
        } else {
            exit_with_marreta_cli_error("invalid init usage", usage);
        }
    }

    (path, InitOptions { services })
}

fn run_init(path: &str, options: InitOptions) {
    match init_project_with_options(path, options) {
        Ok(result) => {
            print!("{}", render_next_steps(&result));
            cli_ux::end(cli_ux::Outcome::Success, "project created");
        }
        Err(err) => exit_with_marreta_cli_error("init failed", err),
    }
}

struct FmtArgs {
    paths: Vec<PathBuf>,
    check: bool,
    stdin: bool,
    file: Option<PathBuf>,
}

const FMT_USAGE: &str = "Usage: marreta fmt [--check] [--stdin --file <path>] [file|dir ...]";

fn parse_fmt_args(args: &[String]) -> FmtArgs {
    let mut paths = Vec::new();
    let mut check = false;
    let mut stdin = false;
    let mut file = None;
    let mut i = 2;

    while i < args.len() {
        match args[i].as_str() {
            "--check" => {
                check = true;
                i += 1;
            }
            "--stdin" => {
                stdin = true;
                i += 1;
            }
            "--file" => {
                let value = args
                    .get(i + 1)
                    .unwrap_or_else(|| exit_with_marreta_cli_error("invalid fmt usage", FMT_USAGE));
                file = Some(PathBuf::from(value));
                i += 2;
            }
            arg if arg.starts_with("--file=") => {
                let value = arg.trim_start_matches("--file=");
                if value.is_empty() {
                    exit_with_marreta_cli_error("invalid fmt usage", FMT_USAGE);
                }
                file = Some(PathBuf::from(value));
                i += 1;
            }
            arg if arg.starts_with("--") => {
                exit_with_marreta_cli_error(
                    "invalid fmt usage",
                    format!("unknown option '{}'. {}", arg, FMT_USAGE),
                );
            }
            arg => {
                paths.push(PathBuf::from(arg));
                i += 1;
            }
        }
    }

    if stdin && !paths.is_empty() {
        exit_with_marreta_cli_error("invalid fmt usage", FMT_USAGE);
    }

    FmtArgs {
        paths,
        check,
        stdin,
        file,
    }
}

fn run_fmt(args: FmtArgs) {
    if args.stdin {
        // Machine mode (editor format-on-save): emits formatted source to stdout,
        // never framed.
        run_fmt_stdin(args.check, args.file);
        return;
    }

    if args.file.is_some() {
        exit_with_marreta_cli_error("invalid fmt usage", "--file can only be used with --stdin");
    }

    let files = if args.paths.is_empty() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        discover_project_files(&cwd)
    } else {
        discover_explicit_files(&args.paths)
    };
    let files = match files {
        Ok(files) => files,
        Err(err) => exit_with_fmt_error(err),
    };

    let mut changed = Vec::new();
    for file in files {
        match format_file(&file, args.check) {
            Ok(true) => changed.push(file),
            Ok(false) => {}
            Err(err) => exit_with_marreta_cli_error(
                "fmt failed",
                format!("{}: {}", fmt_display_path(&file), err),
            ),
        }
    }

    if args.check {
        for file in &changed {
            println!("FORMAT {}", fmt_display_path(file));
        }
        if !changed.is_empty() {
            println!();
            println!(
                "{} file{} need formatting. Run `marreta fmt`.",
                changed.len(),
                if changed.len() == 1 { "" } else { "s" }
            );
            cli_ux::end(
                cli_ux::Outcome::Failure,
                &format!(
                    "{} file{} need formatting",
                    changed.len(),
                    if changed.len() == 1 { "" } else { "s" }
                ),
            );
            process::exit(1);
        }
        println!("All files formatted.");
        cli_ux::end(cli_ux::Outcome::Success, "all files formatted");
    } else {
        println!(
            "Formatted {} file{}.",
            changed.len(),
            if changed.len() == 1 { "" } else { "s" }
        );
        cli_ux::end(
            cli_ux::Outcome::Success,
            &format!(
                "{} file{} formatted",
                changed.len(),
                if changed.len() == 1 { "" } else { "s" }
            ),
        );
    }
}

fn run_fmt_stdin(check: bool, file: Option<PathBuf>) {
    let Some(file) = file else {
        exit_with_fmt_error(FormatError::MissingFileArgument);
    };
    let mut source = String::new();
    if let Err(source_err) = io::stdin().read_to_string(&mut source) {
        exit_with_marreta_cli_error("fmt failed", source_err);
    }
    let result = match format_source(&source) {
        Ok(result) => result,
        Err(err) => exit_with_fmt_error(err),
    };
    if check {
        if result.changed {
            println!("FORMAT {}", fmt_display_path(&file));
            process::exit(1);
        }
        return;
    }
    print!("{}", result.output);
}

fn exit_with_fmt_error(err: FormatError) -> ! {
    exit_with_marreta_cli_error("fmt failed", err)
}

fn fmt_display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

struct LintArgs {
    paths: Vec<PathBuf>,
    strict: bool,
    format: LintFormat,
    stdin: bool,
    file: Option<PathBuf>,
}

const LINT_USAGE: &str =
    "Usage: marreta lint [--strict] [--format human|json] [--stdin --file <path>] [file|dir ...]";

fn parse_lint_args(args: &[String]) -> LintArgs {
    let mut paths = Vec::new();
    let mut strict = false;
    let mut format = LintFormat::Human;
    let mut stdin = false;
    let mut file = None;
    let mut i = 2;

    while i < args.len() {
        match args[i].as_str() {
            "--strict" => {
                strict = true;
                i += 1;
            }
            "--stdin" => {
                stdin = true;
                i += 1;
            }
            "--file" => {
                let value = args.get(i + 1).unwrap_or_else(|| {
                    exit_with_marreta_cli_error("invalid lint usage", LINT_USAGE)
                });
                file = Some(PathBuf::from(value));
                i += 2;
            }
            arg if arg.starts_with("--file=") => {
                let value = arg.trim_start_matches("--file=");
                if value.is_empty() {
                    exit_with_marreta_cli_error("invalid lint usage", LINT_USAGE);
                }
                file = Some(PathBuf::from(value));
                i += 1;
            }
            "--format" => {
                let value = args.get(i + 1).unwrap_or_else(|| {
                    exit_with_marreta_cli_error("invalid lint usage", LINT_USAGE)
                });
                format = parse_lint_format(value);
                i += 2;
            }
            arg if arg.starts_with("--format=") => {
                format = parse_lint_format(arg.trim_start_matches("--format="));
                i += 1;
            }
            arg if arg.starts_with("--") => {
                exit_with_marreta_cli_error(
                    "invalid lint usage",
                    format!("unknown option '{}'. {}", arg, LINT_USAGE),
                );
            }
            arg => {
                paths.push(PathBuf::from(arg));
                i += 1;
            }
        }
    }

    if stdin && !paths.is_empty() {
        exit_with_marreta_cli_error("invalid lint usage", LINT_USAGE);
    }

    LintArgs {
        paths,
        strict,
        format,
        stdin,
        file,
    }
}

fn parse_lint_format(value: &str) -> LintFormat {
    match value {
        "human" => LintFormat::Human,
        "json" => LintFormat::Json,
        _ => exit_with_marreta_cli_error(
            "invalid lint usage",
            format!("unknown lint format '{}'. Expected human or json.", value),
        ),
    }
}

fn run_lint(args: LintArgs) {
    // Human, non-stdin lint is framed (the frame was opened by the caller). `--stdin`
    // (editor) and `--format json` are machine modes and stay frame-free, so `end`
    // is gated on the same condition.
    let framed = !args.stdin && matches!(args.format, LintFormat::Human);

    let report = if args.stdin {
        let Some(file) = args.file else {
            exit_with_lint_error(LintError::MissingFileArgument);
        };
        let mut source = String::new();
        if let Err(source_err) = io::stdin().read_to_string(&mut source) {
            exit_with_marreta_cli_error("lint failed", source_err);
        }
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        match lint_project_stdin(&cwd, file.clone(), source.clone()) {
            Ok(report) => report,
            Err(LintError::MissingProjectRoot(_)) => lint_stdin(file, source),
            Err(err) => exit_with_lint_error(err),
        }
    } else {
        if args.file.is_some() {
            exit_with_marreta_cli_error(
                "invalid lint usage",
                "--file can only be used with --stdin",
            );
        }
        let result = if args.paths.is_empty() {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            lint_project(&cwd)
        } else {
            lint_paths(&args.paths)
        };
        match result {
            Ok(report) => report,
            Err(err) => exit_with_lint_error(err),
        }
    };

    print!("{}", report.render(args.format));
    let fail = report.should_fail(args.strict);
    if framed {
        let count = report.diagnostics.len();
        let summary = format!("{} diagnostic{}", count, if count == 1 { "" } else { "s" });
        let outcome = if fail {
            cli_ux::Outcome::Failure
        } else {
            cli_ux::Outcome::Success
        };
        cli_ux::end(outcome, &summary);
    }
    if fail {
        process::exit(1);
    }
}

fn exit_with_lint_error(err: LintError) -> ! {
    exit_with_marreta_cli_error("lint failed", err)
}

#[derive(Debug, Clone)]
struct ToolingArgs {
    subcommand: String,
    format_json: bool,
    stdin: bool,
    file: Option<PathBuf>,
    line: Option<usize>,
    column: Option<usize>,
}

const TOOLING_USAGE: &str = "Usage: marreta tooling <catalog|symbols|completions|hover|definition> [--format json] [--stdin --file <path>] [--line N --column N]";

fn run_tooling(args: &[String]) {
    let args = parse_tooling_args(args);
    if !args.format_json {
        exit_with_marreta_cli_error(
            "invalid tooling usage",
            "tooling commands currently require --format json",
        );
    }

    match args.subcommand.as_str() {
        "catalog" => {
            println!("{}", catalog::catalog_json());
        }
        "symbols" => match tooling_project_root(args.file.as_deref()) {
            Some(root) => match symbols::symbols_json(&root) {
                Ok(json) => println!("{}", json),
                Err(err) => exit_with_marreta_startup_error(err),
            },
            // Project-less fallback: symbols from the single buffer/file.
            None => {
                let Some(file) = args.file.clone() else {
                    exit_with_marreta_cli_error("invalid tooling usage", "--file is required");
                };
                let source = tooling_source(&args, &file);
                println!(
                    "{}",
                    symbols::symbols_json_from_source(&source, &file.to_string_lossy())
                );
            }
        },
        "definition" => {
            let Some(file) = args.file.clone() else {
                exit_with_marreta_cli_error("invalid tooling usage", "--file is required");
            };
            let line = args.line.unwrap_or_else(|| {
                exit_with_marreta_cli_error("invalid tooling usage", "--line is required")
            });
            let column = args.column.unwrap_or_else(|| {
                exit_with_marreta_cli_error("invalid tooling usage", "--column is required")
            });
            let source = tooling_source(&args, &file);
            // Resolve against the current buffer first (live positions), then the
            // rest of the project for cross-file references.
            let mut symbols = symbols::collect_source_symbols(&source, &file.to_string_lossy());
            if let Some(root) = tooling_project_root(Some(file.as_path()))
                && let Ok(project) = symbols::collect_project_symbols(&root)
            {
                symbols.extend(project);
            }
            println!(
                "{}",
                definition::definition_json(&source, line, column, &symbols)
            );
        }
        "completions" | "hover" => {
            let Some(file) = args.file.clone() else {
                exit_with_marreta_cli_error("invalid tooling usage", "--file is required");
            };
            let line = args.line.unwrap_or_else(|| {
                exit_with_marreta_cli_error("invalid tooling usage", "--line is required")
            });
            let column = args.column.unwrap_or_else(|| {
                exit_with_marreta_cli_error("invalid tooling usage", "--column is required")
            });
            let source = tooling_source(&args, &file);
            let project_symbols = tooling_project_root(Some(file.as_path()))
                .and_then(|root| symbols::collect_project_symbols(&root).ok())
                .unwrap_or_default();
            let json = if args.subcommand == "completions" {
                completions::completions_json(&source, line, column, &project_symbols)
            } else {
                hover::hover_json(&source, line, column, &project_symbols)
            };
            println!("{}", json);
        }
        _ => exit_with_marreta_cli_error("invalid tooling usage", TOOLING_USAGE),
    }
}

fn parse_tooling_args(args: &[String]) -> ToolingArgs {
    let Some(subcommand) = args.get(2).cloned() else {
        exit_with_marreta_cli_error("invalid tooling usage", TOOLING_USAGE);
    };
    let mut format_json = false;
    let mut stdin = false;
    let mut file = None;
    let mut line = None;
    let mut column = None;
    let mut i = 3;

    while i < args.len() {
        match args[i].as_str() {
            "--format" => {
                let value = args.get(i + 1).unwrap_or_else(|| {
                    exit_with_marreta_cli_error("invalid tooling usage", TOOLING_USAGE)
                });
                format_json = parse_tooling_format(value);
                i += 2;
            }
            arg if arg.starts_with("--format=") => {
                format_json = parse_tooling_format(arg.trim_start_matches("--format="));
                i += 1;
            }
            "--stdin" => {
                stdin = true;
                i += 1;
            }
            "--file" => {
                let value = args.get(i + 1).unwrap_or_else(|| {
                    exit_with_marreta_cli_error("invalid tooling usage", TOOLING_USAGE)
                });
                file = Some(PathBuf::from(value));
                i += 2;
            }
            arg if arg.starts_with("--file=") => {
                let value = arg.trim_start_matches("--file=");
                if value.is_empty() {
                    exit_with_marreta_cli_error("invalid tooling usage", TOOLING_USAGE);
                }
                file = Some(PathBuf::from(value));
                i += 1;
            }
            "--line" => {
                let value = args.get(i + 1).unwrap_or_else(|| {
                    exit_with_marreta_cli_error("invalid tooling usage", TOOLING_USAGE)
                });
                line = Some(parse_tooling_usize("--line", value));
                i += 2;
            }
            arg if arg.starts_with("--line=") => {
                line = Some(parse_tooling_usize(
                    "--line",
                    arg.trim_start_matches("--line="),
                ));
                i += 1;
            }
            "--column" => {
                let value = args.get(i + 1).unwrap_or_else(|| {
                    exit_with_marreta_cli_error("invalid tooling usage", TOOLING_USAGE)
                });
                column = Some(parse_tooling_usize("--column", value));
                i += 2;
            }
            arg if arg.starts_with("--column=") => {
                column = Some(parse_tooling_usize(
                    "--column",
                    arg.trim_start_matches("--column="),
                ));
                i += 1;
            }
            arg => exit_with_marreta_cli_error(
                "invalid tooling usage",
                format!("unknown option '{}'. {}", arg, TOOLING_USAGE),
            ),
        }
    }

    ToolingArgs {
        subcommand,
        format_json,
        stdin,
        file,
        line,
        column,
    }
}

fn parse_tooling_format(value: &str) -> bool {
    match value {
        "json" => true,
        _ => exit_with_marreta_cli_error(
            "invalid tooling usage",
            format!("unknown tooling format '{}'. Expected json.", value),
        ),
    }
}

fn parse_tooling_usize(flag: &str, value: &str) -> usize {
    value.parse::<usize>().unwrap_or_else(|_| {
        exit_with_marreta_cli_error(
            "invalid tooling usage",
            format!("{flag} expects a positive integer, got '{}'", value),
        )
    })
}

fn tooling_source(args: &ToolingArgs, file: &Path) -> String {
    if args.stdin {
        let mut source = String::new();
        if let Err(err) = io::stdin().read_to_string(&mut source) {
            exit_with_marreta_cli_error("tooling failed", err);
        }
        return source;
    }

    let path = tooling_project_root(Some(file))
        .map(|root| root.join(file))
        .filter(|candidate| candidate.exists())
        .unwrap_or_else(|| file.to_path_buf());
    std::fs::read_to_string(&path).unwrap_or_else(|err| {
        exit_with_marreta_cli_error(
            "tooling failed",
            format!("cannot read '{}': {err}", path.display()),
        )
    })
}

fn tooling_project_root(file: Option<&Path>) -> Option<PathBuf> {
    let start = file
        .and_then(|path| {
            if path.is_absolute() {
                path.parent().map(Path::to_path_buf)
            } else {
                std::env::current_dir()
                    .ok()
                    .map(|cwd| cwd.join(path))
                    .and_then(|path| path.parent().map(Path::to_path_buf))
            }
        })
        .or_else(|| std::env::current_dir().ok())?;

    for dir in start.ancestors() {
        if dir.join("app.marreta").exists() {
            return Some(dir.to_path_buf());
        }
    }
    None
}

struct TestArgs {
    path: Option<PathBuf>,
    filter: Option<String>,
    list: bool,
    coverage: bool,
}

const TEST_USAGE: &str = "Usage: marreta test [path] [--filter TEXT] [--list] [--coverage]";

fn parse_test_args(args: &[String]) -> TestArgs {
    let mut path = None;
    let mut filter = None;
    let mut list = false;
    let mut coverage = false;
    let mut i = 2;

    while i < args.len() {
        match args[i].as_str() {
            "--list" => {
                list = true;
                i += 1;
            }
            "--coverage" => {
                coverage = true;
                i += 1;
            }
            "--filter" => {
                let value = args.get(i + 1).unwrap_or_else(|| {
                    exit_with_marreta_cli_error("invalid test usage", TEST_USAGE)
                });
                filter = Some(value.clone());
                i += 2;
            }
            arg if arg.starts_with("--") => {
                exit_with_marreta_cli_error(
                    "invalid test usage",
                    format!("unknown option '{}'", arg),
                );
            }
            arg => {
                if path.is_some() {
                    exit_with_marreta_cli_error("invalid test usage", TEST_USAGE);
                }
                path = Some(PathBuf::from(arg));
                i += 1;
            }
        }
    }

    TestArgs {
        path,
        filter,
        list,
        coverage,
    }
}

fn run_test(entrypoint: &ProjectEntrypoint, args: TestArgs) {
    let entrypoint_path = entrypoint.path.as_path();
    let project_root = entrypoint_path.parent().unwrap_or_else(|| Path::new("."));
    apply_project_env_defaults(project_root);
    let config = MarretaConfig::load_from_project_root(project_root);
    if let Some(err) = config.first_feature_flag_config_error() {
        exit_with_marreta_cli_error("invalid feature flag config", err);
    }
    let loaded = match file_loader::load_project_with_feature_flags(
        entrypoint_path,
        config.feature_flags.clone(),
    ) {
        Ok(loaded) => loaded,
        Err(err) => exit_with_marreta_startup_error(err),
    };

    let explicit_path = args.path.clone();
    let paths = if let Some(path) = explicit_path.clone() {
        let path = if path.is_absolute() {
            path
        } else {
            project_root.join(path)
        };
        vec![path]
    } else {
        match discover_scenario_files(project_root) {
            Ok(paths) => paths,
            Err(err) => exit_with_marreta_runtime_error("failed to discover scenario files", err),
        }
    };

    let scenario_files = match load_scenario_files(&paths) {
        Ok(files) => files,
        Err(err) => exit_with_marreta_startup_error(err),
    };

    println!(
        "Project: {} v{}",
        project_metadata_value(
            &loaded.runtime.global_env,
            "project_name",
            "MarretaLang Project"
        ),
        project_metadata_value(&loaded.runtime.global_env, "project_version", "1.0.0")
    );
    if let Some(path) = explicit_path.as_ref()
        && !is_auto_discovered_test_path(path)
    {
        println!(
            "Note: {} was run explicitly. Automatic discovery only includes tests/**/*_test.marreta.",
            path.to_string_lossy().replace('\\', "/")
        );
    }

    if args.list {
        println!("Entrypoint: {}", entrypoint_path.display());
        println!();
        println!("Loaded routes:");
        if loaded.registry.routes.is_empty() {
            println!("  none");
        } else {
            for route in &loaded.registry.routes {
                let source = route.source_file.as_deref().unwrap_or("app.marreta");
                println!(
                    "  {} {} -> {}:{}",
                    route.verb, route.path, source, route.line
                );
            }
        }
        println!();
        println!("Scenario files:");
        if scenario_files.is_empty() {
            println!("  none");
        } else {
            for file in &scenario_files {
                println!("  {}", display_path(project_root, &file.path));
                for scenario in selected_scenarios(file, args.filter.as_deref()) {
                    println!(
                        "    scenario \"{}\" -> line {}",
                        scenario.name, scenario.line
                    );
                }
            }
        }
        println!();
        cli_ux::end(cli_ux::Outcome::Success, "listed");
        return;
    }

    let selected_count: usize = scenario_files
        .iter()
        .map(|file| selected_scenarios(file, args.filter.as_deref()).len())
        .sum();

    if selected_count == 0 {
        println!();
        println!("0 passed, 0 failed");
        cli_ux::end(cli_ux::Outcome::Success, "0 passed, 0 failed");
        return;
    }

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(err) => exit_with_marreta_runtime_error("failed to create async runtime", err),
    };
    let results = rt.block_on(run_scenarios(
        &loaded,
        &scenario_files,
        args.filter.as_deref(),
    ));
    let passed = results.iter().filter(|result| result.passed).count();
    let failed = results.len() - passed;

    println!();
    for file in &scenario_files {
        let file_results: Vec<_> = results
            .iter()
            .filter(|result| result.file == file.path)
            .collect();
        if file_results.is_empty() {
            continue;
        }
        if file_results.iter().all(|result| result.passed) {
            println!("PASS {}", display_path(project_root, &file.path));
        } else {
            println!("FAIL {}", display_path(project_root, &file.path));
        }
        for result in file_results {
            if result.passed {
                println!("  PASS {}", result.name);
            } else {
                println!("  FAIL {}", result.name);
                if let Some(error) = &result.error {
                    println!("    {}", error);
                }
            }
        }
        println!();
    }
    println!("{} passed, {} failed", passed, failed);

    if args.coverage {
        print_api_coverage(&loaded.registry.routes, &results);
    }

    let outcome = if failed > 0 {
        cli_ux::Outcome::Failure
    } else {
        cli_ux::Outcome::Success
    };
    cli_ux::end(outcome, &format!("{} passed, {} failed", passed, failed));

    if failed > 0 {
        process::exit(1);
    }
}

fn is_auto_discovered_test_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with("_test.marreta"))
}

fn print_api_coverage(
    routes: &[marreta::route_loader::RouteDefinition],
    results: &[marreta::scenario_tests::ScenarioRun],
) {
    let mut all_routes = BTreeSet::new();
    for route in routes {
        all_routes.insert(marreta::coverage::route_key(&route.verb, &route.path));
    }

    let mut covered_routes: BTreeMap<String, usize> = BTreeMap::new();
    for result in results.iter().filter(|result| result.passed) {
        if let (Some(verb), Some(path)) = (&result.route_verb, &result.route_path) {
            *covered_routes
                .entry(marreta::coverage::route_key(verb, path))
                .or_insert(0) += 1;
        }
    }

    // Headline route counts go through the shared summarizer so the test runner
    // and doctor compute presence the same way (files_total/unmatched are part of
    // the shared shape but not surfaced by --coverage today).
    let covered_keys: BTreeSet<String> = covered_routes.keys().cloned().collect();
    let files_total = results
        .iter()
        .map(|result| result.file.clone())
        .collect::<BTreeSet<_>>()
        .len();
    let unmatched = results
        .iter()
        .filter(|result| result.passed && result.route_path.is_none())
        .count();
    let summary = marreta::coverage::summarize(
        &all_routes,
        &covered_keys,
        results.len(),
        files_total,
        unmatched,
    );

    let covered_count = summary.routes_with_scenario;
    let total_routes = summary.routes_total;
    let pct = summary.routes_with_scenario_pct();
    let passed = results.iter().filter(|result| result.passed).count();
    let failed = results.len() - passed;
    let assertions: usize = results.iter().map(|result| result.assertion_count).sum();
    let givens: usize = results.iter().map(|result| result.given_count).sum();

    println!();
    println!("API coverage:");
    println!(
        "  scenarios: {} passed, {} failed, {} total",
        passed,
        failed,
        results.len()
    );
    println!("  assertions: {} declared", assertions);
    println!("  given: {} declared", givens);
    println!(
        "  routes covered: {} / {} ({:.1}%)",
        covered_count, total_routes, pct
    );

    println!();
    println!("Covered routes:");
    if covered_routes.is_empty() {
        println!("  none");
    } else {
        for (route, scenario_count) in &covered_routes {
            println!(
                "  {} ({} scenario{})",
                route,
                scenario_count,
                if *scenario_count == 1 { "" } else { "s" }
            );
        }
    }

    let uncovered = all_routes
        .difference(&covered_routes.keys().cloned().collect::<BTreeSet<_>>())
        .cloned()
        .collect::<Vec<_>>();
    println!();
    println!("Uncovered routes:");
    if uncovered.is_empty() {
        println!("  none");
    } else {
        for route in uncovered {
            println!("  {}", route);
        }
    }
}

fn selected_scenarios<'a>(
    file: &'a ScenarioFile,
    filter: Option<&str>,
) -> Vec<&'a ScenarioDefinition> {
    file.scenarios
        .iter()
        .filter(|scenario| filter.is_none_or(|needle| scenario.name.contains(needle)))
        .collect()
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn project_metadata_value(
    env: &marreta::environment::Environment,
    key: &str,
    default: &str,
) -> String {
    env.get(key)
        .and_then(|value| {
            if let Value::String(value) = value {
                Some(value)
            } else {
                None
            }
        })
        .unwrap_or_else(|| default.to_string())
}

fn run_migrate_diff(entrypoint: &ProjectEntrypoint) {
    let (_entrypoint, _migrations_dir, plan, drift) = compute_migration_plan(entrypoint);
    if plan.is_empty() {
        if drift.is_empty() {
            println!("Database schema is up to date.");
        } else {
            println!("No supported migration operations.");
            print_schema_drift(&drift);
        }
        return;
    }
    let create_tables = plan
        .iter()
        .filter(|op| matches!(op, marreta::migrations::MigrationOp::CreateTable { .. }))
        .count();
    let add_columns = plan
        .iter()
        .filter(|op| matches!(op, marreta::migrations::MigrationOp::AddColumn { .. }))
        .count();
    let add_foreign_keys = plan
        .iter()
        .filter(|op| matches!(op, marreta::migrations::MigrationOp::AddForeignKey { .. }))
        .count();
    let sql = match render_postgres_up_sql(&plan) {
        Ok(sql) => sql,
        Err(e) => exit_with_marreta_runtime_error("failed to render migration diff", e),
    };

    println!("Planned migration operations:");
    println!(
        "  {} table{} to create, {} column{} to add, {} foreign key{} to add",
        create_tables,
        if create_tables == 1 { "" } else { "s" },
        add_columns,
        if add_columns == 1 { "" } else { "s" },
        add_foreign_keys,
        if add_foreign_keys == 1 { "" } else { "s" }
    );
    println!();

    for statement in sql {
        println!("{}", statement);
        println!();
    }

    print_schema_drift(&drift);
}

/// Spec 073 (2.2): print the report-only drift block (changes the additive-only planner does not
/// support). Prints nothing when there is no drift.
fn print_schema_drift(drift: &[marreta::migrations::SchemaDrift]) {
    if drift.is_empty() {
        return;
    }
    println!("Unsupported changes detected (migrations are additive-only, handle manually):");
    for entry in drift {
        println!("{}", entry.report_line());
    }
}

fn run_migrate_generate(entrypoint: &ProjectEntrypoint) {
    let (_entrypoint, migrations_dir, plan, drift) = compute_migration_plan(entrypoint);
    if plan.is_empty() {
        if drift.is_empty() {
            println!("Database schema is up to date.");
        } else {
            println!("No supported migration operations.");
            print_schema_drift(&drift);
        }
        return;
    }

    let up_sql = match render_postgres_up_sql(&plan) {
        Ok(sql) => sql,
        Err(e) => exit_with_marreta_runtime_error("failed to render migration up SQL", e),
    };
    let down_sql = match render_postgres_down_sql(&plan) {
        Ok(sql) => sql,
        Err(e) => exit_with_marreta_runtime_error("failed to render migration down SQL", e),
    };
    let name = default_migration_name(&plan);
    let migration = match write_migration_files(&migrations_dir, &name, &up_sql, &down_sql) {
        Ok(migration) => migration,
        Err(e) => exit_with_marreta_runtime_error("failed to write migration files", e),
    };

    println!(
        "Generated migration: {}_{}",
        migration.version, migration.name
    );
    println!("{}", migration.up_path.display());
    if let Some(down_path) = migration.down_path {
        println!("{}", down_path.display());
    }

    print_schema_drift(&drift);
}

fn run_migrate_status(entrypoint: &ProjectEntrypoint) {
    let (_, _, _, _, report, _) = load_migration_state(entrypoint);
    print_status_group("Applied", &report.applied);
    print_status_group("Pending", &report.pending);
    print_status_group("Changed", &report.changed);
    print_status_group("Missing local", &report.missing_local);
    let command_suffix = entrypoint.command_suffix();
    if report.pending.is_empty() && report.changed.is_empty() && report.missing_local.is_empty() {
        println!("Database migration state is clean.");
    } else {
        print_status_suggestions(&report, &command_suffix);
    }
}

fn run_migrate_list(entrypoint: &ProjectEntrypoint) {
    let (_, _, _, _, _, inventory) = load_migration_state(entrypoint);
    print_migration_list(&inventory);
}

fn run_migrate_discard(version: &str, entrypoint: &ProjectEntrypoint) {
    if version.contains('_') && version.split('_').count() > 2 {
        exit_with_marreta_cli_error(
            "invalid migration version",
            "discard expects only the numeric version, for example: 20260410_215746",
        );
    }

    let (_, _, local, applied, _, _) = load_migration_state(entrypoint);
    let discarded = match discard_pending_migration(version, &local, &applied) {
        Ok(name) => name,
        Err(e) => exit_with_marreta_runtime_error("failed to discard pending migration", e),
    };
    println!("Discarded {}", discarded);
}

fn run_migrate_explain(topic: Option<&str>) {
    match topic.unwrap_or("overview") {
        "overview" => print_migrate_explain_overview(),
        "workflow" => print_migrate_explain_workflow(),
        "applied" => print_migrate_explain_applied(),
        "pending" => print_migrate_explain_pending(),
        "changed" => print_migrate_explain_changed(),
        "missing_local" => print_migrate_explain_missing_local(),
        other => exit_with_marreta_cli_error(
            "unknown migration explain topic",
            format!(
                "'{}' is invalid. Use: workflow, applied, pending, changed, missing_local",
                other
            ),
        ),
    }
}

fn run_migrate_apply(entrypoint: &ProjectEntrypoint) {
    let entrypoint = entrypoint.path.as_path();
    let project_root = entrypoint.parent().unwrap_or_else(|| Path::new("."));
    let migrations_dir = migrations_dir_for_project(entrypoint);
    let local = match discover_local_migrations(&migrations_dir) {
        Ok(migrations) => migrations,
        Err(e) => exit_with_marreta_runtime_error("failed to discover local migrations", e),
    };
    let config = MarretaConfig::load_from_project_root(project_root);
    let rt = runtime_or_exit();
    let applied = match rt.block_on(async {
        marreta::db::ensure_migration_table_from_config(&config).await?;
        marreta::db::list_applied_migrations_from_config(&config).await
    }) {
        Ok(applied) => applied,
        Err(e) => exit_with_marreta_startup_error(e),
    };
    let report = compare_migration_state(&local, &applied);
    if !report.changed.is_empty() || !report.missing_local.is_empty() {
        exit_with_marreta_runtime_error(
            "migration state is inconsistent",
            "resolve changed/missing_local entries before apply",
        );
    }

    let pending: Vec<_> = local
        .iter()
        .filter(|migration| {
            !applied
                .iter()
                .any(|applied_migration| applied_migration.version == migration.version)
        })
        .collect();

    if pending.is_empty() {
        println!("No pending migrations.");
        return;
    }

    for migration in pending {
        let result = rt
            .block_on(async { marreta::db::apply_migration_from_config(&config, migration).await });
        match result {
            Ok(()) => println!("Applied {}_{}", migration.version, migration.name),
            Err(e) => exit_with_marreta_startup_error(e),
        }
    }
}

fn run_migrate_rollback(entrypoint: &ProjectEntrypoint) {
    let entrypoint = entrypoint.path.as_path();
    let project_root = entrypoint.parent().unwrap_or_else(|| Path::new("."));
    let migrations_dir = migrations_dir_for_project(entrypoint);
    let local = match discover_local_migrations(&migrations_dir) {
        Ok(migrations) => migrations,
        Err(e) => exit_with_marreta_runtime_error("failed to discover local migrations", e),
    };
    let config = MarretaConfig::load_from_project_root(project_root);
    let rt = runtime_or_exit();
    let applied = match rt.block_on(async {
        marreta::db::ensure_migration_table_from_config(&config).await?;
        marreta::db::list_applied_migrations_from_config(&config).await
    }) {
        Ok(applied) => applied,
        Err(e) => exit_with_marreta_startup_error(e),
    };
    let last_applied = match applied.last() {
        Some(migration) => migration,
        None => {
            println!("No applied migrations.");
            return;
        }
    };
    let migration = match local
        .iter()
        .find(|migration| migration.version == last_applied.version)
    {
        Some(migration) => migration,
        None => exit_with_marreta_runtime_error(
            "cannot rollback migration",
            format!(
                "{}_{} is applied but the local migration files are missing",
                last_applied.version, last_applied.name
            ),
        ),
    };
    if migration.checksum != last_applied.checksum {
        exit_with_marreta_runtime_error(
            "cannot rollback migration",
            format!(
                "{}_{} local checksum differs from the applied record",
                migration.version, migration.name
            ),
        );
    }

    match rt
        .block_on(async { marreta::db::rollback_migration_from_config(&config, migration).await })
    {
        Ok(()) => println!("Rolled back {}_{}", migration.version, migration.name),
        Err(e) => exit_with_marreta_startup_error(e),
    }
}

fn compute_migration_plan(
    entrypoint: &ProjectEntrypoint,
) -> (
    PathBuf,
    PathBuf,
    Vec<marreta::migrations::MigrationOp>,
    Vec<marreta::migrations::SchemaDrift>,
) {
    let entrypoint = entrypoint.path.as_path();
    let project_root = entrypoint.parent().unwrap_or_else(|| Path::new("."));
    apply_project_env_defaults(project_root);
    let loaded = match file_loader::load_project(entrypoint) {
        Ok(r) => r,
        Err(e) => exit_with_marreta_startup_error(e),
    };

    if loaded.registry.persistent_schemas.is_empty() {
        println!("No db: schemas found.");
        process::exit(0);
    }

    let desired_tables = match build_persistent_tables(&loaded.registry.persistent_schemas) {
        Ok(tables) => tables,
        Err(e) => exit_with_marreta_runtime_error("failed to build db: schema model", e),
    };

    let migrations_dir = migrations_dir_for_project(entrypoint);
    let local = match discover_local_migrations(&migrations_dir) {
        Ok(migrations) => migrations,
        Err(e) => exit_with_marreta_runtime_error("failed to discover local migrations", e),
    };
    let current_schema = match build_schema_from_local_migrations(&local) {
        Ok(schema) => schema,
        Err(e) => {
            exit_with_marreta_runtime_error("failed to derive schema from local migrations", e)
        }
    };

    let drift = marreta::migrations::detect_schema_drift(&current_schema, &desired_tables);

    (
        entrypoint.to_path_buf(),
        migrations_dir,
        plan_migration(&current_schema, &desired_tables),
        drift,
    )
}

fn migrations_dir_for_project(entrypoint: &Path) -> PathBuf {
    entrypoint
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("migrations")
}

fn runtime_or_exit() -> tokio::runtime::Runtime {
    match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => exit_with_marreta_runtime_error("failed to create async runtime", e),
    }
}

fn apply_project_env_defaults(project_root: &Path) {
    for (key, value) in MarretaConfig::project_env_vars(project_root) {
        if std::env::var_os(&key).is_none() {
            // SAFETY: runs during single-threaded startup, before the async
            // runtime and any server worker threads exist, so no other thread
            // can observe the environment mid-mutation.
            unsafe {
                std::env::set_var(key, value);
            }
        }
    }
}

fn print_status_group(title: &str, items: &[String]) {
    println!("{}:", title);
    if items.is_empty() {
        println!("  none");
    } else {
        for item in items {
            println!("  {}", item);
        }
    }
    println!();
}

fn load_migration_state(
    entrypoint: &ProjectEntrypoint,
) -> (
    PathBuf,
    PathBuf,
    Vec<marreta::migrations::LocalMigration>,
    Vec<marreta::migrations::AppliedMigration>,
    MigrationStatusReport,
    Vec<MigrationListEntry>,
) {
    let entrypoint = entrypoint.path.as_path();
    let project_root = entrypoint.parent().unwrap_or_else(|| Path::new("."));
    let migrations_dir = migrations_dir_for_project(entrypoint);
    let local = match discover_local_migrations(&migrations_dir) {
        Ok(migrations) => migrations,
        Err(e) => exit_with_marreta_runtime_error("failed to discover local migrations", e),
    };
    let config = MarretaConfig::load_from_project_root(project_root);
    let rt = runtime_or_exit();
    let applied = match rt.block_on(async {
        marreta::db::ensure_migration_table_from_config(&config).await?;
        marreta::db::list_applied_migrations_from_config(&config).await
    }) {
        Ok(applied) => applied,
        Err(e) => exit_with_marreta_startup_error(e),
    };
    let report = compare_migration_state(&local, &applied);
    let inventory = build_migration_inventory(&local, &applied);
    (
        entrypoint.to_path_buf(),
        migrations_dir,
        local,
        applied,
        report,
        inventory,
    )
}

fn print_migration_list(entries: &[MigrationListEntry]) {
    println!("{:<16}  {:<18}  STATE", "VERSION", "NAME");
    if entries.is_empty() {
        println!("(no migrations)");
        return;
    }
    for entry in entries {
        println!(
            "{:<16}  {:<18}  {}",
            entry.version,
            entry.name,
            entry.state.as_str()
        );
    }
}

fn print_status_suggestions(report: &MigrationStatusReport, command_suffix: &str) {
    let mut lines = Vec::new();
    if !report.pending.is_empty() {
        lines.push(format!(
            "  - apply:   marreta migrate apply{}",
            command_suffix
        ));
        if let Some(version) = report.pending.first().map(|item| pending_version(item)) {
            lines.push(format!(
                "  - discard: marreta migrate discard {}{}",
                version, command_suffix
            ));
        }
    }
    if !report.changed.is_empty() {
        lines.push("  - restore the original migration file from version control".to_string());
        lines.push("  - do not edit applied migrations".to_string());
        lines.push("  - run: marreta migrate explain changed".to_string());
    }
    if !report.missing_local.is_empty() {
        lines.push("  - restore the missing migration files from version control".to_string());
        lines.push("  - run: marreta migrate explain missing_local".to_string());
    }
    if lines.is_empty() {
        return;
    }
    println!("Suggested actions:");
    for line in lines {
        println!("{}", line);
    }
    println!();
}

fn pending_version(item: &str) -> String {
    let mut parts = item.split('_');
    match (parts.next(), parts.next()) {
        (Some(date), Some(time)) => format!("{}_{}", date, time),
        _ => item.to_string(),
    }
}

fn print_migrate_explain_overview() {
    println!("Migration states:");
    println!();
    println!("Applied:");
    println!("  Migration exists locally and is recorded in _marreta_migrations.");
    println!();
    println!("Pending:");
    println!("  Migration exists locally but has not been applied.");
    println!("  Typical actions:");
    println!("    - apply it");
    println!("    - discard the local pending migration");
    println!("    - or revert the schema change and discard the local pending migration");
    println!();
    println!("Changed:");
    println!("  Migration was applied, but the local file checksum differs.");
    println!("  Typical action:");
    println!("    - restore the original migration file");
    println!();
    println!("Missing local:");
    println!("  Migration was applied, but the local file is missing.");
    println!("  Typical action:");
    println!("    - restore the missing migration file from version control");
}

fn print_migrate_explain_workflow() {
    println!("Migration workflow:");
    println!();
    println!("  no migration -> pending -> applied");
    println!("  applied -> pending           (rollback)");
    println!("  pending -> discarded         (discard)");
    println!("  applied -> changed -> applied");
    println!("  applied -> missing_local -> applied");
}

fn print_migrate_explain_applied() {
    println!("State: applied");
    println!();
    println!("Meaning:");
    println!(
        "  The migration exists locally and matches the applied record in _marreta_migrations."
    );
    println!();
    println!("Recommended actions:");
    println!("  - continue normally");
    println!("  - rollback the latest applied migration if you need to revert it");
}

fn print_migrate_explain_pending() {
    println!("State: pending");
    println!();
    println!("Meaning:");
    println!(
        "  The migration exists locally in migrations/, but has not been applied to this database."
    );
    println!();
    println!("Common causes:");
    println!("  - you just generated it");
    println!("  - you rolled it back");
    println!();
    println!("Recommended actions:");
    println!("  - apply it:");
    println!("      marreta migrate apply");
    println!("  - discard the local pending migration:");
    println!("      marreta migrate discard <version>");
    println!("  - or revert the schema change and then discard the local pending migration");
}

fn print_migrate_explain_changed() {
    println!("State: changed");
    println!();
    println!("Meaning:");
    println!("  The migration was already applied, but the local file content no longer matches");
    println!("  the checksum stored in _marreta_migrations.");
    println!();
    println!("Common causes:");
    println!("  - someone edited an applied migration file");
    println!("  - the local branch diverged from the applied history");
    println!();
    println!("Recommended actions:");
    println!("  - restore the original migration file from version control");
    println!("  - do not edit applied migrations");
}

fn print_migrate_explain_missing_local() {
    println!("State: missing_local");
    println!();
    println!("Meaning:");
    println!("  The migration was applied to this database, but the local file is missing.");
    println!();
    println!("Common causes:");
    println!("  - a migration file was deleted locally");
    println!("  - the local checkout is incomplete");
    println!();
    println!("Recommended actions:");
    println!("  - restore the missing migration files from version control");
    println!("  - do not apply or rollback further until the history is complete again");
}

fn parse_port_arg(args: &[String]) -> Option<u16> {
    for i in 0..args.len() {
        if args[i] == "--port" {
            return args.get(i + 1).and_then(|p| p.parse().ok());
        }
    }
    None
}

fn parse_doctor_args(args: &[String]) -> (bool, Option<&str>) {
    let mut connect = false;
    let mut path = None;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--connect" => {
                connect = true;
                i += 1;
            }
            arg if arg.starts_with("--") => {
                exit_with_marreta_cli_error(
                    "unknown doctor flag",
                    format!(
                        "'{}' is invalid. Use: marreta doctor [--connect] [app.marreta]",
                        arg
                    ),
                );
            }
            arg => {
                if path.is_some() {
                    exit_with_marreta_cli_error(
                        "invalid doctor usage",
                        "Usage: marreta doctor [--connect] [app.marreta]",
                    );
                }
                path = Some(arg);
                i += 1;
            }
        }
    }

    (connect, path)
}

#[derive(Debug, Clone)]
struct ProjectEntrypoint {
    path: PathBuf,
    explicit: bool,
}

impl ProjectEntrypoint {
    fn command_suffix(&self) -> String {
        if self.explicit {
            format!(" {}", self.path.display())
        } else {
            String::new()
        }
    }
}

fn extract_serve_path_arg(args: &[String]) -> Option<&str> {
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => i += 2,
            arg if arg.starts_with("--") => i += 1,
            _ => return Some(args[i].as_str()),
        }
    }
    None
}

fn resolve_project_entrypoint(path: Option<&str>) -> ProjectEntrypoint {
    let (entrypoint, explicit) = match path {
        Some(path) => (PathBuf::from(path), true),
        None => (
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("app.marreta"),
            false,
        ),
    };

    if !entrypoint.exists() {
        if explicit {
            exit_with_marreta_cli_error(
                "no Marreta project found",
                format!("expected project entrypoint at '{}'", entrypoint.display()),
            );
        } else {
            exit_with_marreta_cli_error(
                "no Marreta project found in the current directory",
                "expected ./app.marreta",
            );
        }
    }

    ProjectEntrypoint {
        path: entrypoint,
        explicit,
    }
}

// =============================================================================
// Debug commands (unadvertised: tokenize, parse — engine debugging only)
// =============================================================================

fn debug_tokenize(path: &str) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => exit_with_marreta_cli_error(&format!("failed to read '{}'", path), e),
    };

    match Lexer::new(&source).tokenize() {
        Ok(tokens) => {
            for token in &tokens {
                println!("{}", token);
            }
        }
        Err(e) => {
            exit_with_marreta_runtime_error("tokenization error", e);
        }
    }
}

fn debug_parse(path: &str) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => exit_with_marreta_cli_error(&format!("failed to read '{}'", path), e),
    };

    let tokens = match Lexer::new(&source).tokenize() {
        Ok(t) => t,
        Err(e) => exit_with_marreta_runtime_error("tokenization error", e),
    };

    match Parser::new(tokens).parse() {
        Ok(program) => {
            for (i, stmt) in program.iter().enumerate() {
                println!("[{}] {:?}", i, stmt);
            }
        }
        Err(e) => {
            exit_with_marreta_runtime_error("parse error", e);
        }
    }
}

// =============================================================================
// Help
// =============================================================================

fn print_help() {
    println!("{} — A DSL for REST APIs", runtime_version_label());
    println!();
    println!("Usage:");
    println!("  marreta init <project-path> [--with LIST]  Create a Marreta project");
    println!("  marreta fmt [--check] [path...] Format Marreta source files");
    println!("  marreta lint [--strict] [--format json] [path...]  Analyze Marreta source files");
    println!(
        "  marreta tooling <catalog|symbols|completions|hover> --format json  Editor tooling API"
    );
    println!(
        "  marreta serve [--port N]        Start HTTP server from the current project (default port: 8080)"
    );
    println!(
        "  marreta doctor [--connect]      Validate project structure, intent, config, and optional connectivity"
    );
    println!("  marreta test [path] [--filter TEXT] [--list] [--coverage]  Run API scenario tests");
    println!("  marreta migrate diff            Print planned migration SQL");
    println!("  marreta migrate generate        Write migration files");
    println!("  marreta migrate list            List migrations and their current state");
    println!("  marreta migrate status          Show applied and pending migrations");
    println!("  marreta migrate explain [state]  Explain migration states and workflow");
    println!("  marreta migrate discard <version>  Remove a pending local migration");
    println!("  marreta migrate apply           Apply pending migrations");
    println!("  marreta migrate rollback        Roll back the latest applied migration");
    println!("  marreta --version               Show version");
    println!("  marreta --help                  Show this help");
}

#[cfg(test)]
mod panic_hook_tests {
    use super::format_panic_message;

    fn assert_marreta_shape(out: &str, expected_msg: &str) {
        assert!(
            out.starts_with("[marreta] Internal error: "),
            "missing [marreta] prefix: {out}"
        );
        assert!(
            out.contains(expected_msg),
            "missing message {expected_msg:?} in: {out}"
        );
        assert!(
            out.contains("engine encountered"),
            "missing guidance line: {out}"
        );
        assert!(
            out.contains("github.com/marreta-lang"),
            "missing report link: {out}"
        );
        for leak in ["panicked at", "RUST_BACKTRACE", "src/", ".rs:"] {
            assert!(!out.contains(leak), "Rust internal leak {leak:?} in: {out}");
        }
    }

    #[test]
    fn format_panic_message_with_str_payload() {
        let payload: &dyn std::any::Any = &"boom";
        assert_marreta_shape(&format_panic_message(payload), "boom");
    }

    #[test]
    fn format_panic_message_with_string_payload() {
        let owned = String::from("internal invariant failed");
        let payload: &dyn std::any::Any = &owned;
        assert_marreta_shape(&format_panic_message(payload), "internal invariant failed");
    }

    #[test]
    fn format_panic_message_with_unknown_payload_falls_back() {
        let payload: &dyn std::any::Any = &42_i32;
        assert_marreta_shape(&format_panic_message(payload), "unexpected internal error");
    }
}
