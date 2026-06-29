//! EPUB back end, part 2: package rendered XHTML into a valid `.epub`.
//!
//! Assembles `epub-builder` over: a resized cover, the synthesized front matter
//! (title / copyright / dedication), an inline ToC, and the body chapters. The
//! stylesheet is the ported `epub.css`, bundled into the binary.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use epub_builder::{EpubBuilder, EpubContent, EpubVersion, ReferenceType, ZipLibrary};

use crate::ingest::{Book, Cover};
use crate::render_html::{self, FrontMatter};

/// The ported EPUB stylesheet (verbatim from the vault's `_templates/epub.css`).
const EPUB_CSS: &str = include_str!("resources/epub.css");

/// Build an EPUB from a strip-fm'd manuscript and its metadata, writing it to
/// `output`. `meta_dir` is the directory of the `_metadata.yml` (used to resolve
/// a relative/vault-absolute cover path); pass `None` when there is no metadata.
pub fn build(
    book: &Book,
    cover: &Cover,
    markdown: &str,
    meta_dir: Option<&Path>,
    output: &Path,
) -> Result<()> {
    let chapters = render_html::render_chapters(markdown, book.scene_break_style());
    let front = render_html::front_matter(book);

    let mut builder = EpubBuilder::new(ZipLibrary::new().context("init epub zip library")?)
        .context("init epub builder")?;

    // EPUB 3 to match the pandoc epub3 baseline and our `epub:type` semantics.
    builder.epub_version(EpubVersion::V30);
    builder
        .metadata("title", &book.title)?
        .metadata("generator", "argraver")?;
    if let Some(author) = nonempty(&book.author) {
        builder.metadata("author", author)?;
    }
    builder.metadata("lang", book.lang.as_deref().unwrap_or("en"))?;
    if let Some(desc) = book.description.as_deref().map(collapse_ws) {
        if !desc.is_empty() {
            builder.metadata("description", desc)?;
        }
    }

    builder.stylesheet(EPUB_CSS.as_bytes())?;

    // Cover (resize to a fitted JPEG). Missing covers are a warning, not an
    // error — matching the bash `book`.
    let mut cover_embedded = false;
    if let Some(raw) = cover.background.image.as_deref() {
        match resolve_cover_path(raw, meta_dir) {
            Some(path) => {
                let jpeg = resize_cover(&path)
                    .with_context(|| format!("resizing cover {}", path.display()))?;
                builder.add_cover_image("cover.jpg", &jpeg[..], "image/jpeg")?;
                cover_embedded = true;
            }
            None => eprintln!("  cover not found, skipping: {raw}"),
        }
    }

    // A full-page cover as the first spine item, so the book *opens* on the cover
    // (not just a library thumbnail). `add_cover_image` only registers the image.
    if cover_embedded {
        let cover_page = render_html::cover_page();
        builder.add_content(
            EpubContent::new("cover.xhtml", cover_page.as_bytes())
                .reftype(ReferenceType::Cover),
        )?;
    }

    add_front_matter(&mut builder, &front)?;

    // No `inline_toc()`: EPUB 3 readers render their own navigation from
    // `nav.xhtml` (+ `toc.ncx` for EPUB 2 compat). Adding an inline ToC page too
    // shows the reader a duplicate table of contents.

    let width = chapters.len().to_string().len();
    for (i, chapter) in chapters.iter().enumerate() {
        let filename = format!("ch_{:0width$}.xhtml", i + 1, width = width.max(2));
        builder.add_content(
            EpubContent::new(filename, chapter.xhtml.as_bytes())
                .title(&chapter.title)
                .reftype(ReferenceType::Text),
        )?;
    }

    let file = std::fs::File::create(output)
        .with_context(|| format!("creating {}", output.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    builder.generate(&mut writer).context("generating epub")?;
    writer.flush().ok();

    Ok(())
}

fn add_front_matter(
    builder: &mut EpubBuilder<ZipLibrary>,
    front: &FrontMatter,
) -> Result<()> {
    builder.add_content(
        EpubContent::new("title.xhtml", front.title_page.as_bytes())
            .title("Title Page")
            .reftype(ReferenceType::TitlePage),
    )?;
    builder.add_content(
        EpubContent::new("copyright.xhtml", front.copyright_page.as_bytes())
            .title("Copyright")
            .reftype(ReferenceType::Copyright),
    )?;
    if let Some(dedication) = &front.dedication_page {
        builder.add_content(
            EpubContent::new("dedication.xhtml", dedication.as_bytes())
                .title("Dedication")
                .reftype(ReferenceType::Text),
        )?;
    }
    Ok(())
}

/// Resize a master cover image to fit within 1600×2560, re-encoded as JPEG q92.
/// The re-encode drops EXIF (a free "strip").
fn resize_cover(path: &Path) -> Result<Vec<u8>> {
    let img = image::open(path).with_context(|| format!("opening {}", path.display()))?;
    let resized = img.resize(1600, 2560, image::imageops::FilterType::Lanczos3);
    let rgb = resized.to_rgb8();
    let (w, h) = rgb.dimensions();

    let mut buf = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 92);
    encoder
        .encode(rgb.as_raw(), w, h, image::ExtendedColorType::Rgb8)
        .context("encoding cover jpeg")?;
    Ok(buf)
}

/// Resolve a cover path that may be plain-relative or vault-absolute
/// (`/Series/_assets/…`). Tries, in order: the path as given; relative to the
/// metadata directory; and — for vault-absolute paths — relative to a vault root
/// found by walking up to a directory containing `_meta` or `.obsidian`.
fn resolve_cover_path(raw: &str, meta_dir: Option<&Path>) -> Option<PathBuf> {
    let as_given = PathBuf::from(raw);
    if as_given.is_file() {
        return Some(as_given);
    }

    let meta_dir = meta_dir?;
    let rel = raw.trim_start_matches('/');

    let beside = meta_dir.join(rel);
    if beside.is_file() {
        return Some(beside);
    }

    if raw.starts_with('/') {
        let mut dir = Some(meta_dir);
        while let Some(d) = dir {
            if d.join("_meta").is_dir() || d.join(".obsidian").is_dir() {
                let cand = d.join(rel);
                if cand.is_file() {
                    return Some(cand);
                }
            }
            dir = d.parent();
        }
    }

    None
}

fn nonempty(field: &Option<String>) -> Option<&str> {
    field.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// Collapse all runs of whitespace (incl. newlines) to single spaces, for the
/// OPF `dc:description` which wants a plain string.
fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
