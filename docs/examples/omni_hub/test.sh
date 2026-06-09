#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Everything runs containerized against the pre-built marreta-lang:dev image
# (built by the marreta-lang repo). This suite never builds the runtime.
export MARRETA_IMAGE="${MARRETA_IMAGE:-marreta-lang:dev}"
export MARRETA_UID="$(id -u)"
export MARRETA_GID="$(id -g)"
BASE="http://127.0.0.1:3941"
SERVER_PID=""
SERVER_LOG=""

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

PASS=0
FAIL=0

cleanup() {
    echo ""
    if [[ -n "$SERVER_PID" ]]; then
        echo "Stopping marreta server (pid $SERVER_PID)…"
        kill "$SERVER_PID" 2>/dev/null || true
    fi
    echo "Stopping infrastructure…"
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" down --remove-orphans -v 2>/dev/null || true
    if [[ -n "$SERVER_LOG" && -f "$SERVER_LOG" ]]; then
        rm -f "$SERVER_LOG"
    fi
}
trap cleanup EXIT INT TERM

section() {
    echo ""
    echo -e "${CYAN}${BOLD}── $1 ──${RESET}"
}

check() {
    local name="$1"; shift
    local method="$1"; shift
    local path="$1"; shift
    local expected_status="$1"; shift
    local jq_expr="${1:-}"; shift || true
    local extra_args=("$@")
    local url="${BASE}${path}"
    local tmpfile
    tmpfile=$(mktemp)

    local actual_status
    actual_status=$(curl -s -o "$tmpfile" -w "%{http_code}" -X "$method" \
        "${extra_args[@]+"${extra_args[@]}"}" "$url" 2>/dev/null)

    local body
    body=$(cat "$tmpfile")
    rm -f "$tmpfile"

    if [[ "$actual_status" != "$expected_status" ]]; then
        echo -e "  ${RED}FAIL${RESET} ${name} — expected HTTP ${expected_status}, got ${actual_status}"
        echo "       body: ${body}"
        FAIL=$((FAIL + 1))
        return
    fi

    if [[ -n "$jq_expr" ]]; then
        if echo "$body" | jq -e "$jq_expr" >/dev/null 2>&1; then
            echo -e "  ${GREEN}PASS${RESET} ${name}"
            PASS=$((PASS + 1))
        else
            echo -e "  ${RED}FAIL${RESET} ${name} — jq assertion failed: ${jq_expr}"
            echo "       body: ${body}"
            FAIL=$((FAIL + 1))
        fi
    else
        echo -e "  ${GREEN}PASS${RESET} ${name}"
        PASS=$((PASS + 1))
    fi
}

post()   { check "$1" POST   "$2" "$3" "$4" -H "Content-Type: application/json" -d "$5"; }
patch()  { check "$1" PATCH  "$2" "$3" "$4" -H "Content-Type: application/json" -d "$5"; }
get()    { check "$1" GET    "$2" "$3" "$4"; }

sql() {
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" exec -T postgres \
        psql -U marreta -d omni_hub -Atqc "$1"
}

redis_cmd() {
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" exec -T redis redis-cli "$@"
}

mongo_eval() {
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" exec -T mongodb \
        mongosh omni_hub --quiet --eval "$1"
}

rabbitmq_queue_messages() {
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" exec -T rabbitmq \
        rabbitmqctl list_queues name messages 2>/dev/null
}

app_logs() {
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" logs --no-color app 2>/dev/null
}

wait_log_contains() {
    local pattern="$1"
    for _ in $(seq 1 40); do
        if app_logs | grep -F "$pattern" >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.5
    done
    return 1
}

echo -e "${BOLD}Omni Hub Example Tests${RESET}"
echo "Runtime: ${MARRETA_IMAGE} (containerized)"
echo ""

if ! docker image inspect "${MARRETA_IMAGE}" > /dev/null 2>&1; then
    echo "marreta image '${MARRETA_IMAGE}' not found; build it in the marreta-lang repo first" >&2
    echo "  cargo build --release && docker build -t ${MARRETA_IMAGE} ." >&2
    exit 1
fi

echo "Starting infrastructure…"
docker compose -f "${SCRIPT_DIR}/docker-compose.yml" up -d --wait postgres mongodb rabbitmq redis 2>&1 | tail -10

echo "Generating migrations when missing…"
if [[ ! -d "${SCRIPT_DIR}/migrations" ]] || [[ -z "$(find "${SCRIPT_DIR}/migrations" -maxdepth 1 -type f -name '*.up.sql' -print -quit)" ]]; then
    mkdir -p "${SCRIPT_DIR}/migrations"
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" run --rm -T app migrate generate >/dev/null
fi

echo "Applying migrations…"
docker compose -f "${SCRIPT_DIR}/docker-compose.yml" run --rm -T app migrate apply >/dev/null

echo "Starting marreta server…"
docker compose -f "${SCRIPT_DIR}/docker-compose.yml" up -d --wait app 2>&1 | tail -10

echo "Waiting for server…"
for i in $(seq 1 60); do
    if curl -sf "${BASE}/health" >/dev/null 2>&1; then
        echo "Server ready."
        break
    fi
    if [[ $i -eq 60 ]]; then
        echo -e "${RED}Server did not start in time.${RESET}"
        app_logs
        exit 1
    fi
    sleep 0.5
done

section "1. Setup"
post "customer create" "/customers" 201 '.name == "Ana Original"' '{"name":"Ana Original","email":"ana@example.com"}'

