//! Wadler-style pretty-printing doc combinators.
//!
//! A [`Doc`] is a value describing how text should lay out across one
//! or more lines, possibly with ANSI styling. The renderer makes
//! width-fitting decisions for each [`Doc::Group`]:
//! - if the group's flat form fits in the remaining width, lay it out
//!   on one line (newlines become spaces),
//! - otherwise lay it out with explicit newlines + indentation.
//!
//! ```
//! use effect_printer::*;
//!
//! let doc = group(concat3(
//!     text("hello"),
//!     line(),
//!     text("world"),
//! ));
//!
//! assert_eq!(render(&doc, 80),  "hello world");        // fits on one line
//! assert_eq!(render(&doc, 8),   "hello\nworld");       // doesn't, breaks
//! ```

use std::fmt::Write;

// ── Doc ──────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum Doc {
    /// The empty document.
    Nil,
    /// Literal text. Must not contain newlines — use [`Doc::Line`] for
    /// line breaks.
    Text(String),
    /// A line break. In a [`Doc::Group`] that fits, becomes a space.
    Line,
    /// Two docs side by side.
    Concat(Box<Doc>, Box<Doc>),
    /// Indent the inner doc by `n` additional spaces after each
    /// line break.
    Nest(usize, Box<Doc>),
    /// A grouping decision: try to fit the inner doc on one line.
    Group(Box<Doc>),
    /// Apply an ANSI style to the inner doc.
    Style(Style, Box<Doc>),
}

// ── Style ────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Bold,
    Italic,
    Underline,
    Dim,
    Fg(Color),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
}

impl Style {
    fn ansi_open(self) -> &'static str {
        match self {
            Style::Bold => "\x1b[1m",
            Style::Italic => "\x1b[3m",
            Style::Underline => "\x1b[4m",
            Style::Dim => "\x1b[2m",
            Style::Fg(Color::Black) => "\x1b[30m",
            Style::Fg(Color::Red) => "\x1b[31m",
            Style::Fg(Color::Green) => "\x1b[32m",
            Style::Fg(Color::Yellow) => "\x1b[33m",
            Style::Fg(Color::Blue) => "\x1b[34m",
            Style::Fg(Color::Magenta) => "\x1b[35m",
            Style::Fg(Color::Cyan) => "\x1b[36m",
            Style::Fg(Color::White) => "\x1b[37m",
        }
    }
}

const ANSI_RESET: &str = "\x1b[0m";

// ── Constructors ─────────────────────────────────────────────────

pub fn nil() -> Doc {
    Doc::Nil
}

pub fn text(s: impl Into<String>) -> Doc {
    Doc::Text(s.into())
}

pub fn line() -> Doc {
    Doc::Line
}

pub fn concat(a: Doc, b: Doc) -> Doc {
    Doc::Concat(Box::new(a), Box::new(b))
}

pub fn concat3(a: Doc, b: Doc, c: Doc) -> Doc {
    concat(a, concat(b, c))
}

/// Concatenate a list of docs.
pub fn hcat<I: IntoIterator<Item = Doc>>(docs: I) -> Doc {
    docs.into_iter().fold(Doc::Nil, concat)
}

/// Concatenate with `sep` between adjacent docs.
pub fn join(sep: Doc, docs: impl IntoIterator<Item = Doc>) -> Doc {
    let mut iter = docs.into_iter();
    let Some(first) = iter.next() else {
        return Doc::Nil;
    };
    iter.fold(first, |acc, d| concat3(acc, sep.clone(), d))
}

pub fn nest(n: usize, d: Doc) -> Doc {
    Doc::Nest(n, Box::new(d))
}

pub fn group(d: Doc) -> Doc {
    Doc::Group(Box::new(d))
}

pub fn with_style(s: Style, d: Doc) -> Doc {
    Doc::Style(s, Box::new(d))
}

// Convenience styling helpers.
pub fn bold(d: Doc) -> Doc      { with_style(Style::Bold, d) }
pub fn italic(d: Doc) -> Doc    { with_style(Style::Italic, d) }
pub fn underline(d: Doc) -> Doc { with_style(Style::Underline, d) }
pub fn dim(d: Doc) -> Doc       { with_style(Style::Dim, d) }
pub fn color(c: Color, d: Doc) -> Doc { with_style(Style::Fg(c), d) }

