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

use crate::ingest::Book;

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
pub fn render_chapters(markdown: &str) -> Vec<Chapter> {
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
            let body = format_node(chap, &opts);
            let body = add_chapter_title_class(&body);
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
    for node in kids {
        let value = node.data.borrow().value.clone();

        // Leading separators before any content are dropped.
        if matches!(value, NodeValue::ThematicBreak) && !content_started {
            node.detach();
            continue;
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

/// Wrap an HTML body fragment in a minimal XHTML document linking the stylesheet.
fn xhtml_doc(title: &str, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <!DOCTYPE html>\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" lang=\"en\">\n\
         <head>\n\
         <meta charset=\"utf-8\"/>\n\
         <title>{}</title>\n\
         <link rel=\"stylesheet\" type=\"text/css\" href=\"stylesheet.css\"/>\n\
         </head>\n\
         <body>\n{}</body>\n\
         </html>\n",
        esc(title), body
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
        let chapters = render_chapters(md);
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
        let chapters = render_chapters(md);
        assert!(chapters[0]
            .xhtml
            .contains("<span class=\"dropcap\">J</span>ake watched."));
    }

    #[test]
    fn scene_break_rule_is_kept() {
        let md = "# Ch\n\nOne.\n\n---\n\nTwo.\n";
        let chapters = render_chapters(md);
        assert!(chapters[0].xhtml.contains("<hr"));
    }

    #[test]
    fn footnotes_render_as_a_section() {
        let md = "# Ch\n\nText with a note.[^1]\n\n[^1]: The note body.\n";
        let chapters = render_chapters(md);
        assert!(chapters[0].xhtml.contains("class=\"footnotes\""));
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