customer_id="$(sql "SELECT id FROM customers ORDER BY id DESC LIMIT 1;")"
if [[ -n "$customer_id" ]]; then
    echo -e "  ${GREEN}PASS${RESET} customer id captured (${customer_id})"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} customer id capture"
    FAIL=$((FAIL + 1))
fi

section "2. Order creation and topic"
post "order create returns open" "/orders" 201 '.status == "OPEN" and (.created_at | test("Z$")) and (.completed_at == null)' "{\"customer_id\": ${customer_id}, \"description\": \"Replace modem\", \"total_amount\": 149.9}"

order_id="$(sql "SELECT id FROM orders ORDER BY id DESC LIMIT 1;")"
if [[ -n "$order_id" ]]; then
    echo -e "  ${GREEN}PASS${RESET} order id captured (${order_id})"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} order id capture"
    FAIL=$((FAIL + 1))
fi

order_status="$(sql "SELECT status FROM orders WHERE id = ${order_id};")"
if [[ "$order_status" == "OPEN" ]]; then
    echo -e "  ${GREEN}PASS${RESET} order persisted in postgres"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} order persisted in postgres"
    FAIL=$((FAIL + 1))
fi

if wait_log_contains "topic order_created received order_id=${order_id}"; then
    echo -e "  ${GREEN}PASS${RESET} topic subscriber logged received event"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} topic subscriber log not found"
    FAIL=$((FAIL + 1))
fi

topic_seen="$(redis_cmd EXISTS "topic:order_created:${order_id}" | tail -n 1 | tr -d '\r')"
if [[ "$topic_seen" == "1" ]]; then
    echo -e "  ${GREEN}PASS${RESET} topic subscriber persisted delivery marker in redis"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} topic subscriber persisted delivery marker in redis"
    FAIL=$((FAIL + 1))
fi

section "3. Read-through cache"
get "first get returns order" "/orders/${order_id}" 200 ".id == ${order_id} and (.created_at | test(\"Z$\")) and (.completed_at == null)"
get "second get returns order" "/orders/${order_id}" 200 ".id == ${order_id} and (.created_at | test(\"Z$\")) and (.completed_at == null)"

if app_logs | grep -F "cache miss for order:${order_id}" >/dev/null 2>&1 && \
   app_logs | grep -F "db fetch for order:${order_id}" >/dev/null 2>&1 && \
   app_logs | grep -F "cache hit for order:${order_id}" >/dev/null 2>&1; then
    echo -e "  ${GREEN}PASS${RESET} logs show miss -> db fetch -> hit"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} logs do not show expected cache path"
    FAIL=$((FAIL + 1))
fi

redis_exists="$(redis_cmd EXISTS "order:${order_id}" | tail -n 1 | tr -d '\r')"
if [[ "$redis_exists" == "1" ]]; then
    echo -e "  ${GREEN}PASS${RESET} redis key created"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} redis key created"
    FAIL=$((FAIL + 1))
fi

section "4. Completion and audit"
patch "complete order closes it" "/orders/${order_id}/complete" 200 '.status == "CLOSED" and (.created_at | test("Z$")) and (.completed_at | test("Z$"))' '{}'

closed_status="$(sql "SELECT status FROM orders WHERE id = ${order_id};")"
if [[ "$closed_status" == "CLOSED" ]]; then
    echo -e "  ${GREEN}PASS${RESET} relational status closed"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} relational status closed"
    FAIL=$((FAIL + 1))
fi

snapshot_count="$(mongo_eval "db.order_audits.countDocuments({ order_id: ${order_id} })" | tail -n 1 | tr -d '\r')"
if [[ "$snapshot_count" == "1" ]]; then
    echo -e "  ${GREEN}PASS${RESET} audit snapshot created"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} audit snapshot created"
    FAIL=$((FAIL + 1))
fi

redis_after_complete="$(redis_cmd EXISTS "order:${order_id}" | tail -n 1 | tr -d '\r')"
if [[ "$redis_after_complete" == "0" ]]; then
    echo -e "  ${GREEN}PASS${RESET} cache invalidated on complete"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} cache invalidated on complete"
    FAIL=$((FAIL + 1))
fi

section "5. Billing queue"
queue_state="$(rabbitmq_queue_messages)"
if echo "$queue_state" | grep -E "^process_billing[[:space:]]+1$" >/dev/null 2>&1; then
    echo -e "  ${GREEN}PASS${RESET} billing message parked in queue"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} billing message parked in queue"
    echo "$queue_state" | sed 's/^/       /'
    FAIL=$((FAIL + 1))
fi

section "6. Snapshot immutability"
patch "rename customer after close" "/customers/${customer_id}/name" 200 '.name == "Ana Renamed"' '{"name":"Ana Renamed"}'
patch "rename missing customer returns 404" "/customers/999999/name" 404 '.error == "customer not found"' '{"name":"Nobody"}'
get "audit snapshot still uses original name" "/audits/orders/${order_id}" 200 '.customer_name == "Ana Original"'
get "audit snapshot carries temporal fields" "/audits/orders/${order_id}" 200 '(.created_at | test("Z$")) and (.completed_at | test("Z$"))'

echo ""
echo -e "${BOLD}Results:${RESET} ${GREEN}${PASS} passed${RESET}, ${RED}${FAIL} failed${RESET}"

if [[ "$FAIL" -gt 0 ]]; then
    echo ""
    echo "Server log:"
    app_logs | sed 's/^/  /'
    exit 1
fi
