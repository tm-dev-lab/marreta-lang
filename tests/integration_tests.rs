use std::io::Write;
use std::process::{Command, Stdio};

fn expected_runtime_label() -> String {
    format!("MarretaLang v{}", env!("CARGO_PKG_VERSION"))
}

// =============================================================================
// CLI tests
// =============================================================================

#[test]
fn test_version_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .arg("--version")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&expected_runtime_label()));
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn test_help_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .arg("--help")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!(
        "{} — A DSL for REST APIs",
        expected_runtime_label()
    )));
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("marreta serve"));
    assert!(stdout.contains("marreta init"));
    assert!(stdout.contains("marreta lint"));
    // Removed commands must not be advertised.
    assert!(!stdout.contains("marreta run"));
    assert!(!stdout.contains("marreta repl"));
    assert!(!stdout.contains("tokenize"));
    assert!(!stdout.contains("parse"));
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn test_bare_invocation_prints_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("marreta serve"));
    assert_eq!(output.status.code(), Some(0));
}

/// Writes `source` to a unique temp file and returns its path. Caller cleans up.
fn temp_marreta_file(source: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tid = std::thread::current().id();
    let path = std::env::temp_dir().join(format!("marreta_cli_{:?}_{}.marreta", tid, id));
    std::fs::write(&path, source).unwrap();
    path
}

// `tokenize` and `parse` are unadvertised debug commands (hidden from --help) but
// must stay callable and wired correctly for engine debugging. These smoke tests
// guard that wiring.

#[test]
fn test_tokenize_debug_command_still_callable() {
    let path = temp_marreta_file("route GET \"/x\"\n    reply 200, { ok: true }\n");
    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["tokenize", path.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout.contains("Identifier") || stdout.contains("at 1:1"));
}

#[test]
fn test_parse_debug_command_still_callable() {
    let path = temp_marreta_file("route GET \"/x\"\n    reply 200, { ok: true }\n");
    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["parse", path.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout.contains("[0]"));
}

#[test]
fn test_parse_debug_command_reports_error_nonzero() {
    let path = temp_marreta_file("route GET \"/x\"\n    reply 200, {\n");
    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["parse", path.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_ne!(output.status.code(), Some(0));
    assert!(stderr.contains("parse error") || stderr.contains("Error"));
}

#[test]
fn test_init_creates_project_scaffold() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("hello-api");

    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["init", project.to_str().unwrap()])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout.contains("Created Marreta project: hello-api"));
    assert!(stdout.contains("marreta serve"));
    assert!(stdout.contains("http://localhost:8080/greetings"));
    assert!(project.join("app.marreta").exists());
    assert!(project.join("routes/greetings.marreta").exists());
    assert!(project.join("schemas/greetings.marreta").exists());
    assert!(project.join("tasks/greetings.marreta").exists());
    assert!(project.join("tests/greetings_test.marreta").exists());
    assert!(project.join("marreta.env").exists());
    assert!(!project.join("Dockerfile").exists());
    assert!(!project.join("docker-compose.yml").exists());
    assert!(project.join("README.md").exists());

    let readme = std::fs::read_to_string(project.join("README.md")).unwrap();
    assert!(readme.contains("marreta serve"));
    assert!(readme.contains("http://localhost:8080/greetings"));
    assert!(!readme.contains("docker run --rm"));
}

