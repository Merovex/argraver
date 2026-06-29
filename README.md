# Argraver

**Markdown-native book compiler.** One Markdown manuscript + one `_metadata.yml`
in → a polished **EPUB** and a print-ready **PDF** out. The headless, single-binary
cousin of Vellum/Atticus.

Argraver is shaped like a compiler: it parses the manuscript **once** with
[comrak](https://github.com/kivikakk/comrak) and fans that one AST out to two
back ends, so the EPUB and the PDF can never disagree about how the Markdown was
understood.

```
                         ┌─ comrak::format_html ─► HTML ─► epub-builder ─► EPUB
md ─ strip-fm ─ comrak ──► AST (one parse)
                         └─ NodeValue→Typst visitor ─► Typst ─► typst ─► PDF
```

## Features

- **One source, two outputs.** Smart punctuation, footnotes/endnotes, lists,
  block quotes, and links render consistently to both formats.
- **Print-ready PDF.** Drop caps, scene-break ornaments, recto chapter openings,
  running heads, and page numbering that restarts at the first chapter.
- **KDP / IngramSpark margins.** Top/bottom/outside scale with the trim; the
  inside (gutter) auto-fits to the actual page count. No manual page-count math.
- **Fonts, embedded.** Pick body/display fonts; typst embeds and subsets them.
  Point at a font directory for non-system fonts.
- **Faithful EPUB.** Comrak-native body, a real cover page, and synthesized
  title/copyright/dedication front matter.
- **Metadata-driven.** Design choices live in `_metadata.yml`, not in code.

## Install

Requires a Rust toolchain. The **PDF path also needs [`typst`](https://typst.app)**
on your `PATH` (the EPUB path is self-contained).

```bash
cargo install --path .
```

This installs `argraver` to `~/.cargo/bin`. If that directory isn't on your
`PATH`, add it (e.g. `export PATH="$HOME/.cargo/bin:$PATH"` in your shell rc).

## Quick start

```bash
argraver init my-book/                 # write a starter _metadata.yml
# ...edit my-book/_metadata.yml and drop in "<Project> - manuscript.md"...

argraver all  "My Book - manuscript.md"        # EPUB + PDF
argraver epub "My Book - manuscript.md"        # just the EPUB
argraver pdf  "My Book - manuscript.md" out.pdf
argraver typst "My Book - manuscript.md"       # emit the generated .typ (no PDF)
```

The output defaults beside the manuscript; pass a second argument to choose the
path. Metadata is found via `--meta <file>`, then `$BOOK_META`, then
`Books/<Name>/<Project>/_metadata.yml`, then beside the manuscript.

## Inputs

- **Manuscript** — a Markdown file, typically a Longform-compiled
  `<Project> - manuscript.md`. `# H1` headings split chapters; a `---` rule
  inside a chapter is a scene break. Inline YAML blocks left by Longform are
  stripped automatically.
- **`_metadata.yml`** — a standalone file with `book:` and `cover:` maps.

### `book:` settings

| Key | Values | Notes |
| --- | --- | --- |
| `title`, `subtitle`, `author`, `publisher` | text | `title` is required |
| `copyright`, `edition`, `isbn` | text | sensible defaults synthesized when omitted |
| `dedication`, `description` | text | description → EPUB `dc:description` |
| `trim` | `pocket` · `small-digest` · `digest` · `trade` · `large` | page size (PDF) |
| `margins` | `narrow` · `normal` · `wide` | picks within the trim's margin ranges |
| `chapter_start` | `recto` · `any` | next right-hand page, or next page either side |
| `scenebreak` | `auto` · `ornament` · `blank` | `auto` = blank mid-page, `* * *` at a page edge |
| `body_font`, `display_font` | font family | PDF; embedded + subset by typst |
| `font_path` | directory | non-system `.ttf`/`.otf` to make available + embed |

`cover.background.image` points at the cover art (relative to the metadata file,
or vault-absolute). The EPUB embeds a resized copy and opens on it.

## Scope (v1)

In: EPUB, print PDF, the EPUB cover. Out: DOCX, `print-ready` (PDF/X-1a +
grayscale, via ghostscript), and the print wraparound cover (spine math).

## Documentation

- [`docs/DESIGN.md`](docs/DESIGN.md) — architecture and the decisions as built.
- [`docs/PORTING.md`](docs/PORTING.md) — the original design/rationale.
- `CLAUDE.md` — build/test/run reference and the module map.

## Credits

- Drop caps via the vendored [typst-droplet](https://github.com/EpicEricEE/typst-droplet)
  (`src/resources/droplet.typ`, MIT).
- The Typst interior design and `typst_escape` are ported from the Verkilo
  editor (lifted as source, not depended on).

## License

Custom license — Attribution · NonCommercial (CC BY-NC in spirit, for software).
You may use, modify, and redistribute (including modified copies) for
non-commercial purposes, with the notice intact; commercial use requires
permission from the Licensor. See [`LICENSE`](LICENSE). The vendored
`droplet.typ` keeps its own MIT license.
