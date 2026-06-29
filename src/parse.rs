//! The shared front end: comrak options + the single parse.
//!
//! These options drive **both** back ends. Setting them once is the whole point
//! of the one-parse design — EPUB and PDF can never disagree about how the
//! Markdown was understood. Pin the comrak version: this `Options` struct churns
//! across releases.

use comrak::Options;

/// Build the comrak options used for every parse in this crate.
///
/// `parse.smart` reproduces today's pandoc `markdown+smart` (smart quotes, em/en
/// dashes). The GFM extensions match what the manuscripts may use; `footnotes`
/// is on so footnote/endnote markup round-trips into both outputs.
pub fn options() -> Options<'static> {
    let mut o = Options::default();
    o.parse.smart = true;
    o.extension.strikethrough = true;
    o.extension.table = true;
    o.extension.autolink = true;
    o.extension.footnotes = true;
    o.extension.tasklist = true;
    o
}
