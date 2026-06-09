// Thresholds for the doc.* (MongoDB) ecommerce load test.
// Intentionally lenient for the first discovery run — tighten after baseline is established.
export const thresholds_doc = {
  "http_req_duration{scenario:health}":   ["p(95)<200"],
  "http_req_duration{scenario:products}": ["p(95)<500"],
  "http_req_duration{scenario:orders}":   ["p(95)<500"],

  "http_req_failed{scenario:health}":   ["rate<0.01"],
  "http_req_failed{scenario:products}": ["rate<0.01"],
  // orders: ~10% are intentional 422s
  "http_req_failed{scenario:orders}":   ["rate<0.15"],
};
