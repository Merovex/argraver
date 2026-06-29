# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Argraver is a **single-source book compiler** written in Rust. It takes one
Longform-compiled Markdown manuscript (`<Project> - manuscript.md`) plus a
standalone `_metadata.yml` and produces an **EPUB** and a print **PDF**. It is the
headless, Markdown-native cousin of Vellum/Atticus and a sibling to the Verkilo
editor.

Read [`docs/DESIGN.md`](docs/DESIGN.md) first (decisions as built) and
[`docs/PORTING.md`](docs/PORTING.md) (the spec / rationale).

## Architecture — a compiler, not a script

One front end, one IR, two back ends off a **single parse**:

```
                         ┌─ comrak::format_html ─► HTML ─► epub-builder ─► EPUB
md ─ strip-fm ─ comrak ──► AST (one parse)
                         └─ ast_to_typst (visitor) ─► Typst ─► typst CLI ─► PDF
```

The shared AST is load-bearing: EPUB and PDF can never disagree about how the
Markdown was parsed because they come off the *same* `comrak` AST. `parse.smart`
is set once (≈ pandoc `markdown+smart`), so smart punctuation is consistent across
both outputs by construction.

- **EPUB path** uses comrak's own renderer (`format_html`) — no hand-written HTML
  body — plus a ported `epub.css` and front-matter synthesis. This is the cheap,
  faithful path and is built **first**.
- **PDF path** is the only renderer we own: a recursive `NodeValue → Typst`
  visitor, typeset by the `typst` CLI. **typst is PDF-only** — never invoke it on
  the EPUB path.

## Module map (`src/`)

| Module | Responsibility |
| --- | --- |
| `main.rs` | thin entry — delegates to `cli` |
| `lib.rs` | crate root; re-exports the modules below |
| `cli` | `clap` subcommands: `epub \| pdf \| all <manuscript.md> [out]` + metadata-discovery UX |
| `ingest` | `_metadata.yml` → `Book`; manuscript load; `strip-fm` port; `project_of`/`find_meta`; H1 chapter split |
| `parse` | comrak `Options` (smart + GFM extensions) and the single `parse_document` |
| `render_html` | AST → per-chapter XHTML; leading-separator strip, `chapter-title` class, drop caps; front-matter pages |
| `epub` | `epub-builder` assembly: metadata, cover resize, css, front matter, chapters, ToC |
| `render_typst` | `typst_escape` (book variant) + `NodeValue → Typst` visitor — **stub** |
| `pdf` | typst CLI subprocess (option C′) — **stub** |

**Status:** the **EPUB path is real**; the **PDF/typst path is honestly stubbed**
(`render_typst` / `pdf` return a not-implemented error). `pdf` and `all` error
until the PDF phase lands. This is by design — see the build order in
`docs/DESIGN.md`.

## Commands

```bash
cargo build                 # compile the crate
cargo build --release       # optimized
cargo test                  # run unit tests
cargo test ingest::         # run one module's tests (path filter)
cargo test strip_fm         # run a single test by name substring

# Run the CLI (note the `--` separating cargo args from program args):
cargo run -- epub "manuscript.md"                # EPUB next to the input
cargo run -- epub "manuscript.md" builds/out.epub
cargo run -- epub "manuscript.md" -m path/_metadata.yml   # explicit metadata
cargo run -- pdf  "manuscript.md"                # stubbed: errors for now
cargo run -- all  "manuscript.md"                # stubbed via pdf

cargo run -- --help
```

Build outputs go to `builds/` (gitignored). `target/` is gitignored.

## Inputs this compiler expects

- **Manuscript:** `<Project> - manuscript.md`. The project name is the basename
  with ` - manuscript.md` (or `.md`) stripped — this drives metadata discovery.
  Longform may compile each scene's YAML front matter inline; `strip-fm` removes
  those blocks while preserving lone `---` scene-break rules. H1 (`# …`) splits
  chapters; a thematic break (`---`) inside content is a scene break.
- **Metadata:** a standalone `_metadata.yml` with a `book:` map (title, subtitle,
  author, publisher, copyright, edition, isbn, trim, dedication, description) and a
  `cover:` map. Resolution precedence: the `--meta/-m <FILE>` flag wins; else the
  `BOOK_META` env var; else discovery — `Books/<Name>/<Project>/_metadata.yml`,
  then beside the manuscript. `book:` is the single source of truth — facts live
  there once. An explicit `--meta` path that doesn't exist is a hard error (it
  does not silently fall back to discovery).

## Conventions specific to this codebase

- **One parse, then two renderers.** Never add a second Markdown parse. New output
  features render off the existing comrak AST.
- **`typst_escape` is the correctness landmine.** Escape `\ # * _ @ $ [ ] < >` in
  prose **text nodes only** — never raw/code nodes. Unlike Verkilo's version,
  **keep** typographic quotes and em-dashes (do not flatten smart quotes or turn
  backticks into apostrophes): this is a book, and `parse.smart` stays on.
- **Faithful, not invented.** The EPUB front matter (copyright/dedication pages,
  drop caps, `chapter-title`, scene-break `<hr>`) mirrors the vault's real
  `_templates/epub-filters.lua` + `epub.css`. Match that output; don't improvise
  new front-matter strings.
- **Pin parser/typesetter versions.** comrak's `Options` struct and typst's
  template syntax both drift across releases. Versions are pinned in `Cargo.toml`;
  the installed typst is **0.14.2** (Verkilo's template is 0.13 — a port, not a
  copy).
- **Verkilo is source-to-port, never a dependency.** Do not add a path dependency
  on `~/Work/verkilo`; lift code (e.g. `typst_escape`, the interior template) by
  copying and adapting.

## v1 scope boundaries (out of scope)

DOCX; `print-ready` (PDF/X-1a + grayscale, ghostscript-only); the print
wraparound cover (`cover.rb` + spine math). The EPUB cover (resize to a fitted
JPEG) **is** in scope. See `docs/DESIGN.md` → "Open items" for the PDF-phase TODOs
(trim-name mapping, ISBN identifier, typst 0.13→0.14, cover path resolution).

## Layout

- `src/` — crate source (library + `argraver` binary).
- `docs/` — design (`DESIGN.md`) and the spec (`PORTING.md`).
- `builds/` — build outputs (**gitignored**).
- `sample-bin-files/` — the bash `book`, `strip-fm`, `cover.rb` etc. that this
  tool replaces; kept as ingestion/UX reference.
- `manuscript.md` — a real sample manuscript for end-to-end testing.
