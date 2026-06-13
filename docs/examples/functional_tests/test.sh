#!/usr/bin/env bash
# ── MarretaLang — Functional Test Runner ─────────────────────────────────────
#
# Starts postgres + mongodb (via docker compose), builds + starts the marreta
# server, then exercises every route in the routes/ directory via curl.
#
# Usage:
#   cd examples/functional_tests
#   ./test.sh              # local binary (requires cargo)
#   ./test.sh --docker     # fully containerized (builds Docker image, no cargo needed)
#
# Prerequisites:
#   - docker / docker compose
#   - cargo (local mode only)
#   - curl, jq
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLES_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
# Everything runs containerized against the pre-built marreta-lang:dev image
# (built by the marreta-lang repo). This suite never builds the runtime.
export MARRETA_IMAGE="${MARRETA_IMAGE:-marreta-lang:dev}"
BASE="http://127.0.0.1:3737"
SERVER_PID=""
SERVER_LOG=""
DOCKER_MODE=true

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

PASS=0
FAIL=0

# ── cleanup ──────────────────────────────────────────────────────────────────

cleanup() {
    echo ""
    if [[ -n "$SERVER_PID" ]]; then
        echo "Stopping marreta server (pid $SERVER_PID)…"
        kill "$SERVER_PID" 2>/dev/null || true
    fi
    if [[ "$DOCKER_MODE" == "true" ]]; then
        echo "Stopping containers…"
        docker compose -f "${SCRIPT_DIR}/docker-compose.yml" down --remove-orphans 2>/dev/null || true
    else
        echo "Stopping postgres…"
        docker compose -f "${SCRIPT_DIR}/docker-compose.yml" stop postgres 2>/dev/null || true
        docker compose -f "${SCRIPT_DIR}/docker-compose.yml" rm -f postgres 2>/dev/null || true
        echo "Stopping mongodb…"
        docker compose -f "${SCRIPT_DIR}/docker-compose.yml" stop mongodb 2>/dev/null || true
        docker compose -f "${SCRIPT_DIR}/docker-compose.yml" rm -f mongodb 2>/dev/null || true
        echo "Stopping rabbitmq…"
        docker compose -f "${SCRIPT_DIR}/docker-compose.yml" stop rabbitmq 2>/dev/null || true
        docker compose -f "${SCRIPT_DIR}/docker-compose.yml" rm -f rabbitmq 2>/dev/null || true
        echo "Stopping redis…"
        docker compose -f "${SCRIPT_DIR}/docker-compose.yml" stop redis 2>/dev/null || true
        docker compose -f "${SCRIPT_DIR}/docker-compose.yml" rm -f redis 2>/dev/null || true
    fi
    if [[ -n "$SERVER_LOG" && -f "$SERVER_LOG" ]]; then
        rm -f "$SERVER_LOG"
    fi
}
trap cleanup EXIT INT TERM

# ── helpers ──────────────────────────────────────────────────────────────────

section() {
    echo ""
    echo -e "${CYAN}${BOLD}── $1 ──${RESET}"
}

# check NAME METHOD PATH [EXTRA_CURL_ARGS...] EXPECTED_STATUS EXPECTED_JQ_EXPR
# EXPECTED_JQ_EXPR is evaluated with `jq -e`; exit 0 means pass.
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

    # Status check
    if [[ "$actual_status" != "$expected_status" ]]; then
        echo -e "  ${RED}FAIL${RESET} ${name} — expected HTTP ${expected_status}, got ${actual_status}"
        echo -e "       body: ${body}"
        FAIL=$((FAIL + 1))
        return
    fi

    # Optional jq assertion
    if [[ -n "$jq_expr" ]]; then
        if echo "$body" | jq -e "$jq_expr" > /dev/null 2>&1; then
            echo -e "  ${GREEN}PASS${RESET} ${name}"
            PASS=$((PASS + 1))
        else
            echo -e "  ${RED}FAIL${RESET} ${name} — jq assertion failed: ${jq_expr}"
            echo -e "       body: ${body}"
            FAIL=$((FAIL + 1))
        fi
    else
        echo -e "  ${GREEN}PASS${RESET} ${name}"
        PASS=$((PASS + 1))
    fi
}

# Helpers for common content-type headers
H_JSON=(-H "Content-Type: application/json")
H_ACCEPT_HTML=(-H "Accept: text/html")

post()   { check "$1" POST   "$2" "$3" "$4" "${H_JSON[@]}" -d "$5"; }
put()    { check "$1" PUT    "$2" "$3" "$4" "${H_JSON[@]}" -d "$5"; }
patch()  { check "$1" PATCH  "$2" "$3" "$4" "${H_JSON[@]}" -d "$5"; }
get()    { check "$1" GET    "$2" "$3" "$4"; }
delete() { check "$1" DELETE "$2" "$3" "$4"; }

mongo_eval() {
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" exec -T mongodb \
        mongosh -u marreta -p marreta-secret --authenticationDatabase admin --quiet \
        marreta_functional --eval "$1"
}

current_app_logs() {
    if [[ "$DOCKER_MODE" == "true" ]]; then
        docker compose -f "${SCRIPT_DIR}/docker-compose.yml" logs --no-color app 2>/dev/null || true
    elif [[ -n "$SERVER_LOG" && -f "$SERVER_LOG" ]]; then
        cat "$SERVER_LOG"
    fi
}

wait_for_log_pattern() {
    local name="$1"; shift
    local pattern="$1"; shift

    for _ in $(seq 1 20); do
        if current_app_logs | grep -F "$pattern" >/dev/null 2>&1; then
            echo -e "  ${GREEN}PASS${RESET} ${name}"
            PASS=$((PASS + 1))
            return
        fi
        sleep 0.2
    done

    echo -e "  ${RED}FAIL${RESET} ${name} — missing log pattern: ${pattern}"
    FAIL=$((FAIL + 1))
}

wait_for_log_fields() {
    local name="$1"; shift

    for _ in $(seq 1 20); do
        local logs
        logs="$(current_app_logs)"
        local matches="$logs"
        local pattern
        for pattern in "$@"; do
            matches="$(printf '%s\n' "$matches" | grep -F "$pattern" || true)"
        done
        if [[ -n "$matches" ]]; then
            echo -e "  ${GREEN}PASS${RESET} ${name}"
            PASS=$((PASS + 1))
            return
        fi
        sleep 0.2
    done

    echo -e "  ${RED}FAIL${RESET} ${name} — missing log fields: $*"
    FAIL=$((FAIL + 1))
}

wait_for_request_log_without_route() {
    local name="$1"; shift
    local method="$1"; shift
    local path="$1"; shift
    local status="$1"; shift

    for _ in $(seq 1 20); do
        local line
        line="$(current_app_logs | grep -F '"kind":"request"' | grep -F "\"method\":\"${method}\"" | grep -F "\"path\":\"${path}\"" | grep -F "\"status\":${status}" | tail -n 1 || true)"
        if [[ -n "$line" ]]; then
            if [[ "$line" == *'"route":'* ]]; then
                echo -e "  ${RED}FAIL${RESET} ${name} — route should be absent in: ${line}"
                FAIL=$((FAIL + 1))
            else
                echo -e "  ${GREEN}PASS${RESET} ${name}"
                PASS=$((PASS + 1))
            fi
            return
        fi
        sleep 0.2
    done

    echo -e "  ${RED}FAIL${RESET} ${name} — missing request log for ${method} ${path} ${status}"
    FAIL=$((FAIL + 1))
}

run_marreta_cmd() {
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" run --rm app "$@"
}

sql_postgres() {
    local query="$1"
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" exec -T postgres \
        psql -U marreta -d marreta -Atqc "$query"
}

seed_relation_navigation_data() {
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" exec -T postgres \
        psql -U marreta -d marreta -v ON_ERROR_STOP=1 >/dev/null <<'SQL'
TRUNCATE TABLE orders RESTART IDENTITY CASCADE;
TRUNCATE TABLE users RESTART IDENTITY CASCADE;
TRUNCATE TABLE addresses RESTART IDENTITY CASCADE;

INSERT INTO addresses (city, zipcode) VALUES
    ('Sao Paulo', '01310-100'),
    ('Rio de Janeiro', '20000-000');

INSERT INTO users (name, address_id) VALUES
    ('Ana', 1),
    ('Bruno', NULL),
    ('Carla', 2);

INSERT INTO orders (total, customer_id) VALUES
    (99.5, 1),
    (42.0, 1),
    (10.0, 2),
    (150.0, 3);
SQL
}

# ── startup ──────────────────────────────────────────────────────────────────

echo -e "${BOLD}MarretaLang Functional Tests${RESET}"
echo "Examples: ${EXAMPLES_ROOT}"
echo "Runtime: ${MARRETA_IMAGE} (containerized)"
echo ""

if ! docker image inspect "${MARRETA_IMAGE}" > /dev/null 2>&1; then
    echo "marreta image '${MARRETA_IMAGE}' not found; build it in the marreta-lang repo first" >&2
    echo "  cargo build --release && docker build -t ${MARRETA_IMAGE} ." >&2
    exit 1
fi

# Defensive cleanup: stale app container from previous runs.
docker compose -f "${SCRIPT_DIR}/docker-compose.yml" rm -sf app 2>/dev/null || true

echo "Starting infrastructure containers…"
docker compose -f "${SCRIPT_DIR}/docker-compose.yml" up -d --wait postgres mongodb rabbitmq redis 2>&1 | tail -10

echo "Applying migrations for db: schemas…"
migrate_apply_output="$(run_marreta_cmd migrate apply)"
if [[ "${migrate_apply_output}" == *"Applied "* ]] || [[ "${migrate_apply_output}" == *"No pending migrations."* ]]; then
    echo -e "  ${GREEN}PASS${RESET} migrations — apply relation tables"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} migrations — apply relation tables"
    echo "${migrate_apply_output}" | sed 's/^/       /'
    FAIL=$((FAIL + 1))
fi

addresses_columns="$(sql_postgres "SELECT column_name FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'addresses' ORDER BY ordinal_position;")"
users_columns="$(sql_postgres "SELECT column_name FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'users' ORDER BY ordinal_position;")"
orders_columns="$(sql_postgres "SELECT column_name FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'orders' ORDER BY ordinal_position;")"
users_fk="$(sql_postgres "SELECT constraint_name FROM information_schema.table_constraints WHERE table_schema = 'public' AND table_name = 'users' AND constraint_type = 'FOREIGN KEY' ORDER BY constraint_name;")"
orders_fk="$(sql_postgres "SELECT constraint_name FROM information_schema.table_constraints WHERE table_schema = 'public' AND table_name = 'orders' AND constraint_type = 'FOREIGN KEY' ORDER BY constraint_name;")"

if [[ "${addresses_columns}" == *"id"* && "${addresses_columns}" == *"city"* && "${addresses_columns}" == *"zipcode"* ]]; then
    echo -e "  ${GREEN}PASS${RESET} migrations — addresses table created"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} migrations — addresses table created"
    echo "${addresses_columns}" | sed 's/^/       /'
    FAIL=$((FAIL + 1))
fi

if [[ "${users_columns}" == *"id"* && "${users_columns}" == *"name"* && "${users_columns}" == *"address_id"* ]]; then
    echo -e "  ${GREEN}PASS${RESET} migrations — users table and address_id created"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} migrations — users table and address_id created"
    echo "${users_columns}" | sed 's/^/       /'
    FAIL=$((FAIL + 1))
fi

if [[ "${orders_columns}" == *"id"* && "${orders_columns}" == *"total"* && "${orders_columns}" == *"customer_id"* ]]; then
    echo -e "  ${GREEN}PASS${RESET} migrations — orders table and customer_id created"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} migrations — orders table and customer_id created"
    echo "${orders_columns}" | sed 's/^/       /'
    FAIL=$((FAIL + 1))
fi

if [[ "${orders_fk}" == *"fk_orders_customer_id"* ]]; then
    echo -e "  ${GREEN}PASS${RESET} migrations — orders.customer_id foreign key created"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} migrations — orders.customer_id foreign key created"
    echo "${orders_fk}" | sed 's/^/       /'
    FAIL=$((FAIL + 1))
fi

if [[ "${users_fk}" == *"fk_users_address_id"* ]]; then
    echo -e "  ${GREEN}PASS${RESET} migrations — users.address_id foreign key created"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} migrations — users.address_id foreign key created"
    echo "${users_fk}" | sed 's/^/       /'
    FAIL=$((FAIL + 1))
fi

echo "Seeding relation-navigation rows…"
seed_relation_navigation_data

echo "Starting marreta server…"
docker compose -f "${SCRIPT_DIR}/docker-compose.yml" up -d --wait app 2>&1 | tail -10

echo "Waiting for server to be ready…"
for i in $(seq 1 60); do
    if curl -sf "${BASE}/types/arithmetic" > /dev/null 2>&1; then
        echo "Server ready."
        break
    fi
    if [[ $i -eq 60 ]]; then
        echo -e "${RED}Server did not start in time.${RESET}"
        exit 1
    fi
    sleep 0.5
done

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 1 — Types & arithmetic
# ═══════════════════════════════════════════════════════════════════════════════
section "1. Types & arithmetic"

get  "arithmetic — sum"       "/types/arithmetic" 200 '.sum == 13'
get  "arithmetic — diff"      "/types/arithmetic" 200 '.diff == 7'
get  "arithmetic — product"   "/types/arithmetic" 200 '.product == 30'
get  "arithmetic — quotient"  "/types/arithmetic" 200 '.quotient == 3'
get  "arithmetic — remainder" "/types/arithmetic" 200 '.remainder == 1'

get  "float — sum"      "/types/float" 200 '.sum == 4.0'
get  "float — division" "/types/float" 200 '(.division > 3.33) and (.division < 3.34)'

get  "boolean — values"   "/types/boolean" 200 '.a == true and .b == false'
get  "boolean — and"      "/types/boolean" 200 '.a_and_b == false'
get  "boolean — or"       "/types/boolean" 200 '.a_or_b == true'
get  "boolean — not"      "/types/boolean" 200 '.not_a == false'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 2 — String methods
# ═══════════════════════════════════════════════════════════════════════════════
section "2. String methods"

get  "strings — upper"    "/strings/methods" 200 '.upper == "  HELLO, WORLD!  "'
get  "strings — lower"    "/strings/methods" 200 '.lower == "  hello, world!  "'
get  "strings — trimmed"  "/strings/methods" 200 '.trimmed == "Hello, World!"'
get  "strings — length"   "/strings/methods" 200 '.length == 13'
get  "strings — contains" "/strings/methods" 200 '.contains == true'
get  "strings — replaced" "/strings/methods" 200 '.replaced == "Hello, MarretaLang!"'

