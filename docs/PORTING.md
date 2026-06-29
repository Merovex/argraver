# Building a standalone Rust book compiler for this vault

A scoping document for replacing `bin/book` — today a 184-line bash script that
shells out to **five external programs** (pandoc, tectonic, typst, ghostscript,
imagemagick) — with a single Rust tool that takes one Longform-compiled
`<Project> - manuscript.md` plus its `_metadata.yml` and produces an **EPUB** and
a **PDF**.

> **TL;DR** — The clean design is **one `comrak` parse fanned out to two
> renderers**: `comrak::format_html` gives the EPUB body (identical to comrak,
> zero new code) and a hand-written `NodeValue → Typst` visitor gives the PDF
> body. typst typesets the PDF; `epub-builder` packages the EPUB. We reuse
> Verkilo's *typst interior design* and its `typst_escape`, but **not** its
> HTML-based pipeline — Verkilo stores HTML in SQLite, this vault is Markdown, so
> the conversion direction is different. This is a small, self-contained crate,
> **not** a dependency on or extraction from `~/Work/verkilo`.

---

## 0. Decided architecture (the spec)

What would be built. No build is authorized by this document — it records *what*
and *why*.

**Shape.** A single CLI binary, same UX as today: `book epub|pdf|all
"<Project> - manuscript.md" [out]`. Project name derived from the filename;
`_metadata.yml` discovered beside it or under `Books/<Name>/<Project>/`; the same
env overrides (fonts, trim).

**One parser, two outputs.** The whole design rests on parsing the manuscript
**once** with `comrak` (CommonMark + GFM) into an AST, then rendering that single
AST two ways:

```
                    ┌─ comrak::format_html(root) ─────► HTML  → epub-builder → EPUB
md ─ comrak::parse ─►AST
                    └─ ast_to_typst(root) ────────────► Typst → typst        → PDF
```

The shared AST is the point: HTML and Typst can never disagree about *how the
Markdown was understood*, because they come off the same parse. This is the
property pandoc gives us today and a naive "two converters" approach would lose.

**Markdown → HTML (EPUB) is free.** `comrak::format_html` is comrak's own,
GFM-complete, battle-tested renderer. We write no HTML-generation code; the EPUB
body is comrak-identical by definition. It feeds `epub-builder` + a ported
`epub.css`.

**Markdown → Typst (PDF) is the only renderer we own.** A recursive visitor over
`comrak::nodes::NodeValue` emitting typst markup. For prose fiction the mapping
is small; the long tail (tables, footnotes, nested lists) is added only if the
manuscripts use it.

**typst is PDF-only.** typst is a fixed-page-layout engine — it emits PDF/PNG/SVG,
never EPUB. The EPUB half is *always* a separate path (here: `epub-builder`).
Keep these two halves mentally split.

**Self-contained, mostly.** No pandoc, no tectonic/LaTeX, no imagemagick. The one
external program we keep is **ghostscript**, only for `print-ready` (PDF/X-1a +
grayscale) — there is no Rust equivalent and it is out of scope (see §6).

**Standalone, no Verkilo coupling.** A new crate that reuses the same *public*
crates Verkilo uses (`comrak`, `typst`/`typst-as-lib`, `typst-pdf`,
`epub-builder`, `image`, `serde_yaml`) and lifts Verkilo's *typst interior
template* and `typst_escape` as source — but does **not** depend on or modify
`~/Work/verkilo`.

### Crate contents (spec, not code)

