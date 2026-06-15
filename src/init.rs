use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitResult {
    pub project_path: PathBuf,
    pub project_name: String,
    pub docker_image_name: String,
    pub services: BTreeSet<InitService>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InitOptions {
    pub services: BTreeSet<InitService>,
    /// When true, skip scaffolding the AI-agent assets (AGENTS.md and its pointers).
    pub no_agents: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InitService {
    Db,
    Cache,
    Doc,
    Queue,
}

impl InitService {
    fn as_str(self) -> &'static str {
        match self {
            InitService::Db => "db",
            InitService::Cache => "cache",
            InitService::Doc => "doc",
            InitService::Queue => "queue",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "db" => Some(Self::Db),
            "cache" => Some(Self::Cache),
            "doc" => Some(Self::Doc),
            "queue" => Some(Self::Queue),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum InitError {
    InvalidProjectName(String),
    PathExistsAndIsFile(PathBuf),
    PathExistsAndIsNotEmpty(PathBuf),
    InvalidService(String),
    EmptyService,
    Io { path: PathBuf, source: io::Error },
}

impl std::fmt::Display for InitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InitError::InvalidProjectName(name) => write!(
                f,
                "invalid project name '{}'. Use letters, digits, hyphen, or underscore.",
                name
            ),
            InitError::PathExistsAndIsFile(path) => {
                write!(
                    f,
                    "project path '{}' exists and is not a directory",
                    path.display()
                )
            }
            InitError::PathExistsAndIsNotEmpty(path) => write!(
                f,
                "project path '{}' already exists and is not empty",
                path.display()
            ),
            InitError::InvalidService(name) => write!(
                f,
                "unknown init service '{}'. Supported: {}",
                name,
                supported_services()
            ),
            InitError::EmptyService => write!(f, "invalid empty init service in --with"),
            InitError::Io { path, source } => {
                write!(f, "could not write '{}': {}", path.display(), source)
            }
        }
    }
}

impl std::error::Error for InitError {}

pub fn init_project(project_path: impl AsRef<Path>) -> Result<InitResult, InitError> {
    init_project_with_options(project_path, InitOptions::default())
}

pub fn init_project_with_options(
    project_path: impl AsRef<Path>,
    options: InitOptions,
) -> Result<InitResult, InitError> {
    let project_path = project_path.as_ref();
    let project_name = derive_project_name(project_path)?;
    ensure_project_directory(project_path)?;

    let docker_image_name = docker_image_name(&project_name);
    let files = scaffold_files(&project_name, &docker_image_name, &options.services);

    for dir in ["routes", "schemas", "tasks", "tests"] {
        create_dir(project_path.join(dir))?;
    }

    for (relative_path, content) in files {
        write_file(project_path.join(relative_path), content)?;
    }

    if !options.no_agents {
        for (relative_path, content) in crate::agents::emitted_files() {
            write_file(project_path.join(relative_path), content)?;
        }
    }

    Ok(InitResult {
        project_path: project_path.to_path_buf(),
        project_name,
        docker_image_name,
        services: options.services,
    })
}

pub fn parse_services(value: &str) -> Result<BTreeSet<InitService>, InitError> {
    let mut services = BTreeSet::new();
    for raw in value.split(',') {
        let item = raw.trim();
        if item.is_empty() {
            return Err(InitError::EmptyService);
        }

        let Some(service) = InitService::parse(item) else {
            return Err(InitError::InvalidService(item.to_string()));
        };

        services.insert(service);
    }
    Ok(services)
}

fn supported_services() -> &'static str {
    "db, cache, doc, queue"
}

