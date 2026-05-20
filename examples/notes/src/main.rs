//! `notes` — a small REST-ish notes service that exercises the full
//! effect-rs stack: CLI entry, env-driven config, sqlite storage,
//! hyper-based HTTP server, schema-validated request/response, and
//! structured logging.
//!
//! Usage:
//!     NOTES_PORT=3000 NOTES_DB_PATH=:memory: cargo run -p effect-example-notes
//!     curl -X POST http://localhost:3000/notes/create \
//!          -H 'Content-Type: application/json' \
//!          -d '{"title":"hello","body":"world"}'

use effect_cli::{parse, Arg, Command};
use effect_config::from_env;
use effect_example_notes::api::build_router;
use effect_example_notes::config::AppConfig;
use effect_example_notes::repo::open_db;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Wire up tracing first so config errors are visible.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // CLI parsing: `notes serve` is the only subcommand for now;
    // we just print help on `notes --help` and otherwise serve.
    let cmd = Command::new("notes", "A small REST-ish notes service")
        .arg(Arg::new("command", "Subcommand to run (only 'serve' is supported)"));
    let parsed = parse(&cmd, std::env::args().collect());

    match parsed {
        Ok(p) => {
            let command = p.arg("command").unwrap_or("serve");
            if command != "serve" {
                eprintln!("unknown command: {command}");
                std::process::exit(2);
            }
        }
        Err(effect_cli::CliError::HelpRequested) => {
            println!("{}", effect_cli::render_help_ansi(&cmd, 80));
            std::process::exit(0);
        }
        Err(effect_cli::CliError::MissingArgument { .. }) => {
            // No subcommand given — default to serve.
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }

    // Load config from env. Falls back to in-memory defaults if
    // NOTES_PORT / NOTES_DB_PATH aren't set.
    let cfg = match from_env::<AppConfig>("NOTES_").execute().await {
        effect::Exit::Success(c) => c,
        _ => {
            tracing::info!(
                "no NOTES_* env vars set — using defaults (port {}, db {})",
                AppConfig::defaults().port,
                AppConfig::defaults().db_path,
            );
            AppConfig::defaults()
        }
    };

    // Open the database and ensure the schema exists.
    let db = match open_db(&cfg.db_path).await {
        Ok(db) => db,
        Err(e) => {
            tracing::error!("failed to open database '{}': {e}", cfg.db_path);
            std::process::exit(1);
        }
    };

    // Build the router (handlers close over the db handle).
    let router = build_router(db);

    // Bind and serve.
    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("failed to bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!("notes service listening on {addr}");

    if let Err(e) = effect_http_server::serve(listener, router).await {
        tracing::error!("server stopped with error: {e}");
        std::process::exit(1);
    }
}
