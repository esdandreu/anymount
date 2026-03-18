use crate::{Logger, TracingLogger};
use clap::Args;
use std::path::PathBuf;

#[derive(Args, Debug, Clone)]
pub struct ProvideCommand {
    #[arg(long)]
    pub name: String,

    #[arg(long)]
    pub config_dir: Option<PathBuf>,
}

impl ProvideCommand {
    pub fn execute(&self) -> Result<(), String> {
        self.run_with(&DefaultProvideRunner)
    }

    pub(crate) fn run_with<R>(&self, runner: &R) -> Result<(), String>
    where
        R: ProvideRunner,
    {
        runner.run(self, &TracingLogger::new())
    }
}

pub trait ProvideRunner {
    fn run<L: Logger>(&self, command: &ProvideCommand, logger: &L) -> Result<(), String>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultProvideRunner;

impl ProvideRunner for DefaultProvideRunner {
    fn run<L: Logger>(&self, command: &ProvideCommand, logger: &L) -> Result<(), String> {
        logger.info(format!("starting provider daemon for {}", command.name));
        Err("startup failed: provide runtime not yet wired".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoOpLogger;

    #[derive(Debug, Clone, Copy, Default)]
    struct FailingProvideRunner;

    impl ProvideRunner for FailingProvideRunner {
        fn run<L: Logger>(&self, _command: &ProvideCommand, _logger: &L) -> Result<(), String> {
            Err("startup failed".to_owned())
        }
    }

    #[test]
    fn provide_returns_error_when_provider_startup_fails() {
        let command = ProvideCommand {
            name: "demo".to_owned(),
            config_dir: None,
        };

        let err = command
            .run_with(&FailingProvideRunner)
            .expect_err("startup should fail");
        assert!(err.contains("startup"));
    }

    #[test]
    fn default_runner_can_be_called_with_injected_logger() {
        let command = ProvideCommand {
            name: "demo".to_owned(),
            config_dir: None,
        };
        let logger = NoOpLogger;
        let err = DefaultProvideRunner
            .run(&command, &logger)
            .expect_err("runtime is not wired yet");
        assert!(err.contains("startup failed"));
    }
}