// ── Rendering ────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Mode {
    Flat,
    Break,
}

/// Render as plain text — ignores any [`Doc::Style`] wrapping.
pub fn render(doc: &Doc, width: usize) -> String {
    render_with(doc, width, false)
}

/// Render with ANSI escape codes for [`Doc::Style`] wrappers.
pub fn render_ansi(doc: &Doc, width: usize) -> String {
    render_with(doc, width, true)
}

fn render_with(root: &Doc, width: usize, emit_ansi: bool) -> String {
    let mut out = String::new();
    let mut col: usize = 0;
    let mut stack: Vec<(usize, Mode, &Doc)> = vec![(0, Mode::Break, root)];

    while let Some((indent, mode, doc)) = stack.pop() {
        match doc {
            Doc::Nil => {}
            Doc::Text(s) => {
                out.push_str(s);
                col += s.chars().count();
            }
            Doc::Line => match mode {
                Mode::Flat => {
                    out.push(' ');
                    col += 1;
                }
                Mode::Break => {
                    out.push('\n');
                    for _ in 0..indent {
                        out.push(' ');
                    }
                    col = indent;
                }
            },
            Doc::Concat(l, r) => {
                // Push right first so left is processed first.
                stack.push((indent, mode, r));
                stack.push((indent, mode, l));
            }
            Doc::Nest(n, d) => {
                stack.push((indent + n, mode, d));
            }
            Doc::Group(d) => {
                let remaining = width.saturating_sub(col);
                let chosen = if fits_flat(d, remaining) {
                    Mode::Flat
                } else {
                    Mode::Break
                };
                stack.push((indent, chosen, d));
            }
            Doc::Style(s, d) => {
                if emit_ansi {
                    out.push_str(s.ansi_open());
                    // We push a synthetic "reset" doc after the inner.
                    // Cheap trick: render inner, then append reset
                    // directly by recursing.
                    let inner = render_with_inner(d, width, emit_ansi, indent, col, mode);
                    out.push_str(&inner.text);
                    out.push_str(ANSI_RESET);
                    col = inner.col;
                } else {
                    stack.push((indent, mode, d));
                }
            }
        }
    }

    out
}

struct Rendered {
    text: String,
    col: usize,
}

fn render_with_inner(
    root: &Doc,
    width: usize,
    emit_ansi: bool,
    start_indent: usize,
    start_col: usize,
    start_mode: Mode,
) -> Rendered {
    let mut out = String::new();
    let mut col = start_col;
    let mut stack: Vec<(usize, Mode, &Doc)> = vec![(start_indent, start_mode, root)];

    while let Some((indent, mode, doc)) = stack.pop() {
        match doc {
            Doc::Nil => {}
            Doc::Text(s) => {
                out.push_str(s);
                col += s.chars().count();
            }
            Doc::Line => match mode {
                Mode::Flat => {
                    out.push(' ');
                    col += 1;
                }
                Mode::Break => {
                    out.push('\n');
                    for _ in 0..indent {
                        out.push(' ');
                    }
                    col = indent;
                }
            },
            Doc::Concat(l, r) => {
                stack.push((indent, mode, r));
                stack.push((indent, mode, l));
            }
            Doc::Nest(n, d) => stack.push((indent + n, mode, d)),
            Doc::Group(d) => {
                let remaining = width.saturating_sub(col);
                let chosen = if fits_flat(d, remaining) {
                    Mode::Flat
                } else {
                    Mode::Break
                };
                stack.push((indent, chosen, d));
            }
            Doc::Style(s, d) => {
                if emit_ansi {
                    out.push_str(s.ansi_open());
                    let inner = render_with_inner(d, width, emit_ansi, indent, col, mode);
                    out.push_str(&inner.text);
                    out.push_str(ANSI_RESET);
                    col = inner.col;
                } else {
                    stack.push((indent, mode, d));
                }
            }
        }
    }

    Rendered { text: out, col }
}

