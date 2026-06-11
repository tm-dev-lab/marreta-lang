import http from "k6/http";
import { check } from "k6";

const baseUrl = __ENV.BASE_URL || "http://localhost:8080";
const rate = Number(__ENV.RATE || "500");
const duration = __ENV.DURATION || "60s";
const accountCount = Number(__ENV.ACCOUNTS || "50");

const headers = { "Content-Type": "application/json" };

const endpoints = [
  "get_balance",
  "get_account",
  "list_transactions",
  "deposit",
  "withdraw",
  "transfer",
  "create_account",
];

const endpointThresholds = Object.fromEntries(
  endpoints.map((endpoint) => [`http_req_duration{endpoint:${endpoint}}`, ["p(95)<500"]]),
);

export const options = {
  summaryTrendStats: ["avg", "min", "med", "p(90)", "p(95)", "p(99)", "max"],
  scenarios: {
    steady: {
      executor: "constant-arrival-rate",
      rate,
      timeUnit: "1s",
      duration,
      preAllocatedVUs: Number(__ENV.PREALLOC_VUS || Math.max(20, Math.ceil(rate / 2))),
      maxVUs: Number(__ENV.MAX_VUS || Math.max(100, rate * 2)),
    },
  },
  thresholds: {
    http_req_failed: ["rate<0.01"],
    http_req_duration: ["p(95)<500"],
    ...endpointThresholds,
  },
};

// Create a pool of funded accounts once, before the measured load.
export function setup() {
  const ids = [];
  for (let i = 0; i < accountCount; i++) {
    const res = http.post(`${baseUrl}/accounts`, JSON.stringify({ owner: `user_${i}` }), {
      headers,
    });
    const id = res.json("_id");
    // Fund generously so withdrawals/transfers never deplete during the run.
    http.post(`${baseUrl}/accounts/${id}/deposit`, JSON.stringify({ amount: 1000000000 }), {
      headers,
    });
    ids.push(id);
  }
  return { ids };
}

function pick(ids) {
  return ids[Math.floor(Math.random() * ids.length)];
}

function amount(min, max) {
  return Math.floor(Math.random() * (max - min + 1)) + min;
}

export default function (data) {
  const ids = data.ids;
  const id = pick(ids);
  const r = Math.random();
  let res;
  let endpoint;

  if (r < 0.25) {
    endpoint = "get_balance";
    res = http.get(`${baseUrl}/accounts/${id}/balance`, { tags: { endpoint } });
  } else if (r < 0.4) {
    endpoint = "get_account";
    res = http.get(`${baseUrl}/accounts/${id}`, { tags: { endpoint } });
  } else if (r < 0.55) {
    endpoint = "list_transactions";
    res = http.get(`${baseUrl}/accounts/${id}/transactions`, { tags: { endpoint } });
  } else if (r < 0.7) {
    endpoint = "deposit";
    res = http.post(`${baseUrl}/accounts/${id}/deposit`, JSON.stringify({ amount: amount(100, 5000) }), {
      headers,
      tags: { endpoint },
    });
  } else if (r < 0.82) {
    endpoint = "withdraw";
    res = http.post(`${baseUrl}/accounts/${id}/withdraw`, JSON.stringify({ amount: amount(100, 5000) }), {
      headers,
      tags: { endpoint },
    });
  } else if (r < 0.95) {
    endpoint = "transfer";
    let to = pick(ids);
    while (to === id) to = pick(ids);
    res = http.post(
      `${baseUrl}/transfers`,
      JSON.stringify({ from_account: id, to_account: to, amount: amount(100, 2000) }),
      { headers, tags: { endpoint } },
    );
  } else {
    endpoint = "create_account";
    res = http.post(`${baseUrl}/accounts`, JSON.stringify({ owner: "walk_in" }), {
      headers,
      tags: { endpoint },
    });
  }

  check(res, { [`${endpoint} ok`]: (r) => r.status >= 200 && r.status < 300 });
}
