//! PDF back end, part 1: the comrak AST -> Typst markup.
//!
//! This owns the only renderer we write by hand. Three pieces:
//!   * [`typst_escape`] — escape Typst metacharacters in prose text nodes.
//!   * [`render_body`] — the `NodeValue -> Typst` visitor over the manuscript.
//!   * [`render_document`] — wrap the body in the interior template (page setup,
//!     fonts, drop caps, scene breaks, running heads) and the front matter.
//!
//! The interior design is ported (in spirit, to Typst 0.14) from Verkilo's
//! `interior_template_engine.rs`; the drop-cap library is the vendored
//! `droplet.typ` (MIT, from typst-droplet). typst is invoked from [`crate::pdf`].

use std::collections::HashMap;

use comrak::nodes::{AstNode, ListType, NodeValue};
use comrak::{parse_document, Arena};

use crate::ingest::{Book, SceneBreakStyle};

/// The vendored drop-cap library (MIT, github.com/EpicEricEE/typst-droplet).
const DROPLET: &str = include_str!("resources/droplet.typ");

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

/// Page margins in inches. `inside` is the spine/gutter side; `outside` the
/// outer edge. KDP/IngramSpark conformance: every side ≥ 0.5", and `inside`
/// grows with the page count (see [`gutter_inside_in`]).
#[derive(Debug, Clone, Copy)]
pub struct PageMargins {
    pub inside_in: f64,
    pub outside_in: f64,
    pub top_in: f64,
    pub bottom_in: f64,
}

/// The inside (gutter) margin in inches for a given page count. A professional
/// working scale that clears both KDP and IngramSpark minimums with a
/// readability cushion (Ingram is the binding constraint): thicker books pull
/// more paper into the spine, so the gutter grows to keep text out of the curve.
pub fn gutter_inside_in(pages: u32) -> f64 {
    match pages {
        0..=150 => 0.5,
        151..=300 => 0.625,
        301..=500 => 0.75,
        501..=700 => 0.875,
        _ => 1.0, // 701–828 (and beyond; 828 is the typical platform page cap)
    }
}

/// Resolve the full page margins for a book at a given gutter: outside/top/bottom
/// come from the trim's ranges (top biased to the upper end for the running
/// header), `inside` is the supplied gutter.
pub fn resolve_margins(book: &Book, inside_in: f64) -> PageMargins {
    let trim = Trim::from_name(book.trim.as_deref());
    let width = book.margin_width();
    PageMargins {
        inside_in,
        outside_in: width.within(trim.outside),
        top_in: trim.top.1, // upper end: leaves room for the running head
        bottom_in: width.within(trim.bottom),
    }
}

/// Render a full Typst document: interior template + front matter + body, with
/// the given inside (gutter) margin (the rest derive from the trim). A trailing
/// `<pagecount>` metadata lets the caller query the physical page count to settle
/// the gutter (see [`crate::pdf`]).
pub fn render_document(book: &Book, markdown: &str, inside_in: f64) -> String {
    let trim = Trim::from_name(book.trim.as_deref());
    let margins = resolve_margins(book, inside_in);
    let body = render_body(markdown);
    let mut doc = preamble(book, trim, margins);
    doc.push_str(&front_matter(book));
    doc.push_str(MAIN_MATTER_HEADER);
    doc.push_str(&body);
    // Physical page count of the last element, for the gutter-fitting pass.
    doc.push_str("\n#context [#metadata(here().page()) <pagecount>]\n");
    doc
}

// --- the NodeValue -> Typst visitor -----------------------------------------