get  "strings — split count" "/strings/split" 200 '.count == 4'
get  "strings — split parts" "/strings/split" 200 '.parts[0] == "alpha"'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 3 — String interpolation
# ═══════════════════════════════════════════════════════════════════════════════
section "3. String interpolation"

get  "interpolation — greeting" "/strings/interpolation" 200 '.greeting == "Hello from MarretaLang v5!"'
get  "interpolation — count"    "/strings/interpolation" 200 '.count == "List has 3 items"'
get  "interpolation — math"     "/strings/interpolation" 200 '.equation == "2 + 2 = 4"'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 4 — List methods
# ═══════════════════════════════════════════════════════════════════════════════
section "4. List methods"

get  "lists — length"   "/lists/methods" 200 '.length == 6'
get  "lists — first"    "/lists/methods" 200 '.first == 3'
get  "lists — last"     "/lists/methods" 200 '.last == 9'
get  "lists — reversed" "/lists/methods" 200 '.reversed[0] == 9'
get  "lists — pushed"   "/lists/methods" 200 '(.pushed | last) == 99'
get  "lists — includes" "/lists/methods" 200 '.includes == true'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 5 — Map methods
# ═══════════════════════════════════════════════════════════════════════════════
section "5. Map methods"

get  "maps — keys"      "/maps/methods" 200 '(.keys | length) == 3'
get  "maps — has_name"  "/maps/methods" 200 '.has_name == true'
get  "maps — has_email" "/maps/methods" 200 '.has_email == false'
get  "maps — nested host" "/maps/access" 200 '.host == "localhost"'
get  "maps — nested port" "/maps/access" 200 '.port == 5432'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 6 — Operators & logic
# ═══════════════════════════════════════════════════════════════════════════════
section "6. Operators & logic"

get  "operators — eq"  "/operators/comparison" 200 '.eq == true'
get  "operators — neq" "/operators/comparison" 200 '.neq == true'
get  "operators — gt"  "/operators/comparison" 200 '.gt == true'
get  "operators — lt"  "/operators/comparison" 200 '.lt == true'
get  "operators — gte" "/operators/comparison" 200 '.gte == true'
get  "operators — lte" "/operators/comparison" 200 '.lte == true'

get  "logic — and_true"  "/operators/logic" 200 '.and_true == true'
get  "logic — and_false" "/operators/logic" 200 '.and_false == false'
get  "logic — or_true"   "/operators/logic" 200 '.or_true == true'
get  "logic — not_true"  "/operators/logic" 200 '.not_true == true'

get  "null_coalesce — present" "/operators/null_coalesce" 200 '.present_or == "hello"'
get  "null_coalesce — missing" "/operators/null_coalesce" 200 '.missing_or == "default"'
get  "null_coalesce — zero"    "/operators/null_coalesce" 200 '.zero_or == "fallback"'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 7 — Conditionals
# ═══════════════════════════════════════════════════════════════════════════════
section "7. Conditionals"

get  "if_suffix — x assigned"   "/conditional/if_suffix" 200 '.x == "assigned"'
get  "if_suffix — y is null"    "/conditional/if_suffix" 200 '.y_val == null'
get  "if_suffix — result"       "/conditional/if_suffix" 200 '.result == "assigned"'

get  "if_else — top branch"        "/conditional/if_else/95" 200 '.bucket == "excellent"'
get  "if_else — middle branch"     "/conditional/if_else/75" 200 '.bucket == "good"'
get  "if_else — fallback branch"   "/conditional/if_else/40" 200 '.bucket == "needs_work"'
get  "if_else — no else returns null" "/conditional/if_else/no_else" 200 '.result == null'
get  "if_else — branch scope is closed" "/conditional/if_else/scope" 200 '.value == "visible" and .branch_only == "outer"'
get  "if_else — pipeline uses full result (double branch)" "/conditional/if_else/pipeline/double" 200 '.result == 10'
get  "if_else — pipeline uses full result (fallback branch)" "/conditional/if_else/pipeline/other" 200 '.result == 14'
get  "if_else — reply early returns from branch" "/conditional/if_else/reply/cache" 200 '.source == "cache"'
get  "if_else — reply falls through when branch is false" "/conditional/if_else/reply/db" 200 '.source == "db"'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 7.4 — Math namespace
# ═══════════════════════════════════════════════════════════════════════════════
section "7.4 Math namespace"

get  "math — abs/floor/ceil" "/math/runtime" 200 '.abs_int == 5 and .abs_float == 5.25 and .floor == 3 and .ceil == 4'
get  "math — round variants" "/math/runtime" 200 '.round_int == 5 and .round_places == 4.88 and .round_places_zero == 5 and .integer_places == 5'
get  "math — min/max/clamp"  "/math/runtime" 200 '.min_float == 10 and .max_float == 10.5 and .clamp_int == 100 and .clamp_float == 9.5'
post "math — contracts integration" "/contracts/math" 200 \
    '.rounded == 12.35 and .bounded == 10' \
    '{"value":12.345,"min":0,"max":10}'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 7.5 — Time API
# ═══════════════════════════════════════════════════════════════════════════════
section "7.5 Time API"

TIME_PAYLOAD='{"created_at":"2026-04-27T13:10:45Z","billing_date":"2026-04-27","opens_at":"09:30:00","sla":"PT5400S","business_window":{"start":"2026-04-27","end":"2026-04-30"}}'

get  "time — runtime year"          "/time/runtime" 200 '.created_year == 2026'
get  "time — runtime local hour"    "/time/runtime" 200 '.created_hour == 10 and .created_minute == 10 and .created_second == 45'
get  "time — runtime unix"          "/time/runtime" 200 '.created_unix == 1777295445'
get  "time — runtime date part"     "/time/runtime" 200 '.created_date == "2026-04-27"'
get  "time — runtime weekday"       "/time/runtime" 200 '.billing_weekday == 0'
get  "time — runtime start of day"  "/time/runtime" 200 '.billing_start_of_day == "2026-04-27T03:00:00Z"'
get  "time — runtime end of day"    "/time/runtime" 200 '.billing_end_of_day == "2026-04-28T02:59:59Z"'
get  "time — runtime opening hour"  "/time/runtime" 200 '.opening_hour == 9'
get  "time — runtime opening on date" "/time/runtime" 200 '.opening_on_date == "2026-04-27T12:30:00Z"'
get  "time — runtime duration"      "/time/runtime" 200 '.sla_hours == 36'
get  "time — runtime interval days" "/time/runtime" 200 '.window_days == 3'
get  "time — runtime interval bounds" "/time/runtime" 200 '.window_start == "2026-04-27" and .window_end == "2026-04-30"'
get  "time — runtime contains"      "/time/runtime" 200 '.contains_date == true'
get  "time — runtime overlaps"      "/time/runtime" 200 '.overlaps_window == true'
get  "time — runtime parse + format" "/time/runtime" 200 '.parsed_instant == "2026-04-27T13:10:45Z" and .parsed_date == "2026-04-27" and .parsed_time == "09:30:00" and .formatted_date == "27/04/2026"'
get  "time — runtime unix roundtrip" "/time/runtime" 200 '.unix_roundtrip == "2026-04-27T13:10:45Z"'

post "time — payload coercion to native values" "/time/payload" 200 \
    '.created_year == 2026 and .created_unix == 1777295445 and .billing_year == 2026 and .billing_weekday == 0 and .opening_hour == 9 and .opening_on_date == "2026-04-27T12:30:00Z" and .sla_hours == 1.5 and .window_days == 3 and .window_contains_date == true' \
    "${TIME_PAYLOAD}"

post "time — task schema coercion uses native values" "/time/task" 200 \
    '.created_year == 2026 and .created_unix == 1777295445 and .billing_year == 2026 and .billing_weekday == 0 and .opening_hour == 9 and .opening_minute == 30 and .opening_on_date == "2026-04-27T12:30:00Z" and .sla_hours == 1.5 and .window_days == 3 and .window_contains_date == true' \
    "${TIME_PAYLOAD}"

post "time — transport through cache" "/time/cache_transport" 200 \
    '.cached_created_at == "2026-04-27T13:10:45Z" and .cached_billing_date == "2026-04-27" and .cached_opens_at == "09:30:00" and .cached_sla == "PT5400S" and .cached_window_start == "2026-04-27"' \
    "${TIME_PAYLOAD}"

post "time — transport through doc" "/time/doc_transport" 200 \
    '.loaded_created_at == "2026-04-27T13:10:45Z" and .loaded_billing_date == "2026-04-27" and .loaded_opens_at == "09:30:00" and .loaded_sla == 5400000 and .loaded_window_start == "2026-04-27"' \
    "${TIME_PAYLOAD}"

post "contract types — doc roundtrip" "/docs/contract-types" 201 \
    '.status == "cancelled" and .amount == "42.42" and .ignored == null' \
    '{"status":"cancelled","amount":"42.42"}'

post "time — contracts preserve canonical forms" "/contracts/time" 200 \
    '.created_at == "2026-04-27T13:10:45Z" and .billing_date == "2026-04-27" and .opens_at == "09:30:00" and .sla == "PT5400S" and .business_window.start == "2026-04-27" and .business_window.end == "2026-04-30"' \
    "${TIME_PAYLOAD}"

get  "time — http client payload roundtrip" "/http-client/time-payload" 200 \
    '.created_year == 2026 and .created_unix == 1777295445 and .billing_year == 2026 and .opening_hour == 9 and .sla_hours == 1.5 and .window_days == 3'

post "time — db roundtrip" "/db/time_entries" 201 \
    '.created_at == "2026-04-27T13:10:45Z" and .created_unix == 1777295445 and .billing_date == "2026-04-27" and .opens_at == "09:30:00" and .opening_hour == 9 and .sla_hours == 1.5 and .window_start == "2026-04-27" and .window_end == "2026-04-30" and .window_days == 3 and .contains_date == true' \
    "${TIME_PAYLOAD}"

post "contract types — db roundtrip" "/db/contract-types" 201 \
    '.status == "paid" and .amount == "19.90" and .ignored == null' \
    '{"status":"paid","amount":"19.90"}'

get  "time — iteration integration" "/iteration/time/windows" 200 \
    '.days == [3,2] and .total_days == 5'

get  "time — parallel integration" "/analyze/time" 200 \
    '.hour == 10 and .unix == 1777295445'
get  "math — parallel integration" "/analyze/math" 200 \
    '.abs == 4.876 and .rounded == -4.88'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 8 — Match expression
# ═══════════════════════════════════════════════════════════════════════════════
section "8. Match expression"

get  "match — 200 OK"          "/match/status/200" 200 '.label == "OK"'
get  "match — 404 Not Found"   "/match/status/404" 200 '.label == "Not Found"'
get  "match — 500 ISE"         "/match/status/500" 200 '.label == "Internal Server Error"'
get  "match — fallback"        "/match/status/302" 200 '.label == "Unknown"'

get  "match — vip discount"     "/match/discount/vip"     200 '.discount == 0.30'
get  "match — premium discount" "/match/discount/premium" 200 '.discount == 0.15'
get  "match — regular discount" "/match/discount/regular" 200 '.discount == 0.05'
get  "match — unknown discount" "/match/discount/guest"   200 '.discount == 0.0'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 9 — Tasks
# ═══════════════════════════════════════════════════════════════════════════════
section "9. Tasks"

get  "tasks — double"  "/tasks/call" 200 '.double == 10'
get  "tasks — triple"  "/tasks/call" 200 '.triple == 15'
get  "tasks — greet"   "/tasks/call" 200 '.greet == "Hello, World!"'
get  "tasks — reply calls task" "/tasks/reply_call" 200 '. == "Hello, Reply!"'
get  "tasks — task returns task" "/tasks/return_task" 200 '.result == 12'

get  "tasks — square"  "/tasks/inline" 200 '.square == 16'
get  "tasks — cube"    "/tasks/inline" 200 '.cube == 27'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 10 — Pipeline
# ═══════════════════════════════════════════════════════════════════════════════
section "10. Pipeline (>>)"

get  "pipeline — scalar"       "/pipeline/tasks"        200 '.result == 30'
get  "pipeline — list iterate" "/pipeline/list_iterate" 200 '.result == [6,12,18]'
get  "pipeline — summarise"    "/pipeline/summarise"    200 '.count == 3 and .first == "alpha" and .last == "gamma"'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 11 — map / keep
# ═══════════════════════════════════════════════════════════════════════════════
section "11. map / keep"

get  "map — tax enrichment"    "/pipeline/map"        200 '.items[0].total == 110.0'
get  "map — double then triple" "/pipeline/map_filter" 200 '.result == [6,12,18,24,30]'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 12 — Parallel broadcast (*>>)
# ═══════════════════════════════════════════════════════════════════════════════
section "12. Parallel broadcast (*>>)"

get  "broadcast — scalar double"  "/broadcast/scalar" 200 '.first == 20'
get  "broadcast — scalar triple"  "/broadcast/scalar" 200 '.last == 30'
get  "broadcast — list count"     "/broadcast/list"   200 '.count == 4'
get  "broadcast — list last"      "/broadcast/list"   200 '.last == "delta"'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 13 — Request bindings
# ═══════════════════════════════════════════════════════════════════════════════
section "13. Request bindings"

post   "payload — name"         "/bindings/payload" 200 '.received_name == "Alice"'      '{"name":"Alice","score":99}'
post   "payload — score"        "/bindings/payload" 200 '.received_score == 99'           '{"name":"Alice","score":99}'
post   "payload — defaults"     "/bindings/payload" 200 '.received_name == "anonymous" and .received_score == 0' '{}'

get    "query — term"           "/bindings/query?term=hello&limit=5" 200 '.term == "hello"'
get    "query — limit"          "/bindings/query?term=hello&limit=5" 200 '.limit == "5"'
get    "query — defaults"       "/bindings/query"                    200 '.term == "none"'

check  "headers — accept"  GET "/bindings/headers" 200 '.accept == "application/json"' \
    -H "Accept: application/json"

check  "raw — length"           POST "/bindings/raw" 200 '.length > 0' \
    -H "Content-Type: text/plain" -d "hello world"
check  "raw — preview"          POST "/bindings/raw" 200 '.preview == "hello world"' \
    -H "Content-Type: text/plain" -d "hello world"

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 14 — Schema validation
# ═══════════════════════════════════════════════════════════════════════════════
section "14. Schema validation"

