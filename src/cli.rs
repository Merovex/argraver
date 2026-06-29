//! The `argraver` command line: `epub | pdf | all <manuscript.md> [out]`.
//!
//! Mirrors the bash `book` UX — project name from the manuscript filename,
//! `_metadata.yml` auto-discovered (or `BOOK_META`), output defaulting beside
//! the input.

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::{epub, ingest, pdf};

#[derive(Parser)]
#[command(
    name = "argraver",
    version,
    about = "Single-source book compiler: one manuscript + _metadata.yml -> EPUB and PDF"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build an EPUB.
    Epub(BuildArgs),
    /// Build a print PDF (not implemented yet).
    Pdf(BuildArgs),
    /// Build both EPUB and PDF.
    All(BuildArgs),
}

#[derive(clap::Args)]
struct BuildArgs {
    /// The Longform-compiled manuscript, e.g. "<Project> - manuscript.md".
    manuscript: PathBuf,
    /// Output path. Defaults to the manuscript path with the format's extension.
    output: Option<PathBuf>,
    /// Path to `_metadata.yml`. Overrides auto-discovery and `BOOK_META`.
    #[arg(short = 'm', long = "meta", value_name = "FILE")]
    meta: Option<PathBuf>,
}

/// Parse args and run.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Epub(args) => build(&args, Target::Epub),
        Command::Pdf(args) => build(&args, Target::Pdf),
        Command::All(args) => {
            build(&args, Target::Epub)?;
            build(&args, Target::Pdf)
        }
    }
}

#[derive(Clone, Copy)]
enum Target {
    Epub,
    Pdf,
}

impl Target {
    fn ext(self) -> &'static str {
        match self {
            Target::Epub => "epub",
            Target::Pdf => "pdf",
        }
    }
}

fn build(args: &BuildArgs, target: Target) -> Result<()> {
    let manuscript = &args.manuscript;
    let output = args
        .output
        .clone()
        .unwrap_or_else(|| manuscript.with_extension(target.ext()));

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
            eprintln!("  no _metadata.yml found; using defaults");
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
            &output,
        )?,
        Target::Pdf => pdf::build(
            &metadata.book,
            &metadata.cover,
            &markdown,
            meta_dir.as_deref(),
            &output,
        )?,
    }

    println!("Wrote {}", output.display());
    Ok(())
}
