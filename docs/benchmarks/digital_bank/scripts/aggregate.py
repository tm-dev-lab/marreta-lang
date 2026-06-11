#!/usr/bin/env python3
"""Aggregate the per-run summaries of a study into median + coefficient-of-variation tables.

Walks <run_root>/rate_<level>/rep_<n>/<target>/summary.json, groups by (level, target), and for
every metric reports the median and the CV across repetitions. Applies the consistency gate: if a
headline metric's CV exceeds the threshold, it is flagged (the triplet should be re-run). Writes
compact summary.json and summary.csv next to the runs (the committed artifacts).
"""
import csv
import json
import statistics
import sys
from pathlib import Path

# Flat metric paths pulled from each run summary.
METRICS = [
    "throughput_rps", "error_rate",
    "latency_ms.avg", "latency_ms.p50", "latency_ms.p90", "latency_ms.p95", "latency_ms.p99",
    "cpu_percent.avg", "cpu_percent.peak",
    "memory_mib.avg", "memory_mib.peak", "memory_mib.idle",
    "startup_ms",
]
HEADLINE = {"throughput_rps", "latency_ms.p95", "memory_mib.peak"}
CV_THRESHOLD = 10.0  # percent


def dig(obj, path):
    for part in path.split("."):
        obj = obj.get(part, {}) if isinstance(obj, dict) else None
    return obj if isinstance(obj, (int, float)) else None


def cv(values):
    vals = [v for v in values if isinstance(v, (int, float))]
    if len(vals) < 2:
        return 0.0
    mean = statistics.mean(vals)
    if mean == 0:
        return 0.0
    return round(statistics.stdev(vals) / mean * 100, 1)


def main():
    if len(sys.argv) != 2:
        print("usage: aggregate.py <run_root>", file=sys.stderr)
        return 2
    root = Path(sys.argv[1])
    runs = {}  # (level, target) -> list[summary dict]
    for summ in root.glob("rate_*/rep_*/*/summary.json"):
        data = json.loads(summ.read_text())
        level = summ.parent.parent.parent.name.removeprefix("rate_")
        runs.setdefault((level, data["target"]), []).append(data)

    if not runs:
        print(f"no run summaries found under {root}", file=sys.stderr)
        return 1

    aggregated = {}
    flagged = []
    rows = []
    for (level, target), summaries in sorted(runs.items(), key=lambda k: (int(k[0][0]), k[0][1])):
        cell = {"reps": len(summaries), "metrics": {}}
        for metric in METRICS:
            series = [dig(s, metric) for s in summaries]
            present = [v for v in series if v is not None]
            if not present:
                continue
            med = round(statistics.median(present), 3)
            variation = cv(series)
            cell["metrics"][metric] = {"median": med, "cv": variation}
            rows.append([level, target, metric, med, variation])
            if metric in HEADLINE and variation > CV_THRESHOLD:
                flagged.append((level, target, metric, variation))
        aggregated[f"rate_{level}/{target}"] = cell

    (root / "summary.json").write_text(json.dumps(aggregated, indent=2))
    with (root / "summary.csv").open("w", newline="") as fh:
        w = csv.writer(fh)
        w.writerow(["level", "target", "metric", "median", "cv_percent"])
        w.writerows(rows)

    print(f"Aggregated {len(runs)} cells -> {root}/summary.json and summary.csv\n")
    for (level, target), summaries in sorted(runs.items(), key=lambda k: (int(k[0][0]), k[0][1])):
        m = aggregated[f"rate_{level}/{target}"]["metrics"]

        def v(metric):
            d = m.get(metric)
            return f"{d['median']}±{d['cv']}%" if d else "-"

        print(f"  rate={level:>5} {target:<8} thr={v('throughput_rps')}  err={v('error_rate')}")
        print(f"      lat ms  avg/p50/p90/p95/p99 = {v('latency_ms.avg')} / {v('latency_ms.p50')} / "
              f"{v('latency_ms.p90')} / {v('latency_ms.p95')} / {v('latency_ms.p99')}")
        print(f"      cpu% avg/peak = {v('cpu_percent.avg')} / {v('cpu_percent.peak')}   "
              f"mem MiB avg/peak/idle = {v('memory_mib.avg')} / {v('memory_mib.peak')} / "
              f"{v('memory_mib.idle')}   startup ms = {v('startup_ms')}")

    if flagged:
        print("\nConsistency gate: FLAGGED (CV over "
              f"{CV_THRESHOLD}% on a headline metric, re-run the triplet):")
        for level, target, metric, variation in flagged:
            print(f"  rate={level} {target} {metric}: CV {variation}%")
        return 1
    print(f"\nConsistency gate: OK (all headline metrics within {CV_THRESHOLD}% CV).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