#[test]
fn test_init_creates_service_scaffold() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("hello-api");

    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args([
            "init",
            project.to_str().unwrap(),
            "--with",
            "db,cache,doc,queue",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout.contains("Selected services: db, cache, doc, queue"));
    assert!(stdout.contains("docker compose up -d"));
    assert!(stdout.contains("marreta serve"));

    let env = std::fs::read_to_string(project.join("marreta.env")).unwrap();
    assert!(env.contains("MARRETA_DB_PROVIDER=postgres"));
    assert!(env.contains("MARRETA_DB_PASSWORD=marreta"));
    assert!(env.contains("MARRETA_CACHE_PROVIDER=redis"));
    assert!(env.contains("MARRETA_CACHE_PASSWORD=redis-secret"));
    assert!(env.contains("MARRETA_DOC_PROVIDER=mongodb"));
    assert!(env.contains("MARRETA_DOC_PASSWORD=marreta-secret"));
    assert!(env.contains("MARRETA_QUEUE_PROVIDER=rabbitmq"));
    assert!(env.contains("MARRETA_QUEUE_PASSWORD=guest"));

    let env_example = std::fs::read_to_string(project.join("marreta.env.example")).unwrap();
    assert!(env_example.contains("Safe to commit"));
    assert!(env_example.contains("MARRETA_DB_PASSWORD=change-me"));
    assert!(env_example.contains("MARRETA_CACHE_PASSWORD=change-me"));
    assert!(env_example.contains("MARRETA_DOC_PASSWORD=change-me"));
    assert!(env_example.contains("MARRETA_QUEUE_PASSWORD=change-me"));

    let compose = std::fs::read_to_string(project.join("docker-compose.yml")).unwrap();
    assert!(compose.contains("postgres:"));
    assert!(compose.contains("redis:"));
    assert!(compose.contains("--requirepass"));
    assert!(compose.contains("redis-secret"));
    assert!(compose.contains("mongodb:"));
    assert!(compose.contains("rabbitmq:"));
    assert!(!compose.contains("app:"));

    let readme = std::fs::read_to_string(project.join("README.md")).unwrap();
    assert!(readme.contains("Selected services: db, cache, doc, queue"));
    assert!(readme.contains("docker compose up -d"));
    assert!(readme.contains("db: PostgreSQL is available"));
    assert!(readme.contains("This requires Docker and Docker Compose"));
    assert!(readme.contains("placeholder credentials"));
    assert!(readme.contains("Example:"));
    assert!(readme.contains("```marreta\n  item = db.items.find(1)\n  ```"));
    assert!(readme.contains("docker compose down"));
    assert!(readme.contains("Point-to-point example:"));
    assert!(readme.contains("Topic example:"));
    assert!(readme.contains("queue.push \"greetings.created\""));
    assert!(readme.contains("topic.publish \"greetings.created\""));
    assert!(!readme.contains("migrate apply"));

    let tests_index = readme.find("## Tests").unwrap();
    let stop_index = readme.find("## Stop Services").unwrap();
    let layout_index = readme.find("## Project Layout").unwrap();
    assert!(tests_index < stop_index);
    assert!(stop_index < layout_index);
}

#[test]
fn test_init_rejects_unknown_service() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("hello-api");

    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["init", project.to_str().unwrap(), "--with", "redis"])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains("unknown init service 'redis'"));
    assert!(!project.exists());
}

#[test]
fn test_init_rejects_non_empty_directory() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("hello-api");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("existing.txt"), "content").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["init", project.to_str().unwrap()])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_ne!(output.status.code(), Some(0));
    assert!(stderr.contains("already exists and is not empty"));
}

#[test]
fn test_generated_project_scenario_tests_pass() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("hello-api");

    let init = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["init", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(init.status.code(), Some(0));

    let test = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .arg("test")
        .current_dir(&project)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&test.stdout);
    let stderr = String::from_utf8_lossy(&test.stderr);
    assert_eq!(
        test.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );
    assert!(stdout.contains("1 passed, 0 failed"));
}

#[test]
fn test_coverage_flag_reports_route_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("hello-api");

    let init = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["init", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(init.status.code(), Some(0));

    let out = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["test", "--coverage"])
        .current_dir(&project)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "stdout:\n{}", stdout);

    // Golden assertion for the full coverage block (headings, order, spacing, and
    // the uncovered section), so any change to the --coverage output is caught,
    // not just a few substrings. The timing footer moved to stderr (Spec 058), so
    // the coverage block now runs to the end of stdout.
    let start = stdout
        .find("API coverage:")
        .unwrap_or_else(|| panic!("no coverage block in:\n{}", stdout));
    let block = stdout[start..].trim_end();
    let expected = "\
API coverage:
  scenarios: 1 passed, 0 failed, 1 total
  assertions: 1 declared
  given: 0 declared
  routes covered: 1 / 1 (100.0%)

Covered routes:
  GET /greetings (1 scenario)

