use super::Result;
use super::edit::UiMode;
use super::model::{ProviderEntry, provider_entry_from_spec};
use crate::application::config::ConfigUseCase;

#[derive(Debug, Clone)]
pub(crate) struct AppState {
    pub(crate) providers: Vec<ProviderEntry>,
    pub(crate) selected: usize,
    pub(crate) hovered: usize,
    pub(crate) is_keyboard_mode: bool,
    pub(crate) status: String,
    pub(crate) mode: UiMode,
}

impl AppState {
    pub(crate) fn load<U>(use_case: &U) -> Result<Self>
    where
        U: ConfigUseCase,
    {
        let names = use_case.list()?;
        let mut providers = Vec::with_capacity(names.len());
        for name in names {
            let spec = use_case.read(&name)?;
            providers.push(provider_entry_from_spec(spec));
        }
        Ok(Self {
            providers,
            selected: 0,
            hovered: 0,
            is_keyboard_mode: true,
            status: String::new(),
            mode: UiMode::Browse,
        })
    }

    pub(crate) fn refresh<U>(&mut self, use_case: &U) -> Result<()>
    where
        U: ConfigUseCase,
    {
        let selected_name = self.selected_name().map(ToOwned::to_owned);
        let refreshed = Self::load(use_case)?;
        self.providers = refreshed.providers;
        self.status = refreshed.status;
        self.hovered = 0;
        if let Some(name) = selected_name {
            if let Some(pos) = self
                .providers
                .iter()
                .position(|provider| provider.name == name)
            {
                self.selected = pos;
                return Ok(());
            }
        }
        self.selected = self.selected.min(self.providers.len().saturating_sub(1));
        Ok(())
    }

    pub(crate) fn selected_name(&self) -> Option<&str> {
        self.providers
            .get(self.selected)
            .map(|provider| provider.name.as_str())
    }

    pub(crate) fn selected_provider(&self) -> Option<&ProviderEntry> {
        self.providers.get(self.selected)
    }

    pub(crate) fn select_next(&mut self) {
        if self.providers.is_empty() {
            return;
        }
        self.hovered = (self.hovered + 1) % (self.providers.len() + 1);
        self.selected = self.hovered;
    }

    pub(crate) fn select_prev(&mut self) {
        if self.providers.is_empty() {
            return;
        }
        if self.hovered == 0 {
            self.hovered = self.providers.len();
        } else {
            self.hovered -= 1;
        }
        self.selected = self.hovered;
    }

    pub(crate) fn is_add_row(&self) -> bool {
        self.hovered >= self.providers.len()
    }
}

pub(crate) fn suggest_new_provider_name(state: &AppState) -> String {
    let mut index = 1;
    loop {
        let candidate = format!("provider-{index}");
        if !state
            .providers
            .iter()
            .any(|provider| provider.name == candidate)
        {
            return candidate;
        }
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::super::adapters::TuiConfigRepository;
    use super::super::model::ProviderEntry;
    use super::*;
    use crate::DriverFileConfig;
    use crate::application::config::Application as ConfigApplication;
    use crate::config::ConfigDir;
    use crate::domain::driver::StorageConfig;
    use std::path::PathBuf;

    fn local_provider(name: &str) -> ProviderEntry {
        ProviderEntry {
            name: name.to_owned(),
            config: DriverFileConfig {
                path: PathBuf::from(format!("/mnt/{name}")),
                storage: StorageConfig::Local {
                    root: PathBuf::from(format!("/data/{name}")),
                },
                telemetry: Default::default(),
            },
        }
    }

    #[test]
    fn app_state_load_wraps_config_error() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let path = tmp.path().join("not-a-directory");
        std::fs::write(&path, "oops").expect("seed file should succeed");
        let cd = ConfigDir::new(path);
        let repository = TuiConfigRepository::new(cd);
        let app = ConfigApplication::new(&repository);
        let err = AppState::load(&app).expect_err("load should fail");

        assert!(matches!(err, crate::tui::Error::Config(_)));
    }

    #[test]
    fn select_next_wraps_to_start() {
        let mut state = AppState {
            providers: vec![local_provider("a"), local_provider("b")],
            selected: 1,
            hovered: 1,
            is_keyboard_mode: true,
            status: String::new(),
            mode: UiMode::Browse,
        };

        state.select_next();

        assert_eq!(state.hovered, 2);
        assert_eq!(state.selected, 2);
    }

    #[test]
    fn select_prev_wraps_to_end() {
        let mut state = AppState {
            providers: vec![
                local_provider("a"),
                local_provider("b"),
                local_provider("c"),
            ],
            selected: 0,
            hovered: 0,
            is_keyboard_mode: true,
            status: String::new(),
            mode: UiMode::Browse,
        };

        state.select_prev();

        assert_eq!(state.hovered, 3);
        assert_eq!(state.selected, 3);
    }

    #[test]
    fn selected_name_none_when_empty() {
        let state = AppState {
            providers: Vec::new(),
            selected: 0,
            hovered: 0,
            is_keyboard_mode: true,
            status: String::new(),
            mode: UiMode::Browse,
        };

        assert!(state.selected_name().is_none());
    }
}
