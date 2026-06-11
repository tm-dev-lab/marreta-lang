# 066 - Launch Benchmark Study

> Status: Delivered
> Type: Benchmark and published study (docs/benchmarks + methodology, no runtime change)
> Scope: Turn `docs/benchmarks/digital_bank` into a rigorous, reproducible experiment that
> validates the three about-page hypotheses with data. Add a fourth feature-identical contender
> in Java Spring Boot, upgrade the load-test harness for statistical rigor (JIT warmup plus a
> long steady-state window, three repetitions with variance reporting, interleaving, multiple
> load levels plus a saturation run, expanded resource metrics), measure developer experience
> objectively from the same four apps, and produce the skeleton of a study to publish on TM Dev
> Lab and later link from the site about page. No change to the language runtime.

---

## 1. Purpose

The about page states three hypotheses and says they are "validated by experiments". Today
`digital_bank` exists and compares Marreta, FastAPI (`motor`), and NestJS (`mongoose`) on a
MongoDB-backed digital bank, but it is a smoke benchmark, not a publishable experiment: a single
run, a 60s window, no JIT warmup, three stacks, and no statistical treatment. This spec turns it
into the launch study that backs the three hypotheses with numbers, adds a JVM contender (the
most-requested "serious backend" comparison), and yields a reproducible artifact plus a written
study. The study is published on TM Dev Lab and the about page links to it.

## 2. The hypotheses and how each is measured

The three hypotheses are taken verbatim from the about page. Each maps to concrete, pre-declared
metrics:

- **H1 - Low, predictable resource usage.** Peak and sustained CPU, peak and steady-state RSS
  memory under load, idle footprint, and startup time. "Predictable" is
  shown by low variance (a low coefficient of variation) across repetitions and load levels.
- **H2 - Strong performance despite high abstraction.** Achieved throughput and latency
  (p50/p90/p95/p99) at several fixed arrival rates, plus maximum sustainable throughput from a
  saturation run, and error rate. The honest claim is "competitive with hand-written stacks", not
  "fastest", read together with H1 (competitive performance at a fraction of the resources).
- **H3 - Good developer experience.** Measured objectively from the four identical apps (section
  5), not by opinion.

## 3. The benchmark surface (extend digital_bank)

Keep the existing domain and contract (accounts, balance, deposit, withdraw, transfer,
transaction history, health; money as integer minor units) and the existing fairness setup (each
app container capped at 1 CPU / 1 GB, k6 in its own container, one runtime exercised at a time).

MongoDB is shared and identical for every stack (same image, version, config, and seeded data),
and runs with **headroom (uncapped) in every run** (fixed-load and saturation). It is the shared
dependency, not the subject of the experiment, so it must not be the bottleneck. Capping MongoDB to
1 CPU was tried; the validity guard caught it saturating a single core even at the lightest level
(200 req/s, MongoDB CPU peak 100%, the apps idle waiting on it), which would have turned the study
into a measurement of the database. Only the **apps** are capped (1 CPU / 1 GB). MongoDB CPU and
memory are monitored every run; if it ever saturates at a level, that level is reported as
combined-system, not app/runtime overhead. The saturation run likewise relies on the app container
being the limiting resource, confirmed by the same guard.

- **Add a fourth app, `apps/spring`** (Java, Spring Boot, `spring-data-mongodb`, Bean Validation),
  feature-identical to the other three and built by Docker Compose like FastAPI and NestJS.
- **Feature parity is enforced**, not assumed: a single contract check runs the same request set
  against each app and asserts identical status codes and response shapes, so no app is a strawman
  or accidentally cheats (for example skipping validation or the funds check).
- Competitor apps stay **idiomatic**: reasonable, conventional implementations a competent
  developer would write, ideally aligned with each framework's official guidance, not
  hyper-optimized and not crippled.

