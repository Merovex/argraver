//! PDF back end, part 2: typeset Typst markup to PDF via the `typst` CLI. **Stub.**
//!
//! Option **C′** from the spec: generate a `.typ`, then shell to `typst compile`.
//! Light build, no version-locked crate; structurally identical to what the bash
//! `book` already does. The C-vs-C′ (typst-as-library) decision is deferred.

use std::path::Path;

use anyhow::{bail, Result};

use crate::ingest::{Book, Cover};

/// Build a PDF from a strip-fm'd manuscript and its metadata. **Not yet
/// implemented** — depends on the `NodeValue -> Typst` visitor
/// ([`crate::render_typst::render`]) and the ported interior template.
pub fn build(
    _book: &Book,
    _cover: &Cover,
    _markdown: &str,
    _meta_dir: Option<&Path>,
    _output: &Path,
) -> Result<()> {
    bail!("PDF output is not implemented yet (EPUB-first; see docs/DESIGN.md)")
}