/// Render the (strip-fm'd) manuscript body to Typst markup. Drops leading
/// separators, wraps the first paragraph after each H1 in `#chapter-dropcap`,
/// and turns scene-break rules into `#scenebreak`.
pub fn render_body(markdown: &str) -> String {
    let opts = crate::parse::options();
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &opts);

    let footnotes = collect_footnote_defs(root);
    let mut out = String::new();
    let mut content_started = false;
    let mut pending_drop_cap = false;

    let kids: Vec<&AstNode> = root.children().collect();
    for (i, node) in kids.iter().enumerate() {
        let node: &AstNode = node;
        let value = node.data.borrow().value.clone();
        match value {
            // Footnote definitions are inlined at their references, not emitted here.
            NodeValue::FootnoteDefinition(_) => {}

            NodeValue::ThematicBreak => {
                // Drop the scene break if it leads the content, or if it sits
                // directly before a heading (the chapter/section break supersedes
                // it — no ornament stranded above a title).
                let before_heading = kids
                    .get(i + 1)
                    .is_some_and(|n| matches!(&n.data.borrow().value, NodeValue::Heading(_)));
                if content_started && !before_heading {
                    out.push_str("\n#scenebreak\n\n");
                }
                pending_drop_cap = false;
            }

            NodeValue::Heading(h) => {
                content_started = true;
                out.push_str(&"=".repeat(h.level as usize));
                out.push(' ');
                emit_inline(node, &footnotes, &mut out);
                out.push_str("\n\n");
                pending_drop_cap = h.level == 1;
            }

            NodeValue::Paragraph => {
                content_started = true;
                if pending_drop_cap {
                    out.push_str("#chapter-dropcap[");
                    emit_inline(node, &footnotes, &mut out);
                    out.push_str("]\n\n");
                    pending_drop_cap = false;
                } else {
                    emit_inline(node, &footnotes, &mut out);
                    out.push_str("\n\n");
                }
            }

            _ => {
                content_started = true;
                pending_drop_cap = false;
                emit_block(node, &footnotes, &mut out);
            }
        }
    }

    out
}

type Footnotes<'a> = HashMap<String, &'a AstNode<'a>>;

fn collect_footnote_defs<'a>(root: &'a AstNode<'a>) -> Footnotes<'a> {
    let mut map = HashMap::new();
    for node in root.descendants() {
        if let NodeValue::FootnoteDefinition(def) = &node.data.borrow().value {
            map.insert(def.name.clone(), node);
        }
    }
    map
}

/// Emit a block-level node (not handled specially by the top-level loop).
fn emit_block<'a>(node: &'a AstNode<'a>, fns: &Footnotes<'a>, out: &mut String) {
    match &node.data.borrow().value {
        NodeValue::Paragraph => {
            emit_inline(node, fns, out);
            out.push_str("\n\n");
        }
        NodeValue::Heading(h) => {
            out.push_str(&"=".repeat(h.level as usize));
            out.push(' ');
            emit_inline(node, fns, out);
            out.push_str("\n\n");
        }
        NodeValue::ThematicBreak => out.push_str("\n#scenebreak\n\n"),
        NodeValue::BlockQuote => {
            out.push_str("#blockquote[\n");
            for child in node.children() {
                emit_block(child, fns, out);
            }
            out.push_str("]\n\n");
        }
        NodeValue::List(list) => {
            for item in node.children() {
                let marker = match list.list_type {
                    ListType::Bullet => "- ",
                    ListType::Ordered => "+ ",
                };
                out.push_str(marker);
                // Item children are usually a single tight paragraph.
                for block in item.children() {
                    match &block.data.borrow().value {
                        NodeValue::Paragraph => emit_inline(block, fns, out),
                        _ => emit_block(block, fns, out),
                    }
                }
                out.push('\n');
            }
            out.push('\n');
        }
        NodeValue::CodeBlock(cb) => {
            out.push_str("#raw(block: true, ");
            out.push_str(&typst_string(&cb.literal));
            out.push_str(")\n\n");
        }
        // Raw HTML in prose is stripped on the PDF path (per the spec).
        NodeValue::HtmlBlock(_) => {}
        _ => {
            for child in node.children() {
                emit_block(child, fns, out);
            }
        }
    }
}

/// Emit the inline children of a node.
fn emit_inline<'a>(node: &'a AstNode<'a>, fns: &Footnotes<'a>, out: &mut String) {
    for child in node.children() {
        emit_inline_node(child, fns, out);
    }
}

