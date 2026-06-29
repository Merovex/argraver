#!/usr/bin/env ruby
# frozen_string_literal: true

# cover.rb — populate a per-book Typst cover from a shared series template.
#
# A *series* defines its cover look once in `series-cover.yaml`. Each *book*
# supplies only what changes — title, author, ISBN, and an optional handful of
# `overrides:` — in the YAML frontmatter of its Markdown file. This tool merges
# the two (book wins), fills defaults, computes the spine, and writes a fully
# populated, self-contained `<book>.cover.typ` you can hand straight to `typst`.
#
# Because the *layout* lives entirely in series_cover.typ, every book in the
# series shares the same design unless it explicitly overrides a value.
#
# Precedence (low → high):
#   built-in defaults  <  series-cover.yaml  <  book frontmatter  <  overrides:
#
# Usage:
#   ruby bin/cover.rb [options] BOOK.md
#     -s, --series PATH     series-cover.yaml (default: nearest one above BOOK.md)
#     -t, --template PATH   series_cover.typ  (default: _templates/series_cover.typ)
#     -o, --output PATH     output .typ       (default: BOOK.cover.typ)
#         --json            also write BOOK.cover_config.json
#
# Usually run via bin/build-cover, which then compiles the .typ to PDF.
# Starter config: _templates/series-cover.example.yaml.
# Stdlib only — no gems required.

require "yaml"
require "json"
require "optparse"

