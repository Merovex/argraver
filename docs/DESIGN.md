# Argraver — design notes

Companion to [`PORTING.md`](PORTING.md) (the spec). PORTING.md records *what and
why* at the scoping level; this file records the **decisions actually taken in
this scaffold** and the open items carried forward. When the two disagree, this
file is the newer word for the code as built; PORTING.md remains the rationale.

## What Argraver is

A single-source book compiler. Input: one Longform-compiled
`<Project> - manuscript.md` plus a standalone `_metadata.yml`. Output: an **EPUB**
and a print **PDF**. It is the headless, Markdown-native cousin of Vellum/Atticus
and a sibling to the Verkilo editor — it reuses Verkilo's *typst interior design*
and `typst_escape` as **source to port**, but does not depend on or modify the
Verkilo repo.

## The compiler shape

One front end, one IR, two back ends:

```
                         ┌─ comrak::format_html ─► HTML ─► epub-builder ─► EPUB
md ─ strip-fm ─ comrak ──► AST (one parse)
                         └─ ast_to_typst (visitor) ─► Typst ─► typst CLI ─► PDF
```

The shared AST is the whole point: HTML and Typst can never disagree about *how
the Markdown was understood*, because they come off the same parse. `parse.smart`
is set once, so smart punctuation is consistent across both outputs by
construction.

- **EPUB** is the cheap, faithful path: comrak's own GFM-complete renderer, plus
  a ported `epub.css` and the front-matter synthesis from `epub-filters.lua`.
- **PDF** is the only renderer we own: a recursive `NodeValue → Typst` visitor.
  typst typesets it. typst is **PDF-only** — it never touches the EPUB path.

## Module map (planned)

| Module | Responsibility | Status |
| --- | --- | --- |
| `ingest` | metadata discovery (`project_of` + `find_meta`), `_metadata.yml` → `Book`, manuscript load, `strip-fm` port, H1 chapter split | **EPUB-real** |
| `parse` | comrak `Options` (smart on, GFM extensions), single `parse_document` | **EPUB-real** |
| `render_html` | AST → per-chapter XHTML; leading-separator strip, `chapter-title` class, drop caps; front-matter pages | **EPUB-real** |
| `epub` | `epub-builder` assembly: metadata, cover resize, css, front matter, chapters, ToC | **EPUB-real** |
| `render_typst` | `typst_escape` (ported, book variant) + `NodeValue → Typst` visitor | **stub** |
| `pdf` | typst CLI subprocess (option **C′**) | **stub** |
| `cli` | `clap` subcommands `epub \| pdf \| all <manuscript.md> [out]` | wired; `pdf`/`all` error until PDF lands |

## Build order

1. **EPUB end to end** (current target). comrak → `format_html` → `epub-builder`
   over a real vault manuscript. Proves the ingestion layer — metadata mapping,
   H1 split, front-matter synthesis — before any typst work. Diff against today's
   `bin/book epub` output.
2. **PDF.** Add the `NodeValue → Typst` visitor + the ported interior template;
   wire `argraver pdf`. Start with the typst **CLI subprocess** (C′) to defer the
   typst-as-library decision.
3. **Cover resize** folds into the EPUB path (the `image` crate).

## Decisions taken in this scaffold

- **`src/` not `code/`.** Cargo requires `src/`; matches the "src/ for code"
  instruction. `builds/` is the gitignored output dir, `docs/` holds design.
- **typst CLI (C′), not `typst-as-lib` (C).** Lighter build; defers the
  single-binary decision. The `NodeValue → Typst` code is identical either way —
  only the invocation differs — so C vs C′ stays a late call.
- **`typst_escape` book variant.** Port Verkilo's escaping of
  `\ # * _ @ $ [ ] < >`, but **drop** its smart-quote-flattening and
  backtick→apostrophe lines: a book keeps typographic quotes and em-dashes
  (`parse.smart` stays on). This is the one real correctness landmine.
- **Footnotes / endnotes are on.** `extension.footnotes` is enabled on the shared
  parse, so `[^id]` references and `[^id]:` definitions round-trip into both
  outputs. On the EPUB path, because the body is split per chapter, comrak renders
  each chapter's definitions as a `<section class="footnotes">` at the end of that
  chapter's file — i.e. **endnotes per chapter**. (A book-wide endnotes section is
  a later option if wanted.) The PDF path will map footnotes when the
  `NodeValue → Typst` visitor lands.
- **Front-matter fidelity comes from the real templates**, not invented strings:
  `epub.css` and `epub-filters.lua` (copyright page, dedication, drop caps,
  `chapter-title`, leading-separator strip, scene-break `<hr>`) are ported from
  `_templates/` in the gendaldea vault.

## Open items carried forward (mostly PDF-phase)

- **Trim-name mapping.** `_metadata.yml` uses `pocket | small-digest | digest |
  trade | large`; Verkilo's `interior_config.toml` uses a *different* set with mm
  dims. The PDF interior port needs an explicit mapping table between the two.
  EPUB is unaffected.
- **ISBN identifier.** `epub-filters.lua` emits `urn:isbn:` as the OPF
  `dc:identifier`; `epub-builder` 0.8's identifier API is uuid-centric. Faithful
  `urn:isbn:` may need a workaround — tracked as an EPUB fidelity gap vs pandoc.
- **typst 0.13 → 0.14.** Verkilo's interior template is 0.13 syntax; the installed
  typst is 0.14.2. The template is a *port to 0.14*, not a verbatim copy.
- **Cover path resolution.** `cover.background.image` is vault-absolute
  (`/Series/_assets/…`). Needs a vault-root rule (a flag, or walk up to the
  folder holding `_meta`/`.obsidian`). For now: try as-given, then relative to the
  metadata file's directory; skip with a warning if not found (matches `bin/book`).
- **`serde_yaml`** is archived upstream (`0.9.34+deprecated`). Chosen per spec;
  logged as tech debt.

## Out of scope for v1

DOCX; `print-ready` (PDF/X-1a + grayscale via ghostscript); the print wraparound
cover (`cover.rb` + spine math). The EPUB cover (resize only) is in scope.

## Source material (ported, not linked)

- Spec: [`PORTING.md`](PORTING.md).
- EPUB templates (verbatim/near-verbatim): vault
  `_templates/epub.css`, `_templates/epub-filters.lua`.
- typst escaper + interior design (to port for the PDF phase): Verkilo
  `editor/verkilo-app/src-tauri/src/html_typst.rs` (`typst_escape`),
  `interior_template_engine.rs` + `interior_config.toml`.
- Ingestion reference: vault `bin/strip-fm`, `bin/book` (the metadata-discovery UX
  this CLI matches), mirrored in `sample-bin-files/`.