fn emit_inline_node<'a>(node: &'a AstNode<'a>, fns: &Footnotes<'a>, out: &mut String) {
    match &node.data.borrow().value {
        NodeValue::Text(t) => out.push_str(&typst_escape(t)),
        NodeValue::SoftBreak => out.push(' '),
        NodeValue::LineBreak => out.push_str(" \\\n"),
        NodeValue::Emph => wrap(node, fns, "#emph[", "]", out),
        NodeValue::Strong => wrap(node, fns, "#strong[", "]", out),
        NodeValue::Strikethrough => wrap(node, fns, "#strike[", "]", out),
        NodeValue::Code(code) => {
            out.push_str("#raw(");
            out.push_str(&typst_string(&code.literal));
            out.push(')');
        }
        NodeValue::Link(link) => {
            out.push_str("#link(");
            out.push_str(&typst_string(&link.url));
            out.push_str(")[");
            emit_inline(node, fns, out);
            out.push(']');
        }
        NodeValue::Image(link) => {
            out.push_str("#image(");
            out.push_str(&typst_string(&link.url));
            out.push(')');
        }
        NodeValue::FootnoteReference(reference) => {
            out.push_str("#footnote[");
            if let Some(def) = fns.get(&reference.name) {
                // Render the definition's blocks inline inside the footnote.
                for block in def.children() {
                    match &block.data.borrow().value {
                        NodeValue::Paragraph => emit_inline(block, fns, out),
                        _ => emit_block(block, fns, out),
                    }
                }
            }
            out.push(']');
        }
        // Strip raw inline HTML in prose.
        NodeValue::HtmlInline(_) => {}
        // Safe fallthrough: emit children for any unhandled inline container.
        _ => emit_inline(node, fns, out),
    }
}

fn wrap<'a>(
    node: &'a AstNode<'a>,
    fns: &Footnotes<'a>,
    open: &str,
    close: &str,
    out: &mut String,
) {
    out.push_str(open);
    emit_inline(node, fns, out);
    out.push_str(close);
}

/// A Typst string literal: wrap in quotes, escaping `\` and `"`.
fn typst_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

// --- the interior template (ported from Verkilo, adapted to Typst 0.14) ------

/// A trim size and its professional margin ranges (inches). `top`/`bottom`/
/// `outside` are `(low, high)`; the configured [`MarginWidth`] picks within them.
/// Outer margins grow with the trim so the text block stays proportional — tight
/// trims get tight margins, large trims need more air. The inside (gutter) is
/// *not* here: it's driven by page count ([`gutter_inside_in`]).
#[derive(Clone, Copy)]
struct Trim {
    width_in: f64,
    height_in: f64,
    top: (f64, f64),
    bottom: (f64, f64),
    outside: (f64, f64),
}

impl Trim {
    /// Map the vault's trim names (the bash `book` vocabulary) to dimensions and
    /// margin ranges. Defaults to `digest` (5.5×8.5), matching `bin/book`.
    fn from_name(name: Option<&str>) -> Trim {
        match name.unwrap_or("digest") {
            "pocket" => Trim {
                width_in: 5.0, height_in: 8.0,
                top: (0.6, 0.7), bottom: (0.6, 0.7), outside: (0.375, 0.5),
            },
            "small-digest" => Trim {
                width_in: 5.25, height_in: 8.0,
                top: (0.65, 0.75), bottom: (0.65, 0.75), outside: (0.4, 0.5),
            },
            "trade" => Trim {
                width_in: 6.0, height_in: 9.0,
                top: (0.75, 0.875), bottom: (0.75, 0.875), outside: (0.5, 0.625),
            },
            "large" => Trim {
                width_in: 7.0, height_in: 10.0,
                top: (0.875, 1.0), bottom: (0.875, 1.0), outside: (0.625, 0.75),
            },
            _ => Trim {
                width_in: 5.5, height_in: 8.5, // digest
                top: (0.75, 0.75), bottom: (0.75, 0.75), outside: (0.5, 0.5),
            },
        }
    }
}

