// Pass/fail gates — adjust after first baseline run.
// These are intentionally lenient for the first run (discovery mode).
export const thresholds = {
  // latency
  "http_req_duration{scenario:health}":   ["p(95)<200"],
  "http_req_duration{scenario:products}": ["p(95)<300"],
  "http_req_duration{scenario:orders}":   ["p(95)<400"],

  // error rate — excludes expected 422s (counted separately)
  "http_req_failed{scenario:health}":   ["rate<0.01"],
  "http_req_failed{scenario:products}": ["rate<0.01"],
  // orders: ~10% are intentional 422s — threshold covers unexpected failures only
  "http_req_failed{scenario:orders}":   ["rate<0.15"],
};
