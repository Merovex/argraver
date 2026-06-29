// =============================================================================
// TYPST-DROPLET LIBRARY (Consolidated)
// =============================================================================
// A library for creating drop capitals in Typst documents.
// Consolidated from: https://github.com/EpicEricEE/typst-droplet
// License: MIT
//
// Usage:
//   #import "droplet.typ": dropcap
//   #dropcap(height: 3)[Lorem ipsum dolor sit amet...]
// =============================================================================

// -----------------------------------------------------------------------------
// UTILITY FUNCTIONS (from util.typ)
// -----------------------------------------------------------------------------

// Elements that can be split and have a 'body' field.
#let splittable = (strong, emph, underline, stroke, overline, highlight, smallcaps)

// Element function of spaces.
#let space = [ ].func()

// Converts the given content to a string.
#let to-string(body) = {
  if type(body) == str {
    body
  } else if body.has("text") {
    to-string(body.text)
  } else if body.has("child") {
    to-string(body.child)
  } else if body.has("children") {
    body.children.map(to-string).join()
  } else if body.func() in splittable {
    to-string(body.body)
  } else if body.func() == smartquote {
    // Unfortunately, we can only use "dumb" quotes here.
    if body.double { "\"" } else { "'" }
  } else if body.func() == enum.item {
    if body.has("number") {
      str(body.number) + ". " + to-string(body.body)
    } else {
      "+ " + to-string(body.body)
    }
  } else if body.func() == space {
    " "
  } else if body.func() == super {
    let body-string = to-string(body.body)
    if body-string != none and regex("^[0-9+\-=\(\)ni\s]+$") in body-string {
      body-string
        .replace("1", "¹")
        .replace("2", "²")
        .replace("3", "³")
        .replace(regex("[04-9]"), it => str.from-unicode(0x2070 + int(it.text)))
        .replace("+", "\u{207A}")
        .replace("-", "\u{207B}")
        .replace("=", "\u{207C}")
        .replace("(", "\u{207D}")
        .replace(")", "\u{207E}")
        .replace("n", "\u{207F}")
        .replace("i", "\u{2071}")
    }
  } else if body.func() == sub {
    let body-string = to-string(body.body)
    if body-string != none and regex("^[0-9+\-=\(\)aehk-pstx\s]+$") in body-string {
      body-string
        .replace(regex("[0-9]"), it => str.from-unicode(0x2080 + int(it.text)))
        .replace("+", "\u{208A}")
        .replace("-", "\u{208B}")
        .replace("=", "\u{208C}")
        .replace("(", "\u{208D}")
        .replace(")", "\u{208E}")
        .replace("a", "\u{2090}")
        .replace("e", "\u{2091}")
        .replace("o", "\u{2092}")
        .replace("x", "\u{2093}")
        .replace("h", "\u{2095}")
        .replace("k", "\u{2096}")
        .replace("l", "\u{2097}")
        .replace("m", "\u{2098}")
        .replace("n", "\u{2099}")
        .replace("p", "\u{209A}")
        .replace("s", "\u{209B}")
        .replace("t", "\u{209C}")
    }
  }
}

// Attaches a label after the split elements.
//
// The label is only attached to one of the elements, preferring the second
// one. If both elements are empty, the label is discarded. If the label is
// empty, the elements remain unchanged.
#let attach-label((first, second), label) = {
  if label == none {
    (first, second)
  } else if second != none {
    (first, [#second#label])
  } else if first != none {
    ([#first#label], second)
  } else {
    (none, none)
  }
}

// Returns whether the element is displayed inline.
// Wrapped in block(breakable: false) to prevent pagebreak errors during measure.
//
// Requires context.
#let inline(element) = {
  if element == none { false }
  else {
    let m1 = measure(block(breakable: false, h(0.1pt) + element))
    let m2 = measure(block(breakable: false, element))
    m1.width > m2.width
  }
}

// Resolve text direction, depending on the language.
//
// Requires context.
#let resolve-dir() = {
  let rtl-languages = (
    "ar", "dv", "fa", "he", "ks", "pa", "ps", "sd", "ug", "ur", "yi"
  )
  if text.dir != auto { text.dir }
  else if text.lang in rtl-languages { rtl }
  else { ltr }
}

// -----------------------------------------------------------------------------
// SPLIT FUNCTIONS (from split.typ)
// -----------------------------------------------------------------------------

// Joins the given children into a single content.
//
// If the children list is empty, an empty content is returned instead of none.
#let join(children) = {
  if children.len() == 0 {
    []
  } else {
    children.join()
  }
}