/// Body font: env `BOOK_MAINFONT` → `book.body_font` → Libertinus Serif.
fn body_font(book: &Book) -> String {
    env_font("BOOK_MAINFONT")
        .or_else(|| nonempty(&book.body_font).map(str::to_owned))
        .unwrap_or_else(|| "Libertinus Serif".to_string())
}
/// Display font: env `BOOK_DISPLAYFONT` → `book.display_font` → the body font.
fn display_font(book: &Book) -> String {
    env_font("BOOK_DISPLAYFONT")
        .or_else(|| nonempty(&book.display_font).map(str::to_owned))
        .unwrap_or_else(|| body_font(book))
}
fn env_font(var: &str) -> Option<String> {
    std::env::var(var).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Marker between front matter and the body: turn the running heads back on.
/// (The page counter is reset to 1 inside the first chapter's heading rule, so
/// arabic page 1 lands on the first chapter recto — not on the blank verso.)
const MAIN_MATTER_HEADER: &str = "\n#set page(header: running-head, footer: page-number)\n\n";

/// The document preamble: page setup, fonts, paragraph defaults, drop caps,
/// scene break, and the chapter/section heading show rules.
fn preamble(book: &Book, trim: Trim, margins: PageMargins) -> String {
    let title = typst_escape(&book.title);
    let author = typst_escape(book.author.as_deref().unwrap_or(""));
    let body = body_font(book);
    let display = display_font(book);

    format!(
        r#"// Generated by argraver — do not edit by hand.

#set document(title: "{title}", author: "{author}")

// KDP / IngramSpark margins: every side ≥ 0.5"; inside (gutter) scales with the
// page count. `page-top-margin` is reused by the scene-break edge test below.
#let page-top-margin = {top}in
#set page(
  width: {w}in,
  height: {h}in,
  margin: (inside: {inside}in, outside: {outside}in, top: page-top-margin, bottom: {bottom}in),
)

#set text(font: "{body}", size: 11pt, ligatures: true, hyphenate: true)
#set smartquote(enabled: false) // content already carries typographic quotes
#set par(leading: 0.65em, first-line-indent: 1.5em, justify: true, spacing: 0.65em)

{droplet}

// Drop cap on the first paragraph after a chapter heading.
#let chapter-dropcap(body) = {{
  set par(first-line-indent: 0pt)
  dropcap(height: 3, gap: 4pt, font: "{display}", weight: "bold", body)
}}

// Scene break: a blank gap mid-page, but the asterisk ornament when the break
// falls at a page boundary — so a scene change is never lost to a page turn.
// `here().position().y` is the absolute distance from the page's top edge; the
// text block runs from `page-top-margin` to height − bottom margin.
#let scene-glyphs = [#sym.ast.basic #h(0.5em) #sym.ast.basic #h(0.5em) #sym.ast.basic]
#let scene-mark = {{
  v(0.4em, weak: true)
  align(center)[#scene-glyphs]
  v(0.8em, weak: true)
}}
{scenebreak}

#let blockquote(body) = {{
  set par(first-line-indent: 0pt, spacing: 0.5em)
  pad(left: 2em, right: 2em)[#text(style: "italic")[#body]]
  v(0.6em)
}}

// A page is "in the body" once a chapter heading lies on an earlier page. This
// single guard suppresses decoration on front matter, blank versos, and chapter
// openings alike (on an opening page the heading is on *this* page, not before).
#let in-body = () => query(heading.where(level: 1)).any(h => h.location().page() < here().page())

// Running head: title verso (even), author recto (odd).
#let running-head = context {{
  if in-body() {{
    if calc.odd(here().page()) {{ align(right, text(size: 9pt, style: "italic")[{author}]) }}
    else {{ align(left, text(size: 9pt, style: "italic")[{title}]) }}
  }}
}}

// Page number: the logical page counter (restarts at 1 at the first chapter).
#let page-number = context {{
  if in-body() {{ align(center, text(size: 9pt)[#counter(page).get().first()]) }}
}}

// Chapter openings: start on a new page, sink the title, set in the display face.
#let argraver-chapter = counter("argraver-chapter")
#show heading.where(level: 1): it => {{
  {chapter_pagebreak}
  argraver-chapter.step()
  context {{ if argraver-chapter.get().first() == 1 {{ counter(page).update(1) }} }}
  v(1.2in)
  block(below: 0.8in)[
    #set par(justify: false, first-line-indent: 0pt)
    #align(center)[#text(font: "{display}", size: 20pt, weight: "bold", hyphenate: false)[#it.body]]
  ]
}}