post   "schema — valid payload"   "/schema/validate" 200 '.valid == true'                   '{"name":"widget","active":true}'
post   "schema — missing field"   "/schema/validate" 422 ''                                  '{"name":"widget"}'
post   "schema — wrong type"      "/schema/validate" 422 ''                                  '{"name":"widget","active":"yes"}'

post   "schema — response strip"  "/schema/response" 200 '.secret == null and .id == 42'     '{"name":"widget","active":true}'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 15 — Error handling
# ═══════════════════════════════════════════════════════════════════════════════
section "15. Error handling"

get    "error — 404"   "/errors/not_found"  404 ''
get    "error — 400"   "/errors/bad_request" 400 ''
wait_for_log_fields "request log — declared route 400 stdout event" \
    '"kind":"request"' \
    '"method":"GET"' \
    '"path":"/errors/bad_request"' \
    '"route":"/errors/bad_request"' \
    '"status":400' \
    '"duration_ms":'
check  "request log — unmatched 404 http" GET "/__request_log_missing__" 404 ''
wait_for_request_log_without_route "request log — unmatched 404 omits route" "GET" "/__request_log_missing__" 404 ''

post   "guard — passes"               "/errors/guard" 200 '.ok == true'            '{"name":"alice"}'
post   "guard — missing name"         "/errors/guard" 400 '' '{}'
post   "guard — name too short"       "/errors/guard" 422 '' '{"name":"ab"}'

get    "conditional fail — ok"        "/errors/conditional_fail/200" 200 '.ok == true'
get    "conditional fail — 404"       "/errors/conditional_fail/404" 404 ''

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 16 — Response content types
# ═══════════════════════════════════════════════════════════════════════════════
section "16. Response content types"

get    "response — json"         "/response/json"              200 '.message == "I am JSON"'
get    "response — status 201"   "/response/status_codes/201" 201 '.created == true'
get    "response — status 404"   "/response/status_codes/404" 404 '.code == 404'
get    "response — variable list"    "/response/variable/list"    200 '. == ["a","b"]'
get    "response — variable map"     "/response/variable/map"     200 '.name == "Ana" and .active == true'
get    "response — variable integer" "/response/variable/integer" 200 '. == 42'
get    "response — variable float"   "/response/variable/float"   200 '. == 19.9'
get    "response — variable string"  "/response/variable/string"  200 '. == "hello"'
get    "response — variable boolean" "/response/variable/boolean" 200 '. == true'
get    "response — variable null"    "/response/variable/null"    200 '. == null'

# HTML and text: just check status + non-empty body (no jq)
check  "response — html status"  GET "/response/html" 200 ''
check  "response — text status"  GET "/response/text" 200 ''

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 17 — Direct CRUD  (requires DB)
# ═══════════════════════════════════════════════════════════════════════════════
section "17. Direct CRUD (DB)"

# Save a new item and capture its id for subsequent tests.
SAVE_RESP=$(curl -s -X POST "${BASE}/db/items" \
    -H "Content-Type: application/json" \
    -d '{"name":"test-item","active":true}' 2>/dev/null)
ITEM_ID=$(echo "$SAVE_RESP" | jq -r '.id // empty')

if [[ -z "$ITEM_ID" ]]; then
    echo -e "  ${RED}FAIL${RESET} db/items save — could not obtain item id"
    FAIL=$((FAIL + 1))
else
    echo -e "  ${GREEN}PASS${RESET} db/items save (id=${ITEM_ID})"
    PASS=$((PASS + 1))

    get    "db/items — find by id"    "/db/items/${ITEM_ID}" 200 ".id == ${ITEM_ID}"
    get    "db/items — find_all"      "/db/items"            200 '(.items | length) >= 1'
    get    "db/items/active"          "/db/items/active"     200 '(.items | length) >= 1'

    put    "db/items — update"        "/db/items/${ITEM_ID}" 200 '.name == "updated-item"' \
        '{"name":"updated-item","active":false}'

    delete "db/items — delete"        "/db/items/${ITEM_ID}" 200 '.deleted == true'
    get    "db/items — 404 after del" "/db/items/${ITEM_ID}" 404 ''
fi

post "db/items — constructor payload" "/db/items/constructor" 201 \
    '.name == "constructor-db" and .active == true and .id != null' '{}'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 18 — Pipeline queries  (requires DB)
# ═══════════════════════════════════════════════════════════════════════════════
section "18. Pipeline queries (DB)"

get    "pipeline — fetch all"     "/db/pipeline/fetch"        200 '(.items | length) >= 1'
get    "pipeline — fetch_one"     "/db/pipeline/fetch_one"    200 '.item != null'
get    "pipeline — count"         "/db/pipeline/count"        200 '.count >= 1'
get    "pipeline — count active"  "/db/pipeline/count/active" 200 '.count >= 1'
get    "pipeline — exists"        "/db/pipeline/exists"       200 '.exists == true'
get    "pipeline — where active"  "/db/pipeline/where"        200 '(.items | map(select(.active == true)) | length) == (.items | length)'
get    "pipeline — order asc"     "/db/pipeline/order"        200 '.items[0].name == "alpha"'
get    "pipeline — page"          "/db/pipeline/page"         200 '(.items | length) == 2'
get    "pipeline — chained where" "/db/pipeline/chained"      200 '.count >= 1'

# Bulk deactivate (idempotent — OK if count=0 because already inactive)
check  "pipeline — bulk deactivate" POST "/db/pipeline/deactivate" 200 '.updated >= 0' \
    "${H_JSON[@]}"

# Re-activate seed rows so later tests still see active items.
# We use a native query because we don't expose a bulk activate endpoint.
curl -sf -X POST "${BASE}/db/pipeline/deactivate" "${H_JSON[@]}" > /dev/null 2>&1 || true

# Bulk delete inactive rows
check  "pipeline — bulk delete inactive" DELETE "/db/pipeline/inactive" 200 '.deleted >= 0'

# Re-seed known rows for like/in tests
curl -s -X POST "${BASE}/db/items" "${H_JSON[@]}" \
    -d '{"name":"alpha","active":true}' > /dev/null 2>&1 || true
curl -s -X POST "${BASE}/db/items" "${H_JSON[@]}" \
    -d '{"name":"beta","active":true}' > /dev/null 2>&1 || true

get  "pipeline — like filter"  "/db/pipeline/like" 200 '(.items | length) >= 1'
get  "pipeline — in filter"    "/db/pipeline/in"   200 '(.items | length) >= 1'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 18D — Identifier hardening (Spec 076, DB)
# ═══════════════════════════════════════════════════════════════════════════════
section "18D. Identifier hardening (DB)"

# Dynamic sort is first-class: a legitimate column from the query string sorts safely.
get  "hardening — order_by dynamic legit"  "/db/hardening/order?sort=name%20desc"                    200 '.items'
# An injection attempt in each identifier surface is rejected with a clean 400, never run.
get  "hardening — order_by injection"      "/db/hardening/order?sort=name%3B%20DROP%20TABLE%20items" 400 '.code == "invalid_identifier"'
# Schema layer (users has a db: schema): a valid-shape but unknown column is rejected.
get  "hardening — order_by unknown column" "/db/hardening/order_known?sort=nope"                     400 '.code == "unknown_column"'
get  "hardening — order_by known legit"    "/db/hardening/order_known?sort=name%20asc"               200 '.items'
get  "hardening — like column injection"   "/db/hardening/like?col=name%29%3B%20--"                  400 '.code == "invalid_identifier"'
get  "hardening — in column injection"     "/db/hardening/in?col=name%29%3B%20--"                    400 '.code == "invalid_identifier"'
get  "hardening — select computed expr"    "/db/hardening/select?col=total%20%2A%200.9"              400 '.code == "invalid_identifier"'
get  "hardening — select legit column"     "/db/hardening/select?col=name"                          200 '.items'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 18B — Relation navigation by convention (DB)
# ═══════════════════════════════════════════════════════════════════════════════
section "18B. Relation navigation by convention (DB)"

get  "relations — direct order fetch exposes fk" \
    "/db/relations/orders/1" 200 '.customer_id == 1'
get  "relations — singular fetch" \
    "/db/relations/orders/1/customer" 200 '.id == 1 and .name == "Ana"'
get  "relations — singular exists" \
    "/db/relations/orders/1/customer/exists" 200 '.exists == true'
get  "relations — chained customer address fetch" \
    "/db/relations/orders/1/customer/address" 200 '.address.id == 1 and .address.city == "Sao Paulo"'
get  "relations — chained customer address fetch null" \
    "/db/relations/orders/3/customer/address" 200 '.address == null'
get  "relations — direct user fetch exposes nullable fk" \
    "/db/relations/users/1" 200 '.address_id == 1'
get  "relations — optional singular fetch present" \
    "/db/relations/users/1/address" 200 '.address.id == 1 and .address.zipcode == "01310-100"'
get  "relations — optional singular exists true" \
    "/db/relations/users/1/address/exists" 200 '.exists == true'
get  "relations — optional singular fetch null" \
    "/db/relations/users/2/address" 200 '.address == null'
get  "relations — optional singular exists false" \
    "/db/relations/users/2/address/exists" 200 '.exists == false'
get  "relations — inverse collection fetch" \
    "/db/relations/users/1/orders" 200 '.count == 2 and (.items | length) == 2 and .items[0].customer_id == 1'
get  "relations — inverse collection count" \
    "/db/relations/users/1/orders/count" 200 '.count == 2'
get  "relations — inverse collection exists" \
    "/db/relations/users/1/orders/exists" 200 '.exists == true'
get  "relations — inverse collection where" \
    "/db/relations/users/1/orders/where" 200 '.count == 1 and (.items | length) == 1 and .items[0].total == 99.5'
# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 19 — Parallel DB queries  (requires DB)
# ═══════════════════════════════════════════════════════════════════════════════
section "19. Parallel DB queries"

# Seed at least one active row for parallel tests.
curl -s -X POST "${BASE}/db/items" "${H_JSON[@]}" \
    -d '{"name":"parallel-seed","active":true}' > /dev/null 2>&1 || true

get  "parallel — count"      "/db/parallel"        200 '.count >= 1'
get  "parallel — items list" "/db/parallel"        200 '(.items | length) >= 1'
get  "parallel — mixed rows" "/db/parallel/mixed"  200 '(.items | length) >= 0'
get  "parallel — mixed meta" "/db/parallel/mixed"  200 '.meta.source == "items"'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 20 — Native query  (requires DB)
# ═══════════════════════════════════════════════════════════════════════════════
section "20. Native query"

# Re-seed a known row before native query tests (previous sections may have deleted all rows).
curl -s -X POST "${BASE}/db/items" "${H_JSON[@]}" \
    -d '{"name":"alpha","active":true}' > /dev/null 2>&1 || true

get  "native — all"          "/db/native/all"                  200 '(.items | length) >= 1'
get  "native — by name"      "/db/native/by_name/alpha"        200 '.items[0].name == "alpha"'
get  "native — search"       "/db/native/search"               200 '(.items | length) >= 0'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 21 — Transactions  (requires DB)
# ═══════════════════════════════════════════════════════════════════════════════
section "21. Transactions"

# Commit path: returns two IDs.
TX_RESP=$(curl -s -X POST "${BASE}/db/transaction/commit" "${H_JSON[@]}" 2>/dev/null)
TX_A=$(echo "$TX_RESP" | jq -r '.a_id // empty')
TX_B=$(echo "$TX_RESP" | jq -r '.b_id // empty')

if [[ -n "$TX_A" && -n "$TX_B" ]]; then
    echo -e "  ${GREEN}PASS${RESET} transaction commit — a_id=${TX_A} b_id=${TX_B}"
    PASS=$((PASS + 1))
    # Both rows must exist.
    get  "transaction — row A exists"  "/db/items/${TX_A}" 200 ".id == ${TX_A}"
    get  "transaction — row B exists"  "/db/items/${TX_B}" 200 ".id == ${TX_B}"
else
    echo -e "  ${RED}FAIL${RESET} transaction commit — unexpected response: ${TX_RESP}"
    FAIL=$((FAIL + 1))
fi

# Rollback path: must return 500, row must NOT exist.
RB_RESP=$(curl -s -o /dev/null -w "%{http_code}" \
    -X POST "${BASE}/db/transaction/rollback" "${H_JSON[@]}" 2>/dev/null)
if [[ "$RB_RESP" == "500" ]]; then
    echo -e "  ${GREEN}PASS${RESET} transaction rollback — HTTP 500 as expected"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} transaction rollback — expected 500, got ${RB_RESP}"
    FAIL=$((FAIL + 1))
fi

# Verify the rolled-back row does NOT exist in the DB.
ROLLBACK_COUNT=$(curl -s "${BASE}/db/native/by_name/tx-will-rollback" 2>/dev/null \
    | jq '.items | length' 2>/dev/null || echo "0")
if [[ "$ROLLBACK_COUNT" == "0" ]]; then
    echo -e "  ${GREEN}PASS${RESET} transaction rollback — row not persisted"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} transaction rollback — row was persisted (count=${ROLLBACK_COUNT})"
    FAIL=$((FAIL + 1))
fi

# Conditional commit path.
post   "transaction — conditional commit"   "/db/transaction/conditional" 200 \
    '.committed == true and .after > .before' \
    '{"should_commit":true}'

# Conditional rollback path: after should equal before (row rolled back).
post   "transaction — conditional rollback" "/db/transaction/conditional" 500 \
    '' \
    '{"should_commit":false}'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 22 — Utility methods
# ═══════════════════════════════════════════════════════════════════════════════
section "22. Utility methods"