Uncovered routes:
  none";
    assert_eq!(block, expected, "coverage block changed:\n{}", block);
}

#[test]
fn test_doctor_reports_test_coverage_section() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("hello-api");

    let init = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["init", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(init.status.code(), Some(0));

    let out = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .arg("doctor")
        .current_dir(&project)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Tests:"), "stdout:\n{}", stdout);
    assert!(
        stdout.contains("scenarios declared: 1 across 1 files"),
        "stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("routes with a scenario: 1 / 1 (100.0%)"),
        "stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("routes without a scenario: 0"),
        "stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("run `marreta test --coverage`"),
        "stdout:\n{}",
        stdout
    );
    // The instruction line is rendered without a status tag.
    let pointer_line = stdout
        .lines()
        .find(|line| line.contains("run `marreta test --coverage`"))
        .expect("pointer line present");
    assert!(
        !pointer_line.contains("OK"),
        "pointer line should be untagged: {:?}",
        pointer_line
    );
    // Consolidated only: doctor does not list route paths in the Tests section.
    let tests_section: String = stdout
        .lines()
        .skip_while(|line| !line.starts_with("Tests:"))
        .take_while(|line| line.starts_with("Tests:") || line.starts_with("  "))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !tests_section.contains("/greetings"),
        "Tests section should not list routes:\n{}",
        tests_section
    );
}

#[test]
fn test_fmt_requires_project_root_without_paths() {
    let dir = tempfile::tempdir().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .arg("fmt")
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains("no app.marreta found"));
}

#[test]
fn test_fmt_formats_explicit_file_without_project_root() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("scratch.marreta");
    std::fs::write(
        &file,
        "# comment   \nroute GET \"/x\"\n  reply 200, { ok: true }   \n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["fmt", file.to_str().unwrap()])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );
    assert!(stdout.contains("Formatted 1 file."));
    let formatted = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        formatted,
        "# comment\nroute GET \"/x\"\n    reply 200, { ok: true }\n"
    );
}

#[test]
fn test_fmt_check_lists_unformatted_project_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("app.marreta"),
        "project_name = \"fmt-test\"\nproject_version = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("routes")).unwrap();
    std::fs::write(
        dir.path().join("routes/greetings.marreta"),
        "route GET \"/greetings\"\n  reply 200, { message: \"Hello\" }\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["fmt", "--check"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout.contains("FORMAT"));
    assert!(stdout.contains("routes/greetings.marreta"));
    assert!(stdout.contains("need formatting"));
}

#[test]
fn test_fmt_check_passes_on_formatted_project() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("app.marreta"),
        "project_name = \"fmt-test\"\nproject_version = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("routes")).unwrap();
    std::fs::write(
        dir.path().join("routes/greetings.marreta"),
        "route GET \"/greetings\"\n    reply 200, { message: \"Hello\" }\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["fmt", "--check"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );
    assert!(stdout.contains("All files formatted."));
}

#[test]
fn test_fmt_invalid_file_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("broken.marreta");
    let source = "route GET \"/broken\"\n  reply 200, null\n x = 1\n";
    std::fs::write(&file, source).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["fmt", file.to_str().unwrap()])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains("fmt failed"));
    assert!(stderr.contains("broken.marreta"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), source);
}

#[test]
fn test_fmt_stdin_formats_without_writing_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("buffer.marreta");
    std::fs::write(&file, "route GET \"/saved\"\n    reply 200, null\n").unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["fmt", "--stdin", "--file", file.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"route GET \"/buffer\"\n  reply 200, { ok: true }\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout,
        "route GET \"/buffer\"\n    reply 200, { ok: true }\n"
    );
    let saved = std::fs::read_to_string(&file).unwrap();
    assert_eq!(saved, "route GET \"/saved\"\n    reply 200, null\n");
}

