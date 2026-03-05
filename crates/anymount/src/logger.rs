use std::fmt::Display;

/// Injectable structured logger port with span support.
///
/// Adapters forward log events to backends like `tracing` or OpenTelemetry.
/// Use static polymorphism via `L: Logger` at module boundaries.
pub trait Logger: Clone + Send + Sync {
    fn trace(&self, msg: impl Display);
    fn debug(&self, msg: impl Display);
    fn info(&self, msg: impl Display);
    fn warn(&self, msg: impl Display);
    fn error(&self, msg: impl Display);

    /// Run `f` inside a named span with optional key-value context.
    ///
    /// The default implementation runs `f` without creating a span.
    fn in_span<F: FnOnce() -> R, R>(
        &self,
        _name: &'static str,
        _context: &[(&str, &str)],
        f: F,
    ) -> R {
        f()
    }
}

/// Adapter that forwards to the `tracing` crate
#[derive(Clone, Default)]
pub struct TracingLogger;

impl TracingLogger {
    pub fn new() -> Self {
        Self
    }
}

impl Logger for TracingLogger {
    fn trace(&self, msg: impl Display) {
        self.in_span("trace", &[], || tracing::trace!("{}", msg));
    }

    fn debug(&self, msg: impl Display) {
        tracing::debug!("{}", msg);
    }

    fn info(&self, msg: impl Display) {
        tracing::info!("{}", msg);
    }

    fn warn(&self, msg: impl Display) {
        tracing::warn!("{}", msg);
    }

    fn error(&self, msg: impl Display) {
        tracing::error!("{}", msg);
    }

    fn in_span<F: FnOnce() -> R, R>(
        &self,
        name: &'static str,
        context: &[(&str, &str)],
        f: F,
    ) -> R {
        if context.is_empty() {
            tracing::info_span!("span", op = name).in_scope(f)
        } else {
            let formatted: String = context
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(" ");
            tracing::info_span!("span", op = name, context = %formatted).in_scope(f)
        }
    }
}

/// No-op logger for tests. Drops all messages.
#[derive(Clone, Default)]
pub struct NoOpLogger;

impl Logger for NoOpLogger {
    fn trace(&self, _msg: impl Display) {}
    fn debug(&self, _msg: impl Display) {}
    fn info(&self, _msg: impl Display) {}
    fn warn(&self, _msg: impl Display) {}
    fn error(&self, _msg: impl Display) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_span_runs_closure() {
        let logger = NoOpLogger;
        let result = logger.in_span("test_span", &[], || 42);
        assert_eq!(result, 42);
    }

    #[test]
    fn in_span_with_context_runs_closure() {
        let logger = NoOpLogger;
        let result = logger.in_span("test_span", &[("key", "value")], || "hello");
        assert_eq!(result, "hello");
    }
}
