# Installation

`effect-rs` is pre-alpha; it isn't published to crates.io yet. For now,
depend on it from this repository.

## As a git dependency

```toml
[dependencies]
effect = { git = "https://github.com/Almaju/effect-rs" }
tokio  = { version = "1", features = ["rt-multi-thread", "macros"] }
```

## In this workspace

Examples live alongside the core crate as workspace members. To run the
included todo example:

```bash
cargo run -p effect-example-todo
```

## Building the book

The documentation site (this site) is built with [mdBook](https://rust-lang.github.io/mdBook/).

```bash
cargo install mdbook
mdbook serve docs   # live-reloads on changes
mdbook build docs   # outputs to docs/book/
```

## Running the tests

```bash
cargo test
```

## MSRV

`effect-rs` tracks current stable Rust. The pinned `rust-version` lives in
the workspace `Cargo.toml`. A six-month support window is intended once
the crate is published.
