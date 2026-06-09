#!/usr/bin/env bash
# ── MarretaLang — Ecommerce Example Test Runner ───────────────────────────────
#
# Starts Postgres + MongoDB (via docker compose), builds + starts the marreta
# server, then exercises every route in the ecommerce example via curl.
#
# Sections:
#   1. Health
#   2. Products (db.*)      — GET /products, POST, GET /:id, DELETE
#   3. Orders (db.*)        — POST with coupon, guard failures, GET, DELETE
#   4. Doc Products (doc.*) — same operations via /doc/products
#   5. Doc Orders (doc.*)   — same operations via /doc/orders
#
# Usage:
#   cd examples/ecommerce
#   ./test.sh              # local binary (requires cargo)
#   ./test.sh --docker     # fully containerized
#
# Prerequisites:
#   - docker / docker compose
#   - cargo (local mode only)
#   - curl, jq
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Everything runs containerized against the pre-built marreta-lang:dev image
# (built by the marreta-lang repo). This suite never builds the runtime.
export MARRETA_IMAGE="${MARRETA_IMAGE:-marreta-lang:dev}"
export MARRETA_UID="$(id -u)"
export MARRETA_GID="$(id -g)"
BASE="http://127.0.0.1:3939"
SERVER_PID=""
DOCKER_MODE=true

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

PASS=0
FAIL=0

# ── cleanup ───────────────────────────────────────────────────────────────────

