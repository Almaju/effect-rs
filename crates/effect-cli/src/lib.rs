//! An Effect-typed CLI parser, with help text rendered via
//! [`effect_printer`].
//!
//! Build a [`Command`] declaratively, parse a `Vec<String>` into a
//! [`ParsedCommand`], and reach for [`render_help`] when the user
//! asks. The Effect-wrapped entry point is [`parse_args`], which reads
//! `std::env::args` and produces `Effect<ParsedCommand, CliError, R>`.
//!
//! ```
//! use effect_cli::*;
//!
//! let cmd = Command::new("greet", "Say hello to someone")
//!     .arg(Arg::new("name", "Whom to greet"))
//!     .option(Opt::new("greeting", "How to greet").short('g').default("hello"))
//!     .flag(Flag::new("loud", "SHOUT IT").short('l'));
//!
//! let parsed = parse(&cmd, vec![
//!     "greet".into(), "alice".into(), "--loud".into(), "-g".into(), "hi".into(),
//! ]).unwrap();
//!
//! assert_eq!(parsed.arg("name"), Some("alice"));
//! assert_eq!(parsed.option("greeting"), Some("hi"));
//! assert_eq!(parsed.flag("loud"), true);
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use effect::Effect;
use effect_printer::{Color, Doc, Style, bold, color, concat, concat3, dim, group, hcat, join, line, nest, nil, render, render_ansi, text, with_style};
use thiserror::Error;

// ── Spec ─────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub args: Vec<Arg>,
    pub options: Vec<Opt>,
    pub flags: Vec<Flag>,
}

#[derive(Clone, Debug)]
pub struct Arg {
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug)]
pub struct Opt {
    pub long: String,
    pub short: Option<char>,
    pub description: String,
    pub default: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Flag {
    pub long: String,
    pub short: Option<char>,
    pub description: String,
}

impl Command {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Command {
            name: name.into(),
            description: description.into(),
            args: Vec::new(),
            options: Vec::new(),
            flags: Vec::new(),
        }
    }
    pub fn arg(mut self, a: Arg) -> Self {
        self.args.push(a);
        self
    }
    pub fn option(mut self, o: Opt) -> Self {
        self.options.push(o);
        self
    }
    pub fn flag(mut self, f: Flag) -> Self {
        self.flags.push(f);
        self
    }
}

impl Arg {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Arg {
            name: name.into(),
            description: description.into(),
        }
    }
}

impl Opt {
    pub fn new(long: impl Into<String>, description: impl Into<String>) -> Self {
        Opt {
            long: long.into(),
            short: None,
            description: description.into(),
            default: None,
        }
    }
    pub fn short(mut self, c: char) -> Self {
        self.short = Some(c);
        self
    }
    pub fn default(mut self, v: impl Into<String>) -> Self {
        self.default = Some(v.into());
        self
    }
}

impl Flag {
    pub fn new(long: impl Into<String>, description: impl Into<String>) -> Self {
        Flag {
            long: long.into(),
            short: None,
            description: description.into(),
        }
    }
    pub fn short(mut self, c: char) -> Self {
        self.short = Some(c);
        self
    }
}

// ── Parse ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ParsedCommand {
    args: HashMap<String, String>,
    options: HashMap<String, String>,
    flags: HashMap<String, bool>,
}

impl ParsedCommand {
    pub fn arg(&self, name: &str) -> Option<&str> {
        self.args.get(name).map(|s| s.as_str())
    }
    pub fn option(&self, long: &str) -> Option<&str> {
        self.options.get(long).map(|s| s.as_str())
    }
    pub fn flag(&self, long: &str) -> bool {
        self.flags.get(long).copied().unwrap_or(false)
    }
}

#[derive(Debug, Error, Clone)]
pub enum CliError {
    #[error("missing required argument <{name}>")]
    MissingArgument { name: String },

    #[error("--{name} requires a value")]
    MissingOptionValue { name: String },

    #[error("unknown flag/option: {0}")]
    Unknown(String),

    #[error("too many positional arguments (expected at most {expected}, got {got})")]
    TooManyArgs { expected: usize, got: usize },

    #[error("help requested")]
    HelpRequested,
}

