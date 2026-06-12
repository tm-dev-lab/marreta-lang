#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE_FILE="${SCRIPT_DIR}/docker-compose.yml"
# Everything runs containerized against the pre-built marreta-lang:dev image
# (built by the marreta-lang repo). This suite never builds the runtime.
export MARRETA_IMAGE="${MARRETA_IMAGE:-marreta-lang:dev}"
# Run the app container as the host user so files it writes into the bind-mounted
# project (generated migrations) remain host-owned and rewritable by this script.
export MARRETA_UID="$(id -u)"
export MARRETA_GID="$(id -g)"
SERVER_PORT=3738
WORKSPACE_DIR="$(mktemp -d /tmp/marreta-migrations-functional.XXXXXX)"
PROJECT_DIR="${WORKSPACE_DIR}/project"
# The app container mounts this staged project dir at /workspace/project.
export MARRETA_MIGRATIONS_PROJECT_DIR="${PROJECT_DIR}"
SCHEMA_FILE="${PROJECT_DIR}/schemas/models.marreta"
MIGRATIONS_DIR="${PROJECT_DIR}/migrations"

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

assert_not_contains() {
    local haystack="$1"
    local needle="$2"
    local label="$3"
    if [[ "${haystack}" == *"${needle}"* ]]; then
        echo "${haystack}" >&2
        fail "${label} -- expected not to contain: ${needle}"
    fi
}

sql() {
    docker compose -f "${COMPOSE_FILE}" exec -T postgres \
        psql -U marreta -d marreta_migrations -Atqc "$1"
}

prepare_project() {
    local version="$1"
    rm -rf "${MIGRATIONS_DIR}"
    mkdir -p "${MIGRATIONS_DIR}"
    write_schema_version "${version}"
}

swap_schema() {
    local version="$1"
    write_schema_version "${version}"
}

run_marreta() {
    docker compose -f "${COMPOSE_FILE}" run --rm -T app "$@"
}

write_schema_version() {
    local version="$1"
    case "${version}" in
        v1)
            cat > "${SCHEMA_FILE}" <<'EOF'
schema User
    db: users

    id: integer
    name: string
    email: string
EOF
            ;;
        v2)
            cat > "${SCHEMA_FILE}" <<'EOF'
schema Address
    db: addresses

    id: integer
    city: string

schema User
    db: users

    id: integer
    name: string
    email: string
    active?: boolean
    address?: Address
    orders: list of Order

schema Order
    db: orders

    id: integer
    total: float
    status?: enum ["pending", "paid", "cancelled"]
    amount?: decimal
    customer: User
EOF
            ;;
        *)
            fail "unknown schema version '${version}'"
            ;;
    esac
}

wait_server() {
    for _ in $(seq 1 60); do
        if curl -sf "http://127.0.0.1:${SERVER_PORT}/_health" >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.5
    done
    fail "server did not start in time"
}

echo "MarretaLang DB Migrations Functional Tests"
echo "Runtime: ${MARRETA_IMAGE} (containerized)"
echo "Workspace: ${PROJECT_DIR}"

if ! docker image inspect "${MARRETA_IMAGE}" > /dev/null 2>&1; then
    echo "marreta image '${MARRETA_IMAGE}' not found; build it in the marreta-lang repo first" >&2
    echo "  cargo build --release && docker build -t ${MARRETA_IMAGE} ." >&2
    exit 1
fi

docker compose -f "${COMPOSE_FILE}" down --remove-orphans -v >/dev/null 2>&1 || true
docker compose -f "${COMPOSE_FILE}" up -d --wait postgres

mkdir -p "${WORKSPACE_DIR}"
cp -R "${SCRIPT_DIR}/." "${PROJECT_DIR}"

prepare_project "v1"

echo ""
echo "Phase A — Empty DB -> v1 schema"

doctor_v1_default="$(run_marreta doctor)"
assert_contains "${doctor_v1_default}" "Project:" "doctor v1 project section"
assert_contains "${doctor_v1_default}" "Intent:" "doctor v1 intent section"
assert_contains "${doctor_v1_default}" "Persistence (db):" "doctor v1 persistence section"
assert_contains "${doctor_v1_default}" "OK    db" "doctor v1 db intent"
assert_contains "${doctor_v1_default}" "OK    migrations" "doctor v1 migrations intent"
assert_contains "${doctor_v1_default}" "schema User persists to table users" "doctor v1 persisted users"
assert_contains "${doctor_v1_default}" "migration state summary requires --connect" "doctor v1 migrations summary hint"
echo "PASS: v1 doctor default"

