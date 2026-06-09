import http from "k6/http";
import { check, sleep } from "k6";
import { thresholds_doc } from "./thresholds_doc.js";

const BASE_URL = __ENV.BASE_URL || "http://localhost:3000";

const orderValid   = open("./payloads/order_valid.json");
const orderMissing = open("./payloads/order_missing.json");
const productValid = open("./payloads/product_valid.json");

const JSON_HEADERS = { "Content-Type": "application/json" };

const stages = [
  { duration: "30s", target: 50  },
  { duration: "2m",  target: 50  },
  { duration: "30s", target: 200 },
  { duration: "30s", target: 0   },
];

export const options = {
  scenarios: {
    health: {
      executor: "ramping-vus",
      exec: "health",
      stages,
      gracefulRampDown: "10s",
      tags: { scenario: "health" },
    },
    products: {
      executor: "ramping-vus",
      exec: "products",
      stages,
      gracefulRampDown: "10s",
      tags: { scenario: "products" },
      startTime: "4m",
    },
    orders: {
      executor: "ramping-vus",
      exec: "orders",
      stages,
      gracefulRampDown: "10s",
      tags: { scenario: "orders" },
      startTime: "8m",
    },
  },
  thresholds: thresholds_doc,
  summaryTrendStats: ["min", "avg", "med", "p(90)", "p(95)", "p(99)", "max"],
};

export function health() {
  const res = http.get(`${BASE_URL}/health`, { tags: { scenario: "health" } });
  check(res, {
    "health: status 200": (r) => r.status === 200,
    "health: ok:true":    (r) => r.json("ok") === true,
  });
  sleep(0.01);
}

export function products() {
  const res = http.post(
    `${BASE_URL}/products`,
    productValid,
    { headers: JSON_HEADERS, tags: { scenario: "products" } }
  );
  check(res, {
    "products: status 201": (r) => r.status === 201,
    "products: created":    (r) => r.json("created") === true,
  });
  sleep(0.01);
}

export function orders() {
  const isErrorInjection = Math.random() < 0.10;
  const payload = isErrorInjection ? orderMissing : orderValid;

  const res = http.post(
    `${BASE_URL}/orders`,
    payload,
    { headers: JSON_HEADERS, tags: { scenario: "orders" } }
  );

  if (isErrorInjection) {
    check(res, {
      "orders(422): status 422": (r) => r.status === 422,
    });
  } else {
    check(res, {
      "orders: status 201":        (r) => r.status === 201,
      "orders: order_created":     (r) => r.json("order_created") === true,
      "orders: discount_rate 0.1": (r) => r.json("discount_rate") === 0.1,
    });
  }
  sleep(0.01);
}