/// Parse `argv` (typically `std::env::args().collect()`) against the
/// command's spec. The first element of `argv` is assumed to be the
/// program name and skipped.
pub fn parse(cmd: &Command, argv: Vec<String>) -> Result<ParsedCommand, CliError> {
    // Build lookup maps for options/flags.
    let mut long_opt: HashMap<&str, &Opt> = HashMap::new();
    let mut short_opt: HashMap<char, &Opt> = HashMap::new();
    for o in &cmd.options {
        long_opt.insert(o.long.as_str(), o);
        if let Some(s) = o.short {
            short_opt.insert(s, o);
        }
    }
    let mut long_flag: HashMap<&str, &Flag> = HashMap::new();
    let mut short_flag: HashMap<char, &Flag> = HashMap::new();
    for f in &cmd.flags {
        long_flag.insert(f.long.as_str(), f);
        if let Some(s) = f.short {
            short_flag.insert(s, f);
        }
    }

    let mut parsed = ParsedCommand::default();
    let mut positional: Vec<String> = Vec::new();

    let mut iter = argv.into_iter().skip(1);   // drop program name
    while let Some(tok) = iter.next() {
        if tok == "--help" || tok == "-h" {
            return Err(CliError::HelpRequested);
        }
        if let Some(stripped) = tok.strip_prefix("--") {
            // long-form, possibly with --key=value
            let (key, inline_value) = match stripped.split_once('=') {
                Some((k, v)) => (k, Some(v.to_string())),
                None => (stripped, None),
            };
            if let Some(f) = long_flag.get(key) {
                if inline_value.is_some() {
                    // --flag=anything is bogus for a boolean flag.
                    return Err(CliError::Unknown(tok));
                }
                parsed.flags.insert(f.long.clone(), true);
            } else if let Some(o) = long_opt.get(key) {
                let val = if let Some(v) = inline_value {
                    v
                } else {
                    iter.next().ok_or_else(|| CliError::MissingOptionValue {
                        name: key.to_string(),
                    })?
                };
                parsed.options.insert(o.long.clone(), val);
            } else {
                return Err(CliError::Unknown(tok));
            }
        } else if let Some(stripped) = tok.strip_prefix("-") {
            // short-form. Two cases:
            //  - "-x"    — single short option or flag
            //  - "-abc"  — combined short flags (only if EVERY char is a Flag)
            let chars: Vec<char> = stripped.chars().collect();
            if chars.is_empty() {
                positional.push(tok);
                continue;
            }
            if chars.len() == 1 {
                let c = chars[0];
                if let Some(f) = short_flag.get(&c) {
                    parsed.flags.insert(f.long.clone(), true);
                } else if let Some(o) = short_opt.get(&c) {
                    let val = iter.next().ok_or_else(|| CliError::MissingOptionValue {
                        name: o.long.clone(),
                    })?;
                    parsed.options.insert(o.long.clone(), val);
                } else {
                    return Err(CliError::Unknown(tok));
                }
            } else {
                // Combined short: every char must be a registered flag.
                let all_flags = chars.iter().all(|c| short_flag.contains_key(c));
                if !all_flags {
                    return Err(CliError::Unknown(tok));
                }
                for c in chars {
                    let f = short_flag
                        .get(&c)
                        .expect("verified all_flags above");
                    parsed.flags.insert(f.long.clone(), true);
                }
            }
        } else {
            positional.push(tok);
        }
    }

    // Assign positionals to declared args.
    if positional.len() > cmd.args.len() {
        return Err(CliError::TooManyArgs {
            expected: cmd.args.len(),
            got: positional.len(),
        });
    }
    for (arg, val) in cmd.args.iter().zip(positional.into_iter()) {
        parsed.args.insert(arg.name.clone(), val);
    }

    // Check required args.
    for a in &cmd.args {
        if !parsed.args.contains_key(&a.name) {
            return Err(CliError::MissingArgument {
                name: a.name.clone(),
            });
        }
    }

    // Apply option defaults.
    for o in &cmd.options {
        if !parsed.options.contains_key(&o.long) {
            if let Some(d) = &o.default {
                parsed.options.insert(o.long.clone(), d.clone());
            }
        }
    }

    Ok(parsed)
}

// ── Effect-wrapped entry point ───────────────────────────────────

/// Read `std::env::args` and parse them against `cmd`.
pub fn parse_args<R>(cmd: Command) -> Effect<ParsedCommand, CliError, R>
where
    R: Send + Sync + 'static,
{
    let cmd = Arc::new(cmd);
    Effect::sync(move || {
        let cmd = cmd.clone();
        let argv: Vec<String> = std::env::args().collect();
        parse(&cmd, argv)
    })
}

