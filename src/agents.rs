//! AI-agent assets (Spec 078).
//!
//! `marreta init` and `marreta agents` write a project `AGENTS.md` primer plus thin pointer
//! files for tools that load their own convention file. The primer is generated at build time
//! (`cargo run -p xtask -- gen`) and baked here version-neutral; the running runtime's version
//! is stamped in at emission, so the committed asset stays stable for the codegen git-diff gate
//! while every emitted copy carries the correct version.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::version::MARRETA_VERSION;

/// The version-neutral primer, baked from the generated asset.
const AGENTS_TEMPLATE: &str = include_str!("../docs/agents/AGENTS.md");

/// The placeholder the generated primer carries in place of a concrete version.
const VERSION_SENTINEL: &str = "vX.Y.Z";

/// The text preceding the version on the primer's title line, used to read a stamp back.
const STAMP_MARKER: &str = "Generated for Marreta v";

const COPILOT_POINTER: &str = "\
This is a Marreta Lang project. The authoritative agent guide for the language is `AGENTS.md`
at the project root. Read it before writing or editing `.marreta` files.
";

/// The primer with the running runtime's version stamped in.
pub fn primer() -> String {
    AGENTS_TEMPLATE.replace(VERSION_SENTINEL, &format!("v{MARRETA_VERSION}"))
}

/// The files emitted into a project: the canonical primer plus a thin pointer to it for
/// GitHub Copilot, which reads its own instructions file rather than `AGENTS.md`.
pub fn emitted_files() -> Vec<(&'static str, String)> {
    vec![
        ("AGENTS.md", primer()),
        (
            ".github/copilot-instructions.md",
            COPILOT_POINTER.to_string(),
        ),
    ]
}

/// Write the emitted set into `project_root`, creating parent directories. Returns written paths.
pub fn write_into(project_root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for (rel, content) in emitted_files() {
        let path = project_root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, content)?;
        written.push(path);
    }
    Ok(written)
}

/// The version stamped into a project's `AGENTS.md`, if present and recognizable.
pub fn stamped_version(agents_md: &str) -> Option<&str> {
    let start = agents_md.find(STAMP_MARKER)? + STAMP_MARKER.len();
    let rest = &agents_md[start..];
    let end = rest
        .find(|c: char| c == ')' || c.is_whitespace())
        .unwrap_or(rest.len());
    let version = rest[..end].trim();
    (!version.is_empty()).then_some(version)
}

/// Whether a project's primer is stamped for a different runtime than the one running.
pub fn is_stale(agents_md: &str) -> bool {
    stamped_version(agents_md) != Some(MARRETA_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primer_stamps_the_running_version() {
        let primer = primer();
        assert!(!primer.contains(VERSION_SENTINEL));
        assert!(primer.contains(&format!("Generated for Marreta v{MARRETA_VERSION}")));
    }

    #[test]
    fn reads_back_its_own_stamp() {
        let primer = primer();
        assert_eq!(stamped_version(&primer), Some(MARRETA_VERSION));
        assert!(!is_stale(&primer));
    }

    #[test]
    fn detects_a_stale_stamp() {
        let old = AGENTS_TEMPLATE.replace(VERSION_SENTINEL, "v0.0.1");
        assert_eq!(stamped_version(&old), Some("0.0.1"));
        assert!(is_stale(&old));
    }

    #[test]
    fn emits_canonical_primer_and_copilot_pointer() {
        let names: Vec<_> = emitted_files().into_iter().map(|(name, _)| name).collect();
        assert!(names.contains(&"AGENTS.md"));
        assert!(names.contains(&".github/copilot-instructions.md"));
        assert!(!names.contains(&".cursor/rules/marreta.mdc"));
    }
}