pub fn render_next_steps(result: &InitResult) -> String {
    let cd_path = result.project_path.display();
    let selected = selected_services(&result.services);
    let service_step = if result.services.is_empty() {
        String::new()
    } else {
        "  docker compose up -d --wait\n".to_string()
    };
    let selected_line = if result.services.is_empty() {
        String::new()
    } else {
        format!("\nSelected services: {selected}\n")
    };

    format!(
        r#"Created Marreta project: {project_name}
{selected_line}
Next steps:
  cd {cd_path}
{service_step}  marreta serve

Open:
  http://localhost:8080/greetings
"#,
        project_name = result.project_name
    )
}

fn derive_project_name(project_path: &Path) -> Result<String, InitError> {
    let Some(name) = project_path.file_name().and_then(|name| name.to_str()) else {
        return Err(InitError::InvalidProjectName(
            project_path.display().to_string(),
        ));
    };

    if is_valid_project_name(name) {
        Ok(name.to_string())
    } else {
        Err(InitError::InvalidProjectName(name.to_string()))
    }
}

fn is_valid_project_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn docker_image_name(project_name: &str) -> String {
    project_name.to_ascii_lowercase()
}

fn ensure_project_directory(project_path: &Path) -> Result<(), InitError> {
    if project_path.exists() {
        if !project_path.is_dir() {
            return Err(InitError::PathExistsAndIsFile(project_path.to_path_buf()));
        }
        let mut entries = fs::read_dir(project_path).map_err(|source| InitError::Io {
            path: project_path.to_path_buf(),
            source,
        })?;
        if entries.next().is_some() {
            return Err(InitError::PathExistsAndIsNotEmpty(
                project_path.to_path_buf(),
            ));
        }
        return Ok(());
    }

    create_dir(project_path.to_path_buf())
}

fn create_dir(path: PathBuf) -> Result<(), InitError> {
    fs::create_dir_all(&path).map_err(|source| InitError::Io { path, source })
}

fn write_file(path: PathBuf, content: String) -> Result<(), InitError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| InitError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(&path, content).map_err(|source| InitError::Io { path, source })
}

fn has(services: &BTreeSet<InitService>, service: InitService) -> bool {
    services.contains(&service)
}

fn scaffold_files(
    project_name: &str,
    docker_image_name: &str,
    services: &BTreeSet<InitService>,
) -> Vec<(&'static str, String)> {
    let mut files = vec![
        (
            "app.marreta",
            format!(
                r#"project_name = "{project_name}"
project_version = "0.1.0"
requires_marreta = ">={floor}"
"#,
                floor = crate::version::COMPAT_FLOOR
            ),
        ),
        (
            "schemas/greetings.marreta",
            r#"export schema GreetingResponse
    message: string
"#
            .to_string(),
        ),
        (
            "tasks/greetings.marreta",
            r#"export task build_greeting(name)
    "Hello, #{name}!"
"#
            .to_string(),
        ),
        (
            "routes/greetings.marreta",
            r#"route GET "/greetings"
    message = greetings.build_greeting("Marreta")
    reply 200 as GreetingResponse, { message: message }
"#
            .to_string(),
        ),
        (
            "tests/greetings_test.marreta",
            r#"scenario "reads greeting"
    when GET "/greetings"

    then response is {
        status: 200,
        body: {
            message: "Hello, Marreta!"
        }
    }
"#
            .to_string(),
        ),
        ("marreta.env", env_file(services)),
        ("marreta.env.example", env_example_file(services)),
        (
            ".gitignore",
            r#"marreta.env
target/
"#
            .to_string(),
        ),
        (
            "README.md",
            readme(project_name, docker_image_name, services),
        ),
    ];

    if !services.is_empty() {
        files.push(("docker-compose.yml", docker_compose(services)));
    }

    files
}

