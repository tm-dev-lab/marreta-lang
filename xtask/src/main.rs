//! Build-time generator for the Spec 078 AI-agent assets.
//!
//! Single source of truth: the authored guide under `docs/guide` (its `SUMMARY.md` order and
//! per-page `summary:` frontmatter). From it this emits, into `docs/agents/`:
//!   - `llms-full.txt`  the full reference, every guide page concatenated in `SUMMARY` order.
//!   - `llms.txt`       a curated index, one line per page from its `summary:` frontmatter.
//!
//! The `AGENTS.md` primer and the catalog-vs-full gate are layered on in later steps. Run with
//! `cargo run -p xtask -- gen`; the build's anti-drift gate runs this then `git diff --exit-code`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let cmd = std::env::args().nth(1).unwrap_or_default();
    let result = match cmd.as_str() {
        "gen" => generate(),
        "" => Err("usage: xtask <gen>".to_string()),
        other => Err(format!("unknown command '{other}' (expected: gen)")),
    };
    match result {
        Ok(written) => {
            for path in written {
                println!("wrote {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("xtask: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Repository root, derived from this crate's manifest directory (`<root>/xtask`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate has a parent directory")
        .to_path_buf()
}

fn generate() -> Result<Vec<PathBuf>, String> {
    let root = repo_root();
    let guide = root.join("docs/guide");
    let out_dir = root.join("docs/agents");
    fs::create_dir_all(&out_dir).map_err(|e| format!("create {}: {e}", out_dir.display()))?;

    let summary_path = guide.join("SUMMARY.md");
    let summary = read(&summary_path)?;
    let entries = parse_summary(&summary);
    if entries.is_empty() {
        return Err(format!("no entries parsed from {}", summary_path.display()));
    }

    let mut pages = Vec::with_capacity(entries.len());
    for entry in &entries {
        let path = guide.join(&entry.rel_path);
        let raw = read(&path)?;
        pages.push(Page::parse(entry, &raw));
    }

    let llms_full = render_llms_full(&pages);
    let llms_index = render_llms_index(&pages);

    // AGENTS.md: the hand-written template with every code block substituted from a named
    // region marker in a tested example. Version-neutral (the `vX.Y.Z` stamp is injected by
    // the runtime at emission), so the codegen git-diff gate stays stable across versions.
    let regions = extract_regions(&[root.join("docs/examples"), root.join("e2e")])?;
    let template = read(&root.join("xtask/templates/AGENTS.md.tmpl"))?;
    let agents = render_agents(&template, &regions)?;

    let full_path = out_dir.join("llms-full.txt");
    let index_path = out_dir.join("llms.txt");
    let agents_path = out_dir.join("AGENTS.md");
    write(&full_path, &llms_full)?;
    write(&index_path, &llms_index)?;
    write(&agents_path, &agents)?;
    Ok(vec![index_path, full_path, agents_path])
}

/// Walk the given roots for `.marreta` files and collect every `# region: NAME` ... `# endregion`
/// block, dedented to its own minimal indentation. Region names must be unique across all files,
/// so a primer snippet has exactly one source of truth.
fn extract_regions(roots: &[PathBuf]) -> Result<BTreeMap<String, String>, String> {
    let mut regions: BTreeMap<String, String> = BTreeMap::new();
    let mut files = Vec::new();
    for root in roots {
        collect_marreta_files(root, &mut files)?;
    }
    files.sort();
    for file in &files {
        let text = read(file)?;
        for (name, body) in parse_regions(&text, file)? {
            if regions.insert(name.clone(), body).is_some() {
                return Err(format!(
                    "duplicate region marker '{name}' (found again in {})",
                    file.display()
                ));
            }
        }
    }
    Ok(regions)
}

fn collect_marreta_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    let entries = fs::read_dir(dir).map_err(|e| format!("read dir {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read entry in {}: {e}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_marreta_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "marreta") {
            out.push(path);
        }
    }
    Ok(())
}

/// Parse `# region: NAME` ... `# endregion` blocks out of one file's text.
fn parse_regions(text: &str, file: &Path) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    let mut open: Option<(String, Vec<String>)> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("# region:") {
            if open.is_some() {
                return Err(format!("nested region in {}", file.display()));
            }
            open = Some((name.trim().to_string(), Vec::new()));
        } else if trimmed == "# endregion" || trimmed.starts_with("# endregion:") {
            let (name, body) = open
                .take()
                .ok_or_else(|| format!("# endregion without # region in {}", file.display()))?;
            out.push((name, dedent(&body)));
        } else if let Some((_, body)) = open.as_mut() {
            body.push(line.to_string());
        }
    }
    if let Some((name, _)) = open {
        return Err(format!("region '{name}' not closed in {}", file.display()));
    }
    Ok(out)
}