/// Does `doc` fit on the current line, given `remaining` chars? Walks
/// the doc assuming flat layout.
fn fits_flat(doc: &Doc, remaining: usize) -> bool {
    let mut rem: isize = remaining as isize;
    let mut stack: Vec<&Doc> = vec![doc];
    while let Some(d) = stack.pop() {
        if rem < 0 {
            return false;
        }
        match d {
            Doc::Nil => {}
            Doc::Text(s) => rem -= s.chars().count() as isize,
            Doc::Line => rem -= 1, // flat: space
            Doc::Concat(l, r) => {
                stack.push(r);
                stack.push(l);
            }
            Doc::Nest(_, d) | Doc::Group(d) | Doc::Style(_, d) => stack.push(d),
        }
    }
    rem >= 0
}

// ── Display ──────────────────────────────────────────────────────

/// Convenience: render `doc` to `f` at `width`, plain text.
pub fn fmt(doc: &Doc, width: usize, f: &mut impl Write) -> std::fmt::Result {
    f.write_str(&render(doc, width))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_renders_as_is() {
        let d = text("hello");
        assert_eq!(render(&d, 80), "hello");
    }

    #[test]
    fn line_breaks_in_break_mode() {
        let d = concat3(text("foo"), line(), text("bar"));
        assert_eq!(render(&d, 80), "foo\nbar");
    }

    #[test]
    fn group_with_room_flattens_lines_to_spaces() {
        let d = group(concat3(text("foo"), line(), text("bar")));
        assert_eq!(render(&d, 80), "foo bar");
    }

    #[test]
    fn group_without_room_keeps_lines() {
        let d = group(concat3(text("foo"), line(), text("bar")));
        assert_eq!(render(&d, 5), "foo\nbar");
    }

    #[test]
    fn nest_indents_after_lines() {
        let d = nest(
            2,
            concat3(text("foo"), line(), text("bar")),
        );
        assert_eq!(render(&d, 80), "foo\n  bar");
    }

    #[test]
    fn nested_indents_compound() {
        let d = nest(
            2,
            concat(
                text("a"),
                nest(
                    2,
                    concat3(line(), text("b"), concat(line(), text("c"))),
                ),
            ),
        );
        assert_eq!(render(&d, 80), "a\n    b\n    c");
    }

    #[test]
    fn join_separates_docs() {
        let d = join(
            text(", "),
            vec![text("a"), text("b"), text("c")],
        );
        assert_eq!(render(&d, 80), "a, b, c");
    }

    #[test]
    fn join_empty_yields_nil() {
        let d: Doc = join(text(", "), Vec::<Doc>::new());
        assert_eq!(render(&d, 80), "");
    }

    #[test]
    fn ansi_render_wraps_styled_text() {
        let d = bold(text("important"));
        assert_eq!(render_ansi(&d, 80), "\x1b[1mimportant\x1b[0m");
    }

    #[test]
    fn plain_render_ignores_style() {
        let d = bold(text("important"));
        assert_eq!(render(&d, 80), "important");
    }

    #[test]
    fn nested_styles_each_get_reset() {
        let d = bold(concat3(text("a "), italic(text("b")), text(" c")));
        let out = render_ansi(&d, 80);
        // Bold open, "a ", italic open, "b", reset, " c", reset
        assert!(out.starts_with("\x1b[1m"));
        assert!(out.contains("\x1b[3m"));
        assert!(out.ends_with("\x1b[0m"));
    }

    #[test]
    fn color_renders_with_correct_code() {
        let d = color(Color::Red, text("error"));
        assert_eq!(render_ansi(&d, 80), "\x1b[31merror\x1b[0m");
    }

    #[test]
    fn pipeline_doc_for_a_struct_renders_two_ways() {
        // { name: "alice", age: 30, tags: [a, b] }
        fn pair(k: &str, v: Doc) -> Doc {
            concat3(text(k), text(": "), v)
        }
        let inner = vec![
            pair("name", text("\"alice\"")),
            pair("age", text("30")),
            pair("tags", text("[a, b]")),
        ];
        let body = join(concat(text(","), line()), inner);
        let d = group(concat3(
            text("{"),
            nest(2, concat(line(), body)),
            concat(line(), text("}")),
        ));

        // Wide: one line
        assert_eq!(
            render(&d, 80),
            "{ name: \"alice\", age: 30, tags: [a, b] }"
        );

        // Narrow: multiline
        let multi = render(&d, 20);
        assert!(multi.contains("\n  name"));
        assert!(multi.contains("\n}"));
    }
}