| Piece | Responsibility | Library / source |
| --- | --- | --- |
| CLI front-end | `book epub\|pdf\|all <manuscript.md> [out]`; metadata discovery; env overrides | `clap` (replaces the bash dispatch) |
| Metadata loader | `_metadata.yml` (`book:` / `cover:`) → typed `Book` struct | `serde_yaml` |
| Manuscript ingester | Port `strip-fm` (drop YAML-ish blocks) + the Lua filters' logic (drop leading separators, scene-break `---`, drop-cap first paragraph, split at H1) | hand-written + `comrak` |
| **Parser (shared)** | Manuscript Markdown → one comrak AST | `comrak`, smart punctuation on |
| EPUB renderer | `format_html` body → front matter, per-chapter XHTML, ToC, embedded cover, stylesheet | `comrak::format_html` + `epub-builder` + ported `epub.css` |
| Cover resize | Master image → 1600×2560-fit JPEG q92 for the EPUB cover | `image` (EXIF dropped on re-encode = "strip" for free) |
| PDF renderer | comrak AST → typst markup → PDF; trim, fonts, drop caps, scene breaks | `NodeValue→Typst` visitor + typst, with the interior template ported from Verkilo |
| Typst escaper | Escape `# $ [ ] _ * @ < > \` in prose text nodes | lift Verkilo `typst_escape` verbatim |

---

## 1. Why not just keep pandoc

The current `bin/book` works and every tool is installed. But the PDF path —
`Markdown → pandoc → LaTeX (classic.latex, memoir) → tectonic` — has two standing
costs:

- **No control of the middle.** Drop caps and scene breaks are coaxed out of
  LaTeX by a Lua filter rewriting pandoc's AST into `\lettrine` / `\scenebreak`
  raw blocks, with `hbadness=10000` / `hfuzz` papering over the justifier. We
  maintain a LaTeX book class to get effects we could emit directly.
- **A five-program toolchain** (pandoc, tectonic, imagemagick, ruby, plus typst
  and ghostscript for covers/print) that a fresh machine or a handoff must
  reproduce.

A native typst writer *does* exist in pandoc 3.6 (`pandoc -t typst`), which is a
real lighter-weight option — keep pandoc as reader + EPUB writer, swap the LaTeX
backend for a typst template. It is recorded here as the fallback. But once we
commit to Rust, comrak gives us pandoc-grade parsing *and* both outputs from one
AST, with no pandoc process at all — see §3.

EPUB is the one place pandoc is genuinely hard to beat (its epub3 writer is
best-in-class). `epub-builder` + `comrak::format_html` is chosen for the
single-binary goal, **with pandoc's EPUB output kept as the diff baseline** to
prove the new path is faithful (see §7).

---

## 2. Why not just extract Verkilo's engine

`~/Work/verkilo/editor/verkilo-app/src-tauri/src` already contains a complete,
pure-Rust, in-process export engine (~6,400 lines: `pdf_exporter.rs`,
`epub_exporter.rs`, `interior_template_engine.rs`, `html_typst.rs`, …). The
renderers are ordinary functions (`export_pdf/epub(project_path, book_id) ->
Result<Vec<u8>>`), not Tauri commands, and they use **zero external programs**.
So extraction is tempting.

The blocker is the **content model**, not Tauri:

1. **Verkilo's content is HTML stored in SQLite.** `Section.content` is an HTML
   string; every renderer assumes HTML. Its `html_typst.rs` (535 ln) is a
   **regex-based** HTML→typst converter that exists *only* because the source is
   HTML — and it survives on the fact that the editor's HTML is clean and
   predictable.
2. **This vault is hand-authored Markdown** (`<Project> - manuscript.md` +
   `_metadata.yml`). There is no `.db`.

So reusing Verkilo wholesale would mean going Markdown → HTML → (regex) → typst —
a lossy double hop through a fragile regex layer. Parsing Markdown straight to an
AST and rendering typst from *that* is cleaner and avoids re-parsing HTML
entirely.

**What we still take from Verkilo** (as source, copied — not linked):

- The **typst interior design** is already built and shipping. Grepping its own
  source confirms it implements everything `classic.latex` does: `#dropcap`,
  `#scenebreak` (fleuron ornament), trim sizes (100+ references), margins/gutter,
  recto/verso running heads, justification, small caps, hyphenation, font
  embedding. The "re-author the PDF look in typst" task that an earlier version
  of this doc called *the single largest piece of new authoring* is **already
  done** — it is a template to port, not design work.
- `typst_escape` — a tuned chained-replace that handles the real correctness
  landmine (see §3).

---

## 3. The renderers in detail

### 3a. comrak options (drive both outputs)

```rust
let mut o = comrak::Options::default();
o.parse.smart = true;            // smart quotes + em/en dashes ≈ pandoc markdown+smart
o.extension.strikethrough = true;
o.extension.table = true;
o.extension.autolink = true;
o.extension.footnotes = true;
o.extension.tasklist = true;
// pin the comrak version — these option structs have churned across releases
```

`parse.smart` is what reproduces today's `markdown+smart`, and because it is set
once on the shared parse, smart punctuation is consistent across PDF and EPUB by
construction.

### 3b. Markdown → HTML (EPUB)

```rust
let arena = comrak::Arena::new();
let root  = comrak::parse_document(&arena, md, &o);
let mut html = Vec::new();
comrak::format_html(root, &o, &mut html)?;     // EPUB body — comrak-identical
```

