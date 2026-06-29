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
| `render_typst` | `typst_escape` (book variant), `NodeValue → Typst` visitor, ported interior template + front matter | **real** |
| `pdf` | assemble document + typeset via typst CLI (option **C′**); also `write_typst` for the `typst` command | **real** |
| `cli` | `clap` subcommands `init`, `epub \| pdf \| all \| typst <manuscript.md> [out]` + `--meta` | **real** |

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
- **Fonts (PDF) — settable and embedded.** `book.body_font` / `book.display_font`
  choose the running-text and chapter/heading families (env `BOOK_MAINFONT` /
  `BOOK_DISPLAYFONT` override; display falls back to body; body defaults to the
  typst-bundled Libertinus Serif). typst **embeds and subsets** every font it uses
  into the PDF automatically — verified with `pdffonts` (`emb=yes sub=yes`). For
  fonts not installed system-wide, `book.font_path` (or env `BOOK_FONTPATH`) names
  a directory of `.ttf`/`.otf` files passed to typst as `--font-path` on both the
  compile and the page-count query. A requested family typst can't find triggers a
  visible `warning: unknown font family …` (it does not silently fail).
- **EPUB cover — embedded image *and* a cover page.** `add_cover_image` embeds the
  resized `cover.jpg` and flags it (`properties="cover-image"` + `meta cover`), so
  it shows as the library thumbnail. We also add `cover.xhtml` as the first spine
  item (reftype `Cover`, `body#cover` styled by `epub.css`), so the book *opens* on
  a full-page cover rather than the title page.
- **Chapter openings — `book.chapter_start`.** `recto` (default) opens each
  chapter on the next right-hand (odd) page, the traditional book look, leaving a
  blank verso when needed; `any` opens on the next page either side (no blank
  versos, fewer total pages). PDF-only — the EPUB reflows. Drives the
  `pagebreak(to: "odd")` vs `pagebreak()` in the level-1 heading rule.
- **Scene breaks: heading-suppressed (both), styled by `book.scenebreak`.** A
  `---` rule directly before a heading is dropped on *both* paths — it's a
  Longform scene-separator ahead of the next chapter, not a real in-chapter
  break, so nothing is stranded at a chapter foot. For real in-chapter breaks,
  the `book.scenebreak` metadata setting (`Book::scene_break_style`) chooses:
  - **`auto`** (default) — PDF: a blank gap mid-page, the `* * *` ornament when
    the break lands against a page edge (detected via `here().position().y`, so
    the break is never lost to a page turn; the ornament opens the next page).
    EPUB: the styled `<hr>` rule. The break is a **fixed-height block** so the
    branch never changes the layout height — otherwise the position read would
    shift the page break, flip the branch, and typst would oscillate ("layout
    did not converge").
  - **`ornament`** — always the centered `* * *`, both outputs.
  - **`blank`** — always a plain gap, both outputs.

  PDF rendering is the generated `#let scenebreak`; EPUB swaps comrak's `<hr />`
  for `<p class="scene-break">` (styled by `epub.css`).
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

## Resolved during the PDF phase

- **Trim-name mapping — done.** `Trim::from_name` maps the vault's
  `pocket | small-digest | digest | trade | large` to inches (defaulting to
  `digest`, 5.5×8.5, like `bin/book`). We did *not* adopt Verkilo's mm trim
  vocabulary; the vault's names are the interface.
- **typst 0.13 → 0.14 — done.** The interior template is authored for the
  installed typst 0.14.2 (the drop-cap library `droplet.typ` is vendored).
- **Interior design parity.** Adopted Verkilo's interior *look* (recto chapter
  starts, sunk display-face titles, drop caps, asterisk scene breaks, verso-title
  /recto-author running heads, page numbering restarting at the first chapter)
  rather than matching `classic.latex` pixel-for-pixel.