cleanup() {
    echo ""
    echo "Stopping containers…"
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" down --remove-orphans 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# ── helpers ───────────────────────────────────────────────────────────────────

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
        echo -e "       body: ${body}"
        FAIL=$((FAIL + 1))
        return
    fi

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

H_JSON=(-H "Content-Type: application/json")

post()   { check "$1" POST   "$2" "$3" "$4" "${H_JSON[@]}" -d "$5"; }
get()    { check "$1" GET    "$2" "$3" "$4"; }
delete() { check "$1" DELETE "$2" "$3" "$4"; }

run_marreta_cmd() {
    docker compose -f "${SCRIPT_DIR}/docker-compose.yml" run --rm -T app "$@"
}

check_marreta_success() {
    local name="$1"; shift
    local expected_pattern="$1"; shift
    local output

    if output="$(run_marreta_cmd "$@" 2>&1)" && echo "$output" | grep -F "$expected_pattern" >/dev/null 2>&1; then
        echo -e "  ${GREEN}PASS${RESET} ${name}"
        PASS=$((PASS + 1))
    else
        echo -e "  ${RED}FAIL${RESET} ${name} — expected output to contain: ${expected_pattern}"
        echo "$output" | sed 's/^/       /'
        FAIL=$((FAIL + 1))
    fi
}

# ── startup ───────────────────────────────────────────────────────────────────

echo -e "${BOLD}MarretaLang Ecommerce Tests${RESET}"
echo "Runtime: ${MARRETA_IMAGE} (containerized)"
echo ""

if ! docker image inspect "${MARRETA_IMAGE}" > /dev/null 2>&1; then
    echo "marreta image '${MARRETA_IMAGE}' not found; build it in the marreta-lang repo first" >&2
    echo "  cargo build --release && docker build -t ${MARRETA_IMAGE} ." >&2
    exit 1
fi

echo "Starting containers…"
docker compose -f "${SCRIPT_DIR}/docker-compose.yml" up -d --wait app 2>&1 | tail -10
BASE="http://127.0.0.1:3939"

echo "Waiting for server to be ready…"
for i in $(seq 1 60); do
    if curl -sf "${BASE}/health" > /dev/null 2>&1; then
        echo "Server ready."
        break
    fi
    if [[ $i -eq 60 ]]; then
        echo -e "${RED}Server did not start in time.${RESET}"
        exit 1
    fi
    sleep 0.5
done

# ═════════════════════════════════════════════════════════════════════════════
# SECTION 1 — Health
# ═════════════════════════════════════════════════════════════════════════════
section "1. Health"

get "health — ok"      "/health" 200 '.ok == true'
get "health — project_name" "/health" 200 '.api != null'

# 025 — doctor stays useful without db: schemas, and migrate reports no work.
check_marreta_success "doctor — project without db schemas still loads" "project loads successfully" doctor
check_marreta_success "doctor — no persistence section for no-db project" "Intent:" doctor
if run_marreta_cmd doctor 2>&1 | grep -F "Persistence (db):" >/dev/null 2>&1; then
    echo -e "  ${RED}FAIL${RESET} doctor — no-db project should not render Persistence (db)"
    FAIL=$((FAIL + 1))
else
    echo -e "  ${GREEN}PASS${RESET} doctor — no-db project omits Persistence (db)"
    PASS=$((PASS + 1))
fi
check_marreta_success "migrate diff — no db schemas" "No db: schemas found." migrate diff

# ═════════════════════════════════════════════════════════════════════════════
# SECTION 2 — Products (db.*)
# ═════════════════════════════════════════════════════════════════════════════
section "2. Products (db.*)"

# Seed products from seed.sql already loaded — find_all should return >= 3
get "products — list"  "/products" 200 '(.items | length) >= 3'

# Create a new product — POST returns product_created schema: {created, name, price} (no id)
check "products — save created=true"  POST "/products" 201 '.created == true' \
    "${H_JSON[@]}" -d '{"name":"TestWidget","price":9.99,"category":"tools"}'
check "products — save name echoed"   POST "/products" 201 '.name == "TestWidget"' \
    "${H_JSON[@]}" -d '{"name":"TestWidget","price":9.99,"category":"tools"}'

# Use seeded product id=1 for find/delete tests
get    "products — find by id"    "/products/1" 200 '.id == 1'
get    "products — name present"  "/products/1" 200 '.name != null'
get    "products — 404 missing"   "/products/99999" 404 ''

# Guard: schema validates required fields → 422 (schema validation fires before require)
check "products — guard missing name" POST "/products" 422 '' "${H_JSON[@]}" \
    -d '{"price":1.0,"category":"x"}'

# ═════════════════════════════════════════════════════════════════════════════
# SECTION 3 — Orders (db.*)
# ═════════════════════════════════════════════════════════════════════════════
section "3. Orders (db.*)"

get "orders — list" "/orders" 200 '(.items | length) >= 0'

ORDER_PAYLOAD='{"billing":{"city":"SP","street":"Av. Paulista","zipcode":"01310-100"},"items":[{"product_id":1,"quantity":2}],"coupon":"SAVE10"}'

ORDER_RESP=$(curl -s -X POST "${BASE}/orders" "${H_JSON[@]}" -d "${ORDER_PAYLOAD}" 2>/dev/null)
ORDER_ID=$(echo "$ORDER_RESP" | jq -r '.order_id // empty')

if [[ -z "$ORDER_ID" ]]; then
    echo -e "  ${RED}FAIL${RESET} orders — save: could not obtain order_id"
    echo -e "       body: ${ORDER_RESP}"
    FAIL=$((FAIL + 1))
else
    echo -e "  ${GREEN}PASS${RESET} orders — save (order_id=${ORDER_ID})"
    PASS=$((PASS + 1))

    check "orders — order_created true"   GET "/orders/${ORDER_ID}" 200 '.id != null'
    check "orders — discount SAVE10"      POST "/orders" 201 '.discount_rate == 0.1' \
        "${H_JSON[@]}" -d "${ORDER_PAYLOAD}"
    check "orders — discount SAVE20"      POST "/orders" 201 '.discount_rate == 0.2' \
        "${H_JSON[@]}" -d '{"billing":{"city":"RJ","street":"Rua X","zipcode":"20040-020"},"items":[{"product_id":1,"quantity":1}],"coupon":"SAVE20"}'
    check "orders — no coupon rate 0"     POST "/orders" 201 '.discount_rate == 0.0' \
        "${H_JSON[@]}" -d '{"billing":{"city":"BH","street":"Rua Y","zipcode":"30130-010"},"items":[{"product_id":1,"quantity":1}]}'

    delete "orders — delete"              "/orders/${ORDER_ID}" 200 '.deleted == true'
    get    "orders — 404 after del"       "/orders/${ORDER_ID}" 404 ''
fi

# Guards
# Guards: schema validates required fields → 422
check "orders — guard missing billing" POST "/orders" 422 '' "${H_JSON[@]}" \
    -d '{"items":[{"product_id":1,"quantity":1}]}'
check "orders — guard missing items"   POST "/orders" 422 '' "${H_JSON[@]}" \
    -d '{"billing":{"city":"SP","street":"X","zipcode":"00000-000"}}'

# ═════════════════════════════════════════════════════════════════════════════
# SECTION 4 — Doc Products (doc.*)
# ═════════════════════════════════════════════════════════════════════════════
section "4. Doc Products (doc.*)"

get "doc/products — list" "/doc/products" 200 '(.items | length) >= 0'

# POST returns product_created schema: {created, name, price} — no _id
# To test find/delete: save directly then list to retrieve _id
check "doc/products — save created=true" POST "/doc/products" 201 '.created == true' \
    "${H_JSON[@]}" -d '{"name":"DocWidget","price":19.99,"category":"electronics"}'
check "doc/products — save name echoed"  POST "/doc/products" 201 '.name == "DocWidget"' \
    "${H_JSON[@]}" -d '{"name":"DocWidget","price":19.99,"category":"electronics"}'

# Retrieve _id from list to test find/delete
DOC_PROD_ID=$(curl -s "${BASE}/doc/products" 2>/dev/null | jq -r '.items[0]._id // empty')
if [[ -n "$DOC_PROD_ID" ]]; then
    get    "doc/products — find by id"    "/doc/products/${DOC_PROD_ID}" 200 '._id != null'
    delete "doc/products — delete"        "/doc/products/${DOC_PROD_ID}" 200 '.deleted == true'
    get    "doc/products — 404 after del" "/doc/products/${DOC_PROD_ID}" 404 ''
else
    echo -e "  ${YELLOW:-}SKIP${RESET} doc/products find/delete — no document found in list"
fi

# Guard: schema validation → 422
check "doc/products — guard missing name" POST "/doc/products" 422 '' "${H_JSON[@]}" \
    -d '{"price":1.0,"category":"x"}'

# ═════════════════════════════════════════════════════════════════════════════
# SECTION 5 — Doc Orders (doc.*)
# ═════════════════════════════════════════════════════════════════════════════
section "5. Doc Orders (doc.*)"

get "doc/orders — list" "/doc/orders" 200 '(.items | length) >= 0'

DOC_ORDER_PAYLOAD='{"billing":{"city":"SP","street":"Av. Paulista","zipcode":"01310-100"},"items":[{"product_id":1,"quantity":2}],"coupon":"SAVE10"}'

DOC_ORDER_RESP=$(curl -s -X POST "${BASE}/doc/orders" "${H_JSON[@]}" -d "${DOC_ORDER_PAYLOAD}" 2>/dev/null)
DOC_ORDER_ID=$(echo "$DOC_ORDER_RESP" | jq -r '.order_id // empty')

if [[ -z "$DOC_ORDER_ID" ]]; then
    echo -e "  ${RED}FAIL${RESET} doc/orders — save: could not obtain order_id"
    echo -e "       body: ${DOC_ORDER_RESP}"
    FAIL=$((FAIL + 1))
else
    echo -e "  ${GREEN}PASS${RESET} doc/orders — save (order_id=${DOC_ORDER_ID})"
    PASS=$((PASS + 1))

    get    "doc/orders — find by id"      "/doc/orders/${DOC_ORDER_ID}" 200 '._id != null'
    check  "doc/orders — discount SAVE10" POST "/doc/orders" 201 '.discount_rate == 0.1' \
        "${H_JSON[@]}" -d "${DOC_ORDER_PAYLOAD}"
    check  "doc/orders — discount SAVE20" POST "/doc/orders" 201 '.discount_rate == 0.2' \
        "${H_JSON[@]}" -d '{"billing":{"city":"RJ","street":"Rua X","zipcode":"20040-020"},"items":[{"product_id":1,"quantity":1}],"coupon":"SAVE20"}'
    check  "doc/orders — no coupon rate 0" POST "/doc/orders" 201 '.discount_rate == 0.0' \
        "${H_JSON[@]}" -d '{"billing":{"city":"BH","street":"Rua Y","zipcode":"30130-010"},"items":[{"product_id":1,"quantity":1}]}'

    delete "doc/orders — delete"          "/doc/orders/${DOC_ORDER_ID}" 200 '.deleted == true'
    get    "doc/orders — 404 after del"   "/doc/orders/${DOC_ORDER_ID}" 404 ''
fi

# Guards: schema validation → 422
check "doc/orders — guard missing billing" POST "/doc/orders" 422 '' "${H_JSON[@]}" \
    -d '{"items":[{"product_id":1,"quantity":1}]}'
check "doc/orders — guard missing items"   POST "/doc/orders" 422 '' "${H_JSON[@]}" \
    -d '{"billing":{"city":"SP","street":"X","zipcode":"00000-000"}}'

# ─────────────────────────────────────────────────────────────────────────────
# Summary
# ─────────────────────────────────────────────────────────────────────────────
TOTAL=$((PASS + FAIL))
echo ""
echo -e "${BOLD}Results: ${GREEN}${PASS} passed${RESET}, ${RED}${FAIL} failed${RESET} / ${TOTAL} total"

if [[ $FAIL -gt 0 ]]; then
    exit 1
fi