#[test]
fn test_fmt_generated_project_still_passes_scenario_tests() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("fmt-app");

    let init = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["init", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(init.status.code(), Some(0));

    std::fs::write(
        project.join("routes/greetings.marreta"),
        "route GET \"/greetings\"\n  message = greetings.build_greeting(\"Marreta\")\n  reply 200 as GreetingResponse, { message: message }   \n",
    )
    .unwrap();

    let fmt = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .arg("fmt")
        .current_dir(&project)
        .output()
        .unwrap();
    assert_eq!(
        fmt.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&fmt.stdout),
        String::from_utf8_lossy(&fmt.stderr)
    );

    let check = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["fmt", "--check"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert_eq!(
        check.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );

    let test = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .arg("test")
        .current_dir(&project)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&test.stdout);
    let stderr = String::from_utf8_lossy(&test.stderr);
    assert_eq!(
        test.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );
    assert!(stdout.contains("1 passed, 0 failed"));
}

#[test]
fn test_lint_requires_project_root_without_paths() {
    let dir = tempfile::tempdir().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .arg("lint")
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains("no app.marreta found"));
}

#[test]
fn test_lint_clean_generated_project_passes() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("lint-app");

    let init = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["init", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(init.status.code(), Some(0));

    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .arg("lint")
        .current_dir(&project)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );
    assert!(stdout.contains("No lint diagnostics."));
}

#[test]
fn test_lint_warning_passes_without_strict_and_fails_with_strict() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("app.marreta"),
        "project_name = \"lint-test\"\nproject_version = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("routes")).unwrap();
    std::fs::write(
        dir.path().join("routes/warn.marreta"),
        "route GET \"/warn\"\n    reply 200, null\n    log.info(\"never\")\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .arg("lint")
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout.contains("warning unreachable_statement"));

    let strict = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["lint", "--strict"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&strict.stdout);
    assert_eq!(strict.status.code(), Some(1));
    assert!(stdout.contains("warning unreachable_statement"));
}

#[test]
fn test_lint_reports_unused_variable_warning() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("scratch.marreta");
    std::fs::write(
        &file,
        "route GET \"/x\"\n    message = \"unused\"\n    reply 200, null\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["lint", file.to_str().unwrap()])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout.contains("warning unused_variable"));
    assert!(stdout.contains("message"));
}

#[test]
fn test_lint_json_reports_stable_shape() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("scratch.marreta");
    std::fs::write(
        &file,
        "route GET \"/x\"\n    enabled = feature.enabled(\"Bad__Name\")\n    reply 200, enabled\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["lint", "--format", "json", file.to_str().unwrap()])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json[0]["severity"], "error");
    assert_eq!(json[0]["code"], "invalid_feature_flag_name");
    assert!(
        json[0]["file"]
            .as_str()
            .unwrap()
            .contains("scratch.marreta")
    );
}

#[test]
fn test_lint_stdin_reports_unsaved_buffer_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("buffer.marreta");
    std::fs::write(&file, "route GET \"/saved\"\n    reply 200, null\n").unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["lint", "--stdin", "--file", file.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"route GET \"/buffer\"\n    reply 200, null\n    log.info(\"never\")\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout.contains("warning unreachable_statement"));
    assert!(stdout.contains("buffer.marreta"));
}

