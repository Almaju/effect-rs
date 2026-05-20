# Pretty Printing

`effect-printer` is a small Wadler-style doc combinator library with
optional ANSI styling. It's the renderer behind
[`effect-cli`](./cli.md)'s help text and is also useful on its own
for building structured terminal output.

## Building documents

A [`Doc`] is composed from a handful of primitives:

| Constructor      | What it produces                                               |
| ---------------- | -------------------------------------------------------------- |
| `nil()`          | empty                                                          |
| `text(s)`        | literal text (no newlines — use `line()` for breaks)           |
| `line()`         | a line break; collapses to a space inside a fitting `group`    |
| `concat(a, b)`   | `a` followed by `b`                                            |
| `concat3(a,b,c)` | three-way concat                                               |
| `hcat([...])`    | concat over an iterable                                        |
| `join(sep, [.])` | join with `sep` between adjacent docs                          |
| `nest(n, d)`     | indent `d`'s line breaks by `n` more spaces                    |
| `group(d)`       | try `d` flat-on-one-line; if it doesn't fit, break             |

Plus styling:

```rust
# use effect_printer::*;
let _ = bold(text("title"));
let _ = italic(text("emphasis"));
let _ = underline(text("section"));
let _ = dim(text("(faint)"));
let _ = color(Color::Red, text("error"));
```

## The Wadler algorithm in one paragraph

A `group` is the layout switch. The renderer measures whether the
group's flat form (every `line` → space) fits in the remaining
horizontal budget. If yes, lay it out flat. If no, lay it out with
explicit `\n` + the current `nest` indent. Nested groups each get
their own fit check, so wide layouts get broken at the outermost level
that needs to break — exactly what you want for source-code-like
output.

## Two simple examples

```rust
use effect_printer::*;

let doc = group(concat3(text("hello"), line(), text("world")));

assert_eq!(render(&doc, 80), "hello world");      // fits → flat
assert_eq!(render(&doc, 8),  "hello\nworld");     // doesn't → breaks
```

A struct-like layout that adapts:

```rust
use effect_printer::*;

fn pair(k: &str, v: Doc) -> Doc {
    concat3(text(k), text(": "), v)
}

let body = join(concat(text(","), line()), vec![
    pair("name", text("\"alice\"")),
    pair("age",  text("30")),
    pair("tags", text("[a, b]")),
]);
let doc = group(concat3(
    text("{"),
    nest(2, concat(line(), body)),
    concat(line(), text("}")),
));

// Wide terminal: one line.
let _wide = render(&doc, 80);
//   { name: "alice", age: 30, tags: [a, b] }

// Narrow terminal: indented and broken.
let _narrow = render(&doc, 20);
//   {
//     name: "alice",
//     age: 30,
//     tags: [a, b]
//   }
```

## Rendering modes

| Renderer                | Output                                          |
| ----------------------- | ----------------------------------------------- |
| `render(&doc, width)`   | plain text — strips all `Style` wrappers        |
| `render_ansi(&doc, w)`  | ANSI escapes around `Style`-wrapped subtrees    |

`render_ansi` opens an escape (e.g. `\x1b[1m` for bold) on enter and
emits `\x1b[0m` on exit. Nested styles each get their own reset.

## When to reach for it

- **CLI help text** — already used inside `effect-cli`.
- **Pretty-printing typed values** — `format!("{:?}", value)` is fine
  for quick debugging; `effect-printer` is for layouts that adapt to
  the terminal width.
- **REPL output** — colourized prompts, structured results.
- **Error reports** — wrap field paths in colour and nest causes for
  readability.

## What's coming

- **`Doc::flat_alt(flat, multi)`** — explicit alternatives for the two
  layouts.
- **`render_to(&doc, width, w: impl io::Write)`** — stream output
  without allocating the whole string.
- **Background colors** + a `RGB(u8, u8, u8)` variant.
- **Width-aware ANSI strip** — count visible columns when computing
  fit, ignoring escape codes (matters when piping styled output to
  pagers).

[`Doc`]: https://docs.rs/effect-printer/latest/effect_printer/enum.Doc.html
