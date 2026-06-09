// Quick smoke test for the doc.* (MongoDB) ecommerce variant.
// Duration: ~30s total (10 VUs, 10s per scenario, sequential).
import http from "k6/http";
import { check } from "k6";

const BASE_URL = __ENV.BASE_URL || "http://localhost:8080";
const JSON_HEADERS = { "Content-Type": "application/json" };

const orderValid   = open("./payloads/order_valid.json");
const orderMissing = open("./payloads/order_missing.json");
const productValid = open("./payloads/product_valid.json");

export const options = {
  scenarios: {
    smoke_health: {
      executor: "constant-vus",
      exec: "smoke_health",
      vus: 10,
      duration: "10s",
      tags: { scenario: "health" },
    },
    smoke_products: {
      executor: "constant-vus",
      exec: "smoke_products",
      vus: 10,
      duration: "10s",
      startTime: "12s",
      tags: { scenario: "products" },
    },
    smoke_orders: {
      executor: "constant-vus",
      exec: "smoke_orders",
      vus: 10,
      duration: "10s",
      startTime: "24s",
      tags: { scenario: "orders" },
    },
  },
  thresholds: {
    "http_req_failed{scenario:health}":   ["rate<0.01"],
    "http_req_failed{scenario:products}": ["rate<0.01"],
    "http_req_failed{scenario:orders}":   ["rate<0.15"],
  },
};

export function smoke_health() {
  const res = http.get(`${BASE_URL}/health`);
  check(res, { "health 200": (r) => r.status === 200 });
}

export function smoke_products() {
  const res = http.post(`${BASE_URL}/products`, productValid, { headers: JSON_HEADERS });
  check(res, { "products 201": (r) => r.status === 201 });
}

export function smoke_orders() {
  const isError = Math.random() < 0.10;
  const payload = isError ? orderMissing : orderValid;
  const res = http.post(`${BASE_URL}/orders`, payload, { headers: JSON_HEADERS });
  check(res, {
    "orders ok": (r) => isError ? r.status === 422 : r.status === 201,
  });
}