#[test]
fn test_lint_stdin_uses_project_schema_context() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("app.marreta"),
        "project_name = \"lint-stdin\"\nproject_version = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("schemas")).unwrap();
    std::fs::write(
        dir.path().join("schemas/contracts.marreta"),
        "export schema GreetingResponse\n    message: string\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("routes")).unwrap();
    std::fs::write(
        dir.path().join("routes/greetings.marreta"),
        "route GET \"/saved\"\n    reply 200, { ok: true }\n",
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args([
            "lint",
            "--stdin",
            "--file",
            "routes/greetings.marreta",
            "--format",
            "json",
        ])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(
            b"route GET \"/greetings\"\n    reply 200 as GreetingResponse, { message: \"Hello\" }\n",
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(
        json.as_array()
            .unwrap()
            .iter()
            .all(|diag| diag["code"] != "unknown_schema_reference")
    );
}

#[test]
fn test_tooling_catalog_symbols_completions_and_hover() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("app.marreta"),
        "project_name = \"tooling-test\"\nproject_version = \"0.1.0\"\n\nroute GET \"/greetings\"\n    reply 200, { ok: true }\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("schemas")).unwrap();
    std::fs::write(
        dir.path().join("schemas/greetings.marreta"),
        "export schema GreetingResponse\n    message: string\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("tasks")).unwrap();
    std::fs::write(
        dir.path().join("tasks/greetings.marreta"),
        "export task greet(name)\n    \"Hello \" + name\n",
    )
    .unwrap();

    let catalog = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["tooling", "catalog", "--format", "json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(catalog.status.code(), Some(0));
    let catalog_json: serde_json::Value =
        serde_json::from_slice(&catalog.stdout).expect("catalog json");
    assert_eq!(catalog_json["version"], 1);
    assert!(
        catalog_json["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == "cache.get"
                && entry["examples"].as_array().unwrap().len() == 1)
    );

    let symbols = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["tooling", "symbols", "--format", "json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(symbols.status.code(), Some(0));
    let symbols_json: serde_json::Value =
        serde_json::from_slice(&symbols.stdout).expect("symbols json");
    assert!(
        symbols_json
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["kind"] == "schema" && symbol["name"] == "GreetingResponse")
    );

    let mut completions = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args([
            "tooling",
            "completions",
            "--stdin",
            "--file",
            "routes/greetings.marreta",
            "--line",
            "1",
            "--column",
            "15",
            "--format",
            "json",
        ])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    completions
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"value = cache.")
        .unwrap();
    let completions = completions.wait_with_output().unwrap();
    assert_eq!(completions.status.code(), Some(0));
    let completions_json: serde_json::Value =
        serde_json::from_slice(&completions.stdout).expect("completions json");
    assert!(
        completions_json
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["label"] == "get" && item["source"] == "builtin")
    );

    let mut schema_completions = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args([
            "tooling",
            "completions",
            "--stdin",
            "--file",
            "routes/greetings.marreta",
            "--line",
            "1",
            "--column",
            "34",
            "--format",
            "json",
        ])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    schema_completions
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"route POST \"/x\" take payload as ")
        .unwrap();
    let schema_completions = schema_completions.wait_with_output().unwrap();
    assert_eq!(schema_completions.status.code(), Some(0));
    let schema_completions_json: serde_json::Value =
        serde_json::from_slice(&schema_completions.stdout).expect("schema completions json");
    assert!(
        schema_completions_json
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["label"] == "GreetingResponse" && item["source"] == "project")
    );

    let mut hover = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args([
            "tooling",
            "hover",
            "--stdin",
            "--file",
            "routes/greetings.marreta",
            "--line",
            "1",
            "--column",
            "15",
            "--format",
            "json",
        ])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    hover
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"value = cache.get(\"x\")")
        .unwrap();
    let hover = hover.wait_with_output().unwrap();
    assert_eq!(hover.status.code(), Some(0));
    let hover_json: serde_json::Value = serde_json::from_slice(&hover.stdout).expect("hover json");
    assert!(
        hover_json["contents"][0]["value"]
            .as_str()
            .unwrap()
            .contains("cache.get")
    );

    let mut symbol_hover = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args([
            "tooling",
            "hover",
            "--stdin",
            "--file",
            "routes/greetings.marreta",
            "--line",
            "1",
            "--column",
            "11",
            "--format",
            "json",
        ])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    symbol_hover
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"result = greet(\"Thiago\")")
        .unwrap();
    let symbol_hover = symbol_hover.wait_with_output().unwrap();
    assert_eq!(symbol_hover.status.code(), Some(0));
    let symbol_hover_json: serde_json::Value =
        serde_json::from_slice(&symbol_hover.stdout).expect("symbol hover json");
    assert!(
        symbol_hover_json["contents"][0]["value"]
            .as_str()
            .unwrap()
            .contains("task greet")
    );
}

#[test]
fn test_lint_explicit_file_works_outside_project_root() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("scratch.marreta");
    std::fs::write(&file, "task loop() => loop()\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["lint", file.to_str().unwrap()])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout.contains("warning suspicious_self_recursive_task"));
}

