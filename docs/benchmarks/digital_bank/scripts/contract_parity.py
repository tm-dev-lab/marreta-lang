#!/usr/bin/env python3
"""Contract-parity check for the digital bank benchmark.

Runs the same request sequence against each contender and asserts they agree on HTTP status
codes, on the response key structure of success bodies, and on the deterministic values (balances,
currency, echoed fields). This is what stops any app from being a strawman or accidentally
skipping validation or the funds check. Error-body shapes are intentionally not compared (each
framework formats errors its own way); only the status code matters for the error cases.

Usage:
    python3 contract_parity.py [name=url ...]
Defaults to the four local compose ports. Pass a subset to check only the apps that are up, e.g.
    python3 contract_parity.py spring=http://localhost:18083 fastapi=http://localhost:18081
"""
import json
import sys
import urllib.error
import urllib.request

DEFAULT_TARGETS = {
    "marreta": "http://localhost:18080",
    "fastapi": "http://localhost:18081",
    "nest": "http://localhost:18082",
    "spring": "http://localhost:18083",
}


def call(base, method, path, body=None):
    url = base + path
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    if data is not None:
        req.add_header("content-type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            raw = resp.read().decode()
            return resp.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as err:
        raw = err.read().decode()
        try:
            return err.code, json.loads(raw) if raw else None
        except json.JSONDecodeError:
            return err.code, None


def shape(value):
    """A structural fingerprint: object -> sorted keys (recursive), list -> first element's shape."""
    if isinstance(value, dict):
        return {k: shape(value[k]) for k in sorted(value)}
    if isinstance(value, list):
        return ["[]" if not value else shape(value[0])]
    return type(value).__name__ if value is not None else "null"


def probe(base):
    """Run the full sequence against one app and return a list of (label, status, shape, values)."""
    steps = []

    def record(label, status, body, values=None):
        steps.append((label, status, shape(body), values or {}))

    s, b = call(base, "POST", "/accounts", {"owner": "alice"})
    acct = b.get("_id") if isinstance(b, dict) else None
    record("create", s, b, {"owner": b.get("owner"), "balance": b.get("balance"),
                            "currency": b.get("currency"), "active": b.get("active")})

    s, b = call(base, "GET", f"/accounts/{acct}")
    record("get", s, b)

    s, b = call(base, "GET", f"/accounts/{acct}/balance")
    record("balance", s, b, {"balance": b.get("balance"), "currency": b.get("currency")})

    s, b = call(base, "POST", f"/accounts/{acct}/deposit", {"amount": 500})
    record("deposit", s, b, {"balance": b.get("balance")})

    s, b = call(base, "POST", f"/accounts/{acct}/withdraw", {"amount": 200})
    record("withdraw", s, b, {"balance": b.get("balance")})

    s, b = call(base, "POST", f"/accounts/{acct}/withdraw", {"amount": 999999})
    record("withdraw_insufficient", s, None)  # error: status only

    s, b = call(base, "POST", f"/accounts/{acct}/deposit", {"amount": -5})
    record("deposit_negative", s, None)

    s, b = call(base, "POST", f"/accounts/{acct}/deposit", {})
    record("deposit_missing_amount", s, None)

    s, b = call(base, "GET", f"/accounts/{acct}/transactions")
    record("transactions", s, b, {"count": len(b.get("transactions", [])) if isinstance(b, dict) else None})

    s, b2 = call(base, "POST", "/accounts", {"owner": "bob"})
    acct2 = b2.get("_id") if isinstance(b2, dict) else None
    s, b = call(base, "POST", "/transfers",
                {"from_account": acct, "to_account": acct2, "amount": 100})
    record("transfer", s, b, {"source_balance": b.get("source_balance"),
                              "target_balance": b.get("target_balance")})

    s, b = call(base, "GET", "/accounts/not-a-real-id")
    record("not_found", s, None)

    return steps


def main():
    args = sys.argv[1:]
    targets = dict(a.split("=", 1) for a in args) if args else DEFAULT_TARGETS

    results = {}
    for name, base in targets.items():
        try:
            results[name] = probe(base)
        except Exception as exc:  # noqa: BLE001
            print(f"FAIL: {name} ({base}) unreachable: {exc}")
            return 1

    names = list(results)
    reference = names[0]
    ok = True
    print(f"Comparing {len(names)} apps against '{reference}':\n")
    for i, (label, status, shp, values) in enumerate(results[reference]):
        row_ok = True
        for other in names[1:]:
            o_label, o_status, o_shape, o_values = results[other][i]
            if o_status != status or o_shape != shp or o_values != values:
                row_ok = False
                ok = False
                print(f"  MISMATCH at '{label}': {reference}=(status={status}, values={values}) "
                      f"vs {other}=(status={o_status}, values={o_values})")
                if o_shape != shp:
                    print(f"    shape: {reference}={shp} vs {other}={o_shape}")
        mark = "ok" if row_ok else "XX"
        print(f"  [{mark}] {label}: status={status} values={values}")

    print("\n" + ("PASS: all apps agree on the contract." if ok else "FAIL: contract divergence above."))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