- **Print margins: trim-proportional + page-count gutter (PDF).** Professional
  working standards (above the KDP/IngramSpark floors, Ingram being the binding
  one). Top/bottom/outside scale with the **trim** (outer margins grow with the
  page so the block stays proportional); each trim carries `(low, high)` ranges
  on `Trim` and `book.margins` (`narrow|normal|wide`) picks within them:

  | Trim | Top | Bottom | Outside |
  |---|---|---|---|
  | pocket 5×8 | 0.6–0.7 | 0.6–0.7 | 0.375–0.5 |
  | small-digest 5.25×8 | 0.65–0.75 | 0.65–0.75 | 0.4–0.5 |
  | digest 5.5×8.5 | 0.75 | 0.75 | 0.5 |
  | trade 6×9 | 0.75–0.875 | 0.75–0.875 | 0.5–0.625 |
  | large 7×10 | 0.875–1.0 | 0.875–1.0 | 0.625–0.75 |

  `top` always takes the upper end (running-header bias); `bottom`/`outside`
  scale with the width setting, so `top ≥ bottom` always. `resolve_margins`
  computes all four.
  - **Inside (gutter)** — independent of trim, derived from the *actual page
    count* (`gutter_inside_in`): ≤150 pp → 0.5", ≤300 → 0.625", ≤500 → 0.75",
    ≤700 → 0.875", 701–828 → 1.0". Because the gutter changes the text width (and
    so the page count), the PDF path typesets, queries the count via a
    `<pagecount>` metadata label (`typst query`), recomputes the band, and repeats
    until it settles (monotonic — a couple of passes; logged as "`N pages →
    margins …`").

  Bleed is out of scope (text-only interior; no edge-to-edge art), consistent
  with the print-cover exclusion.

## Open items carried forward

- **ISBN identifier.** `epub-filters.lua` emits `urn:isbn:` as the OPF
  `dc:identifier`; `epub-builder` 0.8's identifier API is uuid-centric. Faithful
  `urn:isbn:` may need a workaround — tracked as an EPUB fidelity gap vs pandoc.
  (The ISBN does render on the copyright page of both outputs.)
- **Cover path resolution.** `cover.background.image` is vault-absolute
  (`/Series/_assets/…`). Resolution tries: as-given → relative to the metadata
  dir → walk up to a `_meta`/`.obsidian` root; skips with a warning if not found.
  A relative path beside `_metadata.yml` is the reliable form today.
- **PDF body images.** typst resolves `#image` relative to the generated `.typ`
  (written beside the output, e.g. `builds/`), so manuscript images that live by
  the manuscript may not resolve. Rare in prose fiction; revisit if needed.
- **Font availability.** The interior defaults to `Libertinus Serif` (typst-
  bundled), overridable via `BOOK_MAINFONT` / `BOOK_DISPLAYFONT`. A requested font
  missing from the system falls back silently (typst behavior).
- **`serde_yaml`** is archived upstream (`0.9.34+deprecated`). Chosen per spec;
  logged as tech debt.

## Out of scope for v1

DOCX; `print-ready` (PDF/X-1a + grayscale via ghostscript); the print wraparound
cover (`cover.rb` + spine math). The EPUB cover (resize only) is in scope.

## Source material (ported, not linked)

- Spec: [`PORTING.md`](PORTING.md).
- EPUB templates (verbatim/near-verbatim): vault
  `_templates/epub.css`, `_templates/epub-filters.lua`.
- typst escaper + interior design (ported): Verkilo
  `editor/verkilo-app/src-tauri/src/html_typst.rs` (`typst_escape`),
  `interior_template_engine.rs` + `interior_config.toml`.
- Drop-cap library (vendored verbatim, MIT): `src/resources/droplet.typ`, from
  [typst-droplet](https://github.com/EpicEricEE/typst-droplet).
- Ingestion reference: vault `bin/strip-fm`, `bin/book` (the metadata-discovery UX
  this CLI matches), mirrored in `sample-bin-files/`.
