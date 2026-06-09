# 058 - Consistent CLI Framing for One-Shot Commands

> Status: Delivered
> Type: CLI surface / developer experience
> Scope: Give the one-shot, human-facing commands a consistent frame — a header
> rule, the command's existing output, and a footer with a one-line status summary
> and elapsed time — using horizontal rules only (no vertical borders). Long-running
> and machine-output commands are untouched, and the frame is additive so it never
> breaks output that scripts or test harnesses parse.

---

## 1. Purpose

Only `marreta test` reports timing today (`Finished in 240ms`). The other one-shot
commands (`fmt`, `lint`, `doctor`, `init`, `migrate`) print raw output with no
consistent shape, no completion signal, and no timing. The result feels unfinished
and inconsistent across the CLI.

This spec standardizes a lightweight frame for the one-shot, human-facing commands:
a header rule naming the command, the command's existing body output unchanged, and
a footer with a status glyph, a one-line summary, and elapsed time. It uses
**horizontal rules only**, never vertical borders, so terminal wrapping cannot break
the layout.

## 2. Scope

**Framed** (one-shot, human-facing): `init`, `fmt`, `lint`, `doctor`, `test` (align
its existing footer to the shared shape), and the `migrate` subcommands.

**Not framed:**

- `serve` — long-running, it has no completion moment.
- `tooling` and any **machine output mode** (notably `lint --format json`) — a frame
  would corrupt machine-readable output.
- `--version` and `--help` — trivial meta output.
- `tokenize` and `parse` — unadvertised debug dumps, kept raw.

Rule of thumb: only human-facing one-shot output is framed; machine or JSON output
stays clean.

## 3. Visual design

Horizontal rules only. No vertical borders, no box corners.

```
─── marreta fmt ──────────────────
  FORMAT routes/users.marreta
  All files formatted.
──────────────────────────────────
✓ 3 files formatted · 12ms
```

On failure:

```
─── marreta lint ─────────────────
  routes/x.marreta:3:1: unexpected token
──────────────────────────────────
✗ 2 diagnostics · 9ms
```

- **Header**: a single rule line beginning with the command name, for example
  `─── marreta lint ───…`, padded with `─` to the frame width.
- **Body**: the command's existing output, unchanged (see 4.3).
- **Footer**: a closing rule line, then one line of `<glyph> <summary> · <elapsed>`,
  where the glyph is `✓` on success and `✗` on failure, `<summary>` is a one-line,
  per-command status string, and `<elapsed>` reuses the existing `format_elapsed`.
- **Width**: the terminal width when available, clamped to a sane range (for example
  24 to 60 columns); a fixed default when the width is unknown.
- **Color**: ANSI styling (dim rules, green `✓`, red `✗`) only when the frame's
  stream (stderr, see 3.1) is a TTY and `NO_COLOR` is unset; plain text otherwise.

### 3.1 Output streams (frozen)

The frame follows the cargo model: it is **status output, not data**.

- The **frame — header rule, closing rule, and footer summary line — always goes to
  `stderr`**, on both success and failure.
- The command's **body/data output stays on `stdout`**, byte-identical to today
  (the one exception is `test`, see 4.3).
- **Errors stay on `stderr`**, as today.

Because the entire frame lives on one stream (`stderr`) regardless of outcome,
there is no split-frame or ordering confusion: a consumer reading `stderr` sees the
full frame plus any error in order, while a consumer reading `stdout` gets clean,
frame-free data (so `marreta migrate diff > out.sql` and harness greps on `stdout`
are unaffected). Stream routing is asserted by explicit tests.

## 4. Mechanism

### 4.1 A small `cli_ux` module

A focused helper module exposing `print_header(command)` and
`print_footer(outcome, summary, elapsed)`, plus the width/color helpers. It reuses
`format_elapsed`. Nothing in the runtime depends on it; it is CLI presentation only.

### 4.2 Timing and the failure path

- Capture a process-start `Instant` in `main()`.
- On success, the command prints its footer with the elapsed time and its summary.
- On failure, the frame must still close with elapsed time. Today the `exit_with_*`
  helpers call `process::exit` and would skip the footer. They are extended to emit
  the framed failure footer (`✗ … · <elapsed>`) to `stderr` (per 3.1) before
  exiting, but only when the current command is a framed one. So both success and
  failure show consistent framing and timing on the same stream.

### 4.3 Additive only, with one documented exception

