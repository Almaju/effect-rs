//! Effect-wrapped structured logging on top of [`tracing`].
//!
//! - [`trace`], [`debug`], [`info`], [`warn`], [`error`] — produce
//!   `Effect<(), E, R>` that emits a log line when run.
//! - [`instrument`] — wraps an effect in a [`tracing::Span`] so the
//!   subscriber sees field/span context for everything the effect
//!   logs.
//! - [`with_span`] — convenience for the common case of "wrap in an
//!   `info_span!(name)`".
//!
//! Hook up a subscriber once (e.g. with `tracing_subscriber::fmt()`)
//! and these compose with `tracing` macros anywhere else in your
//! program. Fiber boundaries propagate the surrounding span via
//! [`tracing::Instrument`].

use effect::Effect;
pub use tracing;

macro_rules! log_fn {
    ($name:ident, $level_macro:ident, $level_desc:literal) => {
        #[doc = concat!("Emit a ", $level_desc, " log line.")]
        pub fn $name<E, R>(msg: impl Into<String>) -> Effect<(), E, R>
        where
            E: Send + 'static,
            R: Send + Sync + 'static,
        {
            let msg = msg.into();
            Effect::sync(move || {
                tracing::$level_macro!("{}", msg);
                Ok(())
            })
        }
    };
}

log_fn!(trace, trace, "TRACE");
log_fn!(debug, debug, "DEBUG");
log_fn!(info, info, "INFO");
log_fn!(warn, warn, "WARN");
log_fn!(error, error, "ERROR");

/// Wrap `effect` in a [`tracing::Span`] so the subscriber receives
/// the span's name and fields for every event emitted while it runs.
pub fn instrument<A, E, R>(
    effect: Effect<A, E, R>,
    span: tracing::Span,
) -> Effect<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    Effect::from_fn_exit(move |r| {
        let effect = effect.clone();
        let span = span.clone();
        Box::pin(async move {
            use tracing::Instrument;
            effect.run(r).instrument(span).await
        })
    })
}

/// Wrap `effect` in `info_span!(name)`. The name must be `'static`
/// (matching the `info_span!` macro's signature); construct the span
/// directly with `tracing::info_span!` for dynamic fields.
pub fn with_span<A, E, R>(
    name: &'static str,
    effect: Effect<A, E, R>,
) -> Effect<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    instrument(effect, tracing::info_span!(target: module_path!(), "", _name = name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use effect::Effect;
    use std::sync::{Arc, Mutex};
    use tracing::Level;
    use tracing::field::Visit;
    use tracing::subscriber::with_default;
    use tracing::{Event, Subscriber};

    // ── A tiny in-memory subscriber for testing ────────────────

    #[derive(Default, Clone)]
    struct CaptureLayer {
        events: Arc<Mutex<Vec<(Level, String)>>>,
    }

    impl Subscriber for CaptureLayer {
        fn enabled(&self, _meta: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _attrs: &tracing::span::Attributes<'_>) -> tracing::Id {
            tracing::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::Id, _: &tracing::Id) {}
        fn event(&self, event: &Event<'_>) {
            let level = *event.metadata().level();
            let mut grabber = MessageGrabber(String::new());
            event.record(&mut grabber);
            self.events.lock().unwrap().push((level, grabber.0));
        }
        fn enter(&self, _: &tracing::Id) {}
        fn exit(&self, _: &tracing::Id) {}
    }

    struct MessageGrabber(String);
    impl Visit for MessageGrabber {
        fn record_str(&mut self, _f: &tracing::field::Field, value: &str) {
            self.0.push_str(value);
        }
        fn record_debug(
            &mut self,
            _f: &tracing::field::Field,
            value: &dyn std::fmt::Debug,
        ) {
            use std::fmt::Write;
            let _ = write!(&mut self.0, "{value:?}");
        }
    }

    fn capture<F: FnOnce()>(f: F) -> Vec<(Level, String)> {
        let layer = CaptureLayer::default();
        let events = layer.events.clone();
        with_default(layer, f);
        let out = events.lock().unwrap().clone();
        out
    }

    // ── Tests ──────────────────────────────────────────────────

    #[test]
    fn info_emits_an_info_event() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let events = capture(|| {
            runtime.block_on(async {
                let _ = info::<String, ()>("hello").execute().await;
            });
        });
        assert!(
            events
                .iter()
                .any(|(lvl, msg)| *lvl == Level::INFO && msg.contains("hello")),
            "events: {events:?}"
        );
    }

    #[test]
    fn warn_emits_a_warn_event() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let events = capture(|| {
            runtime.block_on(async {
                let _ = warn::<String, ()>("careful").execute().await;
            });
        });
        assert!(
            events
                .iter()
                .any(|(lvl, msg)| *lvl == Level::WARN && msg.contains("careful")),
            "events: {events:?}"
        );
    }

    #[test]
    fn error_emits_an_error_event() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let events = capture(|| {
            runtime.block_on(async {
                let _ = error::<String, ()>("uh oh").execute().await;
            });
        });
        assert!(
            events
                .iter()
                .any(|(lvl, msg)| *lvl == Level::ERROR && msg.contains("uh oh")),
            "events: {events:?}"
        );
    }

    #[test]
    fn instrument_runs_inner_under_span() {
        // Smoke test: instrument runs without error. Verifying that
        // span propagates through subscriber requires more setup; the
        // contract is that the inner future runs to completion.
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let exit = runtime.block_on(async {
            let work = Effect::<i32, String, ()>::succeed(42);
            let span = tracing::info_span!("work");
            instrument(work, span).execute().await
        });
        assert_eq!(exit.ok(), Some(42));
    }

    #[test]
    fn with_span_returns_inner_value() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let exit = runtime.block_on(async {
            let work = Effect::<i32, String, ()>::succeed(7);
            with_span("compute", work).execute().await
        });
        assert_eq!(exit.ok(), Some(7));
    }

    #[test]
    fn log_chains_with_effects() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let events = capture(|| {
            runtime.block_on(async {
                let program: Effect<i32, String, ()> = Effect::block(|g| async move {
                    g.run(info::<String, ()>("step one")).await?;
                    g.run(info::<String, ()>("step two")).await?;
                    Ok(99)
                });
                let _ = program.execute().await;
            });
        });
        let count_info = events
            .iter()
            .filter(|(lvl, _)| *lvl == Level::INFO)
            .count();
        assert!(count_info >= 2, "events: {events:?}");
    }
}
