//! EPUB back end, part 1: the comrak AST -> per-chapter XHTML.
//!
//! "format_html is free" is ~95% true. comrak's own renderer produces the body;
//! we add exactly what `_templates/epub-filters.lua` adds and no more:
//!   * leading-separator strip (drop Longform's `---` before any content),
//!   * the `chapter-title` class on each chapter's H1,
//!   * a drop-cap `<span>` on the first paragraph after an H1 (only when it
//!     begins with a plain text run — mirroring the lua's "Str first" guard).
//!
//! Footnotes ride along: comrak renders a `<section class="footnotes">` at the
//! end of whatever subtree it formats. Because we split per chapter, each
//! chapter's definitions render as endnotes within that chapter's file.

use comrak::nodes::{AstNode, NodeValue};
use comrak::{format_html, parse_document, Arena, Options};

use crate::ingest::{Book, SceneBreakStyle};

/// A rendered body chapter: its plain-text title (for the ToC) and full XHTML.
#[derive(Debug, Clone)]
pub struct Chapter {
    pub title: String,
    pub xhtml: String,
}

/// The three synthesized front-matter pages. `dedication` is omitted when the
/// metadata carries none.
#[derive(Debug, Clone)]
pub struct FrontMatter {
    pub title_page: String,
    pub copyright_page: String,
    pub dedication_page: Option<String>,
}

/// Default rights / disclaimer strings, lifted verbatim from
/// `_templates/epub-filters.lua` so the EPUB stays byte-faithful to today's.
const DEFAULT_RIGHTS: &str = "All rights reserved. No part of this book may be reproduced, distributed, or transmitted in any form or by any means without the prior written permission of the publisher, except for brief quotations embodied in critical reviews.";
const DEFAULT_DISCLAIMER: &str = "This is a work of fiction. Names, characters, places, and incidents either are the product of the author\u{2019}s imagination or are used fictitiously. Any resemblance to actual persons, living or dead, events, or locales is entirely coincidental.";

/// Parse the (already strip-fm'd) manuscript once and render it to body
/// chapters. The single parse here is the shared front end for the EPUB path.
/// `scene_break` controls how in-chapter `---` rules are rendered.
pub fn render_chapters(markdown: &str, scene_break: SceneBreakStyle) -> Vec<Chapter> {
    // EPUB render needs raw HTML passthrough: our injected drop-cap span is an
    // inline HTML node, and authored HTML should survive like it does in pandoc.
    let mut opts: Options = crate::parse::options();
    opts.render.r#unsafe = true;

    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &opts);

    let chapters = split_into_chapters(&arena, root);

    chapters
        .into_iter()
        .map(|chap| {
            let title = chapter_heading_text(chap);
            apply_drop_cap(&arena, chap);
            apply_runin_headers(&arena, chap);
            let body = format_node(chap, &opts);
            let body = add_chapter_title_class(&body);
            let body = apply_scene_break_style(&body, scene_break);
            let xhtml = xhtml_doc(&title, &body);
            Chapter { title, xhtml }
        })
        .collect()
}

/// Split the document's top-level blocks into chapters at each H1, dropping any
/// `---` rules that appear before the first real content (Longform leftovers).
/// Scene-break `---` *inside* content is kept (comrak renders it as `<hr />`).
fn split_into_chapters<'a>(
    arena: &'a Arena<'a>,
    root: &'a AstNode<'a>,
) -> Vec<&'a AstNode<'a>> {
    let mut chapters: Vec<&'a AstNode<'a>> = Vec::new();
    let mut current: Option<&'a AstNode<'a>> = None;
    let mut content_started = false;

    // Collect first: we mutate the tree (detach) as we go.
    let kids: Vec<&'a AstNode<'a>> = root.children().collect();
    for (i, node) in kids.iter().enumerate() {
        let node: &'a AstNode<'a> = node;
        let value = node.data.borrow().value.clone();

        // Drop a scene-break rule that leads the content or sits directly before
        // a heading (a Longform scene-separator ahead of the next chapter): no
        // stray rule stranded at the foot of a chapter.
        if matches!(value, NodeValue::ThematicBreak) {
            let before_heading = kids
                .get(i + 1)
                .is_some_and(|n| matches!(&n.data.borrow().value, NodeValue::Heading(_)));
            if !content_started || before_heading {
                node.detach();
                continue;
            }
        }

        let is_chapter_head = matches!(&value, NodeValue::Heading(h) if h.level == 1);
        content_started = true;
        node.detach();

        if is_chapter_head || current.is_none() {
            let chap = new_document(arena);
            chap.append(node);
            chapters.push(chap);
            current = Some(chap);
        } else if let Some(chap) = current {
            chap.append(node);
        }
    }

    chapters
}