#[test]
fn test_lint_reports_duplicate_route_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("app.marreta"),
        "project_name = \"lint-test\"\nproject_version = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("routes")).unwrap();
    std::fs::write(
        dir.path().join("routes/duplicates.marreta"),
        "route GET \"/same\"\n    reply 200, null\n\nroute GET \"/same\"\n    reply 200, null\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .arg("lint")
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout.contains("error duplicate_route"));
    assert!(stdout.contains("duplicate route GET /same"));
}

#[test]
fn test_unknown_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .arg("foobar")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cli_error: unknown command"));
    assert_ne!(output.status.code(), Some(0));
}

// The removed `run` and `repl` commands must be rejected as unknown by name, so a
// future re-introduction of either arm is caught directly (not only by the diff).
#[test]
fn test_removed_commands_are_unknown() {
    for cmd in ["run", "repl"] {
        let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
            .arg(cmd)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("cli_error: unknown command"),
            "`marreta {cmd}` should be an unknown command, got: {stderr}"
        );
        assert_ne!(output.status.code(), Some(0));
    }
}

// =============================================================================
// Command framing (Spec 058)
// =============================================================================

/// Creates a scratch project via `marreta init` and returns its path.
fn init_scratch_project() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("hello-api");
    let init = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["init", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(init.status.code(), Some(0));
    (dir, project)
}

#[test]
fn test_framed_command_frame_on_stderr_data_on_stdout() {
    let (_dir, project) = init_scratch_project();
    let out = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .arg("lint")
        .current_dir(&project)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    // The frame (header rule + footer with elapsed) is on stderr.
    assert!(
        stderr.contains("─── marreta lint"),
        "header missing on stderr:\n{stderr}"
    );
    assert!(stderr.contains('·'), "footer missing on stderr:\n{stderr}");
    // It never pollutes stdout, which carries only the command's data.
    assert!(!stdout.contains('─'), "frame leaked to stdout:\n{stdout}");
    assert!(
        stdout.contains("No lint diagnostics"),
        "body missing on stdout:\n{stdout}"
    );
}

#[test]
fn test_lint_json_mode_is_frame_free() {
    let (_dir, project) = init_scratch_project();
    let out = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["lint", "--format", "json"])
        .current_dir(&project)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    // Machine mode: no frame on either stream, and stdout is JSON.
    assert!(!stdout.contains('─'), "frame leaked to stdout:\n{stdout}");
    assert!(
        !stderr.contains('─'),
        "frame on stderr in json mode:\n{stderr}"
    );
    let trimmed = stdout.trim_start();
    assert!(
        trimmed.starts_with('[') || trimmed.starts_with('{'),
        "stdout is not json:\n{stdout}"
    );
}

#[test]
fn test_frame_is_plain_without_color() {
    let (_dir, project) = init_scratch_project();
    let out = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .arg("lint")
        .env("NO_COLOR", "1")
        .current_dir(&project)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    // No ANSI escape sequences: stderr is a pipe (non-TTY) and NO_COLOR is set.
    assert!(
        !stderr.contains('\u{1b}'),
        "ANSI escape in frame:\n{stderr}"
    );
    assert!(stderr.contains("─── marreta lint"));
}

#[test]
fn test_framed_command_argument_parse_failure_is_framed() {
    // A bad argument fails during parsing, before the command body runs. The frame
    // must still open and close (Spec 058): the failure is framed on stderr.
    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["fmt", "--unknown"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_ne!(output.status.code(), Some(0));
    assert!(
        stderr.contains("─── marreta fmt"),
        "header missing on framed parse failure:\n{stderr}"
    );
    assert!(
        stderr.contains('✗'),
        "failure footer missing on framed parse failure:\n{stderr}"
    );
    assert!(
        stderr.contains("invalid fmt usage"),
        "error missing on framed parse failure:\n{stderr}"
    );
}

#[test]
fn test_machine_mode_argument_failure_is_frame_free() {
    // `lint --stdin` without `--file` fails during parsing, but `--stdin` is a
    // machine mode, so the failure stays frame-free.
    let output = Command::new(env!("CARGO_BIN_EXE_marreta"))
        .args(["lint", "--stdin"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_ne!(output.status.code(), Some(0));
    assert!(
        !stderr.contains('─'),
        "frame leaked in machine-mode failure:\n{stderr}"
    );
}