get  "utils — starts_with true"    "/utils/string/starts_ends" 200 '.bearer_ok == true'
get  "utils — starts_with false"   "/utils/string/starts_ends" 200 '.bearer_not == false'
get  "utils — ends_with true"      "/utils/string/starts_ends" 200 '.prod_ends == true'
get  "utils — ends_with false"     "/utils/string/starts_ends" 200 '.prod_not == false'
get  "utils — index_of found"      "/utils/string/starts_ends" 200 '.index_found == 4'
get  "utils — index_of missing"    "/utils/string/starts_ends" 200 '.index_missing == -1'
get  "utils — list join"           "/utils/list/join"    200 '.joined == "alpha, beta, gamma"'
get  "utils — list join nums"      "/utils/list/join"    200 '.piped == "1-2-3"'
get  "utils — list sort nums"      "/utils/list/sort"    200 '.nums_sorted == [1,1,2,3,4,5,6,9]'
get  "utils — list sort strs"      "/utils/list/sort"    200 '.strs_sorted == ["apple","banana","cherry"]'
get  "utils — list unique"         "/utils/list/unique"  200 '.unique == [1,2,3,4]'
get  "utils — list flatten"        "/utils/list/flatten" 200 '.flat == [1,2,3,4,5]'
get  "utils — list slice middle"   "/utils/list/slice"   200 '.middle == ["b","c","d"]'
get  "utils — list slice clamped"  "/utils/list/slice"   200 '(.clamped | length) == 2'
get  "utils — map delete"          "/utils/map/delete"   200 '(.without_secret | has("secret")) == false'
get  "utils — map size"            "/utils/map/delete"   200 '.original_size == 3 and .cleaned_size == 2'
get  "utils — float round0"        "/utils/number/round" 200 '.round0 == 3'
get  "utils — float round2"        "/utils/number/round" 200 '.round2 == 3.14'
get  "utils — float floor"         "/utils/number/round" 200 '.floor == 3'
get  "utils — float ceil"          "/utils/number/round" 200 '.ceil == 4'
get  "utils — int min"             "/utils/number/round" 200 '.int_min == 5'
get  "utils — int max"             "/utils/number/round" 200 '.int_max == 10'
get  "utils — float min"           "/utils/number/round" 200 '.flt_min == 2.1'
get  "utils — float max"           "/utils/number/round" 200 '.flt_max == 5.0'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 23 — Language ergonomics
# ═══════════════════════════════════════════════════════════════════════════════
section "23. Language ergonomics"

# Phase 1 — fail with map body
post "fail — map body 404"     "/errors/fail_map" 404 '.error == "not found" and .code == "ITEM_NOT_FOUND"' '{}'
get  "fail — variable body"    "/errors/fail_var" 410 '.error == "gone"'
get  "fail — no engine keys"   "/errors/fail_var" 410 '(has("at") | not) and (has("op") | not)'

# Phase 2 — string interpolation with expressions
get  "interp — method call"   "/strings/interpolation_expr" 200 '.count_str == "Items: 4"'
get  "interp — arithmetic"    "/strings/interpolation_expr" 200 '(.math_str | startswith("With tax:"))'
get  "interp — nested upper"  "/strings/interpolation_expr" 200 '.upper_str == "Hello WORLD"'

# Phase 3 — subscript access
get  "subscript — list[0]"    "/access/subscript_list" 200 '.first == "zero"'
get  "subscript — list[1]"    "/access/subscript_list" 200 '.second == "one"'
get  "subscript — list oor"   "/access/subscript_list" 200 '.missing == null'

# Phase 4a — reply with dynamic status
get  "reply — dynamic 202"    "/response/dynamic_status" 202 '.accepted == true'
get  "reply — no engine keys" "/response/dynamic_status" 202 '(has("at") | not) and (has("op") | not) and (has("code") | not)'

# Phase 4b — keep if cond
get  "keep_if — high"         "/pipeline/keep_if"   200 '(.labeled | map(select(. == "high"))  | length) == 1'
get  "keep_if — medium"       "/pipeline/keep_if"   200 '(.labeled | map(select(. == "medium"))| length) == 2'
get  "keep_if — drop 0score"  "/pipeline/keep_if"   200 '(.labeled | length) == 5'

# Phase 4b — skip if cond
get  "skip_if — active only"  "/pipeline/skip_guard" 200 '.active == ["alpha","gamma"]'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 23.5 — Trace silence on success path (Phase H)
# ═══════════════════════════════════════════════════════════════════════════════
# Sections 1-23 exercise success flows plus user-authored `fail`/`reply`
# responses (HttpResponse, never logged as runtime errors). At this point no
# Marreta-formatted trace should have reached stderr.
section "23.5 Trace silence on success path"

if current_app_logs | grep -F "[marreta]" >/dev/null 2>&1; then
    echo -e "  ${RED}FAIL${RESET} trace silence — unexpected [marreta] line before uncaught section"
    current_app_logs | grep -F "[marreta]" | head -3 | sed 's/^/         /'
    FAIL=$((FAIL + 1))
else
    echo -e "  ${GREEN}PASS${RESET} trace silence — no trace output on success path"
    PASS=$((PASS + 1))
fi
if current_app_logs | grep -F '"kind":"runtime_error"' >/dev/null 2>&1; then
    echo -e "  ${RED}FAIL${RESET} runtime_error silence — unexpected runtime_error before uncaught section"
    current_app_logs | grep -F '"kind":"runtime_error"' | head -3 | sed 's/^/         /'
    FAIL=$((FAIL + 1))
else
    echo -e "  ${GREEN}PASS${RESET} runtime_error silence — no JSON runtime_error on success path"
    PASS=$((PASS + 1))
fi

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 24 — Error Handling
# ═══════════════════════════════════════════════════════════════════════════════
section "24. Error Handling"

get  "raise — uncaught HTTP 500"       "/errors/raise_uncaught"          500 '.error == "something went wrong" and .code == "raise_error" and (has("at") | not) and (has("op") | not)'
wait_for_log_fields \
    "runtime_error — uncaught route emits JSON summary" \
    '"kind":"runtime_error"' \
    '"scope":"request"' \
    '"error_code":"raise_error"' \
    '"operation":"raise"' \
    '"message":"something went wrong"' \
    '"http_status":500'
get  "raise — conditional false"       "/errors/raise_conditional/false" 500 '.error == "not active"'
get  "raise — conditional true"        "/errors/raise_conditional/true"  200 '.ok == true'
get  "raise — from task propagates"    "/errors/raise_from_task"         500 '(.error | startswith("must be positive")) and .code == "raise_error" and (has("at") | not) and (has("op") | not)'
get  "raise — require else raise"      "/errors/require_raise"           500 '.error == "value is required"'
get  "rescue — pipeline catches raise" "/errors/rescue_pipeline"         503 'true'
get  "rescue — expr fallback value"    "/errors/rescue_expr"             200 '.val == "fallback"'
get  "rescue — null silences error"    "/errors/rescue_null"             200 '.val == null'
get  "rescue — block with error map"   "/errors/rescue_block"            503 '.code == "raise_error"'
post "time — task schema coercion rejects invalid payload" "/time/task" 500 \
    '' \
    '{"created_at":"2026-04-27T13:10:45Z","billing_date":"2026-04-27","opens_at":"09:30:00","sla":"not-a-duration","business_window":{"start":"2026-04-27","end":"2026-04-30"}}'
wait_for_log_pattern "trace — route frame logged" "at route GET /errors/raise_from_task"
wait_for_log_pattern "trace — task frame logged" "at task validate"
wait_for_log_pattern "trace — raise op logged" "at raise"

# ── Error identity — semantic codes ──────────────────────────────────────────
get  "error identity — reference error code"  "/errors/identity/reference"     500 '.code == "reference_error"'
get  "error identity — raise with interp msg" "/errors/identity/raise_rescued" 200 '.error | startswith("Invalid value:")'
get  "error identity — raise error code"      "/errors/identity/raise_rescued" 200 '.code == "raise_error"'
get  "error identity — db error rescued"      "/errors/identity/db_rescued"    200 '.code == "db_error"'

# Phase C — uncaught db error through nested task chain emits full trace
get  "trace db — chain HTTP 500"        "/errors/trace/db_uncaught_chain" 500 '.code == "db_error"'
wait_for_log_fields \
    "runtime_error — uncaught db emits operation" \
    '"kind":"runtime_error"' \
    '"scope":"request"' \
    '"error_code":"db_error"' \
    '"operation":"db.query"'
wait_for_log_pattern "trace db — route frame"      "at route GET /errors/trace/db_uncaught_chain"
wait_for_log_pattern "trace db — outer task frame" "at task wrap_query"
wait_for_log_pattern "trace db — inner task frame" "at task deep_query"
wait_for_log_pattern "trace db — db op label"      "at db.query"

# Phase D — invalid operator sets on singular relation handles
get  "relations — singular count rejected" \
    "/db/relations/orders/1/customer/count" 500 '.error | contains("singular relation")'
get  "relations — singular where rejected" \
    "/db/relations/orders/1/customer/where" 500 '.error | contains("singular relation")'
get  "relations — non-relation fetch rejected" \
    "/db/relations/orders/1/total/fetch" 500 '.error | contains("not a relation or query")'
post "relations — persistent save rejects user id" \
    "/db/relations/users/reject-id" 500 '.error | contains("generated by the database")' '{}'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 25 — Doc module — MongoDB
# ═══════════════════════════════════════════════════════════════════════════════
section "25. Doc module — MongoDB (doc.*)"

# ── 25.1 Direct CRUD ──────────────────────────────────────────────────────────

DOC_SAVE_RESP=$(curl -s -X POST "${BASE}/docs/items" \
    -H "Content-Type: application/json" \
    -d '{"name":"test-item","active":true}' 2>/dev/null)
DOC_ITEM_ID=$(echo "$DOC_SAVE_RESP" | jq -r '._id // empty')

if [[ -z "$DOC_ITEM_ID" ]]; then
    echo -e "  ${RED}FAIL${RESET} doc.save — could not obtain _id"
    FAIL=$((FAIL + 1))
else
    echo -e "  ${GREEN}PASS${RESET} doc.save (id=${DOC_ITEM_ID})"
    PASS=$((PASS + 1))

    get    "doc.find by id"             "/docs/items/${DOC_ITEM_ID}" 200 '._id != null'
    get    "doc.find_all"               "/docs/items"                200 '(.items | length) >= 1'

    put    "doc.update"                 "/docs/items/${DOC_ITEM_ID}" 200 '.name == "updated-item"' \
        '{"name":"updated-item","active":false}'

    delete "doc.delete"                 "/docs/items/${DOC_ITEM_ID}" 200 '.deleted == true'
    get    "doc.find — 404 after delete" "/docs/items/${DOC_ITEM_ID}" 404 ''
fi

post "doc.save — constructor payload" "/docs/items/constructor" 201 \
    '.name == "constructor-doc" and .active == true and .id == 1' '{}'

# Nested document: person with embedded address
PERSON_RESP=$(curl -s -X POST "${BASE}/docs/persons" \
    -H "Content-Type: application/json" \
    -d '{"name":"nested-test","address":{"city":"SP","zip":"01310"},"active":true}' 2>/dev/null)
PERSON_ID=$(echo "$PERSON_RESP" | jq -r '._id // empty')

if [[ -z "$PERSON_ID" ]]; then
    echo -e "  ${RED}FAIL${RESET} doc.save nested — could not obtain _id"
    FAIL=$((FAIL + 1))
else
    echo -e "  ${GREEN}PASS${RESET} doc.save nested (id=${PERSON_ID})"
    PASS=$((PASS + 1))
    get "doc.find nested doc" "/docs/persons/${PERSON_ID}" 200 '.address.city == "SP"'
fi

# ── 25.2 Pipeline Queries ─────────────────────────────────────────────────────

# Seed items collection for basic pipeline tests
curl -s -X POST "${BASE}/docs/items" "${H_JSON[@]}" \
    -d '{"name":"test","age":25,"active":true,"status":"A"}' > /dev/null 2>&1 || true

# Seed persons collection for richer query demos (idempotent via upsert)
post "persons — seed" "/docs/persons/seed" 201 '.seeded == true' '{}'

get    "pipeline — fetch_all"       "/docs/pipeline/fetch"          200 '(.items | length) >= 0'
get    "pipeline — where active"    "/docs/pipeline/where"          200 '(.items | length) >= 0'
get    "pipeline — count"           "/docs/pipeline/count"          200 '.count >= 0'
get    "pipeline — exists"          "/docs/pipeline/exists/test"    200 '.exists == true'
get    "pipeline — in filter"       "/docs/pipeline/in"             200 '(.items | length) >= 0'

check  "q — comparison age >= 18"   GET  "/docs/q/comparison" 200 \
    '(.items | map(select(.age < 18)) | length) == 0'

check  "q — ne city != SP"          GET  "/docs/q/ne" 200 \
    '(.items | map(select(.city == "SP")) | length) == 0'

check  "q — AND: active + adult + score > 70" GET "/docs/q/and" 200 \
    '(.items | map(select(.active != true or .age < 18 or .score <= 70)) | length) == 0'

check  "q — like name contains al"   GET "/docs/q/like/%25al%25"   200 \
    '(.items | length) >= 1'

check  "q — pick has name"   GET "/docs/q/pick" 200 '(.items | length) >= 1 and (.items[0] | has("name"))'
check  "q — pick has score"  GET "/docs/q/pick" 200 '(.items[0] | has("score"))'
check  "q — pick has city"   GET "/docs/q/pick" 200 '(.items[0] | has("city"))'
check  "q — pick no age field" GET "/docs/q/pick" 200 \
    '(.items | map(select(has("age"))) | length) == 0'

get    "pipeline — complex chain"   "/docs/pipeline/complex"        200 '(.items | length) >= 0'

check  "q — page returns items + total" GET "/docs/q/page" 200 \
    'has("items") and has("total") and has("page") and (.items | length) <= 3'

check  "q — top returns single map"     GET "/docs/q/top"           200 'has("name")'
check  "q — top is highest active"      GET "/docs/q/top"           200 '.name == "Alice"'

check  "q — by-city SP"             GET  "/docs/q/by-city/SP"       200 \
    '(.items | length) >= 1 and (.items | map(select(.city != "SP")) | length) == 0'
check  "q — count-active"           GET  "/docs/q/count-active"     200 '.active >= 1'
check  "q — exists by name Alice"   GET  "/docs/q/exists/Alice"     200 '.exists == true'
check  "q — exists by name NoOne"   GET  "/docs/q/exists/NoOne"     200 '.exists == false'

post   "pipeline — upsert"          "/docs/pipeline/upsert"         200 '.upserted >= 0' \
    '{"name":"upsert-test","active":true}'
check  "pipeline — deactivate"      POST "/docs/pipeline/deactivate" 200 '.updated >= 0' \
    "${H_JSON[@]}"
check  "pipeline — delete inactive" DELETE "/docs/pipeline/inactive" 200 '.deleted >= 0'

post   "q — deactivate city SP"     "/docs/q/deactivate-city"       200 '.updated >= 0' \
    '{"city":"SP"}'
post   "q — reactivate all"         "/docs/q/reactivate-all"        200 '.updated >= 0' \
    '{}'

# ── 25.3 Aggregation Pipeline ─────────────────────────────────────────────────

post "agg — seed" "/agg/seed" 201 '.seeded == true' '{}'

