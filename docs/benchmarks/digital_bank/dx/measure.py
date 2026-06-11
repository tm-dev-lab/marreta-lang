#!/usr/bin/env python3
"""Objective developer-experience measurement for the digital bank benchmark.

All four apps implement the identical contract, so DX is measured from the same artifacts the load
test uses, not from opinion. This computes, per stack:
  - SLOC (non-blank, non-comment), split into business logic vs infrastructure wiring,
  - direct project dependencies you must manage,
  - and emits the authored built-in-vs-library capability matrix.
Writes DX.md (committed). Re-run any time: it only reads the app trees.

SLOC counting is self-contained (no external tool): block comments and per-language line comments
are stripped, then non-empty lines are counted. The app sources are small and clean, and the files
are in the repo to re-verify.
"""
import json
import re
import sys
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
APPS = BENCH / "apps"

LINE_COMMENT = {".marreta": "#", ".py": "#", ".properties": "#", ".ts": "//", ".java": "//"}
HAS_BLOCK = {".ts", ".java"}

# Each app's source files (everything authored to implement the identical contract). SLOC is
# counted by sloc() below with one rule for all four apps. The earlier business-vs-wiring split was
# dropped from the study: separating the two needs per-file (and, for single-file apps like FastAPI,
# per-line) judgement that a hostile reader can re-classify into a different number, so it is not
# published as a metric. The capability matrix and the direct-dependency count carry the same point
# objectively, and the open sources are the answer to anyone who wants to draw their own split.
SOURCES = {
    "marreta": ["marreta/routes/bank.marreta", "marreta/schemas/bank.marreta", "marreta/tasks/bank.marreta",
                "marreta/app.marreta", "marreta/marreta.env"],
    "fastapi": ["fastapi/app.py"],
    "nest": ["nest/src/bank.controller.ts", "nest/src/bank.service.ts", "nest/src/dto.ts",
             "nest/src/app.module.ts", "nest/src/main.ts",
             "nest/src/schemas/account.schema.ts", "nest/src/schemas/transaction.schema.ts"],
    "spring": ["spring/src/main/java/dev/marreta/bench/BankController.java",
               "spring/src/main/java/dev/marreta/bench/Dtos.java",
               "spring/src/main/java/dev/marreta/bench/BankApplication.java",
               "spring/src/main/java/dev/marreta/bench/Account.java",
               "spring/src/main/java/dev/marreta/bench/Transaction.java",
               "spring/src/main/java/dev/marreta/bench/AccountRepository.java",
               "spring/src/main/java/dev/marreta/bench/TransactionRepository.java",
               "spring/src/main/java/dev/marreta/bench/ApiException.java",
               "spring/src/main/java/dev/marreta/bench/ApiExceptionHandler.java",
               "spring/src/main/resources/application.properties"],
}


def sloc(path: Path) -> int:
    text = path.read_text(encoding="utf-8", errors="replace")
    if path.suffix in HAS_BLOCK:
        text = re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)
    lc = LINE_COMMENT.get(path.suffix)
    count = 0
    for line in text.splitlines():
        s = line.strip()
        if not s:
            continue
        if lc and s.startswith(lc):
            continue
        count += 1
    return count


def sum_sloc(rel_paths) -> int:
    return sum(sloc(APPS / rel) for rel in rel_paths if (APPS / rel).exists())


def direct_deps(app: str) -> int:
    if app == "marreta":
        return 0
    if app == "fastapi":
        req = APPS / "fastapi/requirements.txt"
        return sum(1 for l in req.read_text().splitlines() if l.strip() and not l.strip().startswith("#"))
    if app == "nest":
        pkg = json.loads((APPS / "nest/package.json").read_text())
        return len(pkg.get("dependencies", {}))
    if app == "spring":
        pom = (APPS / "spring/pom.xml").read_text()
        return len(re.findall(r"<dependency>", pom))
    return 0


# Authored capability matrix: is each capability built into the runtime, or an added library?
CAPS = ["Relational DB", "Document DB", "Cache", "Queue / Topic", "HTTP client", "Validation", "OpenAPI", "Tests"]
MATRIX = {
    "marreta": ["built-in"] * 8,
    "fastapi": ["library", "library (motor)", "library", "library", "library (httpx)", "built-in (pydantic)", "built-in", "library (pytest)"],
    "nest": ["library", "library (mongoose)", "library", "library", "library", "library (class-validator)", "library (swagger)", "built-in (jest)"],
    "spring": ["starter", "starter (data-mongodb)", "starter", "starter", "library", "starter (validation)", "library (springdoc)", "starter (test)"],
}


def main():
    apps = ["marreta", "fastapi", "nest", "spring"]
    rows = {}
    for app in apps:
        rows[app] = {"total": sum_sloc(SOURCES[app]), "deps": direct_deps(app)}

    out = ["---",
           'title: "Digital Bank Benchmark - Developer Experience"',
           'summary: "Objective DX metrics computed from the four identical apps: total source size, direct dependencies, and a built-in-vs-library capability matrix. Regenerate with dx/measure.py."',
           "---", "",
           "# Digital Bank Benchmark - Developer Experience", "",
           "Computed by `dx/measure.py` from the four apps that implement the identical contract.",
           "SLOC is non-blank, non-comment lines, counted by one rule for all four apps (block comments",
           "stripped for .ts/.java). A business-vs-wiring breakdown is not published as a number: it",
           "needs subjective per-line classification on single-file apps; anyone who wants it can derive",
           "it from the open sources (see METHODOLOGY).",
           "", "## Code size and dependencies", "",
           "| Stack | Total SLOC | Direct deps |",
           "|---|--:|--:|"]
    for app in apps:
        r = rows[app]
        out.append(f"| {app} | {r['total']} | {r['deps']} |")
    out += ["", "## Built-in vs library capability matrix", "",
            "| Capability | " + " | ".join(apps) + " |",
            "|---|" + "|".join(["---"] * len(apps)) + "|"]
    for i, cap in enumerate(CAPS):
        out.append(f"| {cap} | " + " | ".join(MATRIX[app][i] for app in apps) + " |")
    out += ["", "## Test feedback loop", "",
            "The same provider-free, route-level suite per stack (parity of strategy: each hits the",
            "API in-process, exercising real validation and business logic, with only the data",
            "provider mocked). The time is each framework's own reported test time (marreta, pytest,",
            "jest, surefire), which excludes container start and base-image differences; wall time is",
            "kept alongside in `feedback.json`.", ""]
    fb_path = Path(__file__).resolve().parent / "feedback.json"
    if fb_path.exists():
        fb = json.loads(fb_path.read_text())
        out += ["| Stack | Test run (s) | Isolation |", "|---|--:|---|"]
        for app in apps:
            d = fb.get(app, {})
            sec = d.get("seconds")
            out.append(f"| {app} | {'n/a' if sec is None else sec} | {d.get('note', '')} |")
        out += [""]
    else:
        out += ["_(Pending: run `dx/test_feedback.sh`.)_", ""]
    (BENCH / "DX.md").write_text("\n".join(out))
    print("Wrote", BENCH / "DX.md", "\n")
    print("Stack    total  deps")
    for app in apps:
        r = rows[app]
        print(f"  {app:<8} {r['total']:>5} {r['deps']:>5}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