fn env_file(services: &BTreeSet<InitService>) -> String {
    let mut env = r#"MARRETA_HOST=0.0.0.0
MARRETA_PORT=8080
MARRETA_REQUEST_LOG=true
MARRETA_TRACE_CONTEXT=true
"#
    .to_string();

    if has(services, InitService::Db) {
        env.push_str(
            r#"
MARRETA_DB_PROVIDER=postgres
MARRETA_DB_HOST=127.0.0.1
MARRETA_DB_PORT=5432
MARRETA_DB_NAME=marreta
MARRETA_DB_USER=marreta
MARRETA_DB_PASSWORD=marreta
"#,
        );
    }
    if has(services, InitService::Cache) {
        env.push_str(
            r#"
MARRETA_CACHE_PROVIDER=redis
MARRETA_CACHE_HOST=127.0.0.1
MARRETA_CACHE_PORT=6379
MARRETA_CACHE_PASSWORD=redis-secret
"#,
        );
    }
    if has(services, InitService::Doc) {
        env.push_str(
            r#"
MARRETA_DOC_PROVIDER=mongodb
MARRETA_DOC_HOST=127.0.0.1
MARRETA_DOC_PORT=27017
MARRETA_DOC_NAME=marreta
MARRETA_DOC_USER=marreta
MARRETA_DOC_PASSWORD=marreta-secret
MARRETA_DOC_AUTH_SOURCE=admin
"#,
        );
    }
    if has(services, InitService::Queue) {
        env.push_str(
            r#"
MARRETA_QUEUE_PROVIDER=rabbitmq
MARRETA_QUEUE_HOST=127.0.0.1
MARRETA_QUEUE_PORT=5672
MARRETA_QUEUE_USER=guest
MARRETA_QUEUE_PASSWORD=guest
"#,
        );
    }

    env
}

fn env_example_file(services: &BTreeSet<InitService>) -> String {
    let mut env = r#"# Safe to commit. Replace placeholder credentials for real environments.
MARRETA_HOST=0.0.0.0
MARRETA_PORT=8080
MARRETA_REQUEST_LOG=true
MARRETA_TRACE_CONTEXT=true
"#
    .to_string();

    if has(services, InitService::Db) {
        env.push_str(
            r#"
# PostgreSQL
MARRETA_DB_PROVIDER=postgres
MARRETA_DB_HOST=127.0.0.1
MARRETA_DB_PORT=5432
MARRETA_DB_NAME=marreta
MARRETA_DB_USER=marreta
MARRETA_DB_PASSWORD=change-me
"#,
        );
    }
    if has(services, InitService::Cache) {
        env.push_str(
            r#"
# Redis
MARRETA_CACHE_PROVIDER=redis
MARRETA_CACHE_HOST=127.0.0.1
MARRETA_CACHE_PORT=6379
MARRETA_CACHE_PASSWORD=change-me
"#,
        );
    }
    if has(services, InitService::Doc) {
        env.push_str(
            r#"
# MongoDB
MARRETA_DOC_PROVIDER=mongodb
MARRETA_DOC_HOST=127.0.0.1
MARRETA_DOC_PORT=27017
MARRETA_DOC_NAME=marreta
MARRETA_DOC_USER=marreta
MARRETA_DOC_PASSWORD=change-me
MARRETA_DOC_AUTH_SOURCE=admin
"#,
        );
    }
    if has(services, InitService::Queue) {
        env.push_str(
            r#"
# RabbitMQ
MARRETA_QUEUE_PROVIDER=rabbitmq
MARRETA_QUEUE_HOST=127.0.0.1
MARRETA_QUEUE_PORT=5672
MARRETA_QUEUE_USER=guest
MARRETA_QUEUE_PASSWORD=change-me
"#,
        );
    }

    env
}

