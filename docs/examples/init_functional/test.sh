#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# This harness lives at docs/examples/init_functional, so the repo root is three
# levels up.
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
BIN="${REPO_ROOT}/target/debug/marreta"
WORKSPACE_DIR="$(mktemp -d /tmp/marreta-init-functional.XXXXXX)"
PROJECT_DIR="${WORKSPACE_DIR}/hello-api"
SERVICE_PROJECT_DIR="${WORKSPACE_DIR}/hello-services"
APP_PORT=3740
SERVICE_APP_PORT=3741
SERVER_PID=""
SERVICE_COMPOSE_STARTED=false

cleanup() {
    echo ""
    if [[ -n "${SERVER_PID}" ]]; then
        kill "${SERVER_PID}" >/dev/null 2>&1 || true
        wait "${SERVER_PID}" >/dev/null 2>&1 || true
    fi
    if [[ "${SERVICE_COMPOSE_STARTED}" == "true" ]] && command -v docker >/dev/null 2>&1 && [[ -d "${SERVICE_PROJECT_DIR}" ]]; then
        (
            cd "${SERVICE_PROJECT_DIR}"
            docker compose down -v >/dev/null 2>&1 || true
        )
    fi
    rm -rf "${WORKSPACE_DIR}" >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

fail() {
    echo "FAIL: $1" >&2
    exit 1
}

pass() {
    echo "PASS: $1"
}

assert_contains() {
    local haystack="$1"
    local needle="$2"
    local label="$3"
    if [[ "${haystack}" != *"${needle}"* ]]; then
        echo "${haystack}" >&2
        fail "${label} -- expected to contain: ${needle}"
    fi
}

assert_file_exists() {
    local path="$1"
    [[ -f "${path}" ]] || fail "missing file: ${path}"
}

assert_file_contains() {
    local path="$1"
    local needle="$2"
    assert_file_exists "${path}"
    assert_contains "$(cat "${path}")" "${needle}" "${path}"
}

run_in_project() {
    (
        cd "${PROJECT_DIR}"
        "$@"
    )
}

run_in_service_project() {
    (
        cd "${SERVICE_PROJECT_DIR}"
        "$@"
    )
}

stop_server() {
    if [[ -n "${SERVER_PID}" ]]; then
        kill "${SERVER_PID}" >/dev/null 2>&1 || true
        wait "${SERVER_PID}" >/dev/null 2>&1 || true
        SERVER_PID=""
    fi
}

echo "Marreta init functional test"
echo "Repo: ${REPO_ROOT}"
echo "Workspace: ${WORKSPACE_DIR}"
echo ""

echo "Building marreta binary..."
(
    cd "${REPO_ROOT}"
    cargo build --quiet
)
pass "cargo build"

echo ""
echo "Checking init argument validation..."
if "${BIN}" init "${WORKSPACE_DIR}/-hello" >/tmp/marreta-init-invalid.out 2>&1; then
    fail "init should reject project names starting with separator"
fi
assert_contains "$(cat /tmp/marreta-init-invalid.out)" "invalid project name '-hello'" "invalid project name"
pass "init rejects invalid project name"
rm -f /tmp/marreta-init-invalid.out

mkdir -p "${WORKSPACE_DIR}/non-empty"
printf "content\n" > "${WORKSPACE_DIR}/non-empty/existing.txt"
if "${BIN}" init "${WORKSPACE_DIR}/non-empty" >/tmp/marreta-init-non-empty.out 2>&1; then
    fail "init should reject non-empty directories"
fi
assert_contains "$(cat /tmp/marreta-init-non-empty.out)" "already exists and is not empty" "non-empty directory"
pass "init rejects non-empty directory"
rm -f /tmp/marreta-init-non-empty.out

echo ""
echo "Generating project..."
INIT_OUTPUT="$("${BIN}" init "${PROJECT_DIR}")"
assert_contains "${INIT_OUTPUT}" "Created Marreta project: hello-api" "init output"
assert_contains "${INIT_OUTPUT}" "marreta serve" "init output"
assert_contains "${INIT_OUTPUT}" "http://localhost:8080/greetings" "init output"
pass "init output"

echo ""
echo "Checking generated files..."
for file in \
    app.marreta \
    routes/greetings.marreta \
    schemas/greetings.marreta \
    tasks/greetings.marreta \
    tests/greetings_test.marreta \
    marreta.env \
    marreta.env.example \
    .gitignore \
    README.md \
    AGENTS.md \
    .github/copilot-instructions.md
do
    assert_file_exists "${PROJECT_DIR}/${file}"
done
if [[ -f "${PROJECT_DIR}/Dockerfile" || -f "${PROJECT_DIR}/docker-compose.yml" ]]; then
    fail "basic app should not generate Dockerfile or docker-compose.yml"
fi
pass "generated file tree"

assert_file_contains "${PROJECT_DIR}/app.marreta" 'project_name = "hello-api"'
assert_file_contains "${PROJECT_DIR}/app.marreta" 'requires_marreta = ">='
assert_file_contains "${PROJECT_DIR}/schemas/greetings.marreta" "export schema GreetingResponse"
assert_file_contains "${PROJECT_DIR}/tasks/greetings.marreta" "export task build_greeting(name)"
assert_file_contains "${PROJECT_DIR}/routes/greetings.marreta" 'route GET "/greetings"'
assert_file_contains "${PROJECT_DIR}/routes/greetings.marreta" "reply 200 as GreetingResponse"
assert_file_contains "${PROJECT_DIR}/tests/greetings_test.marreta" 'scenario "reads greeting"'
assert_file_contains "${PROJECT_DIR}/marreta.env" "MARRETA_REQUEST_LOG=true"
assert_file_contains "${PROJECT_DIR}/marreta.env.example" "Safe to commit"
assert_file_contains "${PROJECT_DIR}/marreta.env.example" "MARRETA_REQUEST_LOG=true"
assert_file_contains "${PROJECT_DIR}/marreta.env.example" "MARRETA_TRACE_CONTEXT=true"
assert_file_contains "${PROJECT_DIR}/README.md" "marreta serve"
assert_file_contains "${PROJECT_DIR}/README.md" "http://localhost:8080/greetings"
pass "generated file contents"

# Spec 078: the AI-agent guide is scaffolded by default, stamped with the runtime version,
# and the thin pointers point back to it.
assert_file_contains "${PROJECT_DIR}/AGENTS.md" "Generated for Marreta v"
assert_file_contains "${PROJECT_DIR}/AGENTS.md" "do not write it like Python"
assert_file_contains "${PROJECT_DIR}/AGENTS.md" 'route GET'
assert_file_contains "${PROJECT_DIR}/.github/copilot-instructions.md" "AGENTS.md"
pass "generated agent guide"

# Spec 078: --no-agents opts out, and `marreta agents` writes the set on demand.
NOAGENTS_DIR="${PROJECT_DIR}-noagents"
rm -rf "${NOAGENTS_DIR}"
"${BIN}" init "${NOAGENTS_DIR}" --no-agents >/dev/null
if [[ -f "${NOAGENTS_DIR}/AGENTS.md" ]]; then
    fail "--no-agents should not generate AGENTS.md"
fi
( cd "${NOAGENTS_DIR}" && "${BIN}" agents >/dev/null )
assert_file_exists "${NOAGENTS_DIR}/AGENTS.md"
assert_file_contains "${NOAGENTS_DIR}/AGENTS.md" "Generated for Marreta v"
assert_file_exists "${NOAGENTS_DIR}/.github/copilot-instructions.md"
rm -rf "${NOAGENTS_DIR}"
pass "--no-agents opt-out and marreta agents"

echo ""
echo "Running generated project checks..."
DOCTOR_OUTPUT="$(run_in_project "${BIN}" doctor)"
assert_contains "${DOCTOR_OUTPUT}" "project loads successfully" "doctor output"
pass "generated project doctor"

TEST_OUTPUT="$(run_in_project "${BIN}" test)"
assert_contains "${TEST_OUTPUT}" "1 passed, 0 failed" "test output"
pass "generated project scenario test"

echo ""
echo "Checking local runtime..."
(
    cd "${PROJECT_DIR}"
    "${BIN}" serve --port "${APP_PORT}" >/tmp/marreta-init-serve.log 2>&1
) &
SERVER_PID="$!"

ENDPOINT_OK=false
RESPONSE=""
for _ in $(seq 1 30); do
    if RESPONSE="$(curl -sf "http://127.0.0.1:${APP_PORT}/greetings" 2>/dev/null)"; then
        ENDPOINT_OK=true
        break
    fi
    sleep 0.2
done
if [[ "${ENDPOINT_OK}" != "true" ]]; then
    cat /tmp/marreta-init-serve.log >&2 || true
    fail "generated app did not serve /greetings"
fi
pass "marreta serve generated project"

assert_contains "${RESPONSE}" '"message":"Hello, Marreta!"' "docker curl response"
pass "curl generated endpoint"

stop_server

echo ""
echo "Generating service project..."
SERVICE_INIT_OUTPUT="$("${BIN}" init "${SERVICE_PROJECT_DIR}" --with db,cache,doc,queue)"
assert_contains "${SERVICE_INIT_OUTPUT}" "Selected services: db, cache, doc, queue" "service init output"
assert_contains "${SERVICE_INIT_OUTPUT}" "docker compose up -d" "service init output"
pass "service init output"

assert_file_contains "${SERVICE_PROJECT_DIR}/marreta.env" "MARRETA_DB_PROVIDER=postgres"
assert_file_contains "${SERVICE_PROJECT_DIR}/marreta.env" "MARRETA_DB_PASSWORD=marreta"
assert_file_contains "${SERVICE_PROJECT_DIR}/marreta.env" "MARRETA_CACHE_PROVIDER=redis"
assert_file_contains "${SERVICE_PROJECT_DIR}/marreta.env" "MARRETA_CACHE_PASSWORD=redis-secret"
assert_file_contains "${SERVICE_PROJECT_DIR}/marreta.env" "MARRETA_DOC_PROVIDER=mongodb"
assert_file_contains "${SERVICE_PROJECT_DIR}/marreta.env" "MARRETA_DOC_PASSWORD=marreta-secret"
assert_file_contains "${SERVICE_PROJECT_DIR}/marreta.env" "MARRETA_QUEUE_PROVIDER=rabbitmq"
assert_file_contains "${SERVICE_PROJECT_DIR}/marreta.env" "MARRETA_QUEUE_PASSWORD=guest"
assert_file_contains "${SERVICE_PROJECT_DIR}/marreta.env.example" "Safe to commit"
assert_file_contains "${SERVICE_PROJECT_DIR}/marreta.env.example" "MARRETA_DB_PASSWORD=change-me"
assert_file_contains "${SERVICE_PROJECT_DIR}/marreta.env.example" "MARRETA_CACHE_PASSWORD=change-me"
assert_file_contains "${SERVICE_PROJECT_DIR}/marreta.env.example" "MARRETA_DOC_PASSWORD=change-me"
assert_file_contains "${SERVICE_PROJECT_DIR}/marreta.env.example" "MARRETA_QUEUE_PASSWORD=change-me"
assert_file_contains "${SERVICE_PROJECT_DIR}/docker-compose.yml" "postgres:"
assert_file_contains "${SERVICE_PROJECT_DIR}/docker-compose.yml" "redis:"
assert_file_contains "${SERVICE_PROJECT_DIR}/docker-compose.yml" "--requirepass"
assert_file_contains "${SERVICE_PROJECT_DIR}/docker-compose.yml" "redis-secret"
assert_file_contains "${SERVICE_PROJECT_DIR}/docker-compose.yml" "mongodb:"
assert_file_contains "${SERVICE_PROJECT_DIR}/docker-compose.yml" "rabbitmq:"
assert_file_contains "${SERVICE_PROJECT_DIR}/README.md" "Selected Services"
assert_file_contains "${SERVICE_PROJECT_DIR}/README.md" "This requires Docker and Docker Compose"
assert_file_contains "${SERVICE_PROJECT_DIR}/README.md" "placeholder credentials"
assert_file_contains "${SERVICE_PROJECT_DIR}/README.md" "Example:"
assert_file_contains "${SERVICE_PROJECT_DIR}/README.md" '```marreta'
assert_file_contains "${SERVICE_PROJECT_DIR}/README.md" 'item = db.items.find(1)'
assert_file_contains "${SERVICE_PROJECT_DIR}/README.md" "docker compose down"
assert_file_contains "${SERVICE_PROJECT_DIR}/README.md" "Point-to-point example:"
assert_file_contains "${SERVICE_PROJECT_DIR}/README.md" "Topic example:"
assert_file_contains "${SERVICE_PROJECT_DIR}/README.md" 'queue.push "greetings.created"'
assert_file_contains "${SERVICE_PROJECT_DIR}/README.md" 'topic.publish "greetings.created"'
pass "service generated files"

echo ""
echo "Checking service project local runtime..."
if ! command -v docker >/dev/null 2>&1; then
    fail "docker command not found"
fi

if run_in_service_project docker compose up -d --wait; then
    SERVICE_COMPOSE_STARTED=true
    pass "docker compose up selected services"
else
    echo "WARN: docker compose up failed; trying existing local services from marreta.env"
fi

(
    cd "${SERVICE_PROJECT_DIR}"
    "${BIN}" serve --port "${SERVICE_APP_PORT}" >/tmp/marreta-init-service-serve.log 2>&1
) &
SERVER_PID="$!"

SERVICE_ENDPOINT_OK=false
SERVICE_RESPONSE=""
for _ in $(seq 1 60); do
    if SERVICE_RESPONSE="$(curl -sf "http://127.0.0.1:${SERVICE_APP_PORT}/greetings" 2>/dev/null)"; then
        SERVICE_ENDPOINT_OK=true
        break
    fi
    sleep 0.5
done
if [[ "${SERVICE_ENDPOINT_OK}" != "true" ]]; then
    cat /tmp/marreta-init-service-serve.log >&2 || true
    fail "service project did not serve /greetings"
fi
pass "service project marreta serve"

assert_contains "${SERVICE_RESPONSE}" '"message":"Hello, Marreta!"' "service curl response"
pass "service curl generated endpoint"

echo ""
echo "Results: init functional test passed"
