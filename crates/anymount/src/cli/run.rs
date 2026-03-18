use super::Cli;
use clap::Parser;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub fn run() -> super::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = if cli.verbose { "debug" } else { "info" };
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| format!("anymount={}", log_level).into());

    // Create file appender - writes to logs/anymount.log with daily rotation
    let file_appender = tracing_appender::rolling::daily("logs", "anymount.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stdout)) // Console output
        .with(
            fmt::layer()
                .with_writer(non_blocking) // File output
                .with_ansi(false), // Disable colors in file
        )
        .init();

    // Keep the guard alive for the entire program lifetime
    // This ensures the file writer thread keeps running
    let result = cli.run();
    drop(_guard);
    result
}