AGG_RESP=$(curl -s "${BASE}/agg/by-category" 2>/dev/null)
AGG_LEN=$(echo "$AGG_RESP" | jq 'length' 2>/dev/null || echo "0")
if [[ "$AGG_LEN" -ge 2 ]]; then
    echo -e "  ${GREEN}PASS${RESET} agg/by-category — ${AGG_LEN} groups returned"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} agg/by-category — expected >=2 groups, got ${AGG_LEN}"
    echo -e "       body: ${AGG_RESP}"
    FAIL=$((FAIL + 1))
fi

check "agg — electronics total >= 300"  GET "/agg/by-category" 200 \
    '(map(select(._id == "electronics")) | .[0].total) >= 300'
check "agg — clothing total >= 125"     GET "/agg/by-category" 200 \
    '(map(select(._id == "clothing")) | .[0].total) >= 125'
check "agg — group has _id"             GET "/agg/by-category" 200 '.[0] | has("_id")'
check "agg — group has total"           GET "/agg/by-category" 200 '.[0] | has("total")'
check "agg — group has items"           GET "/agg/by-category" 200 '.[0] | has("items")'

check "agg — totals has grand_total"    GET "/agg/totals" 200 'has("grand_total")'
check "agg — totals has avg_amount"     GET "/agg/totals" 200 'has("avg_amount")'
check "agg — totals has total_items"    GET "/agg/totals" 200 'has("total_items")'
check "agg — totals grand_total >= 425" GET "/agg/totals" 200 '.grand_total >= 425'
check "agg — totals total_items >= 4"   GET "/agg/totals" 200 '.total_items >= 4'

check "agg — top-electronics is list"   GET "/agg/top-electronics" 200 '. | type == "array"'
check "agg — top-electronics has revenue" GET "/agg/top-electronics" 200 '.[0] | has("revenue")'

check "agg — min-max has cheapest"  GET "/agg/min-max" 200 '.[0] | has("cheapest")'
check "agg — min-max has priciest"  GET "/agg/min-max" 200 '.[0] | has("priciest")'
check "agg — min-max cheapest <= priciest" GET "/agg/min-max" 200 \
    '.[0].cheapest <= .[0].priciest'

check "agg — global-stats has total"   GET "/agg/global-stats" 200 'has("total")'
check "agg — global-stats has average" GET "/agg/global-stats" 200 'has("average")'
check "agg — global-stats has min"     GET "/agg/global-stats" 200 'has("min")'
check "agg — global-stats has max"     GET "/agg/global-stats" 200 'has("max")'
check "agg — global-stats has n"       GET "/agg/global-stats" 200 'has("n")'
check "agg — global-stats min <= max"  GET "/agg/global-stats" 200 '.min <= .max'

check "agg — in-stock returns list"            GET "/agg/in-stock" 200 '. | type == "array"'
check "agg — in-stock has available_revenue"   GET "/agg/in-stock" 200 '.[0] | has("available_revenue")'
check "agg — in-stock has available_items"     GET "/agg/in-stock" 200 '.[0] | has("available_items")'

# ── 25.4 Power Pipeline ───────────────────────────────────────────────────────

post "pipeline — seed" "/pipeline/seed" 201 '.seeded == true' '{}'

MATCH_RESP=$(curl -s "${BASE}/pipeline/match" 2>/dev/null)
MATCH_LEN=$(echo "$MATCH_RESP" | jq 'length' 2>/dev/null || echo "0")
if [[ "$MATCH_LEN" -ge 2 ]]; then
    echo -e "  ${GREEN}PASS${RESET} pipeline/match — ${MATCH_LEN} paid documents returned"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} pipeline/match — expected >=2 paid docs, got ${MATCH_LEN}"
    echo -e "       body: ${MATCH_RESP}"
    FAIL=$((FAIL + 1))
fi
check "pipeline — match: all status=paid" GET "/pipeline/match" 200 \
    'map(select(.status != "paid")) | length == 0'

check "pipeline — group has _id"   GET "/pipeline/group" 200 '.[0] | has("_id")'
check "pipeline — group has total" GET "/pipeline/group" 200 '.[0] | has("total")'
check "pipeline — group has n"     GET "/pipeline/group" 200 '.[0] | has("n")'

check "pipeline — sort-limit <= 2"    GET "/pipeline/sort-limit" 200 'length <= 2'
check "pipeline — sort-limit ordered" GET "/pipeline/sort-limit" 200 \
    'if length >= 2 then .[0].amount >= .[1].amount else true end'

check "pipeline — skip returns list"  GET "/pipeline/skip" 200 '. | type == "array"'

check "pipeline — project has status" GET "/pipeline/project" 200 '.[0] | has("status")'
check "pipeline — project has amount" GET "/pipeline/project" 200 '.[0] | has("amount")'

check "pipeline — add-fields has doubled" GET "/pipeline/add-fields" 200 \
    '.[0] | has("doubled")'

check "pipeline — multi has _id"    GET "/pipeline/multi" 200 '.[0] | has("_id")'
check "pipeline — multi has total"  GET "/pipeline/multi" 200 '.[0] | has("total")'
check "pipeline — multi has orders" GET "/pipeline/multi" 200 '.[0] | has("orders")'
check "pipeline — multi sorted desc" GET "/pipeline/multi" 200 \
    'if length >= 2 then .[0].total >= .[1].total else true end'

check "pipeline — region-totals has revenue" GET "/pipeline/region-totals" 200 \
    '.[0] | has("revenue")'
check "pipeline — region-totals is sorted"   GET "/pipeline/region-totals" 200 \
    'if length >= 2 then .[0].revenue >= .[1].revenue else true end'

check "pipeline — match-project has status" GET "/pipeline/match-project" 200 \
    '.[0] | has("status")'
check "pipeline — match-project all paid"  GET "/pipeline/match-project" 200 \
    'map(select(.status != "paid")) | length == 0'
check "pipeline — match-project has amount" GET "/pipeline/match-project" 200 \
    '.[0] | has("amount")'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 26 — Parallel broadcast (POST)
# ═══════════════════════════════════════════════════════════════════════════════
section "26. Parallel broadcast (POST)"

post "analyze — text word_count"  "/analyze/text"  200 '.word_count == 2' '{"text":"hello world"}'
post "analyze — text char_count"  "/analyze/text"  200 '.char_count == 11' '{"text":"hello world"}'
post "analyze — text uppercased"  "/analyze/text"  200 '.uppercased == "HELLO WORLD"' '{"text":"hello world"}'
post "analyze — text lowercased"  "/analyze/text"  200 '.lowercased == "hello world"' '{"text":"hello world"}'
post "analyze — list length"      "/analyze/list"  200 '.length == 5' '{"numbers":[1,2,3,4,5]}'
post "analyze — list last"        "/analyze/list"  200 '.last == 50' '{"numbers":[10,20,30,40,50]}'
post "analyze — chain results"    "/analyze/chain" 200 '.results == [12,17]' '{"value":5}'
post "analyze — chain count"      "/analyze/chain" 200 '.count == 2' '{"value":5}'
get  "analyze — math direct broadcast" "/analyze/math" 200 '.abs == 4.876 and .rounded == -4.88'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 27 — Schema contracts showcase
# ═══════════════════════════════════════════════════════════════════════════════
section "27. Schema contracts"

# Request-only schema: validates input, free response
post "contracts — request-only ok"       "/contracts/request-only" 200 '.received == "Alice"'  '{"name":"Alice","active":true}'
post "contracts — request-only 422"      "/contracts/request-only" 422 ''                       '{"name":"Alice"}'

# Response-only schema: free input, response stripped to item_response fields
post "contracts — response-only strip"   "/contracts/response-only" 200 '.secret == null'       '{"name":"Bob"}'
post "contracts — response-only shape"   "/contracts/response-only" 200 '.id != null and .name != null and .active != null' '{"name":"Bob"}'

# Full contract: validated input + shaped output
post "contracts — full ok"               "/contracts/full"     200 '.secret == null and .name == "Alice"' '{"name":"Alice","active":true}'
post "contracts — full 422"              "/contracts/full"     422 ''                                      '{"name":"Alice"}'

# Composed schema: nested address reference
ADDR='{"name":"Alice","email":"alice@example.com","address":{"street":"Rua A","city":"SP","zipcode":"01310-100"}}'
post "contracts — composed 201"          "/contracts/composed" 201 '.id == 1 and .address.city == "SP"'   "${ADDR}"
post "contracts — composed strip"        "/contracts/composed" 201 '.secret == null'                       "${ADDR}"
post "contracts — composed 422"          "/contracts/composed" 422 ''                                      '{"name":"Alice","email":"alice@example.com"}'

# Optional field: age is optional in contact_payload
ADDR_BASE='{"name":"Bob","email":"bob@example.com","address":{"street":"Av B","city":"RJ","zipcode":"20040-020"}}'
post "contracts — optional without age"  "/contracts/optional" 200 '.name == "Bob"'  "${ADDR_BASE}"
post "contracts — optional with age"     "/contracts/optional" 200 '.id == 30'        '{"name":"Bob","email":"bob@example.com","age":30,"address":{"street":"Av B","city":"RJ","zipcode":"20040-020"}}'
post "contracts — math namespace"        "/contracts/math"     200 '.rounded == 12.35 and .bounded == 10' '{"value":12.345,"min":0,"max":10}'
post "contracts — enum + decimal ok"     "/contracts/api-types" 200 '.status == "paid" and .amount == "19.90"' '{"status":"paid","amount":"19.900"}'
post "contracts — decimal rejects scientific notation" "/contracts/api-types" 422 '' '{"status":"paid","amount":"1e3"}'
post "contracts — enum rejects unknown"  "/contracts/api-types" 422 '' '{"status":"failed","amount":"19.90"}'
post "contracts — decimal rejects float" "/contracts/api-types" 422 '' '{"status":"paid","amount":19.90}'
get  "contracts — enum + decimal constructor" "/contracts/api-types/constructor" 200 '.status == "paid" and .amount == "19.90" and .ignored == null'
get  "contracts — constructor rejects extra field" "/contracts/api-types/constructor-extra" 500 '.error != null'
get  "contracts — decimal methods" "/contracts/api-types/methods" 200 \
     '.sum == "22.40" and .difference == "19.00" and .product == "29.97" and .quotient == "9.95" and .half_even_down == "2.34" and .half_even_up == "2.36" and .floor == "19" and .ceil == "20" and .trunc == "-19" and .abs == "19.90" and .scale == 2 and .to_integer == -19 and .to_float == 1.25 and .to_string == "19.90" and .greater_than == true and .integer_compare == true'
get  "contracts — enum + decimal cache transport" "/contracts/api-types/cache" 200 '.status == "pending" and .amount == "7.50"'
get  "contracts — constructor reply"     "/contracts/constructor/reply" 200 '.message == "constructed" and .count == 2'
get  "contracts — constructor cache"     "/contracts/constructor/cache" 200 '.message == "cached" and .count == 3'

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 28 — Queue module (v0.8)
# Requires structured MARRETA_QUEUE_* config (RabbitMQ). Skipped if queue is not configured.
# ═══════════════════════════════════════════════════════════════════════════════
section "28. Queue module"

QUEUE_ENABLED=false
if curl -sf "${BASE}/_health" | jq -e '.queue == "connected"' > /dev/null 2>&1; then
    QUEUE_ENABLED=true
fi