/// Strip the common leading whitespace shared by all non-empty lines.
fn dedent(lines: &[String]) -> String {
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| if l.len() >= indent { &l[indent..] } else { l })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_matches('\n')
        .to_string()
}

/// Substitute every `{{region:NAME}}` placeholder line with its region, re-indented to the
/// placeholder's own indentation. A placeholder with no matching region is a hard error, which
/// is the snippet-provenance guarantee at generation time (the gate re-asserts it in CI).
fn render_agents(template: &str, regions: &BTreeMap<String, String>) -> Result<String, String> {
    let mut out = String::new();
    for line in template.lines() {
        if let Some((indent, name)) = parse_placeholder(line) {
            let body = regions
                .get(name)
                .ok_or_else(|| format!("AGENTS.md template references unknown region '{name}'"))?;
            for region_line in body.lines() {
                if region_line.is_empty() {
                    out.push('\n');
                } else {
                    out.push_str(indent);
                    out.push_str(region_line);
                    out.push('\n');
                }
            }
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(normalize_trailing(out))
}

/// For a line that is exactly `<indent>{{region:NAME}}`, return `(indent, name)`.
fn parse_placeholder(line: &str) -> Option<(&str, &str)> {
    let indent_len = line.len() - line.trim_start().len();
    let (indent, rest) = line.split_at(indent_len);
    let name = rest.strip_prefix("{{region:")?.strip_suffix("}}")?;
    if name.is_empty() || name.contains(char::is_whitespace) {
        return None;
    }
    Some((indent, name))
}

/// One `SUMMARY.md` list entry: its section heading, link text, and path relative to the guide.
struct Entry {
    section: String,
    title: String,
    rel_path: String,
}

/// A resolved guide page: its summary order metadata plus its parsed frontmatter and body.
struct Page<'a> {
    entry: &'a Entry,
    summary: String,
    body: String,
}

impl<'a> Page<'a> {
    fn parse(entry: &'a Entry, raw: &str) -> Self {
        let (frontmatter, body) = split_frontmatter(raw);
        let summary = frontmatter
            .as_deref()
            .and_then(|fm| frontmatter_value(fm, "summary"))
            .unwrap_or_default();
        Page {
            entry,
            summary,
            body: body.trim().to_string(),
        }
    }
}

/// Parse the `SUMMARY.md` table of contents into ordered entries, keyed by their `## Section`.
fn parse_summary(summary: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut section = String::new();
    for line in summary.lines() {
        let trimmed = line.trim_start();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            section = heading.trim().to_string();
            continue;
        }
        if let Some((title, target)) = parse_list_link(trimmed)
            && target.ends_with(".md")
        {
            entries.push(Entry {
                section: section.clone(),
                title: title.to_string(),
                rel_path: target.to_string(),
            });
        }
    }
    entries
}

/// Extract `(title, target)` from a markdown list link line like `- [Title](path.md)`.
fn parse_list_link(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("- ")?;
    let rest = rest.strip_prefix('[')?;
    let (title, rest) = rest.split_once("](")?;
    let target = rest.strip_suffix(')')?;
    Some((title, target))
}

/// Split a `---` YAML frontmatter block off the front of a markdown file, returning
/// `(frontmatter, body)`. When there is no frontmatter, returns `(None, whole)`.
fn split_frontmatter(raw: &str) -> (Option<String>, String) {
    let rest = match raw.strip_prefix("---\n") {
        Some(rest) => rest,
        None => return (None, raw.to_string()),
    };
    match rest.split_once("\n---\n") {
        Some((fm, body)) => (Some(fm.to_string()), body.to_string()),
        None => (None, raw.to_string()),
    }
}

/// Read a quoted-or-bare scalar value for `key:` from a frontmatter block.
fn frontmatter_value(frontmatter: &str, key: &str) -> Option<String> {
    for line in frontmatter.lines() {
        if let Some((name, value)) = line.split_once(':')
            && name.trim() == key
        {
            let value = value.trim().trim_matches('"').trim();
            return Some(value.to_string());
        }
    }
    None
}

/// The site URL a guide page is served at, from its path (`tutorials/quickstart.md` -> the docs URL).
fn page_url(rel_path: &str) -> String {
    let slug = rel_path.strip_suffix(".md").unwrap_or(rel_path);
    format!("https://marreta.dev/docs/{slug}")
}

