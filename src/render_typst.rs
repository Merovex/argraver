//! PDF back end, part 1: comrak AST -> Typst markup. **Stub.**
//!
//! Only [`typst_escape`] is real so far — it's the one correctness landmine, and
//! it's small, so it lands (and gets tested) now. The `NodeValue -> Typst`
//! visitor and the ported interior template are the PDF phase; see
//! `docs/DESIGN.md`.

use anyhow::{bail, Result};

/// Escape the Typst metacharacters `\ # * _ @ $ [ ] < >` in prose **text nodes**.
///
/// Ported from Verkilo's `typst_escape`, with the two deliberate divergences the
/// spec calls for: this is a *book*, so we do **not** flatten smart quotes to
/// straight, and we do **not** turn backticks into apostrophes. Typographic
/// quotes and em-dashes (produced by comrak's `parse.smart`) are kept verbatim.
///
/// Never call this on raw/code nodes — their literal text must pass through.
pub fn typst_escape(text: &str) -> String {
    text.replace('\\', "\\\\") // must be first, so we don't escape our own backslashes
        .replace('#', "\\#")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('@', "\\@")
        .replace('$', "\\$")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('<', "\\<")
        .replace('>', "\\>")
}

/// Render the manuscript to a Typst document. **Not yet implemented** — the
/// `NodeValue -> Typst` visitor and interior template are the PDF phase.
pub fn render(_markdown: &str) -> Result<String> {
    bail!("PDF/Typst rendering is not implemented yet (EPUB-first; see docs/DESIGN.md)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_typst_metacharacters() {
        assert_eq!(typst_escape("C# costs $5"), "C\\# costs \\$5");
        assert_eq!(typst_escape("a_b *c* [d]"), "a\\_b \\*c\\* \\[d\\]");
        assert_eq!(typst_escape("x < y > z @ w"), "x \\< y \\> z \\@ w");
    }

    #[test]
    fn backslash_escaped_first() {
        assert_eq!(typst_escape("\\#"), "\\\\\\#");
    }

    #[test]
    fn keeps_typographic_quotes_and_dashes() {
        // The book divergence: smart quotes and em-dashes survive untouched.
        let input = "\u{201C}Hello\u{201D} \u{2014} \u{2018}hi\u{2019}";
        assert_eq!(typst_escape(input), input);
    }
}