/// Wrap the first paragraph after an H1 in a drop cap, mirroring
/// `epub-filters.lua`'s `make_dropcap`: only the block immediately following the
/// H1, and only when its first inline is plain text.
fn apply_drop_cap<'a>(arena: &'a Arena<'a>, chapter: &'a AstNode<'a>) {
    let mut pending = false;
    for child in chapter.children() {
        let value = child.data.borrow().value.clone();
        let is_h1 = matches!(&value, NodeValue::Heading(h) if h.level == 1);
        if is_h1 {
            pending = true;
            continue;
        }
        if pending {
            if matches!(value, NodeValue::Paragraph) {
                drop_cap_paragraph(arena, child);
            }
            break;
        }
    }
}

fn drop_cap_paragraph<'a>(arena: &'a Arena<'a>, para: &'a AstNode<'a>) {
    let Some(first) = para.first_child() else {
        return;
    };
    let mut data = first.data.borrow_mut();
    let NodeValue::Text(text) = &data.value else {
        return; // "Str first" guard — leave non-text openings alone.
    };
    let mut chars = text.chars();
    let Some(initial) = chars.next() else {
        return;
    };
    let rest: String = chars.collect();
    let span = format!(
        "<span class=\"dropcap\">{}</span>",
        esc(&initial.to_string())
    );
    data.value = NodeValue::Text(rest.into());
    drop(data);
    let span_node = arena.alloc(AstNode::from(NodeValue::HtmlInline(span)));
    first.insert_before(span_node);
}

/// Turn each chapter-level H4 into a run-in (paragraph) heading, mirroring the
/// PDF `#runin-head`: the heading's inline content becomes a bold
/// `<span class="runin">` lead-in merged into the front of the following
/// paragraph (the `.runin` class supplies the trailing period + spacing via CSS,
/// so both outputs match). When no paragraph follows, the H4 is retyped to a
/// standalone run-in paragraph. Only direct children are visited, so an H4 nested
/// in a blockquote keeps comrak's default `<h4>`.
fn apply_runin_headers<'a>(arena: &'a Arena<'a>, chapter: &'a AstNode<'a>) {
    let kids: Vec<&'a AstNode<'a>> = chapter.children().collect();
    for (i, node) in kids.iter().enumerate() {
        let is_h4 = matches!(&node.data.borrow().value, NodeValue::Heading(h) if h.level == 4);
        if !is_h4 {
            continue;
        }
        let into_para = kids
            .get(i + 1)
            .filter(|n| matches!(&n.data.borrow().value, NodeValue::Paragraph));
        match into_para {
            Some(para) => run_in_into(arena, node, para),
            None => run_in_standalone(arena, node),
        }
    }
}

/// Move an H4's inline children, wrapped in `<span class="runin">`, to the front
/// of `para`, then detach the (now empty) heading.
fn run_in_into<'a>(arena: &'a Arena<'a>, heading: &'a AstNode<'a>, para: &'a AstNode<'a>) {
    let (open, close) = runin_span(arena);
    match para.first_child() {
        Some(first) => first.insert_before(open),
        None => para.append(open),
    }
    open.insert_after(close); // children slot in between, in order
    for child in heading.children().collect::<Vec<_>>() {
        child.detach();
        close.insert_before(child);
    }
    heading.detach();
}