#show heading.where(level: 2): it => {{
  set par(justify: false, first-line-indent: 0pt)
  v(18pt)
  align(left)[#text(font: "{display}", size: 14pt, weight: "bold", hyphenate: false)[#it.body]]
  v(8pt)
}}

#show heading.where(level: 3): it => {{
  set par(justify: false, first-line-indent: 0pt)
  v(12pt)
  align(left)[#text(font: "{display}", size: 12pt, style: "italic", hyphenate: false)[#it.body]]
  v(6pt)
}}
"#,
        title = title,
        author = author,
        w = trim.width_in,
        h = trim.height_in,
        inside = margins.inside_in,
        outside = margins.outside_in,
        top = margins.top_in,
        bottom = margins.bottom_in,
        body = body,
        display = display,
        droplet = DROPLET,
        scenebreak = scene_break_def(book.scene_break_style()),
        chapter_pagebreak = chapter_pagebreak(book),
    )
}

/// The chapter-opening `pagebreak` directive: to the next recto (odd) page, or
/// to the next page either side.
fn chapter_pagebreak(book: &Book) -> &'static str {
    if book.chapter_opens_recto() {
        "pagebreak(to: \"odd\", weak: true)"
    } else {
        "pagebreak(weak: true)"
    }
}

/// The `#let scenebreak` definition for the configured style.
///
/// `Auto` is page-aware: the ornament when the break lands against a page edge,
/// an equal blank gap mid-page. It always lays out the *same* ornament and only
/// `hide()`s it mid-page — `hide` keeps the content's full layout footprint, so
/// the ornament's own height sets the base height and the two branches are
/// identical in size by construction. That matters twice over: the vertical
/// rhythm is the same whether the mark shows or not, and the layout height never
/// depends on the `here().position()` read — if it did, the read would shift the
/// page break, flip the branch, and typst would oscillate ("layout did not
/// converge"). Wrapped in a non-breakable block so a mark opens the next page
/// cleanly rather than splitting.
fn scene_break_def(style: SceneBreakStyle) -> &'static str {
    match style {
        SceneBreakStyle::Ornament => "#let scenebreak = scene-mark\n",
        SceneBreakStyle::Blank => "#let scenebreak = v(1.4em, weak: true)\n",
        SceneBreakStyle::Auto => {
            "#let scenebreak = context {\n  \
             let at-edge = here().position().y <= page-top-margin + 13pt\n  \
             block(width: 100%, breakable: false, {\n    \
             v(0.5em)\n    \
             align(center)[#if at-edge { scene-glyphs } else { hide(scene-glyphs) }]\n    \
             v(0.7em)\n  })\n}\n"
        }
    }
}