if [[ "$QUEUE_ENABLED" == "true" ]]; then
    # ── queue.push (point-to-point) ──────────────────────────────────────────

    # free-form push — full payload forwarded to consumer
    post "queue — push no schema"        "/queue/push"           202 '.queued == true and .order_id == 1' \
         '{"order_id":1,"customer":"Alice","total":99.9,"secret":"hidden"}'

    # schema-filtered push — secret stripped before reaching broker
    post "queue — push with schema"      "/queue/push-schema"    202 '.queued == true and .order_id == 2' \
         '{"order_id":2,"customer":"Bob","total":50.0,"secret":"should-be-stripped"}'

    post "queue — push constructor"      "/queue/push-constructor" 202 '.queued == true and .order_id == 41' \
         '{}'

    # schema validation on HTTP side — missing required field → 422, nothing enqueued
    post "queue — push schema 422"       "/queue/push-schema"    422 '' \
         '{"customer":"Carol","secret":"no-order-id"}'

    # invalid payload to typed queue — consumer schema mismatch → nack (no requeue), server keeps running
    post "queue — push typed invalid"    "/queue/push-typed-invalid" 202 '.queued == true' \
         '{"bad_field":"not-an-order"}'
    wait_for_log_fields \
        "queue — typed invalid emits consumer schema_rejected event" \
        '"kind":"consumer"' \
        '"consumer_kind":"queue"' \
        '"target":"ft.typed"' \
        '"status":"schema_rejected"' \
        '"duration_ms":'

    # push to nack queue — consumer discards the message (nack, no requeue)
    post "queue — push reject"           "/queue/push-reject"    202 '.queued == true' \
         '{"order_id":5,"reason":"bad_payload"}'
    wait_for_log_fields \
        "queue — explicit nack emits consumer event" \
        '"kind":"consumer"' \
        '"consumer_kind":"queue"' \
        '"target":"ft.rejected"' \
        '"status":"nack"' \
        '"duration_ms":'

    # push to requeue queue — consumer nacks with requeue for retry
    post "queue — push requeue"          "/queue/push-requeue"   202 '.queued == true' \
         '{"order_id":6,"reason":"transient_error"}'
    wait_for_log_fields \
        "queue — explicit nack requeue emits consumer event" \
        '"kind":"consumer"' \
        '"consumer_kind":"queue"' \
        '"target":"ft.requeue"' \
        '"status":"nack_requeue"' \
        '"duration_ms":'

    # ── queue pipeline ────────────────────────────────────────────────────────

    # push via pipeline — payload flows through >> and queue.push returns it
    post "queue — push pipeline"           "/queue/push-pipeline"  202 '.order_id == 10' \
         '{"order_id":10,"customer":"Pipeline"}'

    # publish via pipeline — same for topics
    post "queue — publish pipeline"        "/queue/publish-pipeline" 202 '.order_id == 11' \
         '{"order_id":11,"customer":"PipelineTopic"}'

    # ── topic.publish (pub/sub) ───────────────────────────────────────────────

    # free-form publish — full payload to ft.order.created topic
    post "queue — publish no schema"     "/queue/publish"        202 '.published == true' \
         '{"order_id":3,"customer":"Dave","total":75.0}'

    # schema-filtered publish — secret stripped before reaching subscribers
    post "queue — publish with schema"   "/queue/publish-schema" 202 '.published == true and .order_id == 4' \
         '{"order_id":4,"customer":"Eve","total":120.0,"secret":"strip-me"}'

    post "queue — publish constructor"   "/queue/publish-constructor" 202 '.published == true and .order_id == 42' \
         '{}'

    # invalid payload to typed topic — consumer schema mismatch → nack (no requeue), server keeps running
    post "queue — publish topic invalid" "/queue/publish-topic-invalid" 202 '.published == true' \
         '{"bad_field":"not-an-order"}'

    # schema validation on HTTP side — missing required field → 422, nothing published
    post "queue — publish schema 422"    "/queue/publish-schema" 422 '' \
         '{"customer":"Frank"}'

    post "queue — time push typed payload" "/queue/time-push" 202 \
         '.queued == true and .billing_date == "2026-04-27"' \
         "${TIME_PAYLOAD}"

    time_queue_ok=false
    time_queue_body=""
    for _ in $(seq 1 20); do
        time_queue_body="$(curl -s "${BASE}/queue/time-last" 2>/dev/null || true)"
        if echo "${time_queue_body}" | jq -e '.created_unix == 1777295445 and .opening_hour == 9 and .sla_hours == 1.5 and .window_days == 3' >/dev/null 2>&1; then
            time_queue_ok=true
            break
        fi
        sleep 0.2
    done

    if [[ "${time_queue_ok}" == "true" ]]; then
        echo -e "  ${GREEN}PASS${RESET} queue — time payload processed with native semantics"
        PASS=$((PASS + 1))
    else
        echo -e "  ${RED}FAIL${RESET} queue — time payload processed with native semantics"
        echo "       body: ${time_queue_body}"
        FAIL=$((FAIL + 1))
    fi

    post "queue — contract types push typed payload" "/queue/contract-types-push" 202 \
         '.queued == true and .status == "paid"' \
         '{"status":"paid","amount":"13.370"}'

    contract_queue_ok=false
    contract_queue_body=""
    for _ in $(seq 1 20); do
        contract_queue_body="$(curl -s "${BASE}/queue/contract-types-last" 2>/dev/null || true)"
        if echo "${contract_queue_body}" | jq -e '.status == "paid" and .amount == "13.37"' >/dev/null 2>&1; then
            contract_queue_ok=true
            break
        fi
        sleep 0.2
    done

    if [[ "${contract_queue_ok}" == "true" ]]; then
        echo -e "  ${GREEN}PASS${RESET} queue — contract types payload processed"
        PASS=$((PASS + 1))
    else
        echo -e "  ${RED}FAIL${RESET} queue — contract types payload processed"
        echo "       body: ${contract_queue_body}"
        FAIL=$((FAIL + 1))
    fi

    post "queue — contract types publish typed payload" "/queue/contract-types-publish" 202 \
         '.published == true and .status == "pending"' \
         '{"status":"pending","amount":"15.550"}'

    contract_topic_ok=false
    contract_topic_body=""
    for _ in $(seq 1 20); do
        contract_topic_body="$(curl -s "${BASE}/queue/contract-types-topic-last" 2>/dev/null || true)"
        if echo "${contract_topic_body}" | jq -e '.status == "pending" and .amount == "15.55"' >/dev/null 2>&1; then
            contract_topic_ok=true
            break
        fi
        sleep 0.2
    done

    if [[ "${contract_topic_ok}" == "true" ]]; then
        echo -e "  ${GREEN}PASS${RESET} queue — contract types topic payload processed"
        PASS=$((PASS + 1))
    else
        echo -e "  ${RED}FAIL${RESET} queue — contract types topic payload processed"
        echo "       body: ${contract_topic_body}"
        FAIL=$((FAIL + 1))
    fi

    # ── Spec 060: topic fan-out (2N) vs queue point-to-point (N) ──────────────
    # Two consumers on ft.fanout must each receive every publish (2N); two
    # consumers on ft.compete compete, so each push lands on exactly one (N).
    post "fanout — reset counters" "/fanout/reset" 200 '.reset == true' '{}'

    for n in 1 2 3; do
        post "fanout — publish topic #${n}" "/fanout/publish-topic" 202 '.published == true' "{\"n\":${n}}"
    done
    fanout_ok=false
    fanout_body=""
    for _ in $(seq 1 40); do
        fanout_body="$(curl -s "${BASE}/fanout/topic-count" 2>/dev/null || true)"
        if echo "${fanout_body}" | jq -e '(.count | tonumber) == 6' >/dev/null 2>&1; then
            fanout_ok=true
            break
        fi
        sleep 0.2
    done
    if [[ "${fanout_ok}" == "true" ]]; then
        echo -e "  ${GREEN}PASS${RESET} fanout — topic delivers to both consumers (2N)"
        PASS=$((PASS + 1))
    else
        echo -e "  ${RED}FAIL${RESET} fanout — topic delivers to both consumers (2N)"
        echo "       body: ${fanout_body}"
        FAIL=$((FAIL + 1))
    fi

    for n in 1 2 3; do
        post "fanout — push queue #${n}" "/fanout/push-queue" 202 '.queued == true' "{\"n\":${n}}"
    done
    compete_ok=false
    for _ in $(seq 1 40); do
        compete_body="$(curl -s "${BASE}/fanout/queue-count" 2>/dev/null || true)"
        if echo "${compete_body}" | jq -e '(.count | tonumber) == 3' >/dev/null 2>&1; then
            compete_ok=true
            break
        fi
        sleep 0.2
    done
    # Settle window: a fan-out bug would push the count past 3 toward 6.
    sleep 1
    compete_final="$(curl -s "${BASE}/fanout/queue-count" 2>/dev/null || true)"
    if [[ "${compete_ok}" == "true" ]] && echo "${compete_final}" | jq -e '(.count | tonumber) == 3' >/dev/null 2>&1; then
        echo -e "  ${GREEN}PASS${RESET} fanout — queue is point-to-point (N, not 2N)"
        PASS=$((PASS + 1))
    else
        echo -e "  ${RED}FAIL${RESET} fanout — queue is point-to-point (N, not 2N)"
        echo "       body: ${compete_final}"
        FAIL=$((FAIL + 1))
    fi

    post "queue — math push payload" "/queue/math-push" 202 \
         '.queued == true and .order_id == 12' \
         '{"order_id":12,"customer":"Math","total":12.345}'

    math_queue_ok=false
    math_queue_body=""
    for _ in $(seq 1 20); do
        math_queue_body="$(curl -s "${BASE}/queue/math-last" 2>/dev/null || true)"
        if echo "${math_queue_body}" | jq -e '.rounded_total == 12.35 and .bounded_total == 12.345 and .customer_size == 4' >/dev/null 2>&1; then
            math_queue_ok=true
            break
        fi
        sleep 0.2
    done

    if [[ "${math_queue_ok}" == "true" ]]; then
        echo -e "  ${GREEN}PASS${RESET} queue — math payload processed"
        PASS=$((PASS + 1))
    else
        echo -e "  ${RED}FAIL${RESET} queue — math payload processed"
        echo "${math_queue_body:-null}" | sed 's/^/       /'
        FAIL=$((FAIL + 1))
    fi
else
    echo -e "  ${YELLOW}SKIP${RESET} Queue tests — structured MARRETA_QUEUE_* config not configured"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 29. Cache module
# ─────────────────────────────────────────────────────────────────────────────

CACHE_ENABLED=false
if [[ -n "${MARRETA_CACHE_PROVIDER:-}" ]] || curl -sf "${BASE}/cache/get/probe" > /dev/null 2>&1; then
    CACHE_ENABLED=true
fi

if [[ "$CACHE_ENABLED" == "true" ]]; then
    echo -e "\n${BOLD}── 29. Cache module ──${RESET}"

    # -- set / get --
    post "cache — set"                  "/cache/set"            200 '.ok == true' \
         '{"key":"ft:k1","value":"hello"}'

    get  "cache — get hit"              "/cache/get/ft:k1"      200 '.value == "hello"'

    get  "cache — get miss"             "/cache/get/ft:missing" 200 '.value == null'

    # -- set with TTL --
    post "cache — set with ttl"         "/cache/set-ttl"        200 '.ok == true' \
         '{"key":"ft:ttl","value":42}'

    # -- set only_if_absent --
    post "cache — set-nx first"         "/cache/set-nx"         200 '.stored == "first"' \
         '{"key":"ft:nx","value":"first"}'

    post "cache — set-nx second (null)" "/cache/set-nx"         200 '.stored == null' \
         '{"key":"ft:nx","value":"second"}'

    # -- delete --
    delete "cache — delete existing"    "/cache/delete/ft:k1"   200 '.existed == true'

    delete "cache — delete missing"     "/cache/delete/ft:gone" 200 '.existed == false'

    # -- exists --
    post "cache — exists setup"         "/cache/set"            200 '' \
         '{"key":"ft:ex","value":1}'

    get  "cache — exists true"          "/cache/exists/ft:ex"   200 '.exists == true'

    get  "cache — exists false"         "/cache/exists/ft:nope" 200 '.exists == false'

    # -- incr / decr --
    post "cache — incr"                 "/cache/incr"           200 '.value == 1' \
         '{"key":"ft:counter"}'

    post "cache — incr again"           "/cache/incr"           200 '.value == 2' \
         '{"key":"ft:counter"}'

    post "cache — incr by 5"            "/cache/incr-by"        200 '.value == 7' \
         '{"key":"ft:counter"}'

    post "cache — decr"                 "/cache/decr"           200 '.value == 6' \
         '{"key":"ft:counter"}'

    # -- set_many / get_many --
    post "cache — set_many"             "/cache/set-many"       200 '.ok == true' \
         '{"entries":{"ft:a":10,"ft:b":20}}'

    post "cache — get_many"             "/cache/get-many"       200 '."ft:a" == 10 and ."ft:b" == 20' \
         '{"keys":["ft:a","ft:b","ft:miss"]}'

    # -- pipeline (cache.set returns value) --
    post "cache — pipeline"             "/cache/pipeline"       200 '.x == 99' \
         '{"x":99}'

    post "cache — math integration"     "/math/cache_counter"   200 '.step == 3 and .current == 3 and .bounded == 3' \
         '{"step":2.2}'
else
    echo -e "  ${YELLOW}SKIP${RESET} Cache tests — structured MARRETA_CACHE_* config not configured"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 30. Filesystem module
# ─────────────────────────────────────────────────────────────────────────────

echo -e "\n${BOLD}── 30. Filesystem module ──${RESET}"

FS_TMP_BASE="/tmp/marreta-functional-tests-fs-$$"
FS_PATH_A="${FS_TMP_BASE}-a.txt"
FS_PATH_B="${FS_TMP_BASE}-b.txt"

post "fs — write/read" "/fs/write-read" 200 \
     '.written == "hello" and .appended == "\nworld" and .exists == true and .content == "hello\nworld"' \
     "{\"path\":\"${FS_PATH_A}\",\"content\":\"hello\",\"suffix\":\"\\nworld\"}"

post "fs — pipeline pass-through" "/fs/pipeline" 200 \
     '.persisted == "payload" and .loaded == "payload"' \
     "{\"path\":\"${FS_PATH_B}\",\"content\":\"payload\"}"

post "fs — read missing returns io_error" "/fs/read" 500 \
     '.code == "io_error" and (.error | contains("file not found"))' \
     "{\"path\":\"${FS_TMP_BASE}-missing.txt\"}"

post "fs — read missing can be rescued" "/fs/read-rescue" 200 \
     '.code == "io_error" and (.error | contains("file not found"))' \
     "{\"path\":\"${FS_TMP_BASE}-missing.txt\"}"

post "fs — write rejects non-string content" "/fs/write-only" 500 \
     '.code == "type_error" and (.error | contains("must be String"))' \
     "{\"path\":\"${FS_TMP_BASE}-type.txt\",\"content\":42}"

post "fs — status present" "/fs/status" 200 \
     '.status == "present"' \
     "{\"path\":\"${FS_PATH_A}\"}"

post "fs — delete existing" "/fs/delete" 200 \
     '.deleted == true and .still_exists == false' \
     "{\"path\":\"${FS_PATH_A}\"}"

post "fs — status missing" "/fs/status" 200 \
     '.status == "missing"' \
     "{\"path\":\"${FS_PATH_A}\"}"

post "fs — delete missing is idempotent" "/fs/delete" 200 \
     '.deleted == false and .still_exists == false' \
     "{\"path\":\"${FS_PATH_A}\"}"

post "fs — cleanup pipeline file" "/fs/delete" 200 \
     '.deleted == true and .still_exists == false' \
     "{\"path\":\"${FS_PATH_B}\"}"

# ─────────────────────────────────────────────────────────────────────────────
# 31. JSON namespace
# ─────────────────────────────────────────────────────────────────────────────

echo -e "\n${BOLD}── 31. JSON namespace ──${RESET}"

JSON_PATH_A="${FS_TMP_BASE}-json-a.json"

check "json — parse raw body" POST "/json/parse" 200 \
    '.customer == "Ana" and .item_count == 3 and .active == true' \
    "${H_JSON[@]}" -d '{"customer":{"name":"Ana"},"items":[1,2,3],"active":true}'

check "json — parse rescue invalid json" POST "/json/parse-rescue" 200 \
    '.code == "runtime_error" and (.error | contains("invalid JSON"))' \
    "${H_JSON[@]}" -d '{bad'

get  "json — stringify compact + pretty" "/json/stringify" 200 \
    '.compact == "{\"id\":1,\"name\":\"Ana\",\"created_at\":\"2026-04-27T13:10:45Z\",\"billing_date\":\"2026-04-27\",\"opens_at\":\"09:30:00\",\"sla\":\"PT5400S\",\"window\":{\"start\":\"2026-04-27\",\"end\":\"2026-04-30\"}}" and (.pretty | contains("\"id\": 1")) and (.pretty | contains("\"name\": \"Ana\""))'

post "json — fs roundtrip" "/json/fs-roundtrip" 200 \
    '.id == 7 and .name == "File JSON" and .active == true and (.text | contains("\"id\": 7")) and (.text | contains("\"name\": \"File JSON\""))' \
    "{\"path\":\"${JSON_PATH_A}\",\"document\":{\"id\":7,\"name\":\"File JSON\",\"active\":true}}"

if [[ "$CACHE_ENABLED" == "true" ]]; then
    post "json — cache roundtrip" "/json/cache-roundtrip" 200 \
         '.customer.name == "Cache Ana" and .items[1] == 2 and .active == true' \
         '{"document":{"customer":{"name":"Cache Ana"},"items":[1,2],"active":true}}'