// Gets the number of breakpoints in the given content.
//
// A breakpoint must always be at a space. For example, the sequence
//   ([Hello], [ ], [my world!])
// has two inner breakpoints:
//  1. ([Hello],) - ([my world!],) - (sep: [ ])
//  2. ([Hello my],) - ([world!],) - (sep: " ")
//
// Returns: The number of breakpoints.
#let breakpoints(body) = {
  if type(body) == str {
    body.split(" ").len() - 1
  } else if body.has("text") {
    breakpoints(body.text)
  } else if body.has("child") {
    breakpoints(body.child)
  } else if body.has("children") {
    body.children.map(breakpoints).sum(default: 0)
  } else if body.func() in splittable {
    breakpoints(body.body)
  } else if body.func() in (space, linebreak, parbreak) {
    1
  } else {
    0
  }
}

// Splits the given content at a given breakpoint index.
//
// Content is split at spaces. A sequence can be split at any of its childrens'
// breakpoints (spaces), but in general not between children.
//
// Returns: A tuple of the first and second part.
#let split(body, index) = {
  // Shortcut for out-of-bounds indices.
  if index > breakpoints(body) {
    return (body, none, none)
  }
  if index < 0 {
    return split(body, calc.max(0, breakpoints(body) + index + 1))
  }

  // Handle string content.
  if type(body) == str {
    let words = body.split(" ")
    let first = words.slice(0, index).join(" ")
    let second = words.slice(index).join(" ")
    return (first, second, " ")
  }

  // Handle text content.
  if body.has("text") {
    let (text, ..fields) = body.fields()
    let _ = if "label" in fields { fields.remove("label") }
    let label = if body.has("label") { body.label }
    let func(it) = if it != none { body.func()(..fields, it) }
    let (first, second, sep) = split(text, index)
    return (..attach-label((func(first), func(second)), label), sep)
  }

  // Handle content with "body" field.
  if body.func() in splittable {
    let (body: text, ..fields) = body.fields()
    let _ = if "label" in fields { fields.remove("label") }
    let label = if body.has("label") { body.label }
    let func(it) = if it != none { body.func()(..fields, it) }
    let (first, second, sep) = split(text, index)
    return (..attach-label((func(first), func(second)), label), sep)
  }

  // Handle styled content.
  if body.has("child") {
    let (child, styles, ..fields) = body.fields()
    let _ = if "label" in fields { fields.remove("label") }
    let label = if body.has("label") { body.label }
    let func(it) = if it != none { body.func()(it, styles) }
    let (first, second, sep) = split(child, index)
    return (..attach-label((func(first), func(second)), label), sep)
  }

  // Handle sequences.
  if body.has("children") {
    let first = ()
    let second = ()
    let sep = none

    // Find child containing the breakpoint and split it.
    let sub-index = index
    for (i, child) in body.children.enumerate() {
      let child-breakpoints = breakpoints(child)

      // Check if current child contains splitting point.
      if sub-index <= child-breakpoints {
        if child.func() not in (space, linebreak, parbreak) {
          // Push split child (skip trailing spaces)
          let (child-first, child-second, child-sep) = split(child, sub-index)
          first.push(child-first)
          second.push(child-second)
          sep = child-sep
        } else {
          sep = child
        }
        second += body.children.slice(i + 1)
        break
      }
      sub-index -= child-breakpoints
      first.push(child)
    }
    return (join(first), join(second), sep)
  }

  // Handle unbreakable content.
  return if index == 0 { (none, body, none) } else { (body, none, none) }
}

// -----------------------------------------------------------------------------
// EXTRACT FUNCTIONS (from extract.typ)
// -----------------------------------------------------------------------------

// Regex for valid characters in front of the dropped capital.
#let regex-before = regex({
  "["
    "\"'"              // Dumb quotes
    "\p{C}"            // Control characters
    "\p{Pi}"           // Initial punctuation
    "\p{Ps}"           // Opening punctuation
    "\p{Z}"            // Spaces and separators
    "¹²³\u2070-\u209F" // Superscripts and subscripts
  "]+"
})

// Regex for valid characters behind the dropped capital.
#let regex-after = regex({
  "["
    "\."               // Full stop
    "\"'"              // Dumb quotes / apostrophe
    "\p{C}"            // Control characters
    "\p{Pf}"           // Final punctuation
    "\p{Pe}"           // Closing punctuation
    "\p{Z}"            // Spaces and separators
    "\p{M}"            // Combining marks
    "¹²³\u2070-\u209F" // Superscripts and subscripts
  "]+"
})

