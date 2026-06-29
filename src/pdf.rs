//! PDF back end, part 2: typeset Typst markup to PDF via the `typst` CLI.
//!
//! Option **C′** from the spec: render the document to a `.typ` (next to the
//! output, so `typst`'s relative paths and our `--root` resolve predictably),
//! then shell to `typst compile`. Light build, no version-locked crate. The
//! C-vs-C′ (typst-as-library) decision stays deferred — only this invocation
//! would change.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::ingest::{Book, Cover};
use crate::render_typst::{self, gutter_inside_in};

/// Build a PDF from a strip-fm'd manuscript and its metadata.
///
/// `meta_dir` is the directory of `_metadata.yml`; it becomes typst's `--root`
/// so image paths resolve the same way the EPUB cover does. The intermediate
/// `.typ` is written beside `output` while compiling, then removed on success
/// (kept on failure so the error can point at the generated markup; use the
/// `typst` subcommand to emit it deliberately).
pub fn build(
    book: &Book,
    _cover: &Cover,
    markdown: &str,
    meta_dir: Option<&Path>,
    output: &Path,
) -> Result<()> {
    let typ_path = output.with_extension("typ");
    let root = project_root(&typ_path, meta_dir);
    let fonts = font_path(book, meta_dir);
    announce_fonts(fonts.as_deref());

    fit_gutter_and_write(book, markdown, &typ_path, &root, fonts.as_deref())?;

    let mut cmd = Command::new("typst");
    cmd.arg("compile").arg("--root").arg(&root);
    if let Some(p) = &fonts {
        cmd.arg("--font-path").arg(p);
    }
    cmd.arg(&typ_path).arg(output);
    let status = cmd.status().context(
        "running `typst` — is it installed and on PATH? (the PDF path uses the typst CLI)",
    )?;

    if !status.success() {
        bail!(
            "typst failed to compile {} (the generated markup is kept at {} for inspection)",
            typ_path.display(),
            typ_path.display()
        );
    }

    // Success: the intermediate markup is no longer needed. (`argraver typst`
    // is the way to keep it.)
    let _ = std::fs::remove_file(&typ_path);

    Ok(())
}

/// Render the Typst markup (with the fitted gutter) to `output` and keep it —
/// the `typst` subcommand. No PDF is produced.
pub fn write_typst(
    book: &Book,
    markdown: &str,
    meta_dir: Option<&Path>,
    output: &Path,
) -> Result<()> {
    let root = project_root(output, meta_dir);
    let fonts = font_path(book, meta_dir);
    announce_fonts(fonts.as_deref());
    fit_gutter_and_write(book, markdown, output, &root, fonts.as_deref())
}

/// Fit the inside (gutter) margin to the page count and write the settled `.typ`.
///
/// The gutter widens the text block, which can change the page count, so iterate
/// until the count's gutter band stops changing — monotonic, so it settles in a
/// couple of passes. Outside/top/bottom derive from the trim and don't change.
fn fit_gutter_and_write(
    book: &Book,
    markdown: &str,
    typ_path: &Path,
    root: &Path,
    fonts: Option<&Path>,
) -> Result<()> {
    let mut inside = gutter_inside_in(0); // start at the thinnest band
    let mut pages = 0;
    for _ in 0..5 {
        write_typ(typ_path, &render_typst::render_document(book, markdown, inside))?;
        pages = query_page_count(typ_path, root, fonts)?;
        let next = gutter_inside_in(pages);
        if (next - inside).abs() < 1e-9 {
            break;
        }
        inside = next;
    }
    // Final render with the settled gutter (covers a non-converging exit too).
    write_typ(typ_path, &render_typst::render_document(book, markdown, inside))?;
    let m = render_typst::resolve_margins(book, inside);
    println!(
        "  {pages} pages → margins (in): inside {:.3} / outside {:.3} / top {:.3} / bottom {:.3}",
        m.inside_in, m.outside_in, m.top_in, m.bottom_in
    );
    Ok(())
}

fn announce_fonts(fonts: Option<&Path>) {
    if let Some(p) = fonts {
        println!("  fonts: {}", p.display());
    }
}

fn write_typ(typ_path: &Path, document: &str) -> Result<()> {
    std::fs::write(typ_path, document).with_context(|| format!("writing {}", typ_path.display()))
}

/// Resolve the custom font directory: env `BOOK_FONTPATH` overrides
/// `book.font_path`; a relative path resolves against the metadata directory.
fn font_path(book: &Book, meta_dir: Option<&Path>) -> Option<std::path::PathBuf> {
    let raw = std::env::var("BOOK_FONTPATH")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| book.font_path.clone())
        .filter(|s| !s.trim().is_empty())?;
    let p = Path::new(raw.trim());
    Some(if p.is_absolute() {
        p.to_path_buf()
    } else {
        meta_dir.unwrap_or_else(|| Path::new(".")).join(p)
    })
}

/// Query the physical page count from the document's `<pagecount>` metadata.
/// (`typst query` compiles internally; this is the cost of fitting the gutter.)
fn query_page_count(typ_path: &Path, root: &Path, fonts: Option<&Path>) -> Result<u32> {
    let mut cmd = Command::new("typst");
    cmd.arg("query").arg("--root").arg(root);
    if let Some(p) = fonts {
        cmd.arg("--font-path").arg(p);
    }
    let out = cmd
        .arg("--field")
        .arg("value")
        .arg("--one")
        .arg(typ_path)
        .arg("<pagecount>")
        .output()
        .context("running `typst query` — is typst installed and on PATH?")?;
    if !out.status.success() {
        bail!(
            "typst query failed for {}:\n{}",
            typ_path.display(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .trim()
        .trim_matches('"')
        .parse::<u32>()
        .with_context(|| format!("parsing page count from typst query output: {stdout:?}"))
}

/// The typst `--root`: a directory containing both the generated `.typ` and the
/// metadata dir (so the source is under root and images resolve). Falls back to
/// the `.typ`'s own directory when there is no metadata or no shared ancestor.
fn project_root(typ_path: &Path, meta_dir: Option<&Path>) -> std::path::PathBuf {
    let typ_dir = typ_path
        .parent()
        .map(abs)
        .unwrap_or_else(|| Path::new(".").to_path_buf());
    let Some(meta_dir) = meta_dir else {
        return typ_dir;
    };
    common_ancestor(&typ_dir, &abs(meta_dir)).unwrap_or(typ_dir)
}

/// Absolutize a path against the cwd without requiring it to exist.
fn abs(p: &Path) -> std::path::PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    }
}

/// Longest shared directory prefix of two absolute paths.
fn common_ancestor(a: &Path, b: &Path) -> Option<std::path::PathBuf> {
    let mut shared = std::path::PathBuf::new();
    for (ca, cb) in a.components().zip(b.components()) {
        if ca == cb {
            shared.push(ca);
        } else {
            break;
        }
    }
    (shared.components().count() > 0).then_some(shared)
}