/// Wrap an H4's own inline children in `<span class="runin">` and retype the node
/// to a paragraph (used when no paragraph follows the head).
fn run_in_standalone<'a>(arena: &'a Arena<'a>, heading: &'a AstNode<'a>) {
    let (open, close) = runin_span(arena);
    match heading.first_child() {
        Some(first) => first.insert_before(open),
        None => heading.append(open),
    }
    heading.append(close);
    heading.data.borrow_mut().value = NodeValue::Paragraph;
}

/// A fresh `<span class="runin">` open/close inline-HTML node pair.
fn runin_span<'a>(arena: &'a Arena<'a>) -> (&'a AstNode<'a>, &'a AstNode<'a>) {
    let open = arena.alloc(AstNode::from(NodeValue::HtmlInline(
        "<span class=\"runin\">".to_string(),
    )));
    let close = arena.alloc(AstNode::from(NodeValue::HtmlInline("</span>".to_string())));
    (open, close)
}

/// Synthesize the front-matter pages from the book metadata. Mirrors
/// `epub-filters.lua`: a title page, a copyright page (with the default rights /
/// disclaimer when unset), and an optional dedication.
pub fn front_matter(book: &Book) -> FrontMatter {
    FrontMatter {
        title_page: title_page(book),
        copyright_page: copyright_page(book),
        dedication_page: book
            .dedication
            .as_deref()
            .filter(|d| !d.trim().is_empty())
            .map(dedication_page),
    }
}

fn title_page(book: &Book) -> String {
    let mut body = String::from("<section epub:type=\"titlepage\" class=\"titlepage\">\n");
    body.push_str(&format!("<h1 class=\"title\">{}</h1>\n", esc(&book.title)));
    if let Some(sub) = opt(&book.subtitle) {
        body.push_str(&format!("<p class=\"subtitle\">{}</p>\n", esc(sub)));
    }
    if let Some(author) = opt(&book.author) {
        body.push_str(&format!("<p class=\"author\">{}</p>\n", esc(author)));
    }
    if let Some(pubr) = opt(&book.publisher) {
        body.push_str(&format!("<p class=\"publisher\">{}</p>\n", esc(pubr)));
    }
    body.push_str("</section>\n");
    xhtml_doc(&book.title, &body)
}

fn copyright_page(book: &Book) -> String {
    let copyright = book.copyright.clone().unwrap_or_else(|| {
        format!("Copyright \u{00A9} {}", book.author.clone().unwrap_or_default())
    });
    let rights = book.rights.as_deref().unwrap_or(DEFAULT_RIGHTS);
    let disclaimer = book.disclaimer.as_deref().unwrap_or(DEFAULT_DISCLAIMER);

    let mut body =
        String::from("<section epub:type=\"copyright-page\" class=\"copyright-page\">\n");
    body.push_str(&format!("<p>{}</p>\n", esc(&copyright)));
    body.push_str(&format!("<p>{}</p>\n", esc(rights)));
    body.push_str(&format!("<p>{}</p>\n", esc(disclaimer)));
    if let Some(pubr) = opt(&book.publisher) {
        body.push_str(&format!("<p>{}</p>\n", esc(pubr)));
    }
    if let Some(edition) = opt(&book.edition) {
        body.push_str(&format!("<p>{}</p>\n", esc(edition)));
    }
    if let Some(isbn) = opt(&book.isbn) {
        body.push_str(&format!("<p>ISBN: {}</p>\n", esc(isbn)));
    }
    body.push_str("</section>\n");
    xhtml_doc("Copyright", &body)
}

/// The full-page cover XHTML, shown as the first page when a cover is embedded.
/// References the embedded `cover.jpg`; styled by `epub.css` (`body#cover` /
/// `#cover-image`).
pub fn cover_page() -> String {
    let body = "<div id=\"cover-image\"><img src=\"cover.jpg\" alt=\"Cover\"/></div>\n";
    xhtml_body("Cover", "cover", body)
}

