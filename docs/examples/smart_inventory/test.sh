#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Everything runs containerized against the pre-built marreta-lang:dev image
# (built by the marreta-lang repo). This suite never builds the runtime.
export MARRETA_IMAGE="${MARRETA_IMAGE:-marreta-lang:dev}"
export MARRETA_UID="$(id -u)"
export MARRETA_GID="$(id -g)"
BASE="http://127.0.0.1:3942"
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
        echo "Stopping marreta server (pid ${SERVER_PID})..."
        kill "$SERVER_PID" 2>/dev/null || true
    fi
    echo "Stopping infrastructure..."
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" down --remove-orphans -v >/dev/null 2>&1 || true
    if [[ -n "$SERVER_LOG" && -f "$SERVER_LOG" ]]; then
        rm -f "$SERVER_LOG"
    fi
}
trap cleanup EXIT INT TERM

section() {
    echo ""
    echo -e "${CYAN}${BOLD}-- $1 --${RESET}"
}

pass() {
    echo -e "  ${GREEN}PASS${RESET} $1"
    PASS=$((PASS + 1))
}

fail() {
    echo -e "  ${RED}FAIL${RESET} $1"
    FAIL=$((FAIL + 1))
}

check_http() {
    local name="$1"
    local method="$2"
    local path="$3"
    local expected_status="$4"
    local jq_expr="$5"
    local body="${6:-}"
    local tmpfile
    tmpfile="$(mktemp)"

    local args=(-s -o "$tmpfile" -w "%{http_code}" -X "$method")
    if [[ -n "$body" ]]; then
        args+=(-H "Content-Type: application/json" -d "$body")
    fi

    local actual_status
    actual_status="$(curl "${args[@]}" "${BASE}${path}" 2>/dev/null)"
    local response
    response="$(cat "$tmpfile")"
    rm -f "$tmpfile"

    if [[ "$actual_status" != "$expected_status" ]]; then
        fail "${name} -- expected HTTP ${expected_status}, got ${actual_status}. body=${response}"
        return
    fi

    if [[ -n "$jq_expr" ]] && ! echo "$response" | jq -e "$jq_expr" >/dev/null 2>&1; then
        fail "${name} -- jq assertion failed: ${jq_expr}. body=${response}"
        return
    fi

    pass "$name"
}

post() { check_http "$1" POST "$2" "$3" "$4" "$5"; }
get() { check_http "$1" GET "$2" "$3" "$4"; }

sql() {
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" exec -T postgres \
        psql -U marreta -d smart_inventory -Atqc "$1"
}

redis_cmd() {
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" exec -T redis \
        redis-cli -a redis-secret "$@" 2>/dev/null
}

mongo_eval() {
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" exec -T mongodb \
        mongosh smart_inventory -u marreta -p marreta-secret --authenticationDatabase admin --quiet --eval "$1"
}

wait_until() {
    local name="$1"
    local command="$2"
    for _ in $(seq 1 50); do
        if bash -lc "$command" >/dev/null 2>&1; then
            pass "$name"
            return 0
        fi
        sleep 0.2
    done
    fail "$name"
    return 1
}

echo -e "${BOLD}Smart Inventory Benchmark Tests${RESET}"
echo "Runtime: ${MARRETA_IMAGE} (containerized)"
echo ""

if ! docker image inspect "${MARRETA_IMAGE}" > /dev/null 2>&1; then
    echo "marreta image '${MARRETA_IMAGE}' not found; build it in the marreta-lang repo first" >&2
    echo "  cargo build --release && docker build -t ${MARRETA_IMAGE} ." >&2
    exit 1
fi

echo "Starting infrastructure..."
docker compose -f "${SCRIPT_DIR}/docker-compose.yml" up -d --wait postgres mongodb rabbitmq redis 2>&1 | tail -10

echo "Running scenario tests..."
docker compose -f "${SCRIPT_DIR}/docker-compose.yml" run --rm -T app test >/dev/null

echo "Applying migrations..."
docker compose -f "${SCRIPT_DIR}/docker-compose.yml" run --rm -T app migrate apply >/dev/null

echo "Starting marreta server..."
docker compose -f "${SCRIPT_DIR}/docker-compose.yml" up -d --wait app 2>&1 | tail -10

echo "Waiting for server..."
for i in $(seq 1 60); do
    if curl -sf "${BASE}/health" >/dev/null 2>&1; then
        echo "Server ready."
        break
    fi
    if [[ $i -eq 60 ]]; then
        echo -e "${RED}Server did not start in time.${RESET}"
        docker compose -f "${SCRIPT_DIR}/docker-compose.yml" logs --no-color app
        exit 1
    fi
    sleep 0.5
