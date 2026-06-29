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

/// A starter `_metadata.yml`, written by `argraver init`. Documents every
/// supported `book:` / `cover:` setting with sensible defaults.
pub const METADATA_TEMPLATE: &str = r#"# _metadata.yml — single source of truth for this book.
# Standalone YAML (not Obsidian front matter); edit in your editor of choice.
#   book:  the book's facts — consumed by the EPUB and the print PDF.
#   cover: cover art (the EPUB embeds a resized copy).

book:
  title: Untitled
  subtitle:
  author: Anonymous
  publisher:
  # copyright:   (optional — omit for "Copyright © <author>")
  edition: First Edition
  isbn:
  trim: digest          # pocket 5x8 | small-digest 5.25x8 | digest 5.5x8.5 | trade 6x9 | large 7x10
  margins: normal       # narrow | normal | wide — picks within the trim's margin ranges
  chapter_start: recto  # recto (next right-hand page) | any (next page, either side)
  scenebreak: auto      # auto (blank mid-page, * * * at a page edge) | ornament | blank
  # body_font: Libertinus Serif   # PDF body font (installed font or a family under font_path)
  # display_font:                 # chapter/heading font (defaults to body_font)
  # font_path: fonts              # dir of .ttf/.otf to embed (relative to this file)
  dedication:
  description: |
    A one-or-two paragraph book description (back-cover / catalog copy).
  # rights:      (optional — omit for the generic all-rights-reserved text)
  # disclaimer:  (optional — omit for the generic fiction disclaimer)

cover:
  background:
    image: cover.png    # path relative to this file (or vault-absolute)
"#;

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
    /// Scene-break style: `auto` (default) | `ornament` | `blank`. See
    /// [`Book::scene_break_style`].
    pub scenebreak: Option<String>,
    /// Outside-margin width: `narrow` | `normal` (default) | `wide`. See
    /// [`Book::margin_width`]. The inside (gutter) margin is derived from the
    /// page count, not this setting.
    pub margins: Option<String>,
    /// Where a chapter opens: `recto` (default — the next right-hand page, may
    /// leave a blank verso) | `any` (the next page, either side). See
    /// [`Book::chapter_opens_recto`].
    pub chapter_start: Option<String>,
    /// Body (running text) font family name. Must be a system font or live under
    /// `font_path`. Env `BOOK_MAINFONT` overrides. Default: Libertinus Serif.
    pub body_font: Option<String>,
    /// Display (chapter/heading) font family. Env `BOOK_DISPLAYFONT` overrides.
    /// Defaults to the body font.
    pub display_font: Option<String>,
    /// Directory of custom font files (`.ttf`/`.otf`) to make available to the
    /// PDF typesetter (so they embed). Relative paths resolve against the
    /// metadata file's directory. Env `BOOK_FONTPATH` overrides.
    pub font_path: Option<String>,
    /// Optional override for the generic "all rights reserved" text.
    pub rights: Option<String>,
    /// Optional override for the generic fiction disclaimer.
    pub disclaimer: Option<String>,
    pub lang: Option<String>,
}

/// How a scene break (a `---` rule inside a chapter) is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneBreakStyle {
    /// PDF: a blank gap mid-page, the ornament against a page edge. EPUB: the
    /// styled `<hr>` rule. The default.
    Auto,
    /// Always the centered asterisk ornament (`* * *`), both outputs.
    Ornament,
    /// Always a plain blank gap (no rule, no ornament), both outputs.
    Blank,
}

impl Book {
    /// The configured scene-break style, defaulting to `Auto`. Unknown values
    /// warn and fall back to `Auto`.
    pub fn scene_break_style(&self) -> SceneBreakStyle {
        match self.scenebreak.as_deref().map(str::trim) {
            Some("ornament") => SceneBreakStyle::Ornament,
            Some("blank") => SceneBreakStyle::Blank,
            None | Some("") | Some("auto") => SceneBreakStyle::Auto,
            Some(other) => {
                eprintln!("  unknown book.scenebreak '{other}' (auto|ornament|blank); using auto");
                SceneBreakStyle::Auto
            }
        }
    }

    /// The configured margin width, defaulting to `normal`. This selects where
    /// inside the trim's recommended margin ranges the page sits (see the trim
    /// table in `render_typst`); it is not an absolute size.
    pub fn margin_width(&self) -> MarginWidth {
        match self.margins.as_deref().map(str::trim) {
            Some("narrow") => MarginWidth::Narrow,
            Some("wide") => MarginWidth::Wide,
            None | Some("") | Some("normal") => MarginWidth::Normal,
            Some(other) => {
                eprintln!("  unknown book.margins '{other}' (narrow|normal|wide); using normal");
                MarginWidth::Normal
            }
        }
    }
}

impl Book {
    /// Whether chapters open on the next right-hand (recto/odd) page. `recto`
    /// (default) gives the traditional book look — chapters always start on the
    /// right, leaving a blank verso when needed. `any` opens on the next page,
    /// either side (no blank versos). Unknown values warn and default to recto.
    pub fn chapter_opens_recto(&self) -> bool {
        match self.chapter_start.as_deref().map(str::trim) {
            Some("any") | Some("next") => false,
            None | Some("") | Some("recto") | Some("right") => true,
            Some(other) => {
                eprintln!("  unknown book.chapter_start '{other}' (recto|any); using recto");
                true
            }
        }
    }
}

/// How tight the page sits within a trim's recommended margin ranges: `Narrow`
/// is the low end, `Wide` the high end, `Normal` the midpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarginWidth {
    Narrow,
    Normal,
    Wide,
}

impl MarginWidth {
    /// Pick a value within a `(low, high)` inch range for this width.
    pub fn within(self, (low, high): (f64, f64)) -> f64 {
        match self {
            MarginWidth::Narrow => low,
            MarginWidth::Normal => (low + high) / 2.0,
            MarginWidth::Wide => high,
        }
    }
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
    fn init_template_is_valid_metadata() {
        // The `argraver init` template must always parse and round-trip cleanly.
        let meta: Metadata = serde_yaml::from_str(METADATA_TEMPLATE).expect("template parses");
        assert_eq!(meta.book.title, "Untitled");
        assert_eq!(meta.book.trim.as_deref(), Some("digest"));
        assert!(meta.book.chapter_opens_recto());
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
