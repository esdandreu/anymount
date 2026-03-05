use std::fmt::Display;

/// Injected logger for logs (and later traces/metrics). Use static polymorphism via `L: Logger`.
pub trait Logger: Clone + Send + Sync {
    fn trace(&self, msg: impl Display);
    fn debug(&self, msg: impl Display);
    fn info(&self, msg: impl Display);
    fn warn(&self, msg: impl Display);
    fn error(&self, msg: impl Display);

    /// Returns a logger that includes this key-value in all subsequent log calls.
    fn with_context(&self, key: &str, value: impl Display) -> Self;
}

/// Context attached to a logger for correlation (e.g. mount path).
#[derive(Clone, Default)]
pub struct LoggerContext {
    pub key: String,
    pub value: String,
}

/// Adapter that forwards to the `tracing` crate.
#[derive(Clone, Default)]
pub struct TracingLogger {
    context: Option<LoggerContext>,
}

impl TracingLogger {
    pub fn new() -> Self {
        Self::default()
    }

    fn emit(&self, level: tracing::Level, msg: impl Display) {
        let msg = msg.to_string();
        match level {
            tracing::Level::TRACE => match &self.context {
                Some(ctx) => tracing::trace!(key = %ctx.key, value = %ctx.value, "{}", msg),
                None => tracing::trace!("{}", msg),
            },
            tracing::Level::DEBUG => match &self.context {
                Some(ctx) => tracing::debug!(key = %ctx.key, value = %ctx.value, "{}", msg),
                None => tracing::debug!("{}", msg),
            },
            tracing::Level::INFO => match &self.context {
                Some(ctx) => tracing::info!(key = %ctx.key, value = %ctx.value, "{}", msg),
                None => tracing::info!("{}", msg),
            },
            tracing::Level::WARN => match &self.context {
                Some(ctx) => tracing::warn!(key = %ctx.key, value = %ctx.value, "{}", msg),
                None => tracing::warn!("{}", msg),
            },
            tracing::Level::ERROR => match &self.context {
                Some(ctx) => tracing::error!(key = %ctx.key, value = %ctx.value, "{}", msg),
                None => tracing::error!("{}", msg),
            },
        }
    }
}

impl Logger for TracingLogger {
    fn trace(&self, msg: impl Display) {
        self.emit(tracing::Level::TRACE, msg);
    }

    fn debug(&self, msg: impl Display) {
        self.emit(tracing::Level::DEBUG, msg);
    }

    fn info(&self, msg: impl Display) {
        self.emit(tracing::Level::INFO, msg);
    }

    fn warn(&self, msg: impl Display) {
        self.emit(tracing::Level::WARN, msg);
    }

    fn error(&self, msg: impl Display) {
        self.emit(tracing::Level::ERROR, msg);
    }

    fn with_context(&self, key: &str, value: impl Display) -> Self {
        Self {
            context: Some(LoggerContext {
                key: key.to_string(),
                value: value.to_string(),
            }),
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

    fn with_context(&self, _key: &str, _value: impl Display) -> Self {
        Self
    }
}