diff_v1="$(run_marreta migrate diff)"
assert_contains "${diff_v1}" "Planned migration operations:" "v1 diff summary header"
assert_contains "${diff_v1}" "CREATE TABLE users" "v1 diff create table"
assert_contains "${diff_v1}" "email TEXT NOT NULL" "v1 diff email column"
echo "PASS: v1 diff"

generate_v1="$(run_marreta migrate generate)"
assert_contains "${generate_v1}" "Generated migration" "v1 generate output"
v1_id="$(echo "${generate_v1}" | sed -n 's/^Generated migration: //p' | head -1)"
v1_version="$(echo "${v1_id}" | cut -d '_' -f 1,2)"
[[ -n "${v1_id}" ]] || fail "could not parse v1 migration id"
v1_up="${MIGRATIONS_DIR}/${v1_id}.up.sql"
v1_down="${MIGRATIONS_DIR}/${v1_id}.down.sql"
[[ -f "${v1_up}" ]] || fail "missing v1 up.sql"
[[ -f "${v1_down}" ]] || fail "missing v1 down.sql"
assert_contains "$(cat "${v1_up}")" "CREATE TABLE users" "v1 up.sql content"
assert_contains "$(cat "${v1_down}")" "DROP TABLE users;" "v1 down.sql content"
echo "PASS: v1 generate"

status_v1_pending="$(run_marreta migrate status)"
assert_contains "${status_v1_pending}" "Pending:" "v1 status pending section"
assert_contains "${status_v1_pending}" "${v1_id}" "v1 status pending id"
list_v1_pending="$(run_marreta migrate list)"
assert_contains "${list_v1_pending}" "${v1_version}" "v1 list contains pending migration version"
assert_contains "${list_v1_pending}" "create_users" "v1 list contains pending migration name"
assert_contains "${list_v1_pending}" "pending" "v1 list pending state"
echo "PASS: v1 status before apply"

apply_v1="$(run_marreta migrate apply)"
assert_contains "${apply_v1}" "Applied ${v1_id}" "v1 apply output"
echo "PASS: v1 apply"

doctor_v1_connect="$(run_marreta doctor --connect)"
assert_contains "${doctor_v1_connect}" "Connectivity:" "doctor v1 connect section"
assert_contains "${doctor_v1_connect}" "OK    db connection" "doctor v1 db connectivity"
assert_contains "${doctor_v1_connect}" "OK    1 applied" "doctor v1 applied summary"
assert_contains "${doctor_v1_connect}" "OK    no pending migrations" "doctor v1 no pending summary"
echo "PASS: v1 doctor --connect"

tables_after_v1="$(sql "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename;")"
assert_contains "${tables_after_v1}" "_marreta_migrations" "v1 migration table exists"
assert_contains "${tables_after_v1}" "users" "v1 users table exists"
applied_after_v1="$(sql "SELECT version || '_' || name FROM _marreta_migrations ORDER BY version;")"
assert_contains "${applied_after_v1}" "${v1_id}" "v1 applied row exists"
user_columns_v1="$(sql "SELECT column_name FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'users' ORDER BY ordinal_position;")"
assert_contains "${user_columns_v1}" "id" "v1 users.id exists"
assert_contains "${user_columns_v1}" "name" "v1 users.name exists"
assert_contains "${user_columns_v1}" "email" "v1 users.email exists"
unique_count_v1="$(sql "SELECT COUNT(*) FROM information_schema.table_constraints WHERE table_schema = 'public' AND table_name = 'users' AND constraint_type = 'UNIQUE';")"
[[ "${unique_count_v1}" -eq 0 ]] || fail "v1 users should not have explicit unique constraints in 025"
echo "PASS: v1 postgres verification"

echo ""
echo "Phase B — Runtime usage after apply"

export MARRETA_MIGRATIONS_PROJECT_DIR="${PROJECT_DIR}"
docker compose -f "${COMPOSE_FILE}" up -d --wait app
wait_server

create_body="$(curl -s -X POST "http://127.0.0.1:${SERVER_PORT}/users" \
    -H "Content-Type: application/json" \
    -d '{"name":"Ana","email":"ana@example.com"}')"
