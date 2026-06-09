//! Consistent framing for one-shot, human-facing CLI commands (Spec 058).
//!
//! The frame — a header rule, a closing rule, and a footer summary line — is
//! *status* output, not data. It is always written to `stderr`, on both success and
//! failure, so the command's data on `stdout` stays clean and parseable
//! (`marreta migrate diff > out.sql` and harness greps are unaffected). Only
//! horizontal rules are used, never vertical borders, so terminal wrapping cannot
//! break the layout.
//!
//! Long-running commands (`serve`), machine output modes (`--format json`,
//! `tooling`), and the debug/meta commands (`tokenize`, `parse`, `--version`,
//! `--help`) are never framed: they simply never call `begin`.

use std::io::{IsTerminal, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub enum Outcome {
    Success,
    Failure,
}

struct FrameState {
    started: Instant,
}

/// The currently open frame, if any. A command opens it with `begin` and closes it
/// with `end`; if a hard error aborts the command first, `abort` closes it.
static FRAME: Mutex<Option<FrameState>> = Mutex::new(None);

const MIN_WIDTH: usize = 24;
const MAX_WIDTH: usize = 60;
const DEFAULT_WIDTH: usize = 50;

/// Opens a frame for a human-facing command: prints the header rule to `stderr` and
/// records the start time. Call once at the start of a framed command (and not at
/// all for machine modes, so they stay frame-free).
pub fn begin(command: &str) {
    print_header(command);
    *FRAME.lock().expect("cli_ux frame poisoned") = Some(FrameState {
        started: Instant::now(),
    });
}

/// Closes the active frame with an outcome and a one-line summary, then clears it so
/// a later `abort` does not double-print.
pub fn end(outcome: Outcome, summary: &str) {
    if let Some(state) = FRAME.lock().expect("cli_ux frame poisoned").take() {
        print_footer(outcome, summary, state.started.elapsed());
    }
}

/// Called by the error-exit helpers: if a frame is still open (a hard error aborted
/// the command before `end`), close it as a failure. A no-op when no frame is open,
/// so unframed commands and machine modes are untouched.
pub fn abort() {
    if let Some(state) = FRAME.lock().expect("cli_ux frame poisoned").take() {
        print_footer(Outcome::Failure, "failed", state.started.elapsed());
    }
}

/// Formats a duration the way the CLI reports elapsed time.
pub fn format_elapsed(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1_000 {
        format!("{}ms", millis)
    } else {
        format!("{:.2}s", duration.as_secs_f64())
    }
}

fn print_header(command: &str) {
    let label = format!("─── marreta {} ", command);
    let width = frame_width();
    let pad = width.saturating_sub(label.chars().count());
    emit(&dim(&format!("{}{}", label, "─".repeat(pad))));
}

fn print_footer(outcome: Outcome, summary: &str, elapsed: Duration) {
    emit(&dim(&"─".repeat(frame_width())));
    let glyph = match outcome {
        Outcome::Success => color(32, "✓"),
        Outcome::Failure => color(31, "✗"),
    };
    emit(&format!(
        "{} {} · {}",
        glyph,
        summary,
        format_elapsed(elapsed)
    ));
}

fn emit(line: &str) {
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "{}", line);
}

fn frame_width() -> usize {
    match std::env::var("COLUMNS")
        .ok()
        .and_then(|cols| cols.trim().parse::<usize>().ok())
    {
        Some(cols) => cols.clamp(MIN_WIDTH, MAX_WIDTH),
        None => DEFAULT_WIDTH,
    }
}

fn use_color() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::stderr().is_terminal()
}

fn dim(text: &str) -> String {
    if use_color() {
        format!("\x1b[2m{}\x1b[0m", text)
    } else {
        text.to_string()
    }
}

fn color(code: u8, text: &str) -> String {
    if use_color() {
        format!("\x1b[{}m{}\x1b[0m", code, text)
    } else {
        text.to_string()
    }
}