// Extracts the first letter of the given content.
//
// The first letter may be none if the content does not contain any letters.
// If the first child cannot be split further, that child is returned as the
// first letter.
//
// Returns: A tuple of the first letter and the rest.
#let extract-first-letter(body) = {
  // Handle string content.
  if type(body) == str {
    let letter = body.clusters().at(0, default: none)
    if letter == none {
      return (none, body)
    }
    let rest = body.clusters().slice(1).join()
    return (letter, rest)
  }

  // Handle text content.
  if body.has("text") {
    let (text, ..fields) = body.fields()
    let _ = if "label" in fields { fields.remove("label") }
    let label = if body.has("label") { body.label }
    let func(it) = if it != none { body.func()(..fields, it) }
    let (letter, rest) = extract-first-letter(body.text)
    return attach-label((letter, func(rest)), label)
  }

  // Handle content with "body" field.
  if body.func() in splittable {
    let (body: text, ..fields) = body.fields()
    let _ = if "label" in fields { fields.remove("label") }
    let label = if body.has("label") { body.label }
    let func(it) = if it != none { body.func()(..fields, it) }
    let (letter, rest) = extract-first-letter(text)
    return attach-label((letter, func(rest)), label)
  }

  // Handle styled content.
  if body.has("child") {
    let (child, styles, ..fields) = body.fields()
    let _ = if "label" in fields { fields.remove("label") }
    let label = if body.has("label") { body.label }
    let func(it) = if it != none { body.func()(it, styles) }
    let (letter, rest) = extract-first-letter(child)
    return attach-label((letter, func(rest)), label)
  }

  // Handle enumeration items (interpreted as text, e.g. "5. Body" or "+ Body")
  if body.func() == enum.item {
    let (body, ..fields) = body.fields()
    let number = fields.at("number", default: none)
    return if number == none {
      ("+", body)
    } else if number < 10 {
      (str(number), "." + body)
    } else {
      (str(number).first(), str(number).slice(1) + "." + body)
    }
  }

  // Handle list items (interpreted as text, e.g. "- Body")
  if body.func() == list.item {
    return ("-", body.body)
  }

  // Handle sequences.
  if body.has("children") {
    let child-pos = body.children.position(c => {
      c.func() not in (space, parbreak)
    })

    if child-pos == none {
      // There is no non-empty child, so no letter.
      return (none, body)
    }

    let child = body.children.at(child-pos)
    let (letter, rest) = extract-first-letter(child)
    if body.children.len() > child-pos {
      rest = (rest, ..body.children.slice(child-pos+1)).join()
    }
    return (letter, rest)
  }

  // Handle unbreakable content.
  return (body, none)
}

// Extracts the dropped capital from the given content.
//
// The dropped capital contains the first real letter (or number) of the
// content, but can be preceded by opening punctuation characters, and followed
// by a sequence of closing punctuation characters.
//
// For example, the dropped capital of "Hello, world!" is "H", and the
// dropped capital of "1. Hello, world!" is "1." including the dot.
//
// Returns: A tuple of the dropped capital and the rest.
#let extract(body) = {
  let (letter, rest) = extract-first-letter(body)
  if letter == none {
    return (none, body)
  }

  // We can only append punctuation characters if the first letter can be
  // converted to a string, but not if it's e.g. a 'box' or 'image'.
  if to-string(letter) != none {
    // Append opening punctuation characters until the first "real" letter.
    while regex-before in to-string(letter).last() {
      let (next-letter, new-rest) = extract-first-letter(rest)
      if next-letter == none { break }
      letter += next-letter
      rest = new-rest
    }

    // Append closing punctuation characters.
    let (next-letter, new-rest) = extract-first-letter(rest)
    while to-string(next-letter) != none and regex-after in to-string(next-letter) {
      letter += next-letter
      rest = new-rest
      (next-letter, new-rest) = extract-first-letter(rest)
    }
  }

  return (letter, rest)
}

// -----------------------------------------------------------------------------
// MAIN DROPCAP FUNCTION (from droplet.typ)
// -----------------------------------------------------------------------------

// Sets the font size so the resulting text height matches the given height.
// Wrapped in block(breakable: false) to prevent pagebreak errors during measure.
//
// Parameters:
// - height: The target height of the resulting text.
// - text-args: Named arguments to be passed to the underlying text element.
// - body: The content of the text element.
//
// Returns: The text with the adjusted size.
#let sized(height, ..text-args, body) = context {
  let styled-text = text.with(..text-args.named(), body)
  let measured = measure(block(breakable: false, styled-text(1em))).height
  let factor = if measured > 0pt { height / measured } else { 1 }
  styled-text(factor * 1em)
}

// Resolves the given height to an absolute length.
// Wrapped in block(breakable: false) to prevent pagebreak errors during measure.
//
// Height can be given as an integer, which is interpreted as the number of
// lines, or as a length.
//
// Requires context.
#let resolve-height(height) = {
  if type(height) == int {
    measure(block(breakable: false, [x\ ] * height)).height
  } else {
    height.to-absolute()
  }
}

