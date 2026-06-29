//! Ingest: turn the on-disk inputs into typed values the renderers consume.
//!
//! Three jobs, all shared by both back ends:
//!   1. Metadata discovery + parse (`_metadata.yml` -> [`Book`]).
//!   2. `strip-fm`: drop Longform's inline YAML blocks, keep `---` scene rules.
//!   3. Project-name derivation, mirroring the bash `book` UX.
//!
//! The H1 chapter split lives on the comrak AST (see [`crate::render_html`]),
//! not here — this module only produces the cleaned manuscript string.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// The `book:` map from `_metadata.yml` — the single source of truth for the
/// book's facts. Field names match the YAML keys; everything optional but
/// `title` is treated as required at render time.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Book {
    pub title: String,
    pub subtitle: Option<String>,
    pub author: Option<String>,
    pub publisher: Option<String>,
    /// Full copyright line, e.g. "Copyright © 2026 B. Wilson".
    pub copyright: Option<String>,
    pub edition: Option<String>,
    pub isbn: Option<String>,
    /// Trim preset name (`pocket | small-digest | digest | trade | large`).
    /// Consumed by the PDF path only; EPUB ignores it.
    pub trim: Option<String>,
    pub dedication: Option<String>,
    pub description: Option<String>,
    /// Optional override for the generic "all rights reserved" text.
    pub rights: Option<String>,
    /// Optional override for the generic fiction disclaimer.
    pub disclaimer: Option<String>,
    pub lang: Option<String>,
}

/// The `cover:` map. Only the fields the EPUB cover needs are modeled; the rest
/// (print spine math etc.) is out of scope for v1.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Cover {
    #[serde(default)]
    pub background: CoverBackground,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CoverBackground {
    /// Master cover image path. Often vault-absolute (`/Series/_assets/…`).
    pub image: Option<String>,
}

/// The whole `_metadata.yml`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Metadata {
    #[serde(default)]
    pub book: Book,
    #[serde(default)]
    pub cover: Cover,
}

/// Derive the project name from a manuscript path, mirroring the bash
/// `project_of`: strip ` - manuscript.md`, then a trailing `.md`.
///
/// ```
/// # use argraver::ingest::project_of;
/// assert_eq!(project_of("Books/Prequel Reader Magnet - manuscript.md"),
///            "Prequel Reader Magnet");
/// assert_eq!(project_of("draft.md"), "draft");
/// ```
pub fn project_of(manuscript: impl AsRef<Path>) -> String {
    let base = manuscript
        .as_ref()
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let base = base
        .strip_suffix(" - manuscript.md")
        .map(str::to_owned)
        .unwrap_or(base);
    base.strip_suffix(".md").map(str::to_owned).unwrap_or(base)
}

/// Locate the `_metadata.yml` for a manuscript, mirroring the bash `find_meta`:
///   1. `BOOK_META` env override, if set.
///   2. `Books/*/<Project>/_metadata.yml` under a discovered vault root.
///   3. `_metadata.yml` beside the manuscript.
///
/// Returns `None` if nothing is found (the renderers then fall back to defaults).
pub fn find_meta(manuscript: impl AsRef<Path>) -> Option<PathBuf> {
    if let Ok(over) = std::env::var("BOOK_META") {
        let p = PathBuf::from(over);
        if p.is_file() {
            return Some(p);
        }
    }

    let manuscript = manuscript.as_ref();
    let project = project_of(manuscript);

    // `Books/*/<Project>/_metadata.yml` — walk up from the manuscript looking
    // for a `Books` dir, then probe each series subfolder for this project.
    let mut dir = manuscript.parent();
    while let Some(d) = dir {
        let books = d.join("Books");
        if books.is_dir() {
            if let Ok(series) = std::fs::read_dir(&books) {
                for entry in series.flatten() {
                    let cand = entry.path().join(&project).join("_metadata.yml");
                    if cand.is_file() {
                        return Some(cand);
                    }
                }
            }
        }
        dir = d.parent();
    }

    // Beside the manuscript.
    let beside = manuscript
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("_metadata.yml");
    beside.is_file().then_some(beside)
}

/// Load and parse a `_metadata.yml`.
pub fn load_metadata(path: impl AsRef<Path>) -> Result<Metadata> {
    let path = path.as_ref();
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading metadata {}", path.display()))?;
    let meta: Metadata = serde_yaml::from_str(&raw)
        .with_context(|| format!("parsing metadata {}", path.display()))?;
    Ok(meta)
}

