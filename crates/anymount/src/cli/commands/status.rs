use crate::config::{ConfigDir, Error as ConfigError};
use clap::Args;
use std::io::{self, Write};
use std::path::PathBuf;

/// Probes whether a named provider service responds on its control endpoint.
pub(crate) trait ProviderDaemonProbe {
    fn provider_daemon_ready(&self, provider_name: &str) -> bool;
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DefaultProviderDaemonProbe;

impl ProviderDaemonProbe for DefaultProviderDaemonProbe {
    fn provider_daemon_ready(&self, provider_name: &str) -> bool {
        crate::cli::provider_control::provider_daemon_ready(provider_name)
    }
}

/// Show configured providers and whether each service responds on its control endpoint.
#[derive(Args, Debug, Clone)]
pub struct StatusCommand {
    /// Config directory override.
    #[arg(long)]
    pub config_dir: Option<PathBuf>,
}

impl StatusCommand {
    pub fn execute(&self) -> crate::cli::Result<()> {
        let mut out = io::stdout().lock();
        self._execute(&DefaultProviderDaemonProbe, &mut out)
    }

    pub(crate) fn _execute<P: ProviderDaemonProbe, W: Write>(
        &self,
        probe: &P,
        out: &mut W,
    ) -> crate::cli::Result<()> {
        let cd = self.config_dir();
        let mut entries = cd.each_provider()?.peekable();
        if entries.peek().is_none() {
            writeln!(out, "No configured providers.").map_err(write_cli_error)?;
            return Ok(());
        }
        for (name, loaded) in entries {
            Self::write_one_entry(probe, out, &name, loaded)?;
        }
        Ok(())
    }

    fn write_one_entry<P: ProviderDaemonProbe, W: Write>(
        probe: &P,
        out: &mut W,
        name: &str,
        loaded: Result<crate::ProviderFileConfig, ConfigError>,
    ) -> crate::cli::Result<()> {
        match loaded {
            Err(err) => {
                writeln!(out, "{}", format_status_error(name, &err.to_string()))
                    .map_err(write_cli_error)?;
            }
            Ok(cfg) => {
                let storage = cfg.storage.label();
                let path = cfg.path.display().to_string();
                let running = probe.provider_daemon_ready(name);
                writeln!(out, "{}", format_status_ok(name, storage, &path, running))
                    .map_err(write_cli_error)?;
            }
        }
        Ok(())
    }

    fn config_dir(&self) -> ConfigDir {
        match &self.config_dir {
            Some(path) => ConfigDir::new(path.clone()),
            None => ConfigDir::default(),
        }
    }
}

fn format_status_ok(name: &str, storage: &str, path: &str, running: bool) -> String {
    let status = if running { "running" } else { "not running" };
    format!("- {name} ({storage}, {path}): {status}")
}

fn format_status_error(name: &str, detail: &str) -> String {
    format!("- {name}: error — {detail}")
}

fn write_cli_error(err: std::io::Error) -> crate::cli::Error {
    crate::cli::Error::Validation(format!("failed to write status output: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProviderFileConfig;
    use crate::StorageConfig;
    use std::path::PathBuf;

    #[derive(Clone, Copy)]
    struct NeverRunningProbe;

    impl ProviderDaemonProbe for NeverRunningProbe {
        fn provider_daemon_ready(&self, _provider_name: &str) -> bool {
            false
        }
    }

    #[derive(Clone, Copy)]
    struct AlwaysRunningProbe;

    impl ProviderDaemonProbe for AlwaysRunningProbe {
        fn provider_daemon_ready(&self, _provider_name: &str) -> bool {
            true
        }
    }

    #[test]
    fn format_ok_running_matches_spec_shape() {
        let line = format_status_ok("demo", "local", "/mnt/demo", true);
        assert_eq!(line, "- demo (local, /mnt/demo): running");
    }

    #[test]
    fn format_ok_not_running_matches_spec_shape() {
        let line = format_status_ok("other", "onedrive", "/mnt/other", false);
        assert_eq!(line, "- other (onedrive, /mnt/other): not running");
    }

    #[test]
    fn format_error_includes_em_dash_and_detail() {
        let line = format_status_error("broken", "invalid TOML");
        assert!(
            line.starts_with("- broken: error — "),
            "unexpected prefix: {line}"
        );
        assert!(line.contains("invalid TOML"));
    }

    #[test]
    fn empty_config_dir_prints_message() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cmd = StatusCommand {
            config_dir: Some(tmp.path().to_path_buf()),
        };
        let mut buf = Vec::new();
        cmd._execute(&NeverRunningProbe, &mut buf).expect("status");
        let s = String::from_utf8(buf).expect("utf8");
        assert!(s.contains("No configured providers."));
    }

    #[test]
    fn probe_false_yields_not_running_lines() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        cd.write(
            "alpha",
            &ProviderFileConfig {
                path: PathBuf::from("/mnt/a"),
                storage: StorageConfig::Local {
                    root: PathBuf::from("/data/a"),
                },
                telemetry: Default::default(),
            },
        )
        .expect("write");

        let cmd = StatusCommand {
            config_dir: Some(tmp.path().to_path_buf()),
        };
        let mut buf = Vec::new();
        cmd._execute(&NeverRunningProbe, &mut buf).expect("status");
        let s = String::from_utf8(buf).expect("utf8");
        assert!(s.contains("- alpha (local, /mnt/a): not running"));
    }

    #[test]
    fn probe_true_yields_running() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        cd.write(
            "beta",
            &ProviderFileConfig {
                path: PathBuf::from("/mnt/b"),
                storage: StorageConfig::Local {
                    root: PathBuf::from("/data/b"),
                },
                telemetry: Default::default(),
            },
        )
        .expect("write");

        let cmd = StatusCommand {
            config_dir: Some(tmp.path().to_path_buf()),
        };
        let mut buf = Vec::new();
        cmd._execute(&AlwaysRunningProbe, &mut buf).expect("status");
        let s = String::from_utf8(buf).expect("utf8");
        assert!(s.contains("- beta (local, /mnt/b): running"));
    }

    #[test]
    fn invalid_toml_still_lists_other_provider() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        std::fs::write(cd.dir().join("bad.toml"), "not valid toml {{{").expect("write bad");
        cd.write(
            "good",
            &ProviderFileConfig {
                path: PathBuf::from("/mnt/g"),
                storage: StorageConfig::Local {
                    root: PathBuf::from("/data/g"),
                },
                telemetry: Default::default(),
            },
        )
        .expect("write good");

        let cmd = StatusCommand {
            config_dir: Some(tmp.path().to_path_buf()),
        };
        let mut buf = Vec::new();
        cmd._execute(&NeverRunningProbe, &mut buf).expect("status");
        let s = String::from_utf8(buf).expect("utf8");
        assert!(
            s.contains("- bad: error — "),
            "expected error bullet for bad, got: {s}"
        );
        assert!(
            s.contains("- good (local, /mnt/g): not running"),
            "expected good line, got: {s}"
        );
        let err_detail = s
            .lines()
            .find(|l| l.starts_with("- bad: error — "))
            .expect("error line");
        assert!(
            err_detail.len() > "- bad: error — ".len(),
            "detail should be non-empty: {err_detail}"
        );
    }
}