assert_contains "${create_body}" '"name":"Ana"' "runtime create response name"
assert_contains "${create_body}" '"email":"ana@example.com"' "runtime create response email"
created_id="$(echo "${create_body}" | jq -r '.id')"
[[ "${created_id}" != "null" && -n "${created_id}" ]] || fail "runtime create response missing id"
fetch_body="$(curl -s "http://127.0.0.1:${SERVER_PORT}/users/${created_id}")"
assert_contains "${fetch_body}" "\"id\":${created_id}" "runtime fetch response id"
assert_contains "${fetch_body}" '"name":"Ana"' "runtime fetch response name"
docker compose -f "${COMPOSE_FILE}" stop app >/dev/null
docker compose -f "${COMPOSE_FILE}" rm -f app >/dev/null
echo "PASS: runtime db usage"

echo ""
echo "Phase C — v1 -> v2 evolution"

swap_schema "v2"
sleep 1

diff_v2="$(run_marreta migrate diff)"
assert_contains "${diff_v2}" "Planned migration operations:" "v2 diff summary header"
assert_contains "${diff_v2}" "CREATE TABLE addresses" "v2 diff create addresses"
assert_contains "${diff_v2}" "CREATE TABLE orders" "v2 diff create orders"
assert_contains "${diff_v2}" "status TEXT" "v2 diff maps enum to TEXT"
assert_contains "${diff_v2}" "amount NUMERIC" "v2 diff maps decimal to NUMERIC"
assert_not_contains "${diff_v2}" "CREATE TYPE" "v2 diff does not use postgres enum type"
assert_not_contains "${diff_v2}" "CHECK" "v2 diff does not generate enum check constraint"
assert_contains "${diff_v2}" "ALTER TABLE users ADD COLUMN active" "v2 diff add active"
assert_contains "${diff_v2}" "ALTER TABLE users ADD COLUMN address_id" "v2 diff add address_id"
assert_contains "${diff_v2}" "ADD CONSTRAINT fk_users_address_id" "v2 diff add fk"
assert_contains "${diff_v2}" "customer_id" "v2 diff infers order customer fk"
echo "PASS: v2 diff"

generate_v2="$(run_marreta migrate generate)"
assert_contains "${generate_v2}" "Generated migration" "v2 generate output"
v2_id="$(echo "${generate_v2}" | sed -n 's/^Generated migration: //p' | head -1)"
v2_version="$(echo "${v2_id}" | cut -d '_' -f 1,2)"
[[ -n "${v2_id}" ]] || fail "could not parse v2 migration id"
v2_up="${MIGRATIONS_DIR}/${v2_id}.up.sql"
v2_down="${MIGRATIONS_DIR}/${v2_id}.down.sql"
v2_up_backup="${MIGRATIONS_DIR}/${v2_id}.up.sql.bak"
cp "${v2_up}" "${v2_up_backup}"
[[ -f "${v2_down}" ]] || fail "missing v2 down.sql"
list_v2_pending="$(run_marreta migrate list)"
assert_contains "${list_v2_pending}" "${v2_version}" "v2 list contains pending migration version"
assert_contains "${list_v2_pending}" "update_addresses" "v2 list contains pending migration name"
assert_contains "${list_v2_pending}" "pending" "v2 list pending state"
explain_pending="$(run_marreta migrate explain pending)"
assert_contains "${explain_pending}" "State: pending" "explain pending header"
assert_contains "${explain_pending}" "discard the local pending migration" "explain pending discard guidance"
echo "PASS: v2 generate"

apply_v2="$(run_marreta migrate apply)"
assert_contains "${apply_v2}" "Applied ${v2_id}" "v2 apply output"
status_v2_clean="$(run_marreta migrate status)"
assert_contains "${status_v2_clean}" "${v1_id}" "v2 status contains v1"
assert_contains "${status_v2_clean}" "${v2_id}" "v2 status contains v2"
assert_contains "${status_v2_clean}" "Pending:" "v2 status pending section"
assert_contains "${status_v2_clean}" "Changed:" "v2 status changed section"
assert_contains "${status_v2_clean}" "Missing local:" "v2 status missing_local section"
list_v2_clean="$(run_marreta migrate list)"
assert_contains "${list_v2_clean}" "${v1_version}" "v2 clean list contains v1"
assert_contains "${list_v2_clean}" "${v2_version}" "v2 clean list contains v2"
assert_contains "${list_v2_clean}" "applied" "v2 clean list applied state"
echo "PASS: v2 apply and status"