done

SKU="SKU-RED-001"

section "1. Seed and projection"
post "seed product" "/inventory/seed" 201 '.seeded == true and .stock == 100' \
    '{"sku":"SKU-RED-001","name":"Red Widget","initial_stock":100,"low_stock_threshold":5}'

stock_db="$(sql "SELECT current_stock FROM products WHERE sku = '${SKU}';")"
[[ "$stock_db" == "100" ]] && pass "db stock seeded" || fail "db stock seeded -- got ${stock_db}"

stock_cache="$(redis_cmd GET "stock:${SKU}" | tail -n 1 | tr -d '\r')"
[[ "$stock_cache" == "100" ]] && pass "cache stock seeded" || fail "cache stock seeded -- got ${stock_cache}"

get "stock projection reads cache" "/inventory/${SKU}" 200 '.stock == 100 and .source == "cache"'

section "2. HTTP reservation"
post "reserve stock" "/inventory/reserve" 200 '.reserved == true and .stock_after == 90' \
    '{"order_id":"ord-1001","sku":"SKU-RED-001","quantity":10}'

stock_db="$(sql "SELECT current_stock FROM products WHERE sku = '${SKU}';")"
[[ "$stock_db" == "90" ]] && pass "db stock decremented" || fail "db stock decremented -- got ${stock_db}"

reserved_db="$(sql "SELECT reserved_stock FROM products WHERE sku = '${SKU}';")"
[[ "$reserved_db" == "10" ]] && pass "db reserved stock incremented" || fail "db reserved stock incremented -- got ${reserved_db}"

stock_cache="$(redis_cmd GET "stock:${SKU}" | tail -n 1 | tr -d '\r')"
[[ "$stock_cache" == "90" ]] && pass "cache stock decremented" || fail "cache stock decremented -- got ${stock_cache}"

reserved_docs="$(mongo_eval "db.inventory_events.countDocuments({ sku: '${SKU}', event_type: 'order_reserved' })" | tr -d '\r')"
[[ "$reserved_docs" == "1" ]] && pass "doc event order_reserved written" || fail "doc event order_reserved written -- got ${reserved_docs}"

wait_until "topic audit saw inventory.reserved" \
    "docker compose -f '${SCRIPT_DIR}/docker-compose.yml' exec -T redis redis-cli -a redis-secret EXISTS 'audit:inventory.reserved:${SKU}' 2>/dev/null | grep -q '^1$'"

section "3. Incoming shipment queue"
post "enqueue incoming shipment" "/inventory/shipments" 202 '.queued == true' \
    '{"shipment_id":"ship-9001","sku":"SKU-RED-001","quantity":50}'

wait_until "shipment consumer updated db" \
    "docker compose -f '${SCRIPT_DIR}/docker-compose.yml' exec -T postgres psql -U marreta -d smart_inventory -Atqc \"SELECT current_stock FROM products WHERE sku = '${SKU}';\" | grep -q '^140$'"

stock_cache="$(redis_cmd GET "stock:${SKU}" | tail -n 1 | tr -d '\r')"
[[ "$stock_cache" == "140" ]] && pass "shipment updated cache" || fail "shipment updated cache -- got ${stock_cache}"

shipment_docs="$(mongo_eval "db.inventory_events.countDocuments({ sku: '${SKU}', event_type: 'shipment_received' })" | tr -d '\r')"
[[ "$shipment_docs" == "1" ]] && pass "doc event shipment_received written" || fail "doc event shipment_received written -- got ${shipment_docs}"

wait_until "topic audit saw inventory.increased" \
    "docker compose -f '${SCRIPT_DIR}/docker-compose.yml' exec -T redis redis-cli -a redis-secret EXISTS 'audit:inventory.increased:${SKU}' 2>/dev/null | grep -q '^1$'"

section "4. Cancellation compensation"
post "enqueue cancellation" "/inventory/cancellations" 202 '.queued == true' \
    '{"order_id":"ord-1001","reason":"customer_changed_mind"}'

wait_until "cancellation consumer restored db stock" \
    "docker compose -f '${SCRIPT_DIR}/docker-compose.yml' exec -T postgres psql -U marreta -d smart_inventory -Atqc \"SELECT current_stock FROM products WHERE sku = '${SKU}';\" | grep -q '^150$'"