// ── Help rendering ───────────────────────────────────────────────

/// Render `--help` text for `cmd`. Plain text; use [`render_help_ansi`]
/// for the colour version.
pub fn render_help(cmd: &Command, width: usize) -> String {
    render(&help_doc(cmd), width)
}

pub fn render_help_ansi(cmd: &Command, width: usize) -> String {
    render_ansi(&help_doc(cmd), width)
}

fn help_doc(cmd: &Command) -> Doc {
    let mut sections: Vec<Doc> = Vec::new();

    // Title
    sections.push(bold(text(cmd.name.clone())));
    sections.push(text(""));
    sections.push(text(cmd.description.clone()));
    sections.push(text(""));

    // Usage
    let mut usage_parts: Vec<Doc> = vec![text(cmd.name.clone())];
    if !cmd.options.is_empty() || !cmd.flags.is_empty() {
        usage_parts.push(text("[OPTIONS]"));
    }
    for a in &cmd.args {
        usage_parts.push(text(format!("<{}>", a.name)));
    }
    sections.push(with_style(Style::Underline, text("USAGE")));
    sections.push(nest(2, concat(line(), join(text(" "), usage_parts))));
    sections.push(text(""));

    // Args
    if !cmd.args.is_empty() {
        sections.push(with_style(Style::Underline, text("ARGS")));
        let items: Vec<Doc> = cmd
            .args
            .iter()
            .map(|a| {
                let head = color(Color::Cyan, text(format!("<{}>", a.name)));
                concat3(head, text("  "), dim(text(a.description.clone())))
            })
            .collect();
        sections.push(nest(2, concat(line(), join(line(), items))));
        sections.push(text(""));
    }

    // Options
    if !cmd.options.is_empty() {
        sections.push(with_style(Style::Underline, text("OPTIONS")));
        let items: Vec<Doc> = cmd
            .options
            .iter()
            .map(|o| {
                let mut head_parts: Vec<Doc> = Vec::new();
                if let Some(s) = o.short {
                    head_parts.push(text(format!("-{s},")));
                }
                head_parts.push(text(format!("--{} <VALUE>", o.long)));
                let head = color(Color::Cyan, join(text(" "), head_parts));
                let mut tail = vec![dim(text(o.description.clone()))];
                if let Some(d) = &o.default {
                    tail.push(dim(text(format!("[default: {d}]"))));
                }
                concat3(head, text("  "), join(text(" "), tail))
            })
            .collect();
        sections.push(nest(2, concat(line(), join(line(), items))));
        sections.push(text(""));
    }

    // Flags
    if !cmd.flags.is_empty() {
        sections.push(with_style(Style::Underline, text("FLAGS")));
        let items: Vec<Doc> = cmd
            .flags
            .iter()
            .map(|f| {
                let mut head_parts: Vec<Doc> = Vec::new();
                if let Some(s) = f.short {
                    head_parts.push(text(format!("-{s},")));
                }
                head_parts.push(text(format!("--{}", f.long)));
                let head = color(Color::Cyan, join(text(" "), head_parts));
                concat3(head, text("  "), dim(text(f.description.clone())))
            })
            .collect();
        sections.push(nest(2, concat(line(), join(line(), items))));
        sections.push(text(""));
    }

    // Plus a help line.
    sections.push(dim(text("(use --help or -h for this message)")));

    // Stitch.
    let _ = (hcat::<Vec<Doc>>, group::<>, nil);  // keep imports used
    join(line(), sections)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        std::iter::once("prog")
            .chain(parts.iter().copied())
            .map(String::from)
            .collect()
    }

    fn sample_cmd() -> Command {
        Command::new("greet", "Say hello to someone")
            .arg(Arg::new("name", "Whom to greet"))
            .option(
                Opt::new("greeting", "Greeting word")
                    .short('g')
                    .default("hello"),
            )
            .flag(Flag::new("loud", "SHOUT IT").short('l'))
    }

    #[test]
    fn parses_a_positional_arg() {
        let p = parse(&sample_cmd(), argv(&["alice"])).unwrap();
        assert_eq!(p.arg("name"), Some("alice"));
    }

    #[test]
    fn applies_option_default() {
        let p = parse(&sample_cmd(), argv(&["alice"])).unwrap();
        assert_eq!(p.option("greeting"), Some("hello"));
    }

    #[test]
    fn parses_long_option() {
        let p = parse(&sample_cmd(), argv(&["alice", "--greeting", "hi"])).unwrap();
        assert_eq!(p.option("greeting"), Some("hi"));
    }

    #[test]
    fn parses_short_option() {
        let p = parse(&sample_cmd(), argv(&["alice", "-g", "hey"])).unwrap();
        assert_eq!(p.option("greeting"), Some("hey"));
    }

    #[test]
    fn parses_long_flag() {
        let p = parse(&sample_cmd(), argv(&["alice", "--loud"])).unwrap();
        assert!(p.flag("loud"));
    }

    #[test]
    fn parses_short_flag() {
        let p = parse(&sample_cmd(), argv(&["alice", "-l"])).unwrap();
        assert!(p.flag("loud"));
    }

    #[test]
    fn flag_defaults_to_false() {
        let p = parse(&sample_cmd(), argv(&["alice"])).unwrap();
        assert!(!p.flag("loud"));
    }

    #[test]
    fn rejects_missing_required_arg() {
        let err = parse(&sample_cmd(), argv(&[])).unwrap_err();
        assert!(matches!(err, CliError::MissingArgument { .. }));
    }

    #[test]
    fn rejects_too_many_positionals() {
        let err = parse(&sample_cmd(), argv(&["a", "b", "c"])).unwrap_err();
        assert!(matches!(err, CliError::TooManyArgs { .. }));
    }

    #[test]
    fn rejects_unknown_option() {
        let err = parse(&sample_cmd(), argv(&["alice", "--bogus"])).unwrap_err();
        assert!(matches!(err, CliError::Unknown(_)));
    }

    #[test]
    fn rejects_option_with_no_value() {
        let err = parse(&sample_cmd(), argv(&["alice", "--greeting"])).unwrap_err();
        assert!(matches!(err, CliError::MissingOptionValue { .. }));
    }

    #[test]
    fn help_token_is_reported_separately() {
        let err = parse(&sample_cmd(), argv(&["--help"])).unwrap_err();
        assert!(matches!(err, CliError::HelpRequested));
    }

    #[test]
    fn parses_long_option_with_inline_value() {
        let p = parse(&sample_cmd(), argv(&["alice", "--greeting=hi"])).unwrap();
        assert_eq!(p.option("greeting"), Some("hi"));
    }

    #[test]
    fn inline_value_on_a_flag_is_rejected() {
        let err = parse(&sample_cmd(), argv(&["alice", "--loud=yes"])).unwrap_err();
        assert!(matches!(err, CliError::Unknown(_)));
    }

    #[test]
    fn combined_short_flags_are_expanded() {
        // Build a command with two short flags so we can combine them.
        let cmd = Command::new("x", "x")
            .arg(Arg::new("name", "n"))
            .flag(Flag::new("verbose", "v").short('v'))
            .flag(Flag::new("quiet", "q").short('q'));
        let p = parse(&cmd, argv(&["alice", "-vq"])).unwrap();
        assert!(p.flag("verbose"));
        assert!(p.flag("quiet"));
    }

    #[test]
    fn combined_short_with_an_option_is_rejected() {
        // -lg combines flag -l with option -g, which would need a value.
        let err = parse(&sample_cmd(), argv(&["alice", "-lg"])).unwrap_err();
        assert!(matches!(err, CliError::Unknown(_)));
    }

    #[test]
    fn render_help_includes_usage_args_options_flags() {
        let out = render_help(&sample_cmd(), 80);
        assert!(out.contains("USAGE"));
        assert!(out.contains("ARGS"));
        assert!(out.contains("OPTIONS"));
        assert!(out.contains("FLAGS"));
        assert!(out.contains("<name>"));
        assert!(out.contains("--greeting"));
        assert!(out.contains("--loud"));
        assert!(out.contains("[default: hello]"));
    }

    #[test]
    fn render_help_ansi_emits_escape_codes() {
        let out = render_help_ansi(&sample_cmd(), 80);
        assert!(out.contains("\x1b["), "expected ANSI escapes, got: {out}");
    }

    #[tokio::test]
    async fn parse_args_effect_runs() {
        // Smoke test — std::env::args returns the test binary's argv,
        // which doesn't match our spec; we expect a CLI error, not a
        // success. The point is the Effect runs to completion.
        use effect::Exit;
        let exit = parse_args::<()>(sample_cmd()).execute().await;
        // The error path is one of: MissingArgument, Unknown, etc.
        match exit {
            Exit::Failure(_) | Exit::Success(_) => {}
        }
    }
}