fn dedication_page(dedication: &str) -> String {
    let body = format!(
        "<section epub:type=\"dedication\" class=\"dedication\">\n<p>{}</p>\n</section>\n",
        esc(dedication)
    );
    xhtml_doc("Dedication", &body)
}

// --- comrak helpers ---------------------------------------------------------

/// Create a fresh `Document` node to act as a chapter container.
fn new_document<'a>(arena: &'a Arena<'a>) -> &'a AstNode<'a> {
    arena.alloc(AstNode::from(NodeValue::Document))
}

/// Render a single node (and its subtree) to an HTML body fragment.
fn format_node<'a>(node: &'a AstNode<'a>, opts: &Options) -> String {
    // comrak's html formatter writes to a `fmt::Write` (a String) and is
    // infallible against one.
    let mut out = String::new();
    format_html(node, opts, &mut out).expect("comrak html formatting is infallible to a String");
    out
}

/// The plain text of a chapter's leading H1 (its ToC title). Empty when the
/// chapter has no heading (pre-H1 content).
fn chapter_heading_text<'a>(chapter: &'a AstNode<'a>) -> String {
    for child in chapter.children() {
        if matches!(&child.data.borrow().value, NodeValue::Heading(h) if h.level == 1) {
            return node_text(child);
        }
    }
    String::new()
}

/// Collect the plain text of a node's descendants (for ToC titles).
fn node_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut out = String::new();
    collect_text(node, &mut out);
    out.trim().to_string()
}

fn collect_text<'a>(node: &'a AstNode<'a>, out: &mut String) {
    match &node.data.borrow().value {
        NodeValue::Text(t) => out.push_str(t),
        NodeValue::Code(code) => out.push_str(&code.literal),
        _ => {
            for child in node.children() {
                collect_text(child, out);
            }
        }
    }
}

// --- string helpers ---------------------------------------------------------

/// Add `class="chapter-title"` to a chapter's leading `<h1>` (comrak renders a
/// bare `<h1>`; each chapter fragment begins with its heading).
fn add_chapter_title_class(html: &str) -> String {
    html.replacen("<h1>", "<h1 class=\"chapter-title\">", 1)
}

/// Apply the configured scene-break style to comrak's `<hr />` (its rendering of
/// a thematic break). `Auto` keeps the styled rule; the others swap it for the
/// ornament or a plain gap (styled by `epub.css`).
fn apply_scene_break_style(html: &str, style: SceneBreakStyle) -> String {
    let replacement = match style {
        SceneBreakStyle::Auto => return html.to_string(),
        SceneBreakStyle::Ornament => "<p class=\"scene-break\">* * *</p>",
        SceneBreakStyle::Blank => "<p class=\"scene-break scene-blank\"></p>",
    };
    html.replace("<hr />", replacement)
}

/// Wrap an HTML body fragment in a minimal XHTML document linking the stylesheet.
fn xhtml_doc(title: &str, body: &str) -> String {
    xhtml_body(title, "", body)
}

/// As [`xhtml_doc`], but with an `id` on the `<body>` (e.g. `cover`).
fn xhtml_body(title: &str, body_id: &str, body: &str) -> String {
    let id_attr = if body_id.is_empty() {
        String::new()
    } else {
        format!(" id=\"{body_id}\"")
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <!DOCTYPE html>\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" lang=\"en\">\n\
         <head>\n\
         <meta charset=\"utf-8\"/>\n\
         <title>{}</title>\n\
         <link rel=\"stylesheet\" type=\"text/css\" href=\"stylesheet.css\"/>\n\
         </head>\n\
         <body{}>\n{}</body>\n\
         </html>\n",
        esc(title), id_attr, body
    )
}