Because the frame goes to `stderr` (3.1), `stdout` is untouched: every command's
`stdout` body stays byte-identical to today, so shell harnesses and scripts that
parse `stdout` are unaffected:

- `e2e/run.sh` runs `marreta lint` and `marreta test` and greps their output.
- `functional_tests` and `migrations_functional` drive `marreta` and assert on
  output and exit codes.

**The one exception is `test`.** Today `test` prints `Finished in <elapsed>` to
`stdout` as part of its body. That line is **removed from the body and superseded by
the unified footer** (on `stderr`). This is the only body change in this spec, and
it is intentional: timing belongs to the footer now. `test`'s other body lines
(including the `N passed, M failed` summary on `stdout`) are unchanged, so harness
greps on `passed,` still match. No harness in the repository greps `Finished in`;
any consumer that did would be updated, never by weakening an assertion.

Any unintended change to the `stdout` body of `doctor`, `migrate`, `fmt`, `lint`,
or `init` is caught by the shell suites and is out of contract.

## 5. Out of scope

- `serve` and any long-running command.
- Machine or JSON output modes.
- Color theming beyond the status glyph and dim rules; progress bars; spinners.
- Changing any command's body output or exit codes.

## 6. Acceptance criteria

1. `init`, `fmt`, `lint`, `doctor`, `test`, and `migrate` subcommands print a header
   rule, their unchanged body, a closing rule, and a footer line of
   `<glyph> <summary> · <elapsed>`.
2. `serve`, `tooling`, `--version`, `--help`, `tokenize`, and `parse` are unchanged.
3. Machine modes (notably `lint --format json`) emit clean output with no frame.
4. The frame uses horizontal rules only — no vertical borders.
5. The whole frame (header, closing rule, footer) is written to `stderr` on both
   success and failure; the command's data stays on `stdout`. Color is applied only
   when `stderr` is a TTY and `NO_COLOR` is unset; plain otherwise.
6. The failure path prints a framed `✗ … · <elapsed>` footer to `stderr` before
   exiting, for framed commands.
7. Every command's `stdout` body is byte-identical to today, **except `test`**,
   whose `Finished in …` line is removed and superseded by the unified footer.
   `e2e/run.sh`, `functional_tests`, and `migrations_functional` still pass
   (harnesses updated only where they keyed on a superseded line, never by weakening
   an assertion).
8. Tests cover: header and footer present on `stderr` for a couple of framed
   commands, the data still on `stdout`, JSON mode unaffected and frame-free, and
   `NO_COLOR` produces plain output.
9. Standard gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
   the full test suite, `functional_tests`, and `migrations_functional` all green.

## 10. Delivery notes

- `src/cli_ux.rs` (new, bin-only module): `begin(command)` prints the header rule
  and records the start time; `end(outcome, summary)` prints the closing rule and
  the `<glyph> <summary> · <elapsed>` footer; `abort()` closes an open frame as a
  failure. All frame output goes to `stderr`. An `Outcome` enum, `format_elapsed`
  (moved here from `main.rs`), terminal-width (via `COLUMNS`, clamped 24–60, default
  50), and color (ANSI only when `stderr` is a TTY and `NO_COLOR` is unset) helpers
  live here.
- `src/main.rs`: the frame is opened in the `main` dispatch arm **before argument
  parsing**, so a parse failure (for example `marreta fmt --unknown`,
  `marreta lint --format xml`) is also framed. `doctor`, `init`, `test`, and
  `migrate` always open the frame; `fmt` and `lint` open it only in human mode,
  gated by `fmt_is_machine_mode` / `lint_is_machine_mode` (a light pre-scan for
  `--stdin` and `--format json`) so machine modes stay frame-free. Each command
  calls `end` with a per-command summary; the five `exit_with_*` helpers call
  `cli_ux::abort()` before exiting so hard errors and parse failures also close the
  frame. `test`'s `Finished in …` lines were removed and superseded by the unified
  footer; `format_elapsed` was removed from `main.rs`.
- `tests/integration_tests.rs`: added framing tests (frame on `stderr`, data on
  `stdout`, `lint --format json` frame-free, `NO_COLOR` plain, a framed argument
  parse failure, and a machine-mode failure staying frame-free); the `--coverage`
  golden test now reads the block to the end of `stdout` since the timing footer
  moved to `stderr`.
- Gates: `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` clean;
  suite 1462 lib + 3 bin + 35 HTTP + 37 integration; `e2e` 59 + 17 smoke;
  `functional_tests` 548/548; `migrations_functional` PASS.

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md
section 1.3.