else
    echo -e "  ${YELLOW}SKIP${RESET} json — cache roundtrip"
fi

if [[ "$QUEUE_ENABLED" == "true" ]]; then
    post "json — queue push" "/json/queue-push" 202 \
         '.queued == true and .text == "{\"order_id\":88,\"customer\":\"Queue Ana\"}"' \
         '{"document":{"order_id":88,"customer":"Queue Ana"}}'

    json_queue_ok=false
    json_queue_body=""
    for _ in $(seq 1 20); do
        json_queue_body="$(curl -s "${BASE}/json/queue-last" 2>/dev/null || true)"
        if echo "${json_queue_body}" | jq -e '.order_id == 88 and .customer == "Queue Ana"' >/dev/null 2>&1; then
            json_queue_ok=true
            break
        fi
        sleep 0.2
    done

    if [[ "${json_queue_ok}" == "true" ]]; then
        echo -e "  ${GREEN}PASS${RESET} json — queue roundtrip"
        PASS=$((PASS + 1))
    else
        echo -e "  ${RED}FAIL${RESET} json — queue roundtrip"
        echo "${json_queue_body:-null}" | sed 's/^/       /'
        FAIL=$((FAIL + 1))
    fi
else
    echo -e "  ${YELLOW}SKIP${RESET} json — queue roundtrip"
fi

get  "json — http client roundtrip" "/json/http-client-roundtrip" 200 \
    '.id == 42 and .name == "Alice" and .email == "alice@example.com"'

# ─────────────────────────────────────────────────────────────────────────────
# 32. Base64 namespace
# ─────────────────────────────────────────────────────────────────────────────

echo -e "\n${BOLD}── 32. Base64 namespace ──${RESET}"

BASE64_PATH_A="${FS_TMP_BASE}-base64-a.txt"

get  "base64 — encode basic auth token" "/base64/encode-basic" 200 \
    '.token == "Y2xpZW50OnNlY3JldA==" and .authorization == "Basic Y2xpZW50OnNlY3JldA=="'

check "base64 — decode from request header" GET "/base64/decode-header" 200 \
    '.decoded == "client:secret"' \
    -H "xbasictoken: Y2xpZW50OnNlY3JldA=="

post "base64 — url-safe encode decode" "/base64/url-safe" 200 \
    '.encoded == "Pz8_" and .decoded == "???"' \
    '{"text":"???"}'

post "base64 — decode without padding" "/base64/decode-no-padding" 200 \
    '.decoded == "hi"' \
    '{"token":"aGk"}'

check "base64 — rescue invalid input" POST "/base64/decode-rescue" 200 \
    '.code == "runtime_error" and (.error | contains("invalid Base64"))' \
    "${H_JSON[@]}" -d '{"token":"%%%"}'

post "base64 — fs roundtrip" "/base64/fs-roundtrip" 200 \
    '.encoded == "ZmlsZTp0ZXh0" and .decoded == "file:text"' \
    "{\"path\":\"${BASE64_PATH_A}\",\"text\":\"file:text\"}"

if [[ "$CACHE_ENABLED" == "true" ]]; then
    post "base64 — cache roundtrip" "/base64/cache-roundtrip" 200 \
         '.decoded == "cache:text"' \
         '{"text":"cache:text"}'
else
    echo -e "  ${YELLOW}SKIP${RESET} base64 — cache roundtrip"
fi

if [[ "$QUEUE_ENABLED" == "true" ]]; then
    post "base64 — queue push" "/base64/queue-push" 202 \
         '.queued == true and .encoded == "cXVldWU6dGV4dA=="' \
         '{"text":"queue:text"}'

    base64_queue_ok=false
    base64_queue_body=""
    for _ in $(seq 1 20); do
        base64_queue_body="$(curl -s "${BASE}/base64/queue-last" 2>/dev/null || true)"
        if echo "${base64_queue_body}" | jq -e '.decoded == "queue:text"' >/dev/null 2>&1; then
            base64_queue_ok=true
            break
        fi
        sleep 0.2
    done

    if [[ "${base64_queue_ok}" == "true" ]]; then
        echo -e "  ${GREEN}PASS${RESET} base64 — queue roundtrip"
        PASS=$((PASS + 1))
    else
        echo -e "  ${RED}FAIL${RESET} base64 — queue roundtrip"
        echo "${base64_queue_body:-null}" | sed 's/^/       /'
        FAIL=$((FAIL + 1))
    fi
else
    echo -e "  ${YELLOW}SKIP${RESET} base64 — queue roundtrip"
fi

get  "base64 — http client roundtrip" "/base64/http-client-roundtrip" 200 \
    '.decoded == "upstream:secret" and .url_decoded == "???"'

# ─────────────────────────────────────────────────────────────────────────────
# 33. UUID namespace
# ─────────────────────────────────────────────────────────────────────────────

echo -e "\n${BOLD}── 33. UUID namespace ──${RESET}"

post "uuid — generate canonical payload" "/uuid/generate" 200 \
    '(.public_id | test("^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")) and (.record_id | test("^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")) and .ordered == true and .public_length == 36 and .record_length == 36' \
    '{}'

if [[ "$CACHE_ENABLED" == "true" ]]; then
    post "uuid — cache key interpolation" "/uuid/cache-key" 200 \
         '(.key | test("^token:[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")) and .cached.customer == "Cache Ana" and .cached.active == true' \
         '{"customer":"Cache Ana","active":true}'
else
    echo -e "  ${YELLOW}SKIP${RESET} uuid — cache key interpolation"
fi

if [[ "$QUEUE_ENABLED" == "true" ]]; then
    post "uuid — queue event id" "/uuid/queue-push" 202 \
         '(.event_id | test("^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")) and .kind == "order.created"' \
         '{"kind":"order.created"}'

    uuid_queue_ok=false
    uuid_queue_body=""
    for _ in $(seq 1 20); do
        uuid_queue_body="$(curl -s "${BASE}/uuid/queue-last" 2>/dev/null || true)"
        if echo "${uuid_queue_body}" | jq -e '(.event_id | test("^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")) and .kind == "order.created"' >/dev/null 2>&1; then
            uuid_queue_ok=true
            break
        fi
        sleep 0.2
    done

    if [[ "${uuid_queue_ok}" == "true" ]]; then
        echo -e "  ${GREEN}PASS${RESET} uuid — queue roundtrip"
        PASS=$((PASS + 1))
    else
        echo -e "  ${RED}FAIL${RESET} uuid — queue roundtrip"
        echo "${uuid_queue_body:-null}" | sed 's/^/       /'
        FAIL=$((FAIL + 1))
    fi
else
    echo -e "  ${YELLOW}SKIP${RESET} uuid — queue roundtrip"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 34. Log namespace
# ─────────────────────────────────────────────────────────────────────────────

echo -e "\n${BOLD}── 34. Log namespace ──${RESET}"

LOG_PATH_A="${FS_TMP_BASE}-log-a.json"

post "log — process route keeps taps transparent" "/log/process" 200 \
    '.customer == "Ana" and .encoded == "cGF5bG9hZDp0ZXh0" and ((.reread | fromjson).customer == "Ana") and ((.reread | fromjson).items | length == 2)' \
    "{\"message\":\"payload:text\",\"path\":\"${LOG_PATH_A}\",\"document\":{\"customer\":\"Ana\",\"items\":[1,2]}}"
wait_for_log_fields "log — info stdout event" '"kind":"app_log"' '"level":"info"' '"data":{"message":"payload:text"'
wait_for_log_fields "log — debug stdout event" '"kind":"app_log"' '"level":"debug"' '"data":{"customer":"Ana","items":[1,2]}'
wait_for_log_fields "request log — matched route stdout event" \
    '"kind":"request"' \
    '"method":"POST"' \
    '"path":"/log/process"' \
    '"route":"/log/process"' \
    '"status":200' \
    '"duration_ms":'

post "log — interpolation message" "/log/interpolation" 200 \
    '.ok == true and .order_id == 42 and .customer == "Ana"' \
    '{"order_id":42,"customer":"Ana"}'
wait_for_log_fields "log — interpolation stdout message" '"kind":"app_log"' '"level":"info"' '"data":"loading order 42 for Ana"'

post "log — warn degraded flow" "/log/retry-warning" 200 \
    '.status == "degraded" and .retries == 3 and .provider == "stripe"' \
    '{"retries":3,"provider":"stripe"}'
wait_for_log_fields "log — warn stdout event" '"kind":"app_log"' '"level":"warn"' '"data":{"event":"provider.retrying","retries":3,"provider":"stripe"}'

post "log — error failure report" "/log/failure-report" 200 \
    '.accepted == true and .order_id == 99 and .status == "recorded"' \
    '{"order_id":99,"reason":"gateway_timeout"}'
wait_for_log_fields "log — error stdout event" '"kind":"app_log"' '"level":"error"' '"data":{"event":"payment.failed","order_id":99,"reason":"gateway_timeout"}'

post "log — pipeline integration" "/log/pipeline" 200 \
    '.encoded == "cGF5bG9hZDp0ZXh0" and ((.reread | fromjson).customer == "Ana")' \
    "{\"message\":\"payload:text\",\"path\":\"${LOG_PATH_A}\",\"document\":{\"customer\":\"Ana\",\"items\":[1,2]}}"

post "log — broadcast integration" "/log/broadcast" 200 \
    '.first.event == "broadcasted" and .second.event == "broadcasted" and .first.order_id == 9 and .second.order_id == 9' \
    '{"event":"broadcasted","order_id":9}'
wait_for_log_fields "log — broadcast info stdout event" '"kind":"app_log"' '"level":"info"' '"data":{"event":"broadcasted","order_id":9}'
wait_for_log_fields "log — broadcast warn stdout event" '"kind":"app_log"' '"level":"warn"' '"data":{"event":"broadcasted","order_id":9}'

post "log — rescue unsupported runtime value" "/log/rescue" 200 \
    '.code == "type_error" and (.message | contains("log.info()"))' \
    '{}'

if [[ "${QUEUE_ENABLED}" == "true" ]]; then
    post "log — queue push" "/log/queue-push" 202 \
        '.queued == true and .kind == "audit"' \
        '{"kind":"audit","message":"queued event"}'

    log_queue_ok=false
    log_queue_body=""
    for _ in $(seq 1 20); do
        log_queue_body="$(curl -s "${BASE}/log/queue-last" 2>/dev/null || true)"
        if echo "${log_queue_body}" | jq -e '.kind == "audit" and .message == "queued event"' >/dev/null 2>&1; then
            log_queue_ok=true
            break
        fi
        sleep 0.5
    done

    if [[ "${log_queue_ok}" == "true" ]]; then
        echo -e "  ${GREEN}PASS${RESET} log — queue roundtrip"
        PASS=$((PASS + 1))
    else
        echo -e "  ${RED}FAIL${RESET} log — queue roundtrip"
        echo "${log_queue_body:-null}" | sed 's/^/       /'
        FAIL=$((FAIL + 1))
    fi
    wait_for_log_fields \
        "log — queue consumer stdout event" \
        '"kind":"app_log"' \
        '"level":"info"' \
        '"kind":"audit"' \
        '"message":"queued event"'
else
    echo -e "  ${YELLOW}SKIP${RESET} log — queue roundtrip"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 35. W3C Trace Context
# ─────────────────────────────────────────────────────────────────────────────

echo -e "\n${BOLD}── 35. W3C Trace Context ──${RESET}"

TRACE_ID="0af7651916cd43dd8448eb211c80319c"
TRACE_PARENT_SPAN="b7ad6b7169203331"
TRACE_FLAGS="01"
TRACEPARENT="00-${TRACE_ID}-${TRACE_PARENT_SPAN}-${TRACE_FLAGS}"
TRACESTATE="rojo=00f067aa0ba902b7"

check "trace context — inbound log route" POST "/trace-context/log" 200 \
    '.ok == true and .order_id == 77' \
    "${H_JSON[@]}" \
    -H "traceparent: ${TRACEPARENT}" \
    -H "tracestate: ${TRACESTATE}" \
    -d '{"order_id":77}'
wait_for_log_pattern "trace context — app log includes trace fields" "\"kind\":\"app_log\",\"level\":\"info\",\"trace_id\":\"${TRACE_ID}\",\"span_id\":"
wait_for_log_pattern "trace context — request log includes trace fields" "\"kind\":\"request\",\"trace_id\":\"${TRACE_ID}\",\"span_id\":"

check "trace context — invalid traceparent does not fail request" POST "/trace-context/log" 200 \
    '.ok == true and .order_id == 1' \
    "${H_JSON[@]}" \
    -H "traceparent: garbage" \
    -d '{"order_id":1}'

trace_tmp="$(mktemp)"
trace_status="$(curl -s -o "${trace_tmp}" -w "%{http_code}" \
    -H "traceparent: ${TRACEPARENT}" \
    -H "tracestate: ${TRACESTATE}" \
    "${BASE}/trace-context/outbound" 2>/dev/null)"
trace_body="$(cat "${trace_tmp}")"
rm -f "${trace_tmp}"

if [[ "${trace_status}" == "200" ]] && echo "${trace_body}" | jq -e \
    --arg trace_id "${TRACE_ID}" \
    --arg parent_span "${TRACE_PARENT_SPAN}" \
    --arg state "${TRACESTATE}" \
    '(.traceparent | startswith("00-" + $trace_id + "-")) and
     (.traceparent | endswith("-01")) and
     (.traceparent | contains($parent_span) | not) and
     .tracestate == $state' >/dev/null 2>&1; then
    echo -e "  ${GREEN}PASS${RESET} trace context — http_client propagates child traceparent"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} trace context — http_client propagates child traceparent"
    echo "       status: ${trace_status}"
    echo "${trace_body:-null}" | sed 's/^/       /'
    FAIL=$((FAIL + 1))
fi

# ─────────────────────────────────────────────────────────────────────────────
# 35b. Async Trace Propagation
# ─────────────────────────────────────────────────────────────────────────────

echo -e "\n${BOLD}── 35b. Async Trace Propagation ──${RESET}"

ASYNC_TRACE_ID="11111111111111111111111111111111"
ASYNC_PARENT_SPAN="2222222222222222"
ASYNC_TRACEPARENT="00-${ASYNC_TRACE_ID}-${ASYNC_PARENT_SPAN}-01"