/// Front matter: title page, copyright page, optional dedication. No running
/// heads here. Mirrors the EPUB front-matter content and the Verkilo defaults.
fn front_matter(book: &Book) -> String {
    let title = typst_escape(&book.title);
    let display = display_font(book);
    let mut s = String::new();

    s.push_str("#set page(header: none, footer: none)\n\n");

    // Title page.
    s.push_str(&format!(
        "#align(center)[\n  #v(2cm)\n  #text(font: \"{display}\", size: 28pt, weight: \"bold\")[{title}]\n",
        display = display, title = title
    ));
    if let Some(sub) = nonempty(&book.subtitle) {
        s.push_str(&format!(
            "  #v(0.8cm)\n  #text(font: \"{display}\", size: 16pt)[{}]\n",
            typst_escape(sub),
            display = display
        ));
    }
    if let Some(author) = nonempty(&book.author) {
        s.push_str(&format!(
            "  #v(2cm)\n  #text(size: 14pt)[{}]\n",
            typst_escape(author)
        ));
    }
    s.push_str("]\n#pagebreak()\n\n");

    // Copyright page (defaults match epub-filters.lua / Verkilo).
    let copyright = book
        .copyright
        .clone()
        .unwrap_or_else(|| format!("Copyright \u{00A9} {}", book.author.clone().unwrap_or_default()));
    let rights = book.rights.as_deref().unwrap_or(DEFAULT_RIGHTS);
    let disclaimer = book.disclaimer.as_deref().unwrap_or(DEFAULT_DISCLAIMER);
    s.push_str("#set par(first-line-indent: 0pt, justify: false)\n#v(1fr)\n");
    s.push_str(&format!("#text(size: 10pt)[{}]\n#v(0.8em)\n", typst_escape(&copyright)));
    s.push_str(&format!("#text(size: 10pt)[{}]\n#v(0.8em)\n", typst_escape(rights)));
    s.push_str(&format!("#text(size: 10pt)[{}]\n", typst_escape(disclaimer)));
    if let Some(pubr) = nonempty(&book.publisher) {
        s.push_str(&format!("#v(0.8em)\n#text(size: 10pt)[{}]\n", typst_escape(pubr)));
    }
    if let Some(isbn) = nonempty(&book.isbn) {
        s.push_str(&format!("#v(0.8em)\n#text(size: 10pt)[ISBN: {}]\n", typst_escape(isbn)));
    }
    s.push_str("#pagebreak()\n\n");

    // Dedication page.
    if let Some(dedication) = nonempty(&book.dedication) {
        s.push_str(&format!(
            "#align(center + horizon)[#text(style: \"italic\")[{}]]\n#pagebreak()\n\n",
            typst_escape(dedication)
        ));
    }

    s.push_str("#set par(first-line-indent: 1.5em, justify: true)\n");
    s
}

const DEFAULT_RIGHTS: &str = "All rights reserved. No part of this book may be reproduced, distributed, or transmitted in any form or by any means without the prior written permission of the publisher, except for brief quotations embodied in critical reviews.";
const DEFAULT_DISCLAIMER: &str = "This is a work of fiction. Names, characters, places, and incidents either are the product of the author\u{2019}s imagination or are used fictitiously. Any resemblance to actual persons, living or dead, events, or locales is entirely coincidental.";