doctor_v2_connect="$(run_marreta doctor --connect)"
assert_contains "${doctor_v2_connect}" "OK    db connection" "doctor v2 db connectivity"
assert_contains "${doctor_v2_connect}" "OK    2 applied" "doctor v2 applied summary"
assert_contains "${doctor_v2_connect}" "OK    no pending migrations" "doctor v2 no pending summary"
assert_contains "${doctor_v2_connect}" "schema Order.customer persists as orders.customer_id -> users.id" "doctor v2 relation inference"
assert_contains "${doctor_v2_connect}" "schema User.orders is inferred from Order.customer" "doctor v2 inverse inference"
echo "PASS: v2 doctor --connect"

tables_after_v2="$(sql "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename;")"
assert_contains "${tables_after_v2}" "addresses" "v2 addresses table exists"
assert_contains "${tables_after_v2}" "orders" "v2 orders table exists"
user_columns_v2="$(sql "SELECT column_name FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'users' ORDER BY ordinal_position;")"
assert_contains "${user_columns_v2}" "active" "v2 users.active exists"
assert_contains "${user_columns_v2}" "address_id" "v2 users.address_id exists"
assert_not_contains "${user_columns_v2}" "orders" "v2 users should not get a storage column for list relations"
order_columns_v2="$(sql "SELECT column_name FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'orders' ORDER BY ordinal_position;")"
assert_contains "${order_columns_v2}" "customer_id" "v2 orders.customer_id exists"
order_contract_types_v2="$(sql "SELECT column_name || ':' || data_type FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'orders' AND column_name IN ('status', 'amount') ORDER BY column_name;")"
assert_contains "${order_contract_types_v2}" "amount:numeric" "v2 orders.amount is numeric"
assert_contains "${order_contract_types_v2}" "status:text" "v2 orders.status enum is text"
fk_count_v2="$(sql "SELECT COUNT(*) FROM information_schema.table_constraints WHERE table_schema = 'public' AND table_name = 'users' AND constraint_type = 'FOREIGN KEY';")"
[[ "${fk_count_v2}" -ge 1 ]] || fail "v2 users foreign key missing"
order_fk_count_v2="$(sql "SELECT COUNT(*) FROM information_schema.table_constraints WHERE table_schema = 'public' AND table_name = 'orders' AND constraint_type = 'FOREIGN KEY';")"
[[ "${order_fk_count_v2}" -ge 1 ]] || fail "v2 orders foreign key missing"
applied_after_v2="$(sql "SELECT version || '_' || name FROM _marreta_migrations ORDER BY version;")"
assert_contains "${applied_after_v2}" "${v1_id}" "v2 applied contains v1"
assert_contains "${applied_after_v2}" "${v2_id}" "v2 applied contains v2"
echo "PASS: v2 postgres verification"

echo ""
echo "Phase C2 — Query navigation runtime"

docker compose -f "${COMPOSE_FILE}" up -d --wait app
wait_server

order_create_body="$(curl -s -X POST "http://127.0.0.1:${SERVER_PORT}/orders" \
    -H "Content-Type: application/json" \
    -d "{\"total\":99.5,\"customer_id\":${created_id}}")"
assert_contains "${order_create_body}" "\"customer_id\":${created_id}" "runtime order create customer_id"
order_id="$(echo "${order_create_body}" | jq -r '.id')"
[[ "${order_id}" != "null" && -n "${order_id}" ]] || fail "runtime order create response missing id"

customer_nav_body="$(curl -s "http://127.0.0.1:${SERVER_PORT}/orders/${order_id}/customer")"
assert_contains "${customer_nav_body}" "\"id\":${created_id}" "runtime relation fetch customer id"
assert_contains "${customer_nav_body}" '"name":"Ana"' "runtime relation fetch customer name"

orders_nav_body="$(curl -s "http://127.0.0.1:${SERVER_PORT}/users/${created_id}/orders")"
assert_contains "${orders_nav_body}" "\"count\":1" "runtime inverse relation count"
assert_contains "${orders_nav_body}" "\"customer_id\":${created_id}" "runtime inverse relation customer_id"
assert_contains "${orders_nav_body}" "\"total\":99.5" "runtime inverse relation total"

docker compose -f "${COMPOSE_FILE}" stop app >/dev/null
docker compose -f "${COMPOSE_FILE}" rm -f app >/dev/null
echo "PASS: query navigation runtime"

