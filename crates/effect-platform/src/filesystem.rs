//! `FileSystem` — an injectable filesystem service. Live impl uses
//! `tokio::fs`.
//!
//! Methods take owned `PathBuf` (not `&Path`) because the returned
//! future is `'static`.

use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use effect::Effect;

/// A boxed, `'static`, `Send` future used by `FileSystem` methods to
/// stay object-safe without tying into trait-fn-async lifetimes.
pub type AsyncResult<T> = Pin<Box<dyn Future<Output = T> + Send>>;

pub trait FileSystem: Send + Sync + 'static {
    fn read_to_string(&self, path: PathBuf) -> AsyncResult<io::Result<String>>;
    fn write(&self, path: PathBuf, contents: Vec<u8>) -> AsyncResult<io::Result<()>>;
    fn exists(&self, path: PathBuf) -> AsyncResult<bool>;
}

/// `tokio::fs`-backed implementation.
pub struct LiveFileSystem;

impl FileSystem for LiveFileSystem {
    fn read_to_string(&self, path: PathBuf) -> AsyncResult<io::Result<String>> {
        Box::pin(async move { tokio::fs::read_to_string(path).await })
    }
    fn write(&self, path: PathBuf, contents: Vec<u8>) -> AsyncResult<io::Result<()>> {
        Box::pin(async move { tokio::fs::write(path, contents).await })
    }
    fn exists(&self, path: PathBuf) -> AsyncResult<bool> {
        Box::pin(async move { tokio::fs::try_exists(path).await.unwrap_or(false) })
    }
}

// ── Effects ──────────────────────────────────────────────────────

pub fn read_to_string<R>(path: impl Into<PathBuf>) -> Effect<String, io::Error, R>
where
    R: FileSystem,
{
    let path = path.into();
    Effect::from_fn(move |r: Arc<R>| {
        let path = path.clone();
        async move { r.read_to_string(path).await }
    })
}

pub fn write<R>(
    path: impl Into<PathBuf>,
    contents: impl Into<Vec<u8>>,
) -> Effect<(), io::Error, R>
where
    R: FileSystem,
{
    let path = path.into();
    let contents = contents.into();
    Effect::from_fn(move |r: Arc<R>| {
        let path = path.clone();
        let contents = contents.clone();
        async move { r.write(path, contents).await }
    })
}

pub fn exists<E, R>(path: impl Into<PathBuf>) -> Effect<bool, E, R>
where
    E: Send + 'static,
    R: FileSystem,
{
    let path = path.into();
    Effect::from_fn(move |r: Arc<R>| {
        let path = path.clone();
        async move { Ok(r.exists(path).await) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn write_then_read_roundtrips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("hello.txt");
        let _ = write::<LiveFileSystem>(&path, "hi world".as_bytes().to_vec())
            .run_with(LiveFileSystem)
            .await;
        let contents = read_to_string::<LiveFileSystem>(&path)
            .run_with(LiveFileSystem)
            .await
            .ok()
            .unwrap();
        assert_eq!(contents, "hi world");
    }

    #[tokio::test]
    async fn exists_true_for_existing_false_for_missing() {
        let dir = tempdir().unwrap();
        let present = dir.path().join("here.txt");
        let absent = dir.path().join("nope.txt");

        let _ = write::<LiveFileSystem>(&present, b"x".to_vec())
            .run_with(LiveFileSystem)
            .await;

        let t = exists::<io::Error, LiveFileSystem>(&present)
            .run_with(LiveFileSystem)
            .await
            .ok()
            .unwrap();
        let f = exists::<io::Error, LiveFileSystem>(&absent)
            .run_with(LiveFileSystem)
            .await
            .ok()
            .unwrap();
        assert!(t);
        assert!(!f);
    }

    #[tokio::test]
    async fn read_missing_returns_io_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.txt");
        let exit = read_to_string::<LiveFileSystem>(&path)
            .run_with(LiveFileSystem)
            .await;
        match exit {
            effect::Exit::Failure(_) => {}
            other => panic!("expected failure for missing file, got {other:?}"),
        }
    }
}