/// Minimal HTML text escaping for injected metadata.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `Some(&str)` only when present and non-empty after trimming.
fn opt(field: &Option<String>) -> Option<&str> {
    field.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_at_h1_and_drops_leading_rule() {
        let md = "---\n\n# One\n\nAlpha body.\n\n# Two\n\nBeta body.\n";
        let chapters = render_chapters(md, SceneBreakStyle::Auto);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "One");
        assert_eq!(chapters[1].title, "Two");
        // Leading `---` dropped: no <hr before the first heading.
        assert!(!chapters[0].xhtml.contains("<hr"));
        // chapter-title class applied.
        assert!(chapters[0].xhtml.contains("<h1 class=\"chapter-title\">"));
    }

    #[test]
    fn drop_cap_wraps_first_letter() {
        let md = "# Ch\n\nJake watched.\n";
        let chapters = render_chapters(md, SceneBreakStyle::Auto);
        assert!(chapters[0]
            .xhtml
            .contains("<span class=\"dropcap\">J</span>ake watched."));
    }

    #[test]
    fn scene_break_rule_is_kept() {
        let md = "# Ch\n\nOne.\n\n---\n\nTwo.\n";
        let chapters = render_chapters(md, SceneBreakStyle::Auto);
        assert!(chapters[0].xhtml.contains("<hr"));
    }

    #[test]
    fn scene_break_before_heading_is_dropped() {
        // Longform's scene-separator ahead of the next chapter must not leave a
        // trailing rule at the foot of the previous chapter.
        let md = "# One\n\nBody.\n\n---\n\n# Two\n\nBody.\n";
        let chapters = render_chapters(md, SceneBreakStyle::Auto);
        assert_eq!(chapters.len(), 2);
        assert!(!chapters[0].xhtml.contains("<hr"));
    }

    #[test]
    fn scene_break_style_controls_the_rule() {
        let md = "# Ch\n\nOne.\n\n---\n\nTwo.\n";
        let auto = &render_chapters(md, SceneBreakStyle::Auto)[0].xhtml;
        assert!(auto.contains("<hr") && !auto.contains("scene-break"));

        let ornament = &render_chapters(md, SceneBreakStyle::Ornament)[0].xhtml;
        assert!(ornament.contains("<p class=\"scene-break\">* * *</p>") && !ornament.contains("<hr"));

        let blank = &render_chapters(md, SceneBreakStyle::Blank)[0].xhtml;
        assert!(blank.contains("scene-blank") && !blank.contains("<hr"));
    }

    #[test]
    fn h4_becomes_runin_merged_into_paragraph() {
        let md = "# Ch\n\n#### Background\n\nThe subsection body.\n";
        let xhtml = &render_chapters(md, SceneBreakStyle::Auto)[0].xhtml;
        assert!(xhtml.contains("<span class=\"runin\">Background</span>"));
        // Merged into the following paragraph — no standalone <h4>.
        assert!(!xhtml.contains("<h4"));
        assert!(xhtml.contains("The subsection body."));
    }

    #[test]
    fn h4_standalone_when_no_paragraph_follows() {
        let md = "# Ch\n\n#### Background\n";
        let xhtml = &render_chapters(md, SceneBreakStyle::Auto)[0].xhtml;
        assert!(xhtml.contains("<span class=\"runin\">Background</span>"));
        assert!(!xhtml.contains("<h4"));
    }

    #[test]
    fn footnotes_render_as_a_section() {
        let md = "# Ch\n\nText with a note.[^1]\n\n[^1]: The note body.\n";
        let chapters = render_chapters(md, SceneBreakStyle::Auto);
        assert!(chapters[0].xhtml.contains("class=\"footnotes\""));
    }

    #[test]
    fn cover_page_references_the_embedded_image() {
        let html = cover_page();
        assert!(html.contains("<body id=\"cover\">"));
        assert!(html.contains("src=\"cover.jpg\""));
        assert!(html.contains("stylesheet.css"));
    }

    #[test]
    fn front_matter_uses_defaults() {
        let book = Book {
            title: "T".into(),
            author: Some("A".into()),
            ..Default::default()
        };
        let fm = front_matter(&book);
        assert!(fm.title_page.contains("<h1 class=\"title\">T</h1>"));
        assert!(fm.copyright_page.contains("Copyright \u{00A9} A"));
        assert!(fm.copyright_page.contains("All rights reserved"));
        assert!(fm.dedication_page.is_none());
    }
}
