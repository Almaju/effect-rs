# CLI

`effect-cli` is a small, Effect-typed argv parser. Help text is
rendered by [`effect-printer`] (Wadler-style doc combinators with
ANSI styling).

```toml
[dependencies]
effect         = { version = "0.0.1" }
effect-cli     = { version = "0.0.1" }
effect-printer = { version = "0.0.1" }  # optional, if you re-render help yourself
```

## Build a command, parse argv

```rust,no_run
use effect_cli::*;

# fn main() {
let cmd = Command::new("greet", "Say hello to someone")
    .arg(Arg::new("name", "Whom to greet"))
    .option(
        Opt::new("greeting", "Greeting word")
            .short('g')
            .default("hello"),
    )
    .flag(Flag::new("loud", "SHOUT IT").short('l'));

let parsed = parse(&cmd, vec![
    "greet".into(),
    "alice".into(),
    "--loud".into(),
    "-g".into(), "hi".into(),
]).unwrap();

assert_eq!(parsed.arg("name"),       Some("alice"));
assert_eq!(parsed.option("greeting"), Some("hi"));
assert_eq!(parsed.flag("loud"),       true);
# }
```

## The Effect entry point

`parse_args(cmd)` reads `std::env::args` and produces an
`Effect<ParsedCommand, CliError, R>` you can compose with the rest of
your program:

```rust,no_run
use effect::Effect;
use effect_cli::{parse_args, render_help_ansi, Command, Arg};

# fn build_cmd() -> Command { Command::new("x", "x") }
# fn run_with(_: effect_cli::ParsedCommand) -> Effect<(), String, ()> { Effect::succeed(()) }
# #[tokio::main] async fn main() {
let cmd = build_cmd();
let program: Effect<(), String, ()> = Effect::block(move |g| {
    let cmd = cmd.clone();
    async move {
        match g.run_exit(parse_args::<()>(cmd.clone())).await {
            effect::Exit::Success(parsed) => g.run(run_with(parsed)).await,
            effect::Exit::Failure(effect::Cause::Fail(effect_cli::CliError::HelpRequested)) => {
                println!("{}", render_help_ansi(&cmd, 80));
                Ok(())
            }
            effect::Exit::Failure(c) => {
                eprintln!("error: {c}");
                Ok(())
            }
        }
    }
});
let _ = program.execute().await;
# }
```

## Specification

| Builder              | Notes                                                                       |
| -------------------- | --------------------------------------------------------------------------- |
| `Command::new(n, d)` | Name + one-line description                                                 |
| `.arg(Arg)`          | Positional, in declaration order. All required.                             |
| `.option(Opt)`       | Named `--long`, optional `-s` short, optional `.default(...)`               |
| `.flag(Flag)`        | Named `--long`, optional `-s`. Boolean (presence = true).                   |

## Errors

```rust,ignore
pub enum CliError {
    MissingArgument { name: String },
    MissingOptionValue { name: String },
    Unknown(String),                     // unknown --flag / -f
    TooManyArgs { expected: usize, got: usize },
    HelpRequested,                       // user passed --help / -h
}
```

`HelpRequested` is reported as an error (not a success) so the program
can branch on it without conflating help-printing with normal flow.

## Help text

`render_help(&cmd, width)` produces plain text; `render_help_ansi`
adds ANSI colour for terminals. The renderer uses the Wadler pretty
printer, so output adapts to the width:

```text
greet

Say hello to someone

USAGE
  greet [OPTIONS] <name>

ARGS
  <name>  Whom to greet

OPTIONS
  -g, --greeting <VALUE>  Greeting word [default: hello]

FLAGS
  -l, --loud  SHOUT IT

(use --help or -h for this message)
```

## Limits of this starter

For pre-alpha we cover the 80% case. Not (yet) supported:

- **Subcommands.** `Command::sub(name, sub_cmd)` is planned.
- **Combined short flags.** `-abc` for `-a -b -c` is not parsed (each
  flag must be its own token).
- **`--key=value`.** Use `--key value` (space-separated).
- **Type-checked argument values.** Today everything is a `String`;
  the user converts. A future `Arg::<i32>::new(...)` will integrate
  with [`Schema`](../data/schemas.md) for primitive coercion.
- **Env-var fallback.** Combine with
  [`effect-config`](./config.md) for that pattern.

For complex CLIs that need shell completion, `clap` interop, or
grouped flags, today's recommendation is to use `clap` directly and
wrap the parsed value into your effects manually. The combinator API
above is purpose-built for the small/medium case where Schema-derived
types and Effect composition matter more than feature parity.

## What's coming

- Subcommands (`Command::sub`)
- Type-checked args via Schema
- Env-var fallback (overlap with `effect-config`)
- Shell completion (bash / zsh / fish)
- Combined short flags
- `--key=value` syntax

[`effect-printer`]: https://docs.rs/effect-printer