if [[ "$QUEUE_ENABLED" == "true" ]]; then
    check "async trace — queue push accepted" POST "/queue/async-trace-push" 202 \
        '.queued == true and .order_id == 3601' \
        "${H_JSON[@]}" \
        -H "traceparent: ${ASYNC_TRACEPARENT}" \
        -d '{"order_id":3601}'

    wait_for_log_fields \
        "async trace — consumer log keeps trace_id" \
        '"kind":"app_log"' \
        '"event":"async.trace.consumer"' \
        "\"trace_id\":\"${ASYNC_TRACE_ID}\"" \
        '"span_id":'

    wait_for_log_fields \
        "async trace — consumer runtime event keeps trace_id" \
        '"kind":"consumer"' \
        '"consumer_kind":"queue"' \
        '"target":"ft.async.trace"' \
        '"status":"ack"' \
        "\"trace_id\":\"${ASYNC_TRACE_ID}\"" \
        '"span_id":'

    wait_for_log_fields \
        "async trace — consumer outbound request keeps trace_id" \
        '"kind":"request"' \
        '"path":"/trace-context/stub/headers"' \
        "\"trace_id\":\"${ASYNC_TRACE_ID}\""

    FANOUT_TRACE_ID="33333333333333333333333333333333"
    FANOUT_PARENT_SPAN="4444444444444444"
    FANOUT_TRACEPARENT="00-${FANOUT_TRACE_ID}-${FANOUT_PARENT_SPAN}-01"

    check "async trace — topic publish accepted" POST "/queue/async-trace-publish" 202 \
        '.published == true and .order_id == 3602' \
        "${H_JSON[@]}" \
        -H "traceparent: ${FANOUT_TRACEPARENT}" \
        -d '{"order_id":3602}'

    wait_for_log_fields \
        "async trace — fan-out subscriber A keeps trace_id" \
        '"kind":"app_log"' \
        '"event":"async.trace.subscriber_a"' \
        "\"trace_id\":\"${FANOUT_TRACE_ID}\"" \
        '"span_id":'

    wait_for_log_fields \
        "async trace — fan-out subscriber B keeps trace_id" \
        '"kind":"app_log"' \
        '"event":"async.trace.subscriber_b"' \
        "\"trace_id\":\"${FANOUT_TRACE_ID}\"" \
        '"span_id":'

    wait_for_log_fields \
        "async trace — fan-out subscriber A runtime event keeps trace_id" \
        '"kind":"consumer"' \
        '"consumer_kind":"topic"' \
        '"target":"ft.async.trace.created"' \
        "\"trace_id\":\"${FANOUT_TRACE_ID}\"" \
        '"span_id":'

    wait_for_log_fields \
        "async trace — fan-out subscriber B runtime event keeps trace_id" \
        '"kind":"consumer"' \
        '"consumer_kind":"topic"' \
        '"target":"ft.async.trace.created"' \
        "\"trace_id\":\"${FANOUT_TRACE_ID}\"" \
        '"span_id":'
else
    echo -e "  ${YELLOW}SKIP${RESET} async trace — queue propagation"
    echo -e "  ${YELLOW}SKIP${RESET} async trace — topic fan-out propagation"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 36. HTTP Client module
# ─────────────────────────────────────────────────────────────────────────────

echo -e "\n${BOLD}── 36. HTTP Client module ──${RESET}"

# -- 1. Basic verbs --
get  "http_client — GET"    "/http-client/get/42"  200 '.name == "Alice"'

post "http_client — POST"   "/http-client/post"    201 '.order_id == "ord-123"' \
     '{"item":"book"}'

post "http_client — schema constructor + response schema" "/http-client/schema-contract" 201 \
     '.order_id == "ord-123" and .status == "created"' \
     '{"item":"book","quantity":2}'

put  "http_client — PUT"    "/http-client/put/7"   200 '.updated == true' \
     '{"name":"updated"}'

patch "http_client — PATCH" "/http-client/patch/7" 200 '.patched == true' \
      '{"active":false}'

delete "http_client — DELETE" "/http-client/delete/7" 200 '.deleted == true'

# -- 2. Named params --
get  "http_client — query params"   "/http-client/with-query"   200 '.q == "marreta"'
get  "http_client — custom headers" "/http-client/with-headers" 200 '.received_key == "secret-123"'
get  "http_client — timeout param"  "/http-client/with-timeout" 200 '.ok == true'

# -- 3. Response envelope --
get  "http_client — status envelope"    "/http-client/status-envelope"   200 '.status == 404'
get  "http_client — response headers"   "/http-client/response-headers"  200 '.has_content_type == true'

# -- 4. 4xx/5xx not errors --
get  "http_client — 500 not error"      "/http-client/non-2xx-no-error"  200 '.status == 500'
get  "http_client — match 200"          "/http-client/match-200"  200 '.label == "ok"'
get  "http_client — match 404"          "/http-client/match-404"  200 '.label == "not_found"'
get  "http_client — match 500"          "/http-client/match-500"  200 '.label == "server_error"'
get  "http_client — propagate error"    "/http-client/propagate-error"   500 '.error != null'
get  "http_client — math integration"   "/http-client/math-query" 200 '.limit == "10" and .bounded_limit == 10'
get  "http_client — enum + decimal response schema" "/http-client/contract-types" 200 '.status == "paid" and .amount == "88.88" and .upstream_extra == null'

# -- 5. Rescue on connection failure --
get  "http_client — rescue fallback"    "/http-client/rescue-fallback"   200 '.fallback == true'

# -- 6. Fire-and-forget --
post "http_client — fire and forget"    "/http-client/fire-and-forget"   202 '.dispatched == true' \
     '{"event":"user.created"}'

# -- 7. Pipeline input --
post "http_client — pipeline POST"     "/http-client/pipeline-post"     201 '.order_id == "ord-123"' \
     '{"item":"widget"}'

put  "http_client — pipeline PUT"      "/http-client/pipeline-put/5"    200 '.updated == true' \
     '{"name":"updated"}'

patch "http_client — pipeline PATCH"  "/http-client/pipeline-patch/5"  200 '.patched == true' \
      '{"active":false}'

get  "http_client — pipeline GET query" "/http-client/pipeline-query"   200 '.q == "lang"'

# -- 8. Pipeline output --
get  "http_client — pipeline to cache"  "/http-client/pipeline-to-cache/99" 200 '.cached.name == "Alice"'
get  "http_client — read-through"       "/http-client/read-through/88"      200 '.from_cache == "Alice"'
get  "http_client — pipeline map"       "/http-client/pipeline-map"         200 'length == 3'
get  "http_client — chain multi"        "/http-client/chain-multi"          200 '.count == 3'

# ─────────────────────────────────────────────────────────────────────────────
# 34. Iteration & accumulation
# ─────────────────────────────────────────────────────────────────────────────

echo -e "\n${BOLD}── 34. Iteration & accumulation ──${RESET}"

get  "iteration — range default"        "/iteration/range/default"     200 '.values == [1,2,3,4,5]'
get  "iteration — range bounds"         "/iteration/range/bounds"      200 '.values == [3,4,5,6] and .empty == []'
get  "iteration — reduce sum"           "/iteration/reduce/sum"        200 '.total == 15'
get  "iteration — reduce empty"         "/iteration/reduce/empty"      200 '.result == "seed"'
get  "iteration — while counter"        "/iteration/while/counter"     200 '.counter == 4'
get  "iteration — while limit"          "/iteration/while/limit"       500 '.error | contains("10000 iterations")'
get  "iteration — recursion factorial"  "/iteration/recursion/factorial/5" 200 '.result == 120'
get  "iteration — mutual recursion"     "/iteration/recursion/mutual/6"    200 '.even == true'
get  "iteration — list methods sum"     "/iteration/list-methods"      200 '.sum == 15'
get  "iteration — list methods mean"    "/iteration/list-methods"      200 '(.mean > 7.59) and (.mean < 7.61)'
get  "iteration — list methods median"  "/iteration/list-methods"      200 '.median == 7.5'
get  "iteration — list methods stddev"  "/iteration/list-methods"      200 '(.std_dev > 0.86) and (.std_dev < 0.87)'
get  "iteration — list methods zip"     "/iteration/list-methods"      200 '.zipped[0][0] == "Ana" and .zipped[0][1] == 30'
get  "iteration — list methods empty"   "/iteration/list-methods/empty" 200 '.mean == null and .median == null and .std_dev == null'
get  "iteration — zip mismatch"         "/iteration/zip/error"         500 '.error | contains("same length")'
get  "iteration — conversions"          "/iteration/conversions"       200 '.int_value == 25 and .int_fallback == 0 and .float_value == 19.9 and .float_fallback == 0 and .stringified == "42" and .bool_null == false and .bool_text == true'
get  "iteration — math integration"     "/iteration/math/reduce"       200 '.peak == 7.9 and .rounded_peak == 7.9 and .bounded_peak == 5'
post "iteration — private schema ok"    "/iteration/private/schema"    201 '.name == "Widget" and .qty == 2 and .factor == 3' '{"name":"Widget","qty":2}'
post "iteration — private schema bad"   "/iteration/private/schema"    422 '.error | contains("expected integer")' '{"name":"Widget","qty":"two"}'

# ─────────────────────────────────────────────────────────────────────────────
# 35. Authentication & authorization
# ─────────────────────────────────────────────────────────────────────────────

echo -e "\n${BOLD}── 35. Authentication & authorization ──${RESET}"

check "auth — api key missing"    GET "/auth/me"        401 '.error == "unauthorized"'
check "auth — api key valid"      GET "/auth/me"        200 '.provider == "internal_auth" and .type == "api_key" and .subject == "functional-user" and .user_id == "functional-user" and .claims == {}' -H "x-api-key: functional-secret"
check "auth — allow succeeds"     GET "/auth/owned"     200 '.allowed == true and .user_id == "functional-user"' -H "x-api-key: functional-secret"
check "auth — allow forbidden"    GET "/auth/forbidden" 403 '.error == "forbidden"' -H "x-api-key: functional-secret"
check "auth — time on protected route" GET "/auth/time/me" 200 '.provider == "internal_auth" and .subject == "functional-user" and (.checked_at | test("^20[0-9]{2}-")) and (.today | test("^20[0-9]{2}-[0-9]{2}-[0-9]{2}$"))' -H "x-api-key: functional-secret"
check "auth — math on protected route" GET "/auth/math/me" 200 '.subject_size == 15 and .bounded_subject_size == 15 and .rounded_threshold == 4' -H "x-api-key: functional-secret"

# ─────────────────────────────────────────────────────────────────────────────
# 36. Feature flags
# ─────────────────────────────────────────────────────────────────────────────

echo -e "\n${BOLD}── 36. Feature flags ──${RESET}"

get "feature flags — enabled flag gates route" "/feature-flags/enabled" 200 '.feature == "functional_flag" and .enabled == true'
get "feature flags — missing flag is false" "/feature-flags/missing" 200 '.feature == "missing_flag" and .enabled == false'

# ─────────────────────────────────────────────────────────────────────────────
# 37. Schema relation contracts (Spec 062)
# ─────────────────────────────────────────────────────────────────────────────

echo -e "\n${BOLD}── 37. Schema relation contracts (Spec 062) ──${RESET}"

# Persistent schema (DbUser <-> DbOrder relation cycle) as a contract: own fields
# validated, `orders` relation let through.
post "062 — persistent contract ok"          "/relations/persistent-contract" 200 '.name == "Ana"' '{"id":1,"name":"Ana","orders":[]}'
post "062 — persistent contract own-field type" "/relations/persistent-contract" 422 '' '{"id":1,"name":123,"orders":[]}'
# A deeply nested cyclic payload terminates (relation elements are let through, not
# recursively validated) — proves the cycle does not loop.
post "062 — persistent relation let pass (no loop)" "/relations/persistent-contract" 200 '.name == "Ana"' '{"id":1,"name":"Ana","orders":[{"id":7,"total":1.5,"customer":{"id":1,"name":"Ana","orders":[]}}]}'

# Value schema referencing a persistent schema: `note` validated, `customer` relation
# let through (an id or an object).
post "062 — value-ref-persistent id ok"       "/relations/value-ref-persistent" 200 '.note == "hi"' '{"note":"hi","customer":5}'
post "062 — value-ref-persistent own-field"   "/relations/value-ref-persistent" 422 '' '{"note":123,"customer":5}'
post "062 — value-ref-persistent object passes" "/relations/value-ref-persistent" 200 '.note == "hi"' '{"note":"hi","customer":{"any":"thing"}}'

# Persistent self-referential schema (tree) as a contract.
post "062 — self-ref tree ok"                 "/relations/self-ref-tree" 200 '.label == "root"' '{"id":1,"label":"root","parent":0}'

# ─────────────────────────────────────────────────────────────────────────────
# SECTION 67 — Inferred document index (Spec 067)
#
# The GET /docs/indexed/:id route filters ft_index_demo by account_id and sorts by
# _id desc. The runtime infers the ESR composite {account_id:1, _id:-1} from that code
# (no declaration) and ensures it in MongoDB in the background at serve startup. Assert
# the index is physically present, by its owned name and exact keys, via getIndexes.
# Retry while the background build settles.
# ─────────────────────────────────────────────────────────────────────────────
section "67. Inferred document index (Spec 067)"
INFERRED_IDX=""
for _ in $(seq 1 20); do
    INFERRED_IDX=$(mongo_eval 'db.ft_index_demo.getIndexes().map(i => i.name + ":" + JSON.stringify(i.key)).join(",")' 2>/dev/null || true)
    if [[ "$INFERRED_IDX" == *"idx_ft_index_demo_account_id__id_desc"* \
        && "$INFERRED_IDX" == *'"account_id":1'* && "$INFERRED_IDX" == *'"_id":-1'* ]]; then
        break
    fi
    sleep 0.5
done
if [[ "$INFERRED_IDX" == *"idx_ft_index_demo_account_id__id_desc"* \
    && "$INFERRED_IDX" == *'"account_id":1'* && "$INFERRED_IDX" == *'"_id":-1'* ]]; then
    echo -e "  ${GREEN}PASS${RESET} inferred composite index present in MongoDB (${INFERRED_IDX})"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${RESET} inferred index missing or wrong keys — got: ${INFERRED_IDX:-<none>}"
    FAIL=$((FAIL + 1))
fi

# ─────────────────────────────────────────────────────────────────────────────
# Summary
# ─────────────────────────────────────────────────────────────────────────────
TOTAL=$((PASS + FAIL))
echo ""
echo -e "${BOLD}Results: ${GREEN}${PASS} passed${RESET}, ${RED}${FAIL} failed${RESET} / ${TOTAL} total"

if [[ $FAIL -gt 0 ]]; then
    exit 1
fi