echo ""
echo "Phase D — Drift detection"

printf '\n-- edited locally\n' >> "${v2_up}"
status_v2_changed="$(run_marreta migrate status)"
assert_contains "${status_v2_changed}" "Changed:" "drift changed section"
assert_contains "${status_v2_changed}" "${v2_id}" "drift status contains v2"
list_v2_changed="$(run_marreta migrate list)"
assert_contains "${list_v2_changed}" "${v2_version}" "drift list contains v2"
assert_contains "${list_v2_changed}" "changed" "drift list changed state"
explain_changed="$(run_marreta migrate explain changed)"
assert_contains "${explain_changed}" "State: changed" "explain changed header"
assert_contains "${explain_changed}" "do not edit applied migrations" "explain changed guidance"
set +e
apply_drift_output="$(run_marreta migrate apply 2>&1)"
apply_drift_exit=$?
set -e
[[ "${apply_drift_exit}" -ne 0 ]] || fail "apply should fail when migration drift exists"
assert_contains "${apply_drift_output}" "migration state is inconsistent" "drift apply refusal"
mv "${v2_up_backup}" "${v2_up}"
echo "PASS: drift detection"

echo ""
echo "Phase E — Missing local detection"

v1_up_backup="${v1_up}.bak"
v1_down_backup="${v1_down}.bak"
mv "${v1_up}" "${v1_up_backup}"
mv "${v1_down}" "${v1_down_backup}"
status_missing_local="$(run_marreta migrate status)"
assert_contains "${status_missing_local}" "Missing local:" "missing_local section"
assert_contains "${status_missing_local}" "${v1_id}" "missing_local contains v1"
list_missing_local="$(run_marreta migrate list)"
assert_contains "${list_missing_local}" "${v1_version}" "missing_local list contains v1"
assert_contains "${list_missing_local}" "missing_local" "missing_local list state"
explain_missing_local="$(run_marreta migrate explain missing_local)"
assert_contains "${explain_missing_local}" "State: missing_local" "explain missing_local header"
assert_contains "${explain_missing_local}" "restore the missing migration files" "explain missing_local guidance"
mv "${v1_up_backup}" "${v1_up}"
mv "${v1_down_backup}" "${v1_down}"
echo "PASS: missing local detection"

echo ""
echo "Phase F — Rollback and discard"

rollback_v2="$(run_marreta migrate rollback)"
assert_contains "${rollback_v2}" "Rolled back ${v2_id}" "rollback output"
tables_after_rollback="$(sql "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename;")"
assert_not_contains "${tables_after_rollback}" "addresses" "rollback removes addresses table"
assert_not_contains "${tables_after_rollback}" "orders" "rollback removes orders table"
user_columns_after_rollback="$(sql "SELECT column_name FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'users' ORDER BY ordinal_position;")"
assert_not_contains "${user_columns_after_rollback}" "active" "rollback removes active column"
assert_not_contains "${user_columns_after_rollback}" "address_id" "rollback removes address_id column"
applied_after_rollback="$(sql "SELECT version || '_' || name FROM _marreta_migrations ORDER BY version;")"
assert_contains "${applied_after_rollback}" "${v1_id}" "rollback keeps v1 migration"
assert_not_contains "${applied_after_rollback}" "${v2_id}" "rollback removes v2 migration record"
list_after_rollback="$(run_marreta migrate list)"
assert_contains "${list_after_rollback}" "${v2_version}" "rollback list contains v2"
assert_contains "${list_after_rollback}" "pending" "rollback list pending state"
explain_workflow="$(run_marreta migrate explain workflow)"
assert_contains "${explain_workflow}" "applied -> pending" "explain workflow rollback transition"
discard_v2="$(run_marreta migrate discard "${v2_version}")"
assert_contains "${discard_v2}" "Discarded ${v2_id}" "discard output"
[[ ! -f "${v2_up}" ]] || fail "discard should remove v2 up.sql"
[[ ! -f "${v2_down}" ]] || fail "discard should remove v2 down.sql"
list_after_discard="$(run_marreta migrate list)"
assert_not_contains "${list_after_discard}" "${v2_version}" "discard removes v2 from list"
diff_after_discard="$(run_marreta migrate diff)"
assert_contains "${diff_after_discard}" "CREATE TABLE addresses" "discard keeps schema diff visible"
echo "PASS: rollback and discard"

