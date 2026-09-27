//! Argraver — a single-source book compiler.
//!
//! One Markdown manuscript + a standalone `_argraver.yml` in; an EPUB and a print
//! PDF out. The shape is a compiler: one comrak parse fanned out to two back ends
//! (see `docs/DESIGN.md`).
//!
//! ```text
//!                          ┌─ comrak::format_html ─► HTML ─► epub-builder ─► EPUB
//! md ─ strip-fm ─ comrak ──► AST (one parse)
//!                          └─ ast_to_typst (visitor) ─► Typst ─► typst CLI ─► PDF
//! ```
//!
//! The EPUB path is implemented; the PDF/Typst path is stubbed.

pub mod cli;
pub mod epub;
pub mod ingest;
pub mod parse;
pub mod pdf;
pub mod render_html;
pub mod render_typst;
