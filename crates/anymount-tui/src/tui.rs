//! Configures and runs the terminal user interface.

use std::path::PathBuf;

use anymount::ConfigDir;

use super::{app::App, service::AnymountMountService};

/// Configures the anymount terminal user interface.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Tui {
    config_dir: Option<PathBuf>,
}

impl Tui {
    /// Creates a terminal interface using `config_dir`.
    ///
    /// A value of `None` selects the default anymount configuration directory.
    pub fn new(config_dir: Option<PathBuf>) -> Self {
        Self { config_dir }
    }

    /// Runs the terminal user interface.
    ///
    /// # Errors
    ///
    /// Returns an error when configuration, terminal setup, or event handling
    /// fails.
    pub fn run(self) -> color_eyre::Result<()> {
        color_eyre::install()?;
        let repository = self
            .config_dir
            .map_or_else(ConfigDir::default, ConfigDir::new);
        let service = AnymountMountService::new(repository);
        let app = App::new(service)?;
        let terminal = ratatui::init();
        let result = app.run(terminal);
        ratatui::restore();
        result
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::Tui;

    #[test]
    fn stores_config_directory() {
        let path = PathBuf::from("/tmp/anymount-config");

        let tui = Tui::new(Some(path.clone()));

        assert_eq!(tui.config_dir, Some(path));
    }

    #[test]
    fn accepts_default_config_directory() {
        let tui = Tui::new(None);

        assert_eq!(tui.config_dir, None);
    }
}