reservation_status="$(sql "SELECT status FROM reservations WHERE order_id = 'ord-1001';")"
[[ "$reservation_status" == "cancelled" ]] && pass "reservation marked cancelled" || fail "reservation marked cancelled -- got ${reservation_status}"

cancel_docs="$(mongo_eval "db.inventory_events.countDocuments({ sku: '${SKU}', event_type: 'order_cancelled', reason: 'customer_changed_mind' })" | tr -d '\r')"
[[ "$cancel_docs" == "1" ]] && pass "doc event order_cancelled with reason written" || fail "doc event order_cancelled with reason written -- got ${cancel_docs}"

wait_until "topic audit saw inventory.cancelled" \
    "docker compose -f '${SCRIPT_DIR}/docker-compose.yml' exec -T redis redis-cli -a redis-secret EXISTS 'audit:inventory.cancelled:${SKU}' 2>/dev/null | grep -q '^1$'"

section "5. Low stock alert"
post "reserve to low stock" "/inventory/reserve" 200 '.reserved == true and .stock_after == 4' \
    '{"order_id":"ord-1002","sku":"SKU-RED-001","quantity":146}'

low_docs="$(mongo_eval "db.inventory_events.countDocuments({ sku: '${SKU}', event_type: 'low_stock_detected', status_critical: true })" | tr -d '\r')"
[[ "$low_docs" == "1" ]] && pass "low stock document marked critical" || fail "low stock document marked critical -- got ${low_docs}"

wait_until "topic audit saw inventory.low_stock" \
    "docker compose -f '${SCRIPT_DIR}/docker-compose.yml' exec -T redis redis-cli -a redis-secret EXISTS 'audit:inventory.low_stock:${SKU}' 2>/dev/null | grep -q '^1$'"

section "6. Manual reconciliation"
sql "UPDATE products SET current_stock = 999 WHERE sku = '${SKU}';" >/dev/null

post "manual reconcile restores stock" "/inventory/${SKU}/reconcile" 200 '.changed == true and .before == 999 and .after == 4' ""

reconciled_db="$(sql "SELECT current_stock FROM products WHERE sku = '${SKU}';")"
[[ "$reconciled_db" == "4" ]] && pass "db stock reconciled" || fail "db stock reconciled -- got ${reconciled_db}"

reconciled_cache="$(redis_cmd GET "stock:${SKU}" | tail -n 1 | tr -d '\r')"
[[ "$reconciled_cache" == "4" ]] && pass "cache stock reconciled" || fail "cache stock reconciled -- got ${reconciled_cache}"

reconcile_docs="$(mongo_eval "db.inventory_events.countDocuments({ sku: '${SKU}', event_type: 'reconciliation_applied' })" | tr -d '\r')"
[[ "$reconcile_docs" == "1" ]] && pass "reconciliation event written" || fail "reconciliation event written -- got ${reconcile_docs}"

section "7. Runtime observability"
TRACE_ID="11111111111111111111111111111111"
curl -s -o /dev/null -X POST "${BASE}/inventory/shipments" \
    -H "Content-Type: application/json" \
    -H "traceparent: 00-${TRACE_ID}-00f067aa0ba902b7-01" \
    -d '{"shipment_id":"ship-9002","sku":"SKU-RED-001","quantity":1}'

app_logs="docker compose -f ${SCRIPT_DIR}/docker-compose.yml logs --no-color app"

wait_until "async trace_id preserved in consumer event" \
    "${app_logs} 2>/dev/null | grep -F '\"kind\":\"consumer\"' | grep -F '\"trace_id\":\"${TRACE_ID}\"'"

if ${app_logs} 2>/dev/null | grep -F '"kind":"consumer"' >/dev/null 2>&1; then
    pass "consumer runtime events emitted"
else
    fail "consumer runtime events emitted"
fi

if ${app_logs} 2>/dev/null | grep -F 'smart_inventory.audit_recorded' >/dev/null 2>&1; then
    pass "audit app log emitted"
else
    fail "audit app log emitted"
fi

echo ""
if [[ "$FAIL" -eq 0 ]]; then
    echo -e "${BOLD}Results: ${GREEN}${PASS} passed${RESET}, ${RED}0 failed${RESET} / ${PASS} total"
    exit 0
else
    total=$((PASS + FAIL))
    echo -e "${BOLD}Results: ${GREEN}${PASS} passed${RESET}, ${RED}${FAIL} failed${RESET} / ${total} total"
    echo ""
    echo "Server log:"
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" logs --no-color app
    exit 1
fi
