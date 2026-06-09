// db_ops.js — functional test for examples/db_ops/app.marreta.
//
// Runs sequentially (1 VU, 1 iteration) so each step can depend on state
// from previous steps. Every check must pass (threshold: rate==1.0).
//
// Coverage:
//   Direct CRUD:  save, find, find_all, find_all(filter), update, delete
//   Pipeline:     fetch, fetch_one, count, exists, where, order_by,
//                 limit+offset, bulk update, bulk delete
//   Chained:      where+order_by+limit, where+where
//   Parallel:     *>> active_count+active_list, *>> db+meta
//   Native query: plain, $1 param, $1+$2 params
//   Transaction:  commit path, rollback path

import http from "k6/http";
import { check } from "k6";

const BASE = __ENV.BASE_URL || "http://localhost:3000";
const H = { "Content-Type": "application/json" };

export const options = {
  scenarios: {
    db_ops: { executor: "shared-iterations", vus: 1, iterations: 1 },
  },
  thresholds: { checks: ["rate==1.0"] },
};

function j(res) {
  try { return JSON.parse(res.body); } catch (_) { return {}; }
}

function assert(name, res, status, bodyChecks) {
  const ok = check(res, {
    [`${name} — status ${status}`]: (r) => r.status === status,
    ...bodyChecks,
  });
  if (!ok) console.error(`FAILED: ${name} | status=${res.status} body=${res.body}`);
}