fn docker_compose(services: &BTreeSet<InitService>) -> String {
    if services.is_empty() {
        return "services: {}\n".to_string();
    }

    let mut compose = "services:\n".to_string();
    if has(services, InitService::Db) {
        compose.push_str(
            r#"  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_DB: marreta
      POSTGRES_USER: marreta
      POSTGRES_PASSWORD: marreta
    ports:
      - "5432:5432"
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U marreta -d marreta"]
      interval: 5s
      timeout: 5s
      retries: 20
"#,
        );
    }
    if has(services, InitService::Cache) {
        compose.push_str(
            r#"  redis:
    image: redis:7-alpine
    command: ["redis-server", "--requirepass", "redis-secret"]
    ports:
      - "6379:6379"
    healthcheck:
      test: ["CMD", "redis-cli", "-a", "redis-secret", "ping"]
      interval: 5s
      timeout: 5s
      retries: 20
"#,
        );
    }
    if has(services, InitService::Doc) {
        compose.push_str(
            r#"  mongodb:
    image: mongo:latest
    environment:
      MONGO_INITDB_DATABASE: marreta
      MONGO_INITDB_ROOT_USERNAME: marreta
      MONGO_INITDB_ROOT_PASSWORD: marreta-secret
    ports:
      - "27017:27017"
    healthcheck:
      test: ["CMD", "mongosh", "--quiet", "-u", "marreta", "-p", "marreta-secret", "--authenticationDatabase", "admin", "--eval", "db.adminCommand('ping').ok"]
      interval: 10s
      timeout: 5s
      retries: 20
"#,
        );
    }
    if has(services, InitService::Queue) {
        compose.push_str(
            r#"  rabbitmq:
    image: rabbitmq:4-management-alpine
    environment:
      RABBITMQ_DEFAULT_USER: guest
      RABBITMQ_DEFAULT_PASS: guest
    ports:
      - "5672:5672"
      - "15672:15672"
    healthcheck:
      test: ["CMD", "rabbitmq-diagnostics", "-q", "ping"]
      interval: 10s
      timeout: 5s
      retries: 20
"#,
        );
    }
    compose
}

