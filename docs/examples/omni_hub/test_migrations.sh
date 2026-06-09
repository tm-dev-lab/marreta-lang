#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE_FILE="${SCRIPT_DIR}/docker-compose.yml"
# Everything runs containerized against the pre-built marreta-lang:dev image
# (built by the marreta-lang repo). This suite never builds the runtime.
export MARRETA_IMAGE="${MARRETA_IMAGE:-marreta-lang:dev}"
export MARRETA_UID="$(id -u)"
export MARRETA_GID="$(id -g)"
WORKSPACE_DIR="$(mktemp -d /tmp/omni-hub-migrations.XXXXXX)"
PROJECT_DIR="${WORKSPACE_DIR}/project"
MIGRATIONS_DIR="${PROJECT_DIR}/migrations"
# The app container mounts this staged project at /app.
export MARRETA_APP_PROJECT_DIR="${PROJECT_DIR}"

cleanup() {
    echo ""
    docker compose -f "${COMPOSE_FILE}" down --remove-orphans -v >/dev/null 2>&1 || true
    rm -rf "${WORKSPACE_DIR}" >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

fail() {
    echo "FAIL: $1" >&2
    exit 1
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

assert_equals() {
    local actual="$1"
    local expected="$2"
    local label="$3"
    if [[ "${actual}" != "${expected}" ]]; then
        echo "actual: ${actual}" >&2
        echo "expected: ${expected}" >&2
        fail "${label}"
    fi
}

sql() {
    docker compose -f "${COMPOSE_FILE}" exec -T postgres \
        psql -U marreta -d omni_hub -Atqc "$1"
}

run_marreta() {
    docker compose -f "${COMPOSE_FILE}" run --rm -T app "$@"
}

echo "Omni Hub Migrations Tests"
echo "Runtime: ${MARRETA_IMAGE} (containerized)"
echo "Workspace: ${PROJECT_DIR}"

if ! docker image inspect "${MARRETA_IMAGE}" > /dev/null 2>&1; then
    echo "marreta image '${MARRETA_IMAGE}' not found; build it in the marreta-lang repo first" >&2
    echo "  cargo build --release && docker build -t ${MARRETA_IMAGE} ." >&2
    exit 1
fi

docker compose -f "${COMPOSE_FILE}" down --remove-orphans -v >/dev/null 2>&1 || true
docker compose -f "${COMPOSE_FILE}" up -d --wait postgres mongodb redis rabbitmq

cp -R "${SCRIPT_DIR}/." "${PROJECT_DIR}"
rm -rf "${MIGRATIONS_DIR}"
mkdir -p "${MIGRATIONS_DIR}"

echo ""
echo "Phase A — Source-first planning"

doctor_default="$(run_marreta doctor)"
assert_contains "${doctor_default}" "schema Customer persists to table customers" "doctor customer persistence"
assert_contains "${doctor_default}" "schema ServiceOrder persists to table orders" "doctor order persistence"
assert_contains "${doctor_default}" "schema ServiceOrder.customer persists as orders.customer_id -> customers.id" "doctor fk inference"
assert_contains "${doctor_default}" "schema Customer.orders is inferred from ServiceOrder.customer" "doctor inverse inference"
assert_contains "${doctor_default}" "0 local migrations" "doctor zero migrations"
echo "PASS: doctor default"

diff_output="$(run_marreta migrate diff)"
assert_contains "${diff_output}" "Planned migration operations:" "diff summary header"
assert_contains "${diff_output}" "2 tables to create, 0 columns to add, 1 foreign key to add" "diff summary counts"
assert_contains "${diff_output}" "CREATE TABLE customers" "diff creates customers"
assert_contains "${diff_output}" "CREATE TABLE orders" "diff creates orders"
assert_contains "${diff_output}" "customer_id BIGINT NOT NULL" "diff customer_id column"
assert_contains "${diff_output}" "created_at TIMESTAMPTZ NOT NULL" "diff created_at column"
assert_contains "${diff_output}" "completed_at TIMESTAMPTZ" "diff completed_at column"
assert_contains "${diff_output}" "ADD CONSTRAINT fk_orders_customer_id" "diff foreign key"
echo "PASS: migrate diff"

generate_output="$(run_marreta migrate generate)"
assert_contains "${generate_output}" "Generated migration:" "generate output"
migration_id="$(echo "${generate_output}" | sed -n 's/^Generated migration: //p' | head -1)"
migration_version="$(echo "${migration_id}" | cut -d '_' -f 1,2)"
[[ -n "${migration_id}" ]] || fail "could not parse generated migration id"
[[ -f "${MIGRATIONS_DIR}/${migration_id}.up.sql" ]] || fail "missing up.sql"
[[ -f "${MIGRATIONS_DIR}/${migration_id}.down.sql" ]] || fail "missing down.sql"
echo "PASS: migrate generate"

list_pending="$(run_marreta migrate list)"
assert_contains "${list_pending}" "${migration_version}" "list pending version"
assert_contains "${list_pending}" "update_customers" "list pending name"
assert_contains "${list_pending}" "pending" "list pending state"
echo "PASS: migrate list pending"

echo ""
echo "Phase B — Apply and inspect database"

apply_output="$(run_marreta migrate apply)"
assert_contains "${apply_output}" "Applied ${migration_id}" "apply output"
echo "PASS: migrate apply"

doctor_connect="$(run_marreta doctor --connect)"
assert_contains "${doctor_connect}" "Connectivity:" "doctor connect section"
assert_contains "${doctor_connect}" "OK    db connection" "doctor db connection"
assert_contains "${doctor_connect}" "OK    doc connection" "doctor doc connection"
assert_contains "${doctor_connect}" "OK    cache connection" "doctor cache connection"
assert_contains "${doctor_connect}" "OK    queue connection" "doctor queue connection"
assert_contains "${doctor_connect}" "OK    1 applied" "doctor applied summary"
assert_contains "${doctor_connect}" "OK    no pending migrations" "doctor no pending summary"
echo "PASS: doctor --connect"

status_clean="$(run_marreta migrate status)"
assert_contains "${status_clean}" "Database migration state is clean." "status clean summary"
echo "PASS: migrate status clean"

tables_after_apply="$(sql "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename;")"
assert_contains "${tables_after_apply}" "_marreta_migrations" "apply migration table"
assert_contains "${tables_after_apply}" "customers" "apply customers table"
assert_contains "${tables_after_apply}" "orders" "apply orders table"
echo "PASS: postgres tables after apply"

columns_after_apply="$(sql "SELECT table_name || ':' || column_name FROM information_schema.columns WHERE table_schema = 'public' AND table_name IN ('customers', 'orders') ORDER BY table_name, ordinal_position;")"
assert_contains "${columns_after_apply}" "customers:id" "customers id column"
assert_contains "${columns_after_apply}" "customers:name" "customers name column"
assert_contains "${columns_after_apply}" "customers:email" "customers email column"
assert_contains "${columns_after_apply}" "orders:id" "orders id column"
assert_contains "${columns_after_apply}" "orders:customer_id" "orders customer_id column"
assert_contains "${columns_after_apply}" "orders:description" "orders description column"
assert_contains "${columns_after_apply}" "orders:total_amount" "orders total_amount column"
assert_contains "${columns_after_apply}" "orders:status" "orders status column"
assert_contains "${columns_after_apply}" "orders:created_at" "orders created_at column"
assert_contains "${columns_after_apply}" "orders:completed_at" "orders completed_at column"
echo "PASS: postgres columns after apply"

fk_after_apply="$(sql "SELECT conname || ':' || rel.relname FROM pg_constraint c JOIN pg_class rel ON rel.oid = c.conrelid WHERE contype = 'f' ORDER BY conname;")"
assert_contains "${fk_after_apply}" "fk_orders_customer_id:orders" "orders customer fk"
echo "PASS: postgres foreign keys after apply"

echo ""
echo "Phase C — Rollback and discard"

rollback_output="$(run_marreta migrate rollback)"
assert_contains "${rollback_output}" "Rolled back ${migration_id}" "rollback output"
echo "PASS: migrate rollback"

status_after_rollback="$(run_marreta migrate status)"
assert_contains "${status_after_rollback}" "Pending:" "status after rollback pending section"
assert_contains "${status_after_rollback}" "${migration_id}" "status after rollback pending id"
echo "PASS: migrate status after rollback"

tables_after_rollback="$(sql "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename;")"
assert_equals "${tables_after_rollback}" "_marreta_migrations" "rollback should leave only migration table"
echo "PASS: postgres tables after rollback"

discard_output="$(run_marreta migrate discard "${migration_version}")"
assert_contains "${discard_output}" "Discarded ${migration_id}" "discard output"
echo "PASS: migrate discard"

list_after_discard="$(run_marreta migrate list)"
assert_contains "${list_after_discard}" "(no migrations)" "list after discard"
echo "PASS: migrate list after discard"

echo ""
echo "Results: PASS"
