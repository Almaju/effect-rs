//! Configuration loaded from the environment via effect-config.

use effect::Schema;

#[derive(Debug, Clone, Schema)]
pub struct AppConfig {
    /// TCP port to listen on. Env var: `NOTES_PORT`.
    pub port: u16,
    /// Sqlite path. Use `:memory:` for an ephemeral DB. Env var: `NOTES_DB_PATH`.
    pub db_path: String,
}

impl AppConfig {
    /// Sensible defaults for `notes demo` if the env vars aren't set.
    pub fn defaults() -> Self {
        AppConfig {
            port: 3000,
            db_path: ":memory:".into(),
        }
    }
}
