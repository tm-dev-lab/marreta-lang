//! Anti-drift gate for the Spec 078 AI-agent assets.
//!
//! These checks cover what the CI codegen-freshness step (`xtask gen` + `git diff --exit-code`)
//! cannot see on its own: a new language surface or a new guide page that was committed without
//! being documented or summarized still regenerates to an identical (stale) artifact, so only a
//! catalog-driven assertion catches it. The freshness step and the generator's hard error on an
//! unknown region cover snippet provenance and exactness; here we assert the template's region
//! references resolve to real markers, and that the committed primer is fully substituted.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use marreta::tooling::catalog::{CatalogKind, catalog};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Whole-word containment, so a namespace like `db` is not matched inside `double`.
fn contains_word(haystack: &str, word: &str) -> bool {
    let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric() && c != '_');
    haystack.match_indices(word).any(|(i, _)| {
        boundary(haystack[..i].chars().next_back())
            && boundary(haystack[i + word.len()..].chars().next())
    })
}

fn collect_marreta(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_marreta(&path, out);
        } else if path.extension().is_some_and(|e| e == "marreta") {
            out.push(path);
        }
    }
}

/// Every `# region: NAME` marker name defined across the tested example corpus.
fn defined_region_names() -> BTreeSet<String> {
    let root = repo_root();
    let mut files = Vec::new();
    collect_marreta(&root.join("docs/examples"), &mut files);
    collect_marreta(&root.join("e2e"), &mut files);
    let mut names = BTreeSet::new();
    for file in files {
        for line in read(&file).lines() {
            if let Some(name) = line.trim().strip_prefix("# region:") {
                names.insert(name.trim().to_string());
            }
        }
    }
    names
}

// The language surface the generator must keep documented: catalog ⊆ llms-full.txt. This is the
// whole anti-drift promise (AC4), so it must cover the operations too, not only namespaces and
// keywords: operations are the surface that grows fastest. Namespaces/keywords match on their
// name; functions/methods match on the bare operation name (the part after the dot, as the docs
// write it in a namespace page), which the catalog already derives via `completion_label`.
#[test]
fn catalog_surface_is_covered_by_llms_full() {
    let full = read(&repo_root().join("docs/agents/llms-full.txt"));
    for entry in catalog() {
        let label = entry.completion_label();
        let needle: &str = match entry.kind {
            CatalogKind::Namespace | CatalogKind::Keyword => entry.name,
            CatalogKind::Function | CatalogKind::Method => &label,
        };
        assert!(
            contains_word(&full, needle),
            "docs/agents/llms-full.txt is missing the {} '{}' (looked for '{}'). Document it under \
             docs/guide and run `cargo run -p xtask -- gen`.",
            entry.kind.as_str(),
            entry.name,
            needle
        );
    }
}

// Every guide page is reachable from SUMMARY (no orphan) and carries a non-empty `summary:`,
// so llms.txt is complete and no link degrades to an empty description.
#[test]
fn every_guide_page_is_in_summary_and_summarized() {
    let guide = repo_root().join("docs/guide");
    let summary = read(&guide.join("SUMMARY.md"));

    let mut pages = Vec::new();
    collect_md(&guide, &mut pages);
    for page in pages {
        let rel = page
            .strip_prefix(&guide)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel == "SUMMARY.md" {
            continue;
        }
        assert!(
            summary.contains(&format!("({rel})")),
            "docs/guide/{rel} is not linked from SUMMARY.md (orphan page)"
        );
        let text = read(&page);
        let has_summary = frontmatter_summary(&text).is_some_and(|s| !s.is_empty());
        assert!(
            has_summary,
            "docs/guide/{rel} has a missing or empty `summary:` frontmatter (llms.txt needs it)"
        );
    }
}

// The primer's snippet provenance: every region the template references exists as a marker in a
// tested example, and the committed primer has no unsubstituted placeholder left.
#[test]
fn agents_template_regions_resolve_and_primer_is_substituted() {
    let template = read(&repo_root().join("xtask/templates/AGENTS.md.tmpl"));
    let defined = defined_region_names();
    for line in template.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed
            .strip_prefix("{{region:")
            .and_then(|r| r.strip_suffix("}}"))
        {
            assert!(
                defined.contains(name),
                "AGENTS.md template references region '{name}' with no `# region: {name}` marker \
                 in any tested example under docs/examples or e2e"
            );
        }
    }

    let primer = read(&repo_root().join("docs/agents/AGENTS.md"));
    assert!(
        !primer.contains("{{region:"),
        "docs/agents/AGENTS.md has an unsubstituted region placeholder; run `cargo run -p xtask -- gen`"
    );
}

fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

fn frontmatter_summary(text: &str) -> Option<String> {
    let rest = text.strip_prefix("---\n")?;
    let (frontmatter, _) = rest.split_once("\n---\n")?;
    for line in frontmatter.lines() {
        if let Some(value) = line.strip_prefix("summary:") {
            return Some(value.trim().trim_matches('"').trim().to_string());
        }
    }
    None
}
