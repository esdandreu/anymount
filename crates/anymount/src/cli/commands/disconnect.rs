use crate::cli::provider_control::try_disconnect_provider;
use crate::config::ConfigDir;
use crate::Logger;
use crate::TracingLogger;
use clap::Args;
use std::path::PathBuf;

/// Stop background provider services via the control endpoint (idempotent).
#[derive(Args, Debug, Clone)]
pub struct DisconnectCommand {
    /// Disconnect a named provider.
    #[arg(long, conflicts_with = "all")]
    pub name: Option<String>,

    /// Disconnect all configured providers.
    #[arg(long, conflicts_with = "name")]
    pub all: bool,

    /// Config directory override.
    #[arg(long)]
    pub config_dir: Option<PathBuf>,
}

impl DisconnectCommand {
    pub fn execute(&self) -> crate::cli::Result<()> {
        let logger = TracingLogger::new();
        self._execute(try_disconnect_provider, &logger)
    }

    pub(crate) fn _execute<F, L>(&self, try_disconnect: F, logger: &L) -> crate::cli::Result<()>
    where
        F: Fn(&str) -> Result<(), String>,
        L: Logger,
    {
        if self.all {
            let cd = self.config_dir();
            let mut failures = Vec::new();
            for (name, _) in cd.each_provider()? {
                if let Err(error) = try_disconnect(&name) {
                    failures.push(format!("{name}: {error}"));
                } else {
                    logger.info(format!("Disconnected (or already stopped) provider {name}"));
                }
            }
            if failures.is_empty() {
                Ok(())
            } else {
                Err(crate::cli::Error::DisconnectFailures {
                    failures: failures.join(", "),
                })
            }
        } else if let Some(name) = &self.name {
            try_disconnect(name).map_err(|e| crate::cli::Error::DisconnectFailures {
                failures: format!("{name}: {e}"),
            })?;
            logger.info(format!("Disconnected (or already stopped) provider {name}"));
            Ok(())
        } else {
            Err(crate::cli::Error::MissingDisconnectTarget)
        }
    }

    fn config_dir(&self) -> ConfigDir {
        match &self.config_dir {
            Some(p) => ConfigDir::new(p.clone()),
            None => ConfigDir::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoOpLogger;
    use std::sync::{Arc, Mutex};

    #[test]
    fn disconnect_all_uses_each_provider_order() {
        let tmp = tempfile::tempdir().expect("tmp");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        cd.write(
            "b",
            &crate::ProviderFileConfig {
                path: std::path::PathBuf::from("/b"),
                storage: crate::StorageConfig::Local {
                    root: std::path::PathBuf::from("/d/b"),
                },
                telemetry: Default::default(),
            },
        )
        .expect("write");
        cd.write(
            "a",
            &crate::ProviderFileConfig {
                path: std::path::PathBuf::from("/a"),
                storage: crate::StorageConfig::Local {
                    root: std::path::PathBuf::from("/d/a"),
                },
                telemetry: Default::default(),
            },
        )
        .expect("write");

        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let calls_cb = Arc::clone(&calls);
        let cmd = DisconnectCommand {
            name: None,
            all: true,
            config_dir: Some(tmp.path().to_path_buf()),
        };
        cmd._execute(
            move |n| {
                calls_cb.lock().expect("lock").push(n.to_owned());
                Ok(())
            },
            &NoOpLogger,
        )
        .expect("disconnect");

        assert_eq!(*calls.lock().expect("lock"), vec!["a", "b"]);
    }

    #[test]
    fn disconnect_without_target_errors() {
        let cmd = DisconnectCommand {
            name: None,
            all: false,
            config_dir: None,
        };
        let err = cmd
            ._execute(|_| Ok(()), &NoOpLogger)
            .expect_err("missing target");
        assert!(matches!(err, crate::cli::Error::MissingDisconnectTarget));
    }

    #[test]
    fn disconnect_all_aggregates_failures() {
        let tmp = tempfile::tempdir().expect("tmp");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        cd.write(
            "ok",
            &crate::ProviderFileConfig {
                path: std::path::PathBuf::from("/o"),
                storage: crate::StorageConfig::Local {
                    root: std::path::PathBuf::from("/d/o"),
                },
                telemetry: Default::default(),
            },
        )
        .expect("write");
        cd.write(
            "bad",
            &crate::ProviderFileConfig {
                path: std::path::PathBuf::from("/x"),
                storage: crate::StorageConfig::Local {
                    root: std::path::PathBuf::from("/d/x"),
                },
                telemetry: Default::default(),
            },
        )
        .expect("write");

        let cmd = DisconnectCommand {
            name: None,
            all: true,
            config_dir: Some(tmp.path().to_path_buf()),
        };
        let err = cmd
            ._execute(
                |n| {
                    if n == "bad" {
                        Err("nope".to_owned())
                    } else {
                        Ok(())
                    }
                },
                &NoOpLogger,
            )
            .expect_err("partial failure");
        assert!(matches!(err, crate::cli::Error::DisconnectFailures { .. }));
    }
}