fn nonempty(field: &Option<String>) -> Option<&str> {
    field.as_deref().map(str::trim).filter(|s| !s.is_empty())
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
        let input = "\u{201C}Hello\u{201D} \u{2014} \u{2018}hi\u{2019}";
        assert_eq!(typst_escape(input), input);
    }

    #[test]
    fn body_drops_leading_rule_and_marks_drop_cap() {
        let md = "---\n\n# One\n\nAlpha body.\n\n# Two\n\nBeta.\n";
        let body = render_body(md);
        assert!(body.contains("= One"));
        assert!(body.contains("#chapter-dropcap[Alpha body.]"));
        // Leading separator before content produced no scenebreak.
        assert!(!body.trim_start().starts_with("#scenebreak"));
    }

    #[test]
    fn scene_break_becomes_scenebreak() {
        let md = "# Ch\n\nOne.\n\n---\n\nTwo.\n";
        let body = render_body(md);
        assert!(body.contains("#scenebreak"));
    }

    #[test]
    fn scene_break_before_heading_is_dropped() {
        let md = "# One\n\nBody.\n\n---\n\n# Two\n\nBody.\n";
        let body = render_body(md);
        assert!(!body.contains("#scenebreak"));
        assert!(body.contains("= One"));
        assert!(body.contains("= Two"));
    }

    #[test]
    fn scene_break_def_per_style() {
        // Auto is page-aware; ornament always marks; blank is always a gap.
        assert!(scene_break_def(SceneBreakStyle::Auto).contains("here().position()"));
        // Auto hides the same ornament mid-page so both branches share a height.
        assert!(scene_break_def(SceneBreakStyle::Auto).contains("hide(scene-glyphs)"));
        assert!(scene_break_def(SceneBreakStyle::Ornament).contains("scene-mark"));
        assert!(scene_break_def(SceneBreakStyle::Ornament).contains("position()") == false);
        assert!(scene_break_def(SceneBreakStyle::Blank).contains("v(1.4em"));
    }

    #[test]
    fn gutter_scales_with_page_count() {
        // The combined KDP + IngramSpark bands.
        assert_eq!(gutter_inside_in(50), 0.5);
        assert_eq!(gutter_inside_in(150), 0.5);
        assert_eq!(gutter_inside_in(151), 0.625);
        assert_eq!(gutter_inside_in(300), 0.625);
        assert_eq!(gutter_inside_in(301), 0.75);
        assert_eq!(gutter_inside_in(500), 0.75);
        assert_eq!(gutter_inside_in(700), 0.875);
        assert_eq!(gutter_inside_in(900), 1.0);
        // Never below IngramSpark's 0.5" floor.
        assert!(gutter_inside_in(1) >= 0.5);
    }

    #[test]
    fn margins_scale_with_trim_and_width() {
        // Trade 6x9: outside range 0.5–0.625; top biased to the upper end (0.875)
        // for the running header; bottom scales with width.
        let mut book = Book {
            trim: Some("trade".into()),
            ..Default::default()
        };
        let normal = resolve_margins(&book, 0.5);
        assert_eq!(normal.outside_in, 0.5625); // midpoint of 0.5–0.625
        assert_eq!(normal.top_in, 0.875); // upper end
        assert_eq!(normal.inside_in, 0.5); // the supplied gutter

        book.margins = Some("narrow".into());
        assert_eq!(resolve_margins(&book, 0.5).outside_in, 0.5); // low end

        book.margins = Some("wide".into());
        assert_eq!(resolve_margins(&book, 0.5).outside_in, 0.625); // high end

        // Pocket is tighter than trade; digest's single-value ranges don't move.
        let pocket = Book { trim: Some("pocket".into()), ..Default::default() };
        assert_eq!(resolve_margins(&pocket, 0.5).outside_in, 0.4375); // mid of 0.375–0.5
        let digest = Book::default();
        assert_eq!(resolve_margins(&digest, 0.5).outside_in, 0.5);
        assert_eq!(resolve_margins(&digest, 0.5).top_in, 0.75);
    }

    #[test]
    fn fonts_from_metadata_with_fallback() {
        // Assumes BOOK_MAINFONT / BOOK_DISPLAYFONT are unset in the test env.
        let mut book = Book::default();
        assert_eq!(body_font(&book), "Libertinus Serif"); // default
        assert_eq!(display_font(&book), "Libertinus Serif"); // falls back to body
        book.body_font = Some("EB Garamond".into());
        assert_eq!(body_font(&book), "EB Garamond");
        assert_eq!(display_font(&book), "EB Garamond"); // display still follows body
        book.display_font = Some("Cinzel".into());
        assert_eq!(display_font(&book), "Cinzel");
    }

    #[test]
    fn chapter_start_directive() {
        let mut book = Book::default();
        assert!(book.chapter_opens_recto()); // default
        assert!(chapter_pagebreak(&book).contains("to: \"odd\""));
        book.chapter_start = Some("any".into());
        assert!(!book.chapter_opens_recto());
        assert_eq!(chapter_pagebreak(&book), "pagebreak(weak: true)");
    }

    #[test]
    fn book_parses_scene_break_setting() {
        let mut book = Book::default();
        assert_eq!(book.scene_break_style(), SceneBreakStyle::Auto);
        book.scenebreak = Some("ornament".into());
        assert_eq!(book.scene_break_style(), SceneBreakStyle::Ornament);
        book.scenebreak = Some("blank".into());
        assert_eq!(book.scene_break_style(), SceneBreakStyle::Blank);
    }

    #[test]
    fn emphasis_and_footnotes() {
        let md = "# Ch\n\n**Bold** and _it_ and a note.[^1]\n\n[^1]: Note body.\n";
        let body = render_body(md);
        assert!(body.contains("#strong[Bold]"));
        assert!(body.contains("#emph[it]"));
        assert!(body.contains("#footnote[Note body.]"));
    }
}