echo ""
echo "Phase G — Hand-written SQL replay tolerance (Spec 073, the trap made green)"

# Reset to the v1 schema so the only desired table is users (already applied): a clean baseline.
swap_schema "v1"
sleep 1

hand_index_id="20300101_000001_hand_index"
printf 'CREATE INDEX idx_users_email ON users (email);\n' > "${MIGRATIONS_DIR}/${hand_index_id}.up.sql"
printf 'DROP INDEX idx_users_email;\n' > "${MIGRATIONS_DIR}/${hand_index_id}.down.sql"
apply_hand="$(run_marreta migrate apply)"
assert_contains "${apply_hand}" "Applied ${hand_index_id}" "hand-written index applies"
idx_exists="$(sql "SELECT indexname FROM pg_indexes WHERE tablename = 'users' AND indexname = 'idx_users_email';")"
assert_contains "${idx_exists}" "idx_users_email" "hand-written index exists in postgres"
# The trap, made green: generate/diff keep working after a hand-written CREATE INDEX is applied.
generate_after_hand="$(run_marreta migrate generate)"
assert_contains "${generate_after_hand}" "up to date" "generate works after a hand-written index"
diff_after_hand="$(run_marreta migrate diff)"
assert_contains "${diff_after_hand}" "up to date" "diff works after a hand-written index"
echo "PASS: hand-written SQL replay tolerance"

echo ""
echo "Phase H — skip-replay marker (Spec 073)"

ext_id="20300101_000002_ext"
printf -- '-- marreta: skip-replay\nCREATE EXTENSION IF NOT EXISTS pgcrypto;\n' > "${MIGRATIONS_DIR}/${ext_id}.up.sql"
printf -- '-- marreta: skip-replay\nDROP EXTENSION IF EXISTS pgcrypto;\n' > "${MIGRATIONS_DIR}/${ext_id}.down.sql"
apply_ext="$(run_marreta migrate apply)"
assert_contains "${apply_ext}" "Applied ${ext_id}" "skip-replay migration applies"
generate_after_ext="$(run_marreta migrate generate)"
assert_contains "${generate_after_ext}" "up to date" "generate works past a skip-replay statement"
echo "PASS: skip-replay marker"

echo ""
echo "Phase I — Rejected column-mutating DDL error (Spec 073)"

bad_id="20300101_000003_baddl"
printf 'ALTER TABLE users DROP COLUMN email;\n' > "${MIGRATIONS_DIR}/${bad_id}.up.sql"
printf 'SELECT 1;\n' > "${MIGRATIONS_DIR}/${bad_id}.down.sql"
set +e
bad_output="$(run_marreta migrate diff 2>&1)"
bad_exit=$?
set -e
[[ "${bad_exit}" -ne 0 ]] || fail "diff should fail on a column-mutating hand-written statement"
assert_contains "${bad_output}" "${bad_id}" "rejected error names the file"
assert_contains "${bad_output}" "DROP COLUMN email" "rejected error names the statement"
assert_contains "${bad_output}" "skip-replay" "rejected error offers the escape valve"
rm "${MIGRATIONS_DIR}/${bad_id}.up.sql" "${MIGRATIONS_DIR}/${bad_id}.down.sql"
echo "PASS: rejected DDL error"

echo ""
echo "Phase J — Schema drift report (Spec 073, the silence made loud)"

# Change users.email from string to integer: a type change the additive-only planner does not
# support. The previous behaviour was a silent "up to date"; now it must be reported, not acted on.
cat > "${SCHEMA_FILE}" <<'EOF'
schema User
    db: users

    id: integer
    name: string
    email: integer
EOF
drift_diff="$(run_marreta migrate diff)"
assert_contains "${drift_diff}" "Unsupported changes detected" "drift block header in diff"
assert_contains "${drift_diff}" "users.email: type differs" "drift names the email type change"
assert_not_contains "${drift_diff}" "Database schema is up to date." "drift is no longer silent"
drift_generate="$(run_marreta migrate generate)"
assert_contains "${drift_generate}" "Unsupported changes detected" "generate also reports drift"
assert_not_contains "${drift_generate}" "Generated migration" "generate writes nothing for a drift-only change"
swap_schema "v1"
echo "PASS: schema drift report"

echo ""
echo "Migrations directory: ${MIGRATIONS_DIR}"
echo "Results: PASS"