fn render_llms_full(pages: &[Page]) -> String {
    let mut out = String::new();
    out.push_str("# Marreta Lang — full reference for AI agents\n\n");
    out.push_str(
        "Marreta is a DSL for REST APIs. This file concatenates the full documentation guide in\n\
         reading order. It is generated; do not edit by hand.\n\n",
    );
    for page in pages {
        out.push_str("---\n\n");
        out.push_str(&page.body);
        out.push_str("\n\n");
    }
    normalize_trailing(out)
}

fn render_llms_index(pages: &[Page]) -> String {
    let mut out = String::new();
    out.push_str("# Marreta Lang\n\n");
    out.push_str(
        "> A focused DSL for REST APIs, with routes, validation, databases, cache, messaging,\n\
         > authentication, tests, and generated OpenAPI docs as first-class language concepts.\n\n",
    );
    out.push_str("Full reference for agents: https://marreta.dev/llms-full.txt\n\n");

    let mut current = "";
    for page in pages {
        if page.entry.section != current {
            if !current.is_empty() {
                out.push('\n');
            }
            current = &page.entry.section;
            out.push_str(&format!("## {current}\n\n"));
        }
        let url = page_url(&page.entry.rel_path);
        if page.summary.is_empty() {
            out.push_str(&format!("- [{}]({})\n", page.entry.title, url));
        } else {
            out.push_str(&format!(
                "- [{}]({}): {}\n",
                page.entry.title, url, page.summary
            ));
        }
    }
    normalize_trailing(out)
}

/// Collapse trailing whitespace to a single terminating newline (stable output for the git-diff gate).
fn normalize_trailing(mut out: String) -> String {
    while out.ends_with(['\n', ' ']) {
        out.pop();
    }
    out.push('\n');
    out
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))
}

fn write(path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents).map_err(|e| format!("write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_section_and_links() {
        let summary = "# Summary\n\n## Tutorials\n\n- [Quickstart](tutorials/quickstart.md)\n\n## Reference\n\n- [Keywords](reference/keywords.md)\n  - [db](reference/namespaces/db.md)\n";
        let entries = parse_summary(summary);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].section, "Tutorials");
        assert_eq!(entries[0].rel_path, "tutorials/quickstart.md");
        assert_eq!(entries[2].section, "Reference");
        assert_eq!(entries[2].rel_path, "reference/namespaces/db.md");
    }

    #[test]
    fn splits_frontmatter_and_reads_summary() {
        let raw = "---\ntitle: \"Schemas\"\nsummary: \"One schema describes your data once.\"\n---\n\n# Schemas\n\nBody text.\n";
        let (fm, body) = split_frontmatter(raw);
        let fm = fm.expect("frontmatter present");
        assert_eq!(
            frontmatter_value(&fm, "summary").as_deref(),
            Some("One schema describes your data once.")
        );
        assert!(body.trim_start().starts_with("# Schemas"));
    }

    #[test]
    fn page_without_frontmatter_keeps_body() {
        let (fm, body) = split_frontmatter("# Title\n\nBody.\n");
        assert!(fm.is_none());
        assert_eq!(body, "# Title\n\nBody.\n");
    }

    #[test]
    fn url_strips_md_suffix() {
        assert_eq!(
            page_url("concepts/schemas.md"),
            "https://marreta.dev/docs/concepts/schemas"
        );
    }

    #[test]
    fn parses_and_dedents_a_region() {
        let text = "route GET \"/x\"\n    # region: body\n    a = 1\n    b = 2\n    # endregion\n    reply 200, a\n";
        let regions = parse_regions(text, Path::new("x.marreta")).unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].0, "body");
        assert_eq!(regions[0].1, "a = 1\nb = 2");
    }

    #[test]
    fn unclosed_region_errors() {
        let text = "# region: open\nx = 1\n";
        assert!(parse_regions(text, Path::new("x.marreta")).is_err());
    }

    #[test]
    fn placeholder_only_matches_a_lone_region_line() {
        assert_eq!(
            parse_placeholder("    {{region:route}}"),
            Some(("    ", "route"))
        );
        assert_eq!(parse_placeholder("text {{region:route}}"), None);
        assert_eq!(parse_placeholder("plain line"), None);
    }

    #[test]
    fn renders_placeholder_with_indent() {
        let mut regions = BTreeMap::new();
        regions.insert(
            "route".to_string(),
            "route GET \"/x\"\n    reply 200, ok".to_string(),
        );
        let out = render_agents("intro\n    {{region:route}}\nend", &regions).unwrap();
        assert_eq!(
            out,
            "intro\n    route GET \"/x\"\n        reply 200, ok\nend\n"
        );
    }

    #[test]
    fn unknown_region_is_an_error() {
        let regions = BTreeMap::new();
        assert!(render_agents("{{region:missing}}", &regions).is_err());
    }
}
