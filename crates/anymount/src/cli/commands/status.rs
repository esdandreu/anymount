use crate::application::status::{
    Application as StatusApplication, Error as StatusError, ServiceControl, StatusEntry,
    StatusRepository, StatusUseCase,
};
use crate::application::types::ProviderStatusRow;
use crate::config::ConfigDir;
use crate::domain::provider::ProviderSpec;
use clap::Args;
use std::io::{self, Write};
use std::path::PathBuf;

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
        let config_dir = self.config_dir();
        let repository = ConfigRepository::new(config_dir);
        let control = ProviderServiceControl;
        let app = StatusApplication::new(&repository, &control);
        self._execute(&app, &mut out)
    }

    pub(crate) fn _execute<U: StatusUseCase, W: Write>(
        &self,
        use_case: &U,
        out: &mut W,
    ) -> crate::cli::Result<()> {
        let rows = use_case.list().map_err(map_status_error)?;
        if rows.is_empty() {
            writeln!(out, "No configured providers.").map_err(write_cli_error)?;
            return Ok(());
        }

        for row in rows {
            Self::write_one_entry(out, row)?;
        }
        Ok(())
    }

    fn write_one_entry<W: Write>(out: &mut W, row: ProviderStatusRow) -> crate::cli::Result<()> {
        if let Some(error) = row.error {
            writeln!(out, "{}", format_status_error(&row.name, &error)).map_err(write_cli_error)?;
            return Ok(());
        }

        let storage = row.storage.unwrap_or_else(|| "unknown".to_owned());
        let path = row
            .path
            .map(|value| value.display().to_string())
            .unwrap_or_default();
        writeln!(
            out,
            "{}",
            format_status_ok(&row.name, &storage, &path, row.ready)
        )
        .map_err(write_cli_error)?;
        Ok(())
    }

    fn config_dir(&self) -> ConfigDir {
        match &self.config_dir {
            Some(path) => ConfigDir::new(path.clone()),
            None => ConfigDir::default(),
        }
    }
}

#[derive(Debug, Clone)]
struct ConfigRepository {
    config_dir: ConfigDir,
}

impl ConfigRepository {
    fn new(config_dir: ConfigDir) -> Self {
        Self { config_dir }
    }
}

impl StatusRepository for ConfigRepository {
    fn list_entries(&self) -> crate::application::status::Result<Vec<StatusEntry>> {
        let entries = self
            .config_dir
            .each_provider()?
            .map(|(name, loaded)| match loaded {
                Ok(config) => {
                    let spec = ProviderSpec {
                        name: name.clone(),
                        path: config.path,
                        storage: config.storage.into(),
                        telemetry: config.telemetry.into(),
                    };
                    match spec.validate() {
                        Ok(()) => StatusEntry::Loaded(spec),
                        Err(error) => StatusEntry::Error {
                            name,
                            detail: error.to_string(),
                        },
                    }
                }
                Err(error) => StatusEntry::Error {
                    name,
                    detail: error.to_string(),
                },
            })
            .collect();
        Ok(entries)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ProviderServiceControl;

impl ServiceControl for ProviderServiceControl {
    fn ready(&self, provider_name: &str) -> bool {
        crate::cli::provider_control::provider_daemon_ready(provider_name)
    }
}

fn map_status_error(error: StatusError) -> crate::cli::Error {
    match error {
        StatusError::Config(source) => crate::cli::Error::Config(source),
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
    use crate::application::status::StatusUseCase;

    #[derive(Default)]
    struct StaticStatusUseCase {
        rows: Vec<ProviderStatusRow>,
    }

    impl StatusUseCase for StaticStatusUseCase {
        fn list(&self) -> crate::application::status::Result<Vec<ProviderStatusRow>> {
            Ok(self.rows.clone())
        }
    }

    fn local_row(name: &str, ready: bool) -> ProviderStatusRow {
        ProviderStatusRow {
            name: name.to_owned(),
            storage: Some("local".to_owned()),
            path: Some(PathBuf::from(format!("/mnt/{name}"))),
            ready,
            error: None,
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
        let cmd = StatusCommand { config_dir: None };
        let mut buf = Vec::new();
        cmd._execute(&StaticStatusUseCase::default(), &mut buf)
            .expect("status");
        let s = String::from_utf8(buf).expect("utf8");
        assert!(s.contains("No configured providers."));
    }

    #[test]
    fn probe_false_yields_not_running_lines() {
        let cmd = StatusCommand { config_dir: None };
        let mut buf = Vec::new();
        let use_case = StaticStatusUseCase {
            rows: vec![local_row("alpha", false)],
        };
        cmd._execute(&use_case, &mut buf).expect("status");
        let s = String::from_utf8(buf).expect("utf8");
        assert!(s.contains("- alpha (local, /mnt/alpha): not running"));
    }

    #[test]
    fn probe_true_yields_running() {
        let cmd = StatusCommand { config_dir: None };
        let mut buf = Vec::new();
        let use_case = StaticStatusUseCase {
            rows: vec![local_row("beta", true)],
        };
        cmd._execute(&use_case, &mut buf).expect("status");
        let s = String::from_utf8(buf).expect("utf8");
        assert!(s.contains("- beta (local, /mnt/beta): running"));
    }

    #[test]
    fn invalid_entry_still_lists_other_provider() {
        let cmd = StatusCommand { config_dir: None };
        let mut buf = Vec::new();
        let use_case = StaticStatusUseCase {
            rows: vec![
                ProviderStatusRow {
                    name: "bad".to_owned(),
                    storage: None,
                    path: None,
                    ready: false,
                    error: Some("invalid TOML".to_owned()),
                },
                local_row("good", false),
            ],
        };
        cmd._execute(&use_case, &mut buf).expect("status");
        let s = String::from_utf8(buf).expect("utf8");
        assert!(s.contains("- bad: error — invalid TOML"));
        assert!(s.contains("- good (local, /mnt/good): not running"));
    }
}