module Cover
  # Built-in defaults. These guarantee every field the template hard-accesses
  # exists, so a minimal book file still renders. Mirrors series_cover.typ.
  DEFAULTS = {
    "page" => {
      "trim_width" => 6.0, "trim_height" => 9.0, "bleed" => 0.125,
      "spine_width" => 0.0, "show_spine_text" => true,
      "page_count" => 0, "paper" => "white"
    },
    "margins" => {
      "front_top" => 18, "front_bottom" => 18, "front_outer" => 18, "front_spine" => 18,
      "back_top" => 18, "back_bottom" => 108, "back_outer" => 18, "back_spine" => 27,
      "spine_top" => 18, "spine_bottom" => 18, "spine_side_safety" => 9
    },
    "background" => {
      "style" => "solid", "base_color" => "#333333", "image" => "",
      "zoom" => 1.0, "offset_x" => 0.0, "offset_y" => 0.0,
      "darken" => 0, "gradient" => "none"
    },
    "fonts" => {
      "title" => {
        "family" => "Source Sans 3", "weight" => 700, "size" => 48, "color" => "#ffffff",
        "tracking" => 0.0, "align" => "center", "anchor" => "top", "offset" => 96,
        "shadow_opacity" => 0
      },
      "author" => {
        "family" => "Source Sans 3", "weight" => 400, "size" => 24, "color" => "#ffffff",
        "tracking" => 0.0, "align" => "center", "anchor" => "bottom", "offset" => 24,
        "shadow_opacity" => 0
      }
      # fonts.series / fonts.blurb are intentionally omitted: the template hides
      # them unless the series or book defines them.
    },
    "spine" => { "imprint_upright" => true, "imprint_scale" => 0.8, "imprint_gap" => 8 },
    "isbn" => {
      "show" => false, "display" => "", "barcode" => "", "width" => 2.0,
      "barcode_width" => 1.8, "from_right" => 0.25, "dy_from_bottom" => 0.5,
      "barcode_font" => "OCRB", "label_size" => 9, "number_size" => 10
    },
    "imprint" => {
      "show" => false, "logo" => "", "height" => 36, "from_left" => 18, "zone_height" => 1.0
    },
    "content" => {
      "title" => "", "author" => "", "series_text" => "", "subtitle" => "", "blurb" => ""
    }
  }.freeze

  # Named trim presets (book.trim) — keep the cover locked to the interior.
  TRIM_PRESETS = {
    "pocket"       => [5.0,  8.0],
    "small-digest" => [5.25, 8.0],
    "digest"       => [5.5,  8.5],
    "trade"        => [6.0,  9.0],
    "large"        => [7.0,  10.0],
  }.freeze

  # Paper thickness (inches/page) — matches the frontend spine-calculator.
  SPINE_RATE = { "white" => 0.002252, "cream" => 0.002347 }.freeze

  CONFIG_LINE = /^#let cfg = .*@cover-config-line.*$/.freeze

  module_function

  # --- public entry ----------------------------------------------------------

  def build(series_yaml:, book_md:, template:)
    config = resolve(series_yaml, book_md)
    typst = populate(template, config)
    [typst, config]
  end

  def resolve(series_yaml, book_md)
    series = series_yaml.to_s.strip.empty? ? {} : (YAML.safe_load(series_yaml) || {})
    raise "series-cover.yaml must be a mapping" unless series.is_a?(Hash)

    book, body = parse_frontmatter(book_md)

    # Unified _metadata.yml: a standalone YAML file (no `---` fence) carrying
    # `book:` (facts) and `cover:` (design) namespaces.
    facts = {}
    if book.empty? && !body.strip.empty?
      doc = (YAML.safe_load(body) rescue nil)
      if doc.is_a?(Hash) && (doc["cover"] || doc["book"])
        facts = doc["book"] || {}
        book  = doc["cover"] || {}
        body  = ""
      end
    else
      # Legacy markdown front matter: cover config under `cover:`, flat fallback.
      book = book["cover"] if book.is_a?(Hash) && book["cover"].is_a?(Hash)
    end

    # book: is authoritative — pull shared facts in so they are never duplicated
    # in the cover block. (Only fill what the cover didn't explicitly override.)
    if facts.is_a?(Hash) && !facts.empty?
      book["title"]  ||= facts["title"]
      book["author"] ||= facts["author"]
      book["blurb"]  ||= facts["description"]
      book["isbn"]   ||= facts["isbn"]
    end

    overrides = book.delete("overrides") || {}
    lift_shortcuts!(book)

    # The Markdown body is the back-cover blurb (Typst markup: **bold**, _italic_,
    # etc.), unless the frontmatter set `blurb:` explicitly.
    if !body.strip.empty? && (book.dig("content", "blurb").to_s.strip.empty?)
      (book["content"] ||= {})["blurb"] = body.strip
    end

    merged = deep_merge(deep_merge(deep_dup(DEFAULTS), series), book)
    apply_overrides!(merged, overrides)

    # Named trim preset (book.trim) is authoritative — keeps the cover trim
    # locked to the interior's.
    tname = facts["trim"] if facts.is_a?(Hash)
    if tname && (dims = TRIM_PRESETS[tname.to_s])
      merged["page"]["trim_width"], merged["page"]["trim_height"] = dims
    end

    # The blurb is authored in Markdown; convert to Typst markup so the template
    # can `eval` it safely (a bare `#` would otherwise be read as Typst code).
    blurb = merged.dig("content", "blurb")
    merged["content"]["blurb"] = md_to_typst(blurb) if blurb.is_a?(String) && !blurb.empty?

    page = merged["page"]
    if page["spine_width"].to_f <= 0 && page["page_count"].to_i > 0
      page["spine_width"] = compute_spine(page["page_count"].to_i, page["paper"].to_s)
    end
    merged
  end

  # --- frontmatter -----------------------------------------------------------

  # Returns [frontmatter_hash, body_string].
  def parse_frontmatter(md)
    text = md.to_s.dup.force_encoding("UTF-8").sub(/\A\xEF\xBB\xBF/, "") # strip BOM
    m = text.match(/\A---\s*\n(.*?)\n---\s*(?:\n|\z)/m)
    return [{}, text] unless m

    data = YAML.safe_load(m[1], permitted_classes: [], aliases: false) || {}
    raise "book frontmatter must be a mapping" unless data.is_a?(Hash)

    [data, m.post_match]
  end

  # Lift convenient top-level keys into the nested config shape so a book file
  # can stay terse (title:, author:, isbn: instead of content.title, ...).
  def lift_shortcuts!(book)
    content = (book["content"] ||= {})
    %w[title author subtitle blurb].each do |k|
      content[k] = book.delete(k) if book.key?(k)
    end

    if book.key?("series")
      series = book.delete("series").to_s
      num = book.delete("book_number")
      content["series_text"] = num.nil? ? series : "#{series} : #{num}"
    end

    if book.key?("isbn")
      isbn = book.delete("isbn").to_s
      unless isbn.empty?
        node = (book["isbn"] = book["isbn"].is_a?(Hash) ? book["isbn"] : {})
        node["display"] = isbn
        node["show"] = true unless node.key?("show")
      end
    end
  end

  # --- merge helpers ---------------------------------------------------------

  def deep_merge(a, b)
    return deep_dup(b) unless a.is_a?(Hash) && b.is_a?(Hash)

    out = deep_dup(a)
    b.each do |k, v|
      out[k] = out.key?(k) ? deep_merge(out[k], v) : deep_dup(v)
    end
    out
  end

  def deep_dup(v)
    case v
    when Hash  then v.each_with_object({}) { |(k, val), h| h[k] = deep_dup(val) }
    when Array then v.map { |e| deep_dup(e) }
    else v
    end
  end

  # Apply dotted-key overrides like {"title.size" => 42} onto the merged tree.
  def apply_overrides!(root, overrides)
    return unless overrides.is_a?(Hash)

    overrides.each do |dotted, value|
      parts = dotted.to_s.split(".")
      cursor = root
      parts.each_with_index do |part, i|
        if i == parts.length - 1
          cursor[part] = value
        else
          cursor[part] = {} unless cursor[part].is_a?(Hash)
          cursor = cursor[part]
        end
      end
    end
  end

  def compute_spine(page_count, paper)
    rate = SPINE_RATE.fetch(paper.downcase, SPINE_RATE["white"])
    (page_count * rate).round(4)
  end

  # --- EAN-13 barcode (an ISBN-13 *is* an EAN-13) ----------------------------

  # Left-hand digit encodings (odd/L and even/G parity) and right-hand (R).
  EAN_L = %w[0001101 0011001 0010011 0111101 0100011
             0110001 0101111 0111011 0110111 0001011].freeze
  EAN_G = %w[0100111 0110011 0011011 0100001 0011101
             0111001 0000101 0010001 0001001 0010111].freeze
  EAN_R = %w[1110010 1100110 1101100 1000010 1011100
             1001110 1010000 1000100 1001000 1110100].freeze
  # Which of the six left digits use L vs G, selected by the first digit.
  EAN_PARITY = %w[LLLLLL LLGLGG LLGGLG LLGGGL LGLLGG
                  LGGLLG LGGGLL LGLGLG LGLGGL LGGLGL].freeze

  # EAN-13 check digit over the first 12 digits.
  def ean13_check(d12)
    sum = d12.each_with_index.sum { |n, i| n * (i.even? ? 1 : 3) }
    (10 - (sum % 10)) % 10
  end

  # Normalize any ISBN/EAN string to 13 valid digits, or nil if it can't be one.
  # Accepts EAN-13/ISBN-13 (check verified), 12 digits (check appended), and
  # ISBN-10 (converted to 978-prefixed ISBN-13).
  def ean13_digits(str)
    digits = str.to_s.gsub(/\D/, "").chars.map(&:to_i)
    case digits.length
    when 13 then ean13_check(digits[0, 12]) == digits[12] ? digits : nil
    when 12 then digits + [ean13_check(digits)]
    when 10 then (body = [9, 7, 8] + digits[0, 9]) && body + [ean13_check(body)]
    end
  end

  # 95-module bit string: start(101) · 6 left · center(01010) · 6 right · end(101).
  def ean13_bits(digits)
    parity = EAN_PARITY[digits[0]]
    left = digits[1, 6].each_with_index.map do |d, i|
      parity[i] == "L" ? EAN_L[d] : EAN_G[d]
    end
    right = digits[7, 6].map { |d| EAN_R[d] }
    "101" + left.join + "01010" + right.join + "101"
  end

  # Render the barcode as a self-contained SVG (bars only; the cover prints the
  # human-readable number separately). Quiet zones included for scannability.
  def ean13_svg(digits)
    bits = ean13_bits(digits)
    quiet_l = 11
    quiet_r = 7
    width = quiet_l + bits.length + quiet_r # 11 + 95 + 7 = 113 modules
    height = 70
    rects = +""
    x = quiet_l
    bits.each_char do |bit|
      rects << %(<rect x="#{x}" y="0" width="1" height="#{height}"/>) if bit == "1"
      x += 1
    end
    %(<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 #{width} 76">) +
      %(<rect width="#{width}" height="76" fill="#fff"/><g fill="#000">#{rects}</g></svg>)
  end

  # --- template population ---------------------------------------------------

  def populate(template, config)
    line = "#let cfg = #{to_typst(config)}"
    if template =~ CONFIG_LINE
      template.sub(CONFIG_LINE, line)
    else
      # No marker found — prepend so cfg is defined before the layout runs.
      "#{line}\n\n#{template}"
    end
  end

  # --- Markdown → Typst markup (just enough for back-cover blurbs) -----------

  # Converts a small, common Markdown subset to Typst markup: paragraphs,
  # `#`–`######` headings, `-`/`*`/`+` bullet lists, **bold**, *italic*/_italic_.
  # Everything else is escaped so it renders literally (no stray Typst code).
  def md_to_typst(md)
    md.to_s.strip.split(/\n{2,}/).map { |para| md_block(para.strip) }.join("\n\n")
  end

  def md_block(block)
    lines = block.split("\n")
    if lines.length == 1 && (m = lines[0].match(/\A(\#{1,6})\s+(.*)\z/))
      return ("=" * m[1].length) + " " + md_inline(m[2])
    end
    if !lines.empty? && lines.all? { |l| l =~ /\A\s*[-*+]\s+/ }
      return lines.map { |l| "- " + md_inline(l.sub(/\A\s*[-*+]\s+/, "")) }.join("\n")
    end
    md_inline(lines.join(" ")) # collapse soft-wrapped lines into one paragraph
  end

  def md_inline(text)
    bolds = []
    italics = []
    t = text.gsub(/\*\*(.+?)\*\*/) { bolds << Regexp.last_match(1); " B#{bolds.size - 1} " }
    t = t.gsub(/\*(.+?)\*/) { italics << Regexp.last_match(1); " I#{italics.size - 1} " }
    t = t.gsub(/_(.+?)_/) { italics << Regexp.last_match(1); " I#{italics.size - 1} " }
    t = escape_typst(t)
    t = t.gsub(/ B(\d+) /) { "*#{escape_typst(bolds[Regexp.last_match(1).to_i])}*" }
    t.gsub(/ I(\d+) /) { "_#{escape_typst(italics[Regexp.last_match(1).to_i])}_" }
  end

  # Escape characters that would otherwise trigger Typst markup or code.
  def escape_typst(s)
    s.gsub(/([\\#$@*_`<>=\[\]])/) { "\\#{Regexp.last_match(1)}" }
  end

  # Render a Ruby value as a Typst literal (dict / array / scalar), pretty.
  IDENT = /\A[a-zA-Z_][a-zA-Z0-9_-]*\z/.freeze

  # Typst keywords can't be used as bare dict keys (e.g. `show`, `in`). Quote
  # them — the template reads such fields via `getd(d, "show", …)` so the string
  # key resolves identically.
  RESERVED = %w[
    none auto true false not and or let set show if else for while in
    break continue return import include as context
  ].freeze

  def to_typst(value, indent = 0)
    pad = "  " * indent
    inner = "  " * (indent + 1)
    case value
    when Hash
      return "(:)" if value.empty?

      pairs = value.map do |k, v|
        bare = k.to_s =~ IDENT && !RESERVED.include?(k.to_s)
        key = bare ? k.to_s : k.to_s.inspect
        "#{inner}#{key}: #{to_typst(v, indent + 1)}"
      end
      "(\n#{pairs.join(",\n")},\n#{pad})"
    when Array
      return "()" if value.empty?

      items = value.map { |e| "#{inner}#{to_typst(e, indent + 1)}" }
      "(\n#{items.join(",\n")},\n#{pad})"
    when String  then typst_string(value)
    when Integer then value.to_s
    when Float   then format_float(value)
    when true    then "true"
    when false   then "false"
    when nil     then "none"
    else typst_string(value.to_s)
    end
  end

  def typst_string(str)
    %("#{str.gsub("\\", "\\\\\\\\").gsub('"', '\"').gsub("\n", "\\n")}")
  end

  def format_float(f)
    return f.to_i.to_s if f.finite? && f == f.to_i # 6.0 -> "6", Typst reads both

    f.to_s
  end
end

# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------
if $PROGRAM_NAME == __FILE__
  options = { json: false }
  # Template ships in the vault's _templates/ (bin/ -> vault root -> _templates).
  default_template = File.expand_path("../_templates/series_cover.typ", __dir__)

  parser = OptionParser.new do |o|
    o.banner = "Usage: ruby bin/cover.rb [options] BOOK.md"
    o.on("-s", "--series PATH", "series-cover.yaml (default: nearest one above BOOK.md)") { |v| options[:series] = v }
    o.on("-t", "--template PATH", "series_cover.typ template") { |v| options[:template] = v }
    o.on("-o", "--output PATH", "output .typ (default: BOOK.cover.typ)") { |v| options[:output] = v }
    o.on("--json", "also write BOOK.cover_config.json") { options[:json] = true }
    o.on("-h", "--help") { puts o; exit 0 }
  end
  parser.parse!

  book_path = ARGV.shift
  abort parser.to_s if book_path.nil?
  abort "book not found: #{book_path}" unless File.file?(book_path)

  # Find series-cover.yaml by walking up from the book toward the vault root —
  # a book lives at Series/<Name>/<Book>/, the series config at Series/<Name>/.
  def find_series(start)
    dir = File.expand_path(File.dirname(start))
    loop do
      candidate = File.join(dir, "series-cover.yaml")
      return candidate if File.file?(candidate)

      parent = File.dirname(dir)
      break if parent == dir # reached filesystem root

      dir = parent
    end
    nil
  end

  series_path = options[:series] || find_series(book_path)
  series_yaml = series_path && File.file?(series_path) ? File.read(series_path) : ""
  if series_yaml.empty?
    warn "warning: no series-cover.yaml found above #{book_path}; using defaults only"
  else
    warn "series: #{series_path}"
  end

  template_path = options[:template] || default_template
  abort "template not found: #{template_path}" unless File.file?(template_path)

  base = book_path.sub(/\.[^.\/]+\z/, "")
  out_path = options[:output] || "#{base}.cover.typ"
  out_base = out_path.sub(/\.[^.\/]+\z/, "")

  begin
    config = Cover.resolve(series_yaml, File.read(book_path))

    # Auto-generate an EAN-13 barcode from the ISBN, unless the book supplied its
    # own barcode image. Written beside the .typ; referenced with a relative path
    # so it resolves regardless of Typst's --root.
    isbn = config["isbn"]
    if isbn["show"] && !isbn["display"].to_s.empty? && isbn["barcode"].to_s.empty?
      digits = Cover.ean13_digits(isbn["display"])
      if digits
        svg_name = "#{File.basename(out_base)}.barcode.svg"
        File.write(File.join(File.dirname(out_path), svg_name), Cover.ean13_svg(digits))
        isbn["barcode"] = "./#{svg_name}"
        # Keep the author's hyphenated text when they gave a full ISBN-13; only
        # replace it when we computed/converted the digits (12-digit or ISBN-10).
        isbn["display"] = digits.join unless isbn["display"].to_s.gsub(/\D/, "").length == 13
        puts "wrote #{svg_name} (EAN-13 #{digits.join})"
      else
        warn "warning: ISBN #{isbn['display'].inspect} is not a valid ISBN/EAN-13; barcode skipped"
      end
    end

    typst = Cover.populate(File.read(template_path), config)
  rescue StandardError => e
    abort "error: #{e.message}"
  end

  File.write(out_path, typst)
  puts "wrote #{out_path}"

  if options[:json]
    json_path = "#{base}.cover_config.json"
    File.write(json_path, JSON.pretty_generate(config))
    puts "wrote #{json_path}"
  end
end