export default function () {

  // ── 1. Direct: save ────────────────────────────────────────────────────────
  let savedId;
  {
    const res = http.post(`${BASE}/save`,
      JSON.stringify({ name: "k6-item", active: true }), { headers: H });
    assert("save", res, 201, {
      "save — id present":    (r) => typeof j(r).id === "number",
      "save — name matches":  (r) => j(r).name === "k6-item",
      "save — active true":   (r) => j(r).active === true,
    });
    savedId = j(res).id;
  }

  // ── 2. Direct: find by id ──────────────────────────────────────────────────
  {
    const res = http.get(`${BASE}/find/${savedId}`);
    assert("find", res, 200, {
      "find — id matches":   (r) => j(r).id === savedId,
      "find — name matches": (r) => j(r).name === "k6-item",
    });
  }

  // ── 3. Direct: find non-existent → 404 ────────────────────────────────────
  {
    const res = http.get(`${BASE}/find/9999999`);
    assert("find 404", res, 404, {});
  }

  // ── 4. Direct: find_all (no filter) ───────────────────────────────────────
  {
    const res = http.get(`${BASE}/find_all`);
    assert("find_all", res, 200, {
      "find_all — items array":    (r) => Array.isArray(j(r).items),
      "find_all — at least 1":     (r) => j(r).items.length >= 1,
    });
  }

  // ── 5. Direct: find_all with filter ───────────────────────────────────────
  {
    const res = http.get(`${BASE}/find_all/filter`);
    assert("find_all/filter", res, 200, {
      "find_all/filter — all active": (r) => j(r).items.every(i => i.active === true),
    });
  }

  // ── 6. Direct: update ─────────────────────────────────────────────────────
  {
    // Keep active:true so the bulk-delete step (which deletes inactive items)
    // doesn't remove this item before the final explicit delete.
    const res = http.put(`${BASE}/update/${savedId}`,
      JSON.stringify({ name: "k6-updated", active: true }), { headers: H });
    assert("update", res, 200, {
      "update — name changed":  (r) => j(r).name === "k6-updated",
      "update — active true":   (r) => j(r).active === true,
    });
  }

  // ── 7. Pipeline: fetch ────────────────────────────────────────────────────
  {
    const res = http.get(`${BASE}/pipeline/fetch`);
    assert("pipeline/fetch", res, 200, {
      "fetch — items array": (r) => Array.isArray(j(r).items),
    });
  }

  // ── 8. Pipeline: fetch_one ────────────────────────────────────────────────
  {
    const res = http.get(`${BASE}/pipeline/fetch_one`);
    assert("pipeline/fetch_one", res, 200, {
      "fetch_one — item present": (r) => j(r).item !== null && j(r).item !== undefined,
    });
  }

  // ── 9. Pipeline: count ────────────────────────────────────────────────────
  {
    const res = http.get(`${BASE}/pipeline/count`);
    assert("pipeline/count", res, 200, {
      "count — integer": (r) => typeof j(r).count === "number",
      "count — >= 1":    (r) => j(r).count >= 1,
    });
  }

  // ── 10. Pipeline: exists ──────────────────────────────────────────────────
  {
    const res = http.get(`${BASE}/pipeline/exists`);
    assert("pipeline/exists", res, 200, {
      "exists — boolean": (r) => typeof j(r).exists === "boolean",
    });
  }

  // ── 11. Pipeline: where ───────────────────────────────────────────────────
  {
    const res = http.get(`${BASE}/pipeline/where`);
    assert("pipeline/where", res, 200, {
      "where — all active": (r) => j(r).items.every(i => i.active === true),
    });
  }

  // ── 12. Pipeline: order_by ────────────────────────────────────────────────
  {
    const res = http.get(`${BASE}/pipeline/order_by`);
    assert("pipeline/order_by", res, 200, {
      "order_by — items array": (r) => Array.isArray(j(r).items),
    });
  }

  // ── 13. Pipeline: limit + offset ─────────────────────────────────────────
  {
    const res = http.get(`${BASE}/pipeline/limit_offset`);
    assert("pipeline/limit_offset", res, 200, {
      "limit_offset — at most 2 items": (r) => j(r).items.length <= 2,
    });
  }

  // ── 14. Pipeline: bulk update ─────────────────────────────────────────────
  {
    // First save two fresh active items so we have something to bulk-update
    http.post(`${BASE}/save`, JSON.stringify({ name: "bulk-a", active: true }), { headers: H });
    http.post(`${BASE}/save`, JSON.stringify({ name: "bulk-b", active: true }), { headers: H });

    const res = http.post(`${BASE}/pipeline/update`, null, { headers: H });
    assert("pipeline/update", res, 200, {
      "bulk update — updated >= 0": (r) => typeof j(r).updated === "number",
    });
  }

  // ── 15. Pipeline: bulk delete ─────────────────────────────────────────────
  {
    // Ensure at least one inactive row exists (the bulk update just deactivated some)
    const res = http.del(`${BASE}/pipeline/delete`);
    assert("pipeline/delete", res, 200, {
      "bulk delete — deleted >= 0": (r) => typeof j(r).deleted === "number",
    });
  }

  // ── 16. Pipeline: chained (where + order_by + limit → fetch) ──────────────
  {
    const res = http.get(`${BASE}/pipeline/chained`);
    assert("pipeline/chained", res, 200, {
      "chained — at most 2 items": (r) => j(r).items.length <= 2,
      "chained — all active":      (r) => j(r).items.every(i => i.active === true),
    });
  }

  // ── 17. Pipeline: chained count (where + where → count) ───────────────────
  {
    const res = http.get(`${BASE}/pipeline/chained/count`);
    assert("pipeline/chained/count", res, 200, {
      "chained count — integer": (r) => typeof j(r).count === "number",
    });
  }

  // ── 18. Pipeline: parallel *>> ────────────────────────────────────────────
  {
    const res = http.get(`${BASE}/pipeline/parallel`);
    assert("pipeline/parallel", res, 200, {
      "parallel — count integer": (r) => typeof j(r).count === "number",
      "parallel — items array":   (r) => Array.isArray(j(r).items),
    });
  }

  // ── 19. Pipeline: parallel db + meta ─────────────────────────────────────
  {
    const res = http.get(`${BASE}/pipeline/parallel/db_and_meta`);
    assert("pipeline/parallel/db_and_meta", res, 200, {
      "parallel/db_meta — items array":   (r) => Array.isArray(j(r).items),
      "parallel/db_meta — meta source":   (r) => j(r).meta && j(r).meta.source === "db.items",
    });
  }

  // ── 20. Native query: plain ───────────────────────────────────────────────
  {
    const res = http.get(`${BASE}/native_query`);
    assert("native_query", res, 200, {
      "native_query — items array": (r) => Array.isArray(j(r).items),
    });
  }

  // ── 21. Native query: $1 param ────────────────────────────────────────────
  {
    const res = http.get(`${BASE}/native_query/param/alpha`);
    assert("native_query/param", res, 200, {
      "native_query/param — items array": (r) => Array.isArray(j(r).items),
    });
  }

  // ── 22. Native query: $1 + $2 params ─────────────────────────────────────
  {
    const res = http.get(`${BASE}/native_query/multi_param`);
    assert("native_query/multi_param", res, 200, {
      "multi_param — items array": (r) => Array.isArray(j(r).items),
    });
  }

  // ── 23. Transaction: commit path ──────────────────────────────────────────
  {
    const res = http.post(`${BASE}/transaction`, null, { headers: H });
    assert("transaction/commit", res, 201, {
      "transaction — a_id present": (r) => typeof j(r).a_id === "number",
      "transaction — b_id present": (r) => typeof j(r).b_id === "number",
    });
  }

  // ── 24. Transaction: rollback path ────────────────────────────────────────
  {
    // Get count before
    const before = j(http.get(`${BASE}/pipeline/count`)).count;

    // Trigger rollback — expect 500 (intentional guard failure)
    const res = http.post(`${BASE}/transaction/rollback_test`,
      JSON.stringify({ should_commit: false }), { headers: H });
    assert("transaction/rollback", res, 500, {});

    // Count must be unchanged
    const after = j(http.get(`${BASE}/pipeline/count`)).count;
    check({ before, after }, {
      "rollback — row count unchanged": ({ before, after }) => before === after,
    });
  }

  // ── 25. Transaction: rollback_test commit path ────────────────────────────
  {
    const before = j(http.get(`${BASE}/pipeline/count`)).count;

    const res = http.post(`${BASE}/transaction/rollback_test`,
      JSON.stringify({ should_commit: true }), { headers: H });
    assert("transaction/rollback_test commit", res, 200, {
      "rollback_test — committed:true": (r) => j(r).committed === true,
    });

    const after = j(http.get(`${BASE}/pipeline/count`)).count;
    check({ before, after }, {
      "commit — row count increased by 2": ({ before, after }) => after === before + 2,
    });
  }

  // ── 26. Direct: delete — save a fresh item then delete it ─────────────────
  // (savedId was removed by the bulk update + bulk delete cycle in steps 14–15)
  {
    const freshRes = http.post(`${BASE}/save`,
      JSON.stringify({ name: "to-delete", active: true }), { headers: H });
    const freshId = j(freshRes).id;

    const res = http.del(`${BASE}/delete/${freshId}`);
    assert("delete", res, 200, {
      "delete — deleted:true": (r) => j(r).deleted === true,
    });
  }

  // ── 27. Direct: delete non-existent → 404 ─────────────────────────────────
  {
    const res = http.del(`${BASE}/delete/9999999`);
    assert("delete 404", res, 404, {});
  }
}