Into `epub-builder`: title / copyright / dedication front matter (ported from
`epub-filters.lua`'s synthesized pages and default rights/disclaimer strings),
per-chapter XHTML split at H1, ToC, embedded resized cover, `epub.css`.

### 3c. Markdown → Typst (PDF)

Walk the *same* `root`, matching on `NodeValue`. Sketch:

```rust
match &node.data.borrow().value {
    NodeValue::Heading(h)      => emit_heading(h.level, node, out),     // "= "/"== "
    NodeValue::Paragraph       => { children(node, out); out.push_str("\n\n"); }
    NodeValue::Text(t)         => out.push_str(&typst_escape(t)),       // ← the landmine
    NodeValue::Emph            => wrap("_", node, out),
    NodeValue::Strong          => wrap("*", node, out),
    NodeValue::ThematicBreak   => out.push_str("\n#scenebreak\n"),      // scene-break ornament
    NodeValue::Link(l)         => link(l, node, out),                   // #link("url")[..]
    NodeValue::Image(l)        => image(l, out),                        // #image("path")
    NodeValue::Code(c)         => raw_inline(c, out),
    NodeValue::CodeBlock(c)    => raw_block(c, out),
    NodeValue::List(_)         => children(node, out),
    NodeValue::Item(_)         => list_item(node, out),
    NodeValue::BlockQuote      => quote(node, out),
    NodeValue::SoftBreak       => out.push(' '),
    NodeValue::LineBreak       => out.push_str(" \\\n"),
    NodeValue::HtmlInline(_) | NodeValue::HtmlBlock(_) => {}            // strip raw HTML in prose
    _                          => children(node, out),                 // safe fallthrough
}
```

The drop-cap pass (first paragraph after each H1) and the leading-separator drop
mirror today's `classic-filters.lua` / `epub-filters.lua` and run on the AST
before/within this walk. The resulting typst goes into the ported interior
template (trim, fonts, running heads, `#dropcap`, `#scenebreak`) and is compiled
to PDF.

### 3d. Escaping — the one correctness landmine

`typst_escape` (lift Verkilo's) escapes `\ # * _ @ $ [ ] < >` in prose text
nodes; unescaped, prose like `C# costs $5` silently becomes typst markup/math.
**Two deliberate divergences from Verkilo's version:** Verkilo flattens smart
quotes to straight and backtick→apostrophe because its source was straight-quoted
editor HTML. For a *book* we want the opposite — keep typographic quotes and
em-dashes — so leave `parse.smart` on and drop those replacement lines on the
typst path. Code spans/blocks are raw: escape text nodes, never raw nodes.

---

## 4. typst as library vs CLI (open: C vs C′)

Two ways to run typst on the generated markup:

- **C — `typst-as-lib` (single self-contained binary).** typst linked in; the
  binary needs nothing on PATH. Cost: the typst crate is heavy to compile and its
  template syntax must be pinned (Verkilo pins 0.13; the vault CLI is 0.14.2).
- **C′ — `typst` CLI as a subprocess.** Generate the `.typ`, shell to
  `typst compile`. Lighter build, no version-locked crate — but typst must be
  installed. Structurally identical to what `bin/book` already does (preprocess,
  then call an external typesetter).

C′ is the lighter first step; C is the endgame if a single distributable binary
is the actual goal. Either way the `NodeValue→Typst` code is the same — only the
invocation differs — so this can be decided late.

---

## 5. Build order

1. **EPUB first.** comrak → `format_html` → `epub-builder` over one vault
   manuscript, end to end. EPUB is the fastest faithfulness check because the
   output can be diffed against today's `bin/book epub` (and pandoc's epub3 is the
   trusted baseline). This proves the ingestion layer — metadata mapping, H1
   chapter split, front-matter synthesis — before any typst work.
2. **PDF.** Add the `NodeValue→Typst` visitor + the ported interior template;
   wire `book pdf`. Start with C′ (typst CLI) to defer the typst-crate decision.
3. **Cover resize** (the `image` crate) folds into the EPUB path.
4. Keep `bin/book print-ready` (ghostscript) and the bash `book` as a fallback
   until the Rust tool is proven.

---

## 6. Out of scope (first version)

- **DOCX.** Not needed here.
- **`print-ready` (PDF/X-1a + grayscale).** ghostscript only; no clean Rust
  equivalent. Keep shelling to `gs` indefinitely — this means the tool is *not*
  truly zero-external-program for the print path, and that is an accepted
  trade, not an oversight.
- **Print wraparound cover** (`cover.rb` + `series_cover.typ`, spine math). A
  later port; the EPUB cover (resize only) is in scope, the print cover is not.

---

## 7. Open questions to settle before coding

- **C vs C′** — typst as library (self-contained binary, heavy build, pinned
  syntax) or typst CLI subprocess (light, needs typst installed)?
- **PDF design parity** — adopt Verkilo's interior look as-is (one standard;
  probably desirable), or match `classic.latex` exactly? Confirm against a real
  built book before trusting it; this is a "do I like this design" check, not a
  re-authoring project.
- **EPUB writer** — `epub-builder` (single-binary goal) vs keeping pandoc's
  epub3 (best-in-class, but reintroduces a pandoc dependency). Default:
  `epub-builder`, diffed against pandoc output.
- **Front-matter modeling** — how much of `epub-filters.lua`'s logic to reproduce
  (default rights/disclaimer strings, ISBN→`urn:isbn:`, synthesized
  copyright/dedication pages).
- **Cover path resolution** — `cover.background.image` is vault-absolute
  (`/Series/_assets/…`); the binary needs a vault-root rule (a flag, or walk up
  to the folder holding `_meta`/`.obsidian`).
- **Where the crate lives** and the binary name — replace `bin/book` in place or
  sit beside it. `target/` is git-ignored regardless.
- **comrak + typst versions** — pin both; comrak's `Options` struct and typst's
  template syntax both drift across releases.
