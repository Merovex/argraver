//! The `argraver` command line: `epub | pdf | all <manuscript.md> [out]`.
//!
//! Mirrors the bash `book` UX — project name from the manuscript filename,
//! `_argraver.yml` auto-discovered (or `BOOK_META`), output defaulting beside
//! the input.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::{epub, ingest, pdf};

#[derive(Parser)]
#[command(
    name = "argraver",
    version,
    about = "Single-source book compiler: one manuscript + _argraver.yml -> EPUB and PDF"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write a starter `_argraver.yml` into a directory.
    Init(InitArgs),
    /// Build an EPUB.
    Epub(BuildArgs),
    /// Build a print PDF.
    Pdf(BuildArgs),
    /// Build both EPUB and PDF.
    All(BuildArgs),
    /// Emit the generated Typst markup (`.typ`) without compiling a PDF.
    Typst(BuildArgs),
}

#[derive(clap::Args)]
struct InitArgs {
    /// Directory to write `_argraver.yml` into (default: current directory).
    dir: Option<PathBuf>,
    /// Overwrite an existing `_argraver.yml`.
    #[arg(short, long)]
    force: bool,
}

#[derive(clap::Args)]
struct BuildArgs {
    /// The Longform-compiled manuscript, e.g. "<Project> - manuscript.md".
    manuscript: PathBuf,
    /// Output path. Defaults to the manuscript path with the format's extension.
    output: Option<PathBuf>,
    /// Path to `_argraver.yml`. Overrides auto-discovery and `BOOK_META`.
    #[arg(short = 'm', long = "meta", value_name = "FILE")]
    meta: Option<PathBuf>,
}

/// Parse args and run.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init(args) => init(&args),
        Command::Epub(args) => {
            let out = output_for(&args, Target::Epub);
            build(&args, Target::Epub, &out)
        }
        Command::Pdf(args) => {
            let out = output_for(&args, Target::Pdf);
            build(&args, Target::Pdf, &out)
        }
        Command::Typst(args) => {
            let out = output_for(&args, Target::Typst);
            build(&args, Target::Typst, &out)
        }
        // `all`: treat any explicit output as a stem and give each format its own
        // extension (so `out` -> out.epub + out.pdf).
        Command::All(args) => {
            let base = args.output.clone().unwrap_or_else(|| args.manuscript.clone());
            build(&args, Target::Epub, &base.with_extension("epub"))?;
            build(&args, Target::Pdf, &base.with_extension("pdf"))
        }
    }
}

/// Write a starter `_argraver.yml` into the target directory.
fn init(args: &InitArgs) -> Result<()> {
    let dir = args.dir.clone().unwrap_or_else(|| PathBuf::from("."));
    let path = dir.join("_argraver.yml");
    if path.exists() && !args.force {
        anyhow::bail!(
            "{} already exists (use --force to overwrite)",
            path.display()
        );
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&path, ingest::METADATA_TEMPLATE)
        .with_context(|| format!("writing {}", path.display()))?;
    println!("Wrote {}", path.display());
    Ok(())
}

/// The output path for a single-format build: the explicit one, else the
/// manuscript path with the format's extension.
fn output_for(args: &BuildArgs, target: Target) -> PathBuf {
    args.output
        .clone()
        .unwrap_or_else(|| args.manuscript.with_extension(target.ext()))
}

#[derive(Clone, Copy)]
enum Target {
    Epub,
    Pdf,
    Typst,
}

impl Target {
    fn ext(self) -> &'static str {
        match self {
            Target::Epub => "epub",
            Target::Pdf => "pdf",
            Target::Typst => "typ",
        }
    }
}

fn build(args: &BuildArgs, target: Target, output: &Path) -> Result<()> {
    let manuscript = &args.manuscript;

    // Explicit `--meta` wins; otherwise discover (BOOK_META > Books/*/<Project> >
    // beside the manuscript).
    let meta_path = match &args.meta {
        Some(p) => Some(p.clone()),
        None => ingest::find_meta(manuscript),
    };
    let (metadata, meta_dir) = match &meta_path {
        Some(p) => {
            let m = ingest::load_metadata(p)?;
            let dir = p.parent().map(Path::to_path_buf);
            (m, dir)
        }
        None => {
            eprintln!("  no _argraver.yml found; using defaults");
            (ingest::Metadata::default(), None)
        }
    };

    let markdown = ingest::load_manuscript(manuscript)?;

    println!("Building {} ...", output.display());
    if let Some(p) = &meta_path {
        println!("  metadata: {}", p.display());
    }

    match target {
        Target::Epub => epub::build(
            &metadata.book,
            &metadata.cover,
            &markdown,
            meta_dir.as_deref(),
            output,
        )?,
        Target::Pdf => pdf::build(
            &metadata.book,
            &metadata.cover,
            &markdown,
            meta_dir.as_deref(),
            output,
        )?,
        Target::Typst => {
            pdf::write_typst(&metadata.book, &markdown, meta_dir.as_deref(), output)?
        }
    }

    println!("Wrote {}", output.display());
    Ok(())
}