/// Read a manuscript file and run [`strip_fm`] over it.
pub fn load_manuscript(path: impl AsRef<Path>) -> Result<String> {
    let path = path.as_ref();
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading manuscript {}", path.display()))?;
    Ok(strip_fm(&raw))
}

/// Port of `bin/strip-fm`: remove YAML metadata blocks from a Markdown stream
/// while preserving lone `---` horizontal rules (scene breaks).
///
/// A block delimited by `---` … `---`/`...` is removed **only** when every inner
/// line is YAML-ish (a `key:`, a `- item`, an indented continuation, a comment,
/// or blank) **and** at least one line is a real `key:`. Prose and scene-break
/// rules are therefore never touched. Mirrors the Ruby heuristics exactly,
/// including the 60-line lookahead cap.
pub fn strip_fm(input: &str) -> String {
    let lines: Vec<&str> = input.split_inclusive('\n').collect();

    let is_fence = |s: &str| {
        let t = s.trim_end();
        t == "---" || t == "..."
    };
    let has_key = |s: &str| yaml_has_key(s);
    let is_yamlish = |s: &str| {
        let t = s.trim_end();
        t.is_empty()
            || t.trim_start().starts_with('#')
            || yaml_has_key(s)
            || yaml_is_list_item(t)
            || yaml_is_indented_continuation(t)
    };

    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_end() == "---" {
            // Scan forward for a closing fence, capped at 60 lines like the Ruby.
            let mut j = i + 1;
            while j < lines.len() && !is_fence(lines[j]) && (j - i) < 60 {
                j += 1;
            }
            if j < lines.len() && is_fence(lines[j]) && j > i + 1 {
                let block = &lines[(i + 1)..j];
                if block.iter().any(|l| has_key(l)) && block.iter().all(|l| is_yamlish(l)) {
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(lines[i]);
        i += 1;
    }

    out.concat()
}

// --- YAML-ish line classifiers (the Ruby regexes, transliterated) -----------

/// `\A\s*[\w][\w ./-]*:(\s|\z)` — a `key:` possibly with spaces/dots/slashes.
fn yaml_has_key(s: &str) -> bool {
    let t = s.trim_end();
    let trimmed = t.trim_start();
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    if !(first.is_alphanumeric() || first == '_') {
        return false;
    }
    // Find the colon that ends the key.
    let Some(colon) = trimmed.find(':') else {
        return false;
    };
    let key = &trimmed[..colon];
    if !key
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '_' | ' ' | '.' | '/' | '-'))
    {
        return false;
    }
    // `(\s|\z)` — colon is end-of-line or followed by whitespace.
    match trimmed[colon + 1..].chars().next() {
        None => true,
        Some(c) => c.is_whitespace(),
    }
}

/// `\A\s*-\s` — a `- ` list item.
fn yaml_is_list_item(t: &str) -> bool {
    let trimmed = t.trim_start();
    trimmed
        .strip_prefix('-')
        .is_some_and(|rest| rest.starts_with(char::is_whitespace))
}

/// `\A\s+\S` — an indented continuation (leading whitespace then non-space).
fn yaml_is_indented_continuation(t: &str) -> bool {
    let mut chars = t.chars();
    let first = chars.next();
    matches!(first, Some(c) if c.is_whitespace()) && t.trim_start().chars().next().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_name_strips_manuscript_suffix() {
        assert_eq!(
            project_of("/x/Prequel Reader Magnet - manuscript.md"),
            "Prequel Reader Magnet"
        );
        assert_eq!(project_of("draft.md"), "draft");
        assert_eq!(project_of("plain"), "plain");
    }

    #[test]
    fn strip_fm_drops_leading_yaml_block() {
        let input = "---\ntype: scene\ntitle: The Departure\ntags:\n  - scene\n---\n# The Departure\n\nBody.\n";
        let out = strip_fm(input);
        assert_eq!(out, "# The Departure\n\nBody.\n");
    }

    #[test]
    fn strip_fm_keeps_scene_break_rule() {
        // A lone `---` between prose paragraphs is a scene break, not YAML.
        let input = "Para one.\n\n---\n\nPara two.\n";
        assert_eq!(strip_fm(input), input);
    }

    #[test]
    fn strip_fm_keeps_block_without_a_key() {
        // `---` fences around non-key lines (e.g. a list) are not metadata.
        let input = "---\n- just a list\n- of things\n---\n";
        assert_eq!(strip_fm(input), input);
    }

    #[test]
    fn strip_fm_handles_terminal_dots_fence() {
        let input = "---\nkey: value\n...\nProse.\n";
        assert_eq!(strip_fm(input), "Prose.\n");
    }
}