// Shows the first letter of the given content in a larger font.
//
// If the first letter is not given as a positional argument, it is extracted
// from the content. The rest of the content is split into two pieces, where
// one is positioned next to the dropped capital, and the other below it.
//
// Parameters:
// - height: The height of the first letter. Can be given as the number of
//           lines (integer) or as a length. If set to `auto`, no scaling is
//           applied.
// - justify: Whether to justify the text next to the first letter.
// - gap: The space between the first letter and the text.
// - hanging-indent: The indent of lines after the first line.
// - overhang: The amount by which the first letter should overhang into the
//             margin. Ratios are relative to the width of the first letter.
// - depth: The minimum space below the first letter. Can be given as the
//          number of lines (integer) or as a length.
// - transform: A function to be applied to the first letter.
// - text-args: Named arguments to be passed to the underlying text element.
// - body: The content to be shown.
//
// Returns: The content with the first letter shown in a larger font.
#let dropcap(
  height: 2,
  justify: auto,
  gap: 0pt,
  hanging-indent: 0pt,
  overhang: 0pt,
  depth: 0pt,
  transform: none,
  ..text-args,
  body
) = layout(bounds => {
  let text-args = text-args

  if height != auto {
    // Set default top and bottom edge to "bounds" if not specified.
    if "top-edge" not in text-args.named() {
      text-args = arguments(..text-args, top-edge: "bounds")
    }
    if "bottom-edge" not in text-args.named() {
      text-args = arguments(..text-args, bottom-edge: "bounds")
    }
  }

  let (letter, rest) = if text-args.pos() == () {
    extract(body)
  } else {
    // First letter already given.
    (text-args.pos().first(), body)
  }

  if transform != none {
    letter = context transform(letter)
  }

  let letter-height = if height == auto {
    // Don't rescale if height is set to auto.
    measure(block(breakable: false, text(..text-args.named(), letter))).height
  } else {
    resolve-height(height)
  }

  let depth = resolve-height(depth)

  // Create dropcap with the height of sample content.
  let letter = box(
    height: letter-height + depth,
    sized(letter-height, letter, ..text-args.named())
  )
  let letter-width = measure(block(breakable: false, letter)).width

  // Resolve overhang if given as percentage.
  let overhang = if type(overhang) == ratio {
    letter-width * overhang
  } else if type(overhang) == relative {
    letter-width * overhang.ratio + overhang.length
  } else {
    overhang
  }

  // Resolve justify if given as auto.
  let justify = if justify == auto { par.justify } else { justify }

  // Try to justify as many words as possible next to dropcap.
  let bounded = box.with(width: bounds.width - letter-width - gap + overhang)

  let index = 1
  let top-position = 0pt
  let prev-height = 0pt
  let (first, second, sep) = while true {
    let (first, second, _) = split(rest, index)

    // Wrap in block(breakable: false) to prevent pagebreak errors during measure
    let height = measure(block(breakable: false, bounded({
      set par(justify: justify)
      let start = if resolve-dir() == ltr { "left" } else { "right" }
      pad(..((start): hanging-indent), h(-hanging-indent) + first)
    }))).height

    let (_, new, sep) = split(first, -1)
    top-position = calc.max(
      top-position,
      height - measure(block(breakable: false, new)).height - par.leading.to-absolute()
    )

    if top-position >= letter-height + depth - 1e-6pt and height > prev-height {
      // Limit reached, new element doesn't fit anymore
      split(rest, index - 1)
      break
    }

    if second == none {
      // All content fits next to dropcap.
      (first, none, none)
      break
    }

    index += 1
    prev-height = height
  }

  // Layout dropcap and aside text as grid.
  set par(justify: justify)

  // Find out whether there is a break between the first and second part.
  let has-break = type(sep) == content and sep.func() in (linebreak, parbreak)
  if not has-break {
    let sep = split(first, -1).at(2)
    has-break = type(sep) == content and sep.func() in (linebreak, parbreak)
  }

  // Find elements at boundary.
  let last-of-first = split(first, -1).at(1)
  let first-of-second = if second == none { none } else { split(second, 1).at(0) }

  let func(body) = if inline(last-of-first) {
    box(body) + linebreak()
  } else {
    block(body)
  }

  func(grid(
    column-gutter: gap,
    columns: (letter-width - overhang, 1fr),
    move(dx: -overhang, letter),
    {
      let start = if resolve-dir() == ltr { "left" } else { "right" }

      pad(..((start): hanging-indent), h(-hanging-indent) + {
        first

        if not has-break and inline(last-of-first) and inline(first-of-second) {
          linebreak(justify: justify)
        }
      })
    }
  ))

  if type(sep) == content and sep.func() in (linebreak, parbreak) { sep }

  second
})