**Why these contenders.** Marreta is an interpreted language, so the comparison set is other
high-level, managed-runtime backends in the same productivity niche, not systems languages like
Go or Rust whose audience and trade-offs differ. FastAPI (Python) is interpreted; NestJS runs
JavaScript on V8 (JIT over a managed runtime); Spring Boot runs on the JVM. The JVM is not purely
interpreted (it JITs to native), and that is deliberate: it is the heavyweight,
performance-respected option of the group and the toughest bar, so staying competitive against it
is the meaningful result (and the reason the protocol has a JIT warmup and a long window). The
position the study tests is interpreted-language developer experience delivered on a native
runtime (Marreta's runtime is Rust), measured against the managed and interpreted backends teams
actually reach for.

**Versions.** Each contender runs the latest stable release of both its language runtime and its
framework, pinned and recorded: the current stable Python with the latest stable FastAPI, the
current stable Node.js with the latest stable NestJS, and **Java 25** with the latest stable
Spring Boot. Marreta runs **v0.2.0**, the public launch release of the language (the
`marreta-lang:dev` image built from that release, digest recorded). "Latest stable" is the
selection rule, but the **exact resolved versions and container image digests** (Java, Spring
Boot, Python, FastAPI, Node, NestJS, MongoDB, k6, and the `marreta-lang:dev` image) are **pinned
and recorded in `METHODOLOGY.md` before any measurement run**, so the comparison is against
current, fully-identified stacks and is reproducible.

## 4. Rigorous, pre-registered protocol

The protocol and the metric list are fixed **before** any measurement run, recorded in the bench
README, so results cannot be cherry-picked after the fact.

- **JIT warmup then a long window.** Each measured run is a warmup phase (load applied, results
  discarded, long enough for the JVM to reach steady JIT state) followed by a **steady-state
  measurement window of about five minutes**. Windows must be well over one minute precisely
  because Spring Boot needs JIT warmup, and a short window would measure a half-compiled JVM and
  misrepresent every stack.
- **Three repetitions** per stack per load level. Report the **median** and the **coefficient of
  variation** for every metric. A **consistency gate**: if CV exceeds a declared threshold for a
  metric, that cell is flagged and the triplet is re-run, so an incoherent run never reaches the
  study silently.
- **Interleaved order.** Repetitions are interleaved across stacks (not all of stack A, then all
  of B) to avoid thermal, caching, or ordering bias favoring whoever ran first or last.
- **Load levels plus saturation.** Several fixed arrival rates (for example a light, a moderate,
  and a heavy rate) to compare latency under identical load, plus a separate **saturation run**
  to find each stack's maximum sustainable throughput within the 1 CPU / 1 GB app cap. For the
  saturation run MongoDB is given headroom so the app container is the limiting resource, not the
  database (the frozen rule in section 3).
- **Cold start measured separately.** Time from container start to first successful request is its
  own metric (it favors H1 and matters for serverless or scale-from-zero), kept out of the
  steady-state numbers.
- **Environment fixed and recorded.** Host hardware, OS, Docker, MongoDB version, each app's
  language and framework versions, the container limits, the dataset, and the exact image digests
  under test are captured into the results so a run is fully described and reproducible.
- **Metrics collected:** throughput (achieved req/s), latency avg/p50/p90/p95/p99, error rate,
  average and peak CPU, average and peak RSS memory, idle footprint, and startup /
  time-to-first-request.

## 5. Developer experience, measured objectively

DX is measured from the **same four identical apps** that run the load test, so the evidence is
reproducible and derived from one artifact set, not a survey. The objective core:

- **Code size (SLOC).** Non-comment, non-blank source lines to implement the identical contract
  per stack, via a standard tool (`tokei` or `cloc`). The headline number.
- **Dependencies and footprint.** Direct and transitive dependency counts and the installed size
  (`node_modules`, the Python environment, the resolved `.m2` jars) per stack. This is the literal
  evidence for "zero project dependencies".
- **Built-in vs library capability matrix.** A table of which capabilities are built into the
  runtime versus require an added library, across the four stacks: relational DB, document DB,
  cache, queue and topic, HTTP client, request validation, OpenAPI, and tests.
- **Total SLOC** with one counting rule for all four apps (non-blank, non-comment). A
  business-logic-vs-wiring split is deliberately not published as a number: separating the two needs
  subjective per-line classification on single-file apps, which a hostile reader re-classifies into
  a different result. The capability matrix and the dependency count carry the "without redoing the
  same infrastructure boilerplate" evidence objectively, and the open sources answer anyone who
  wants to draw their own split.
- **Test feedback loop.** Wall-clock time to run an **equivalent, provider-free** test suite per
  stack: the same handful of behaviors (a created account, a rejected over-balance withdrawal, a
  transfer), with all external calls **stubbed or mocked** so no suite touches a real MongoDB.
  Marreta scenario tests run in memory with `given` stubs; the others use their idiomatic runner
  (pytest, jest, JUnit) with the framework's mocking. To keep the number honest rather than a
  comparison of test styles, the report **records per stack whether the suite boots a server or
  process and whether it touches any real provider** (a stack that cannot run isolated, stubbed
  tests cheaply is itself a DX data point). If a stack genuinely cannot avoid a process or
  provider, the metric is split into isolated/scenario feedback and live integration feedback so
  like is compared with like.

A `dx/` script computes these from the app trees and emits the tables, so the numbers regenerate
on demand and are checked into the results alongside the load-test data.

## 6. Credibility and honesty

A benchmark published by the language's own authors is only worth anything if it is trustworthy:

- **Full reproducibility.** All apps, the harness, and the exact protocol live in the bench
  directory, with a size-bounded result-artifact policy (section 8): the committed results are the
  small, final artifacts, and anyone can re-run the harness to regenerate the bulky raw data.
- **Idiomatic competitors**, as in section 3, with the implementations open to inspection and
  review.
- **Honest caveats stated in the study**: it is one workload, the competitors are idiomatic but
  not hyper-tuned, the hardware is a single described machine, and the results generalize only so
  far.
- **Publish the result even if a hypothesis is only partially supported.** The protocol is fixed
  in advance; the study reports what the data shows, including where Marreta is merely competitive
  rather than ahead.

## 7. The published study (skeleton)

The deliverable includes a drafted study outline (kept with the benchmark) ready to publish on TM
Dev Lab: abstract, the three hypotheses, the rationale for the contender set (why managed and
interpreted backends, and why the JVM is included as the toughest bar), methodology and
environment, the workload and contract, the protocol (warmup, window, repetitions, statistics),
results per hypothesis with tables and charts and variance, the DX measurement, threats to
validity and caveats, and reproducibility instructions. Publishing the article on TM Dev Lab and adding the "Read the study" link on the
site about page are follow-ups that happen once the data is in (see Out of scope).

The repository keeps a versioned `RESULTS.md` inside the benchmark directory as the **data of
record** (same frontmatter header style as the guide docs, all final numbers). The external
article is the narrative written from that file, so the published study and the repo never drift.

## 8. Implementation outline

This touches `docs/benchmarks/digital_bank` and adds no runtime source:

- `apps/spring/` - the Spring Boot app, Dockerfile, and Compose wiring.
- The contract-parity check that runs the same requests against all four apps.
- Harness scripts upgraded for warmup, the long window, three interleaved repetitions, the load
  levels and the saturation run, metric capture (app CPU/memory/startup/image plus MongoDB
  CPU/memory as the validity guard), and the statistics (median, CV, consistency gate).
- An equivalent minimal test suite per app (the same asserted behaviors) so the test
  feedback-loop time is comparable across stacks.
- `dx/` - the developer-experience measurement script and its output tables (SLOC, dependencies
  and footprint, capability matrix, infrastructure-boilerplate breakdown, test feedback-loop time).
- `results/` with a **size-bounded artifact policy** (the repo just dropped a large legacy
  benchmark, so this stays disciplined): **committed** are `RESULTS.md`, `METHODOLOGY.md`, the
  per-run **config snapshots**, and **compact summary** CSV/JSON (the final aggregated numbers).
  **Bulky** per-run logs and full raw k6 dumps are **gitignored** (a `results/.gitignore`) and kept
  locally or attached externally.
- README and a `METHODOLOGY.md` recording the pre-registered protocol and environment.
- A versioned **results report** committed in the benchmark directory (`RESULTS.md`), using the
  same frontmatter header style as the guide docs (title, summary, and a version or date), holding
  all the final data: the per-hypothesis tables, the per-stack metrics with median and CV across
  the load levels and the saturation run, and the DX numbers. This is the in-repo record of
  record; the external TM Dev Lab article (section 7) is written from it.
- The study draft / outline for the external article (per section 7).

## 9. Out of scope

- Any change to the language runtime, CLI, or `src/`. This is benchmark and study work only.
- GraalVM native-image Spring Boot, additional databases, and additional stacks. Recorded as
  possible future variants, not part of the launch study.
- Actually publishing the article on TM Dev Lab and adding the about-page link. The study is
  drafted here and published once the run produces final data; the about link follows the
  publication.

## 10. Acceptance criteria

1. A fourth app, `apps/spring` (Spring Boot, `spring-data-mongodb`, validation), exists and the
   contract-parity check passes identically against all four apps.
2. The harness runs a JIT warmup followed by an about-five-minute steady-state window, three
   interleaved repetitions per stack per load level, multiple fixed arrival rates plus a
   saturation run, and persists the per-run artifacts. MongoDB runs with headroom (uncapped) in
   every run, with its CPU and memory monitored as the validity guard; only the apps are capped at
   1 CPU / 1 GB. A level (or saturation point) where MongoDB itself saturates is reported as
   combined-system, not app overhead.
3. Every metric in section 4 is captured, and the summary reports median plus coefficient of
   variation with the consistency gate applied.
4. The DX core (total SLOC under one stated rule, dependencies and footprint, capability matrix,
   and the test feedback-loop time) is computed by a script from the four apps and
   emitted as tables. The feedback-loop suites are equivalent and provider-free (stubbed, no real
   MongoDB), and the report records per stack whether each boots a server/process or touches a
   provider.
5. A `METHODOLOGY.md` records the pre-registered protocol and environment, with the exact resolved
   versions and image digests pinned before the run; a versioned `RESULTS.md` (guide-style
   frontmatter header) inside the benchmark directory holds all final data as the in-repo record
   of record; the whole `results/` tree is git-ignored and regenerable, with `RESULTS.md`,
   `METHODOLOGY.md`, and `DX.md` as the committed data of record; and a study draft covers all
   of section 7 including caveats and reproducibility instructions.
6. The results explicitly map data to H1, H2, and H3, stated honestly (including where Marreta is
   only competitive).
7. Standard gates pass. No `src/` change, so the runtime tiers are unaffected; `cargo fmt --check`
   and `cargo clippy --all-targets -- -D warnings` stay green.

---

## Delivery notes

Delivered as the **post-067 re-run** of the study, on a dedicated Azure VM (`Standard_F8s_v2`), with
the protocol consolidated to its final form. What landed in `docs/benchmarks/digital_bank`:

- The four feature-identical apps (Marreta, FastAPI, NestJS, Spring Boot), the rigorous harness
  (120s warmup + 300s window, three interleaved repetitions, median + CV with a consistency gate
  run over the full grid, a 250-step saturation ladder with the same granularity for all), and the
  objective DX measurement.
- `METHODOLOGY.md` (pre-registered protocol, contender selection criterion with survey citations,
  the neutral no-manual-optimization statement, the re-run policy, the at/above-ceiling tail-variance
  rule, the generator-ceiling note, 20ms TTFR), `RESULTS.md` (the data of record), and `DX.md`.

Decisions made during the review rounds, reflected here:

- The business-vs-wiring SLOC split was **dropped** (it needs subjective per-line classification on
  single-file apps); total SLOC under one stated rule plus the capability matrix carry the point.
- The whole `results/` tree is git-ignored and regenerable; the committed docs are the record.
- The zero-throughput above 1250 req/s was identified as the load generator's VU-pool ceiling, not
  an application collapse; Marreta's sustainable ceiling is 1250 (CPU-bound) and higher rates are
  not measured under protocol. No causal claims are made about any contender.

The three hypotheses read as supported with the honest caveats kept (Spring edges Marreta on p90 at
500 req/s; the two at-ceiling cells are marked, not hidden). `cargo fmt` and `clippy` stay green (no
`src/` change). Closing review approved.

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md section 1.3.
