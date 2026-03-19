use super::Cli;
use clap::Parser;
use opentelemetry::trace::TracerProvider;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use tracing_opentelemetry;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub fn run() -> super::Result<()> {
    let cli = Cli::parse();

    let otel = crate::telemetry::OtelHandles::try_from_cli(&cli)?;

    let log_level = if cli.verbose { "debug" } else { "info" };
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| format!("anymount={}", log_level).into());

    let file_appender = tracing_appender::rolling::daily("logs", "anymount.log");
    let (non_blocking, file_guard) = tracing_appender::non_blocking(file_appender);

    let stdout_layer = fmt::layer().with_writer(std::io::stdout);
    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false);

    let result = if let Some(otel) = otel {
        let trace_layer = tracing_opentelemetry::layer()
            .with_tracer(otel.tracer_provider().tracer("anymount"));
        let log_layer = OpenTelemetryTracingBridge::new(otel.logger_provider());

        tracing_subscriber::registry()
            .with(env_filter)
            .with(stdout_layer)
            .with(file_layer)
            .with(trace_layer)
            .with(log_layer)
            .init();

        let outcome = cli.run();
        otel.shutdown();
        outcome
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(stdout_layer)
            .with(file_layer)
            .init();

        cli.run()
    };

    drop(file_guard);
    result
}