fn selected_services(services: &BTreeSet<InitService>) -> String {
    if services.is_empty() {
        "none".to_string()
    } else {
        services
            .iter()
            .map(|service| service.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn selected_services_readme(services: &BTreeSet<InitService>) -> String {
    if services.is_empty() {
        return String::new();
    }

    let mut section = r#"
## Selected Services

These are local backing services selected with `--with`. Start them before
`marreta serve` so the providers configured in `marreta.env` are reachable.
This requires Docker and Docker Compose.

If you already run these services elsewhere (locally, in another Compose stack,
or in the cloud), skip `docker compose up` and edit `marreta.env` with the
correct hosts, ports, and credentials. The configured providers must match the
selected services. The cleanup command is shown after the tests.

"#
    .to_string();
    if has(services, InitService::Db) {
        section.push_str(
            r#"- db: PostgreSQL is available through the Marreta `db` namespace.

  Example:

  ```marreta
  item = db.items.find(1)
  ```
"#,
        );
    }
    if has(services, InitService::Cache) {
        section.push_str(
            r#"- cache: Redis is available through the Marreta `cache` namespace.

  Example:

  ```marreta
  cache.set("greeting", "Hello")
  ```
"#,
        );
    }
    if has(services, InitService::Doc) {
        section.push_str(
            r#"- doc: MongoDB is available through the Marreta `doc` namespace.

  Example:

  ```marreta
  doc.events.save({ kind: "greeting" })
  ```
"#,
        );
    }
    if has(services, InitService::Queue) {
        section.push_str(
            r#"- queue: RabbitMQ is available through Marreta queue producers, consumers, and topics.

  Point-to-point example:

  ```marreta
  queue.push "greetings.created", { message: "Hello" }
  ```

  Topic example:

  ```marreta
  topic.publish "greetings.created", { message: "Hello" }
  ```
"#,
        );
    }
    section
}

fn run_steps_readme(services: &BTreeSet<InitService>) -> String {
    if services.is_empty() {
        r#"```bash
marreta serve
```"#
            .to_string()
    } else {
        r#"Start selected local services. This requires Docker and Docker Compose:

```bash
docker compose up -d --wait
```

Start the app:

```bash
marreta serve
```"#
            .to_string()
    }
}

fn tests_readme(services: &BTreeSet<InitService>) -> &'static str {
    if services.is_empty() {
        r#"## Tests

```bash
marreta test
```"#
    } else {
        r#"## Tests

Tests do not require Docker or selected services.

```bash
marreta test
```"#
    }
}

fn cleanup_readme(services: &BTreeSet<InitService>) -> &'static str {
    if services.is_empty() {
        ""
    } else {
        r#"
## Stop Services

When you are done, stop selected local services:

```bash
docker compose down
```"#
    }
}

fn readme(
    project_name: &str,
    _docker_image_name: &str,
    services: &BTreeSet<InitService>,
) -> String {
    let selected = selected_services(services);
    let run_steps = run_steps_readme(services);
    let selected_services = selected_services_readme(services);
    let tests = tests_readme(services);
    let cleanup = cleanup_readme(services);
    let compose_layout = if services.is_empty() {
        ""
    } else {
        "- `docker-compose.yml` — selected local services\n"
    };

    format!(
        r#"# {project_name}

A MarretaLang project generated by `marreta init`.

Selected services: {selected}

`marreta.env` is generated with local defaults. Edit it if your environment
uses different hosts, ports, or credentials. `marreta.env.example` is safe to
commit and uses placeholder credentials for real environments.

## Run

{run_steps}

Open:

```text
http://localhost:8080/greetings
```

Expected response:

```json
{{
  "message": "Hello, Marreta!"
}}
```
{selected_services}
{tests}
{cleanup}

## Project Layout

- `app.marreta` — project metadata and entrypoint
- `routes/` — HTTP routes
- `schemas/` — request and response contracts
- `tasks/` — reusable application logic
- `tests/` — tests run by `marreta test`
- `marreta.env` — local runtime configuration
{compose_layout}"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_project_name() {
        let err = init_project("bad/name!").unwrap_err();
        assert!(matches!(err, InitError::InvalidProjectName(_)));
    }

    #[test]
    fn rejects_project_name_starting_with_separator() {
        let err = init_project("-hello").unwrap_err();
        assert!(matches!(err, InitError::InvalidProjectName(_)));
    }

    #[test]
    fn rejects_existing_non_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("hello-api");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("existing.txt"), "content").unwrap();

        let err = init_project(&project).unwrap_err();
        assert!(matches!(err, InitError::PathExistsAndIsNotEmpty(_)));
    }

    #[test]
    fn creates_expected_project_files() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("hello-api");

        let result = init_project(&project).unwrap();

        assert_eq!(result.project_name, "hello-api");
        assert_eq!(result.docker_image_name, "hello-api");
        for path in [
            "app.marreta",
            "routes/greetings.marreta",
            "schemas/greetings.marreta",
            "tasks/greetings.marreta",
            "tests/greetings_test.marreta",
            "marreta.env",
            "marreta.env.example",
            ".gitignore",
            "README.md",
        ] {
            assert!(project.join(path).exists(), "missing {path}");
        }
        assert!(!project.join("Dockerfile").exists());
        assert!(!project.join("docker-compose.yml").exists());
    }

    #[test]
    fn writes_expected_scaffold_content() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("Hello_API");

        let result = init_project(&project).unwrap();

        assert_eq!(result.project_name, "Hello_API");
        assert_eq!(result.docker_image_name, "hello_api");
        assert_eq!(
            fs::read_to_string(project.join(".gitignore")).unwrap(),
            "marreta.env\ntarget/\n"
        );
        assert!(
            fs::read_to_string(project.join("app.marreta"))
                .unwrap()
                .contains("project_name = \"Hello_API\"")
        );
        assert!(
            fs::read_to_string(project.join("README.md"))
                .unwrap()
                .contains("http://localhost:8080/greetings")
        );
    }

    #[test]
    fn parses_service_list() {
        let services = parse_services("db, cache,db").unwrap();

        assert!(services.contains(&InitService::Db));
        assert!(services.contains(&InitService::Cache));
        assert_eq!(services.len(), 2);
    }

    #[test]
    fn rejects_invalid_service_list() {
        let err = parse_services("db,,queue").unwrap_err();
        assert!(matches!(err, InitError::EmptyService));

        let err = parse_services("redis").unwrap_err();
        assert!(matches!(err, InitError::InvalidService(_)));
        assert!(err.to_string().contains("Supported: db, cache"));
    }

    #[test]
    fn creates_service_scaffold() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("hello-api");
        let services = parse_services("db,cache,doc,queue").unwrap();

        let result = init_project_with_options(
            &project,
            InitOptions {
                services: services.clone(),
                no_agents: false,
            },
        )
        .unwrap();

        assert_eq!(result.services, services);

        let env = fs::read_to_string(project.join("marreta.env")).unwrap();
        assert!(env.contains("MARRETA_DB_PROVIDER=postgres"));
        assert!(env.contains("MARRETA_DB_PASSWORD=marreta"));
        assert!(env.contains("MARRETA_CACHE_PROVIDER=redis"));
        assert!(env.contains("MARRETA_CACHE_PASSWORD=redis-secret"));
        assert!(env.contains("MARRETA_DOC_PROVIDER=mongodb"));
        assert!(env.contains("MARRETA_DOC_PASSWORD=marreta-secret"));
        assert!(env.contains("MARRETA_QUEUE_PROVIDER=rabbitmq"));
        assert!(env.contains("MARRETA_QUEUE_PASSWORD=guest"));

        let env_example = fs::read_to_string(project.join("marreta.env.example")).unwrap();
        assert!(env_example.contains("Safe to commit"));
        assert!(env_example.contains("MARRETA_DB_PASSWORD=change-me"));
        assert!(env_example.contains("MARRETA_CACHE_PASSWORD=change-me"));
        assert!(env_example.contains("MARRETA_DOC_PASSWORD=change-me"));
        assert!(env_example.contains("MARRETA_QUEUE_PASSWORD=change-me"));

        let compose = fs::read_to_string(project.join("docker-compose.yml")).unwrap();
        assert!(compose.contains("postgres:"));
        assert!(compose.contains("redis:"));
        assert!(compose.contains("--requirepass"));
        assert!(compose.contains("redis-secret"));
        assert!(compose.contains("mongodb:"));
        // The mongo healthcheck must authenticate so `docker compose up --wait` does not
        // report healthy during MongoDB's first-run init window (Spec 065).
        assert!(compose.contains("--authenticationDatabase"));
        assert!(compose.contains("rabbitmq:"));
        assert!(!compose.contains("app:"));

        let readme = fs::read_to_string(project.join("README.md")).unwrap();
        assert!(readme.contains("Selected services: db, cache, doc, queue"));
        assert!(readme.contains("docker compose up -d"));
        assert!(readme.contains("marreta serve"));
        assert!(readme.contains("This requires Docker and Docker Compose"));
        assert!(readme.contains("placeholder credentials"));
        assert!(readme.contains("db: PostgreSQL is available"));
        assert!(readme.contains("Example:"));
        assert!(readme.contains("```marreta\n  item = db.items.find(1)\n  ```"));
        assert!(readme.contains("Point-to-point example:"));
        assert!(readme.contains("Topic example:"));
        assert!(readme.contains("## Stop Services"));
        assert!(!readme.contains("migrate apply"));
    }

    #[test]
    fn next_steps_include_local_and_container_paths() {
        let result = InitResult {
            project_path: PathBuf::from("hello-api"),
            project_name: "hello-api".to_string(),
            docker_image_name: "hello-api".to_string(),
            services: BTreeSet::new(),
        };

        let output = render_next_steps(&result);

        assert!(output.contains("marreta serve"));
        assert!(output.contains("http://localhost:8080/greetings"));
        assert!(!output.contains("docker compose up -d"));
    }
}
