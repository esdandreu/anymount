use std::path::PathBuf;

use crate::domain::{Config, ConfigRepository, StorageConfig};

use super::event::{AppEvent, Event, EventHandler};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    DefaultTerminal,
    buffer::Buffer,
    layout::Rect,
    widgets::{StatefulWidget, Widget},
};

use super::widgets::{MountItem, MountsList, MountsListState};

pub struct MountConfig {
    pub name: String,
    /// Local mount path.
    pub path: PathBuf,
    /// Storage backend configuration.
    pub storage: Box<dyn StorageConfig>,
    /// Whether the mount is currently connected.
    pub is_connected: bool,
}

/// Application.
pub struct App {
    /// Is the application running?
    pub running: bool,
    /// Event handler.
    pub events: EventHandler,
    /// List of configured mounts.
    pub mounts: Vec<MountConfig>,
    /// Stateful navigation for the mounts list.
    pub mounts_list: MountsListState,
    // TODO mount_service
}

impl Default for App {
    fn default() -> Self {
        Self {
            running: true,
            events: EventHandler::new(),
            mounts: Vec::new(),
            mounts_list: MountsListState::default(),
        }
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new(config_repository: impl ConfigRepository) -> Self {
        Self {
            mounts: config_repository.list().map(MountConfig::from).collect(),
            ..Self::default()
        }
    }

    /// Run the application's main loop.
    pub fn run(
        mut self,
        mut terminal: DefaultTerminal,
    ) -> color_eyre::Result<()> {
        while self.running {
            terminal
                .draw(|frame| frame.render_widget(&mut self, frame.area()))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn handle_events(&mut self) -> color_eyre::Result<()> {
        match self.events.next()? {
            Event::Tick => self.tick(),
            Event::Crossterm(event) => match event {
                crossterm::event::Event::Key(key_event)
                    if key_event.kind
                        == crossterm::event::KeyEventKind::Press =>
                {
                    self.handle_key_event(key_event)?
                }
                _ => {}
            },
            Event::App(app_event) => match app_event {
                AppEvent::SelectPrevious => self.select_previous(),
                AppEvent::SelectNext => self.select_next(),
                AppEvent::Quit => self.quit(),
                AppEvent::Connect => {}
                AppEvent::Disconnect => {}
                AppEvent::Edit => {}
            },
        }
        Ok(())
    }

    /// Handles the key events and updates the state of [`App`].
    fn handle_key_event(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<()> {
        match key_event.code {
            // Quit
            KeyCode::Esc | KeyCode::Char('q') => {
                self.events.send(AppEvent::Quit)
            }
            KeyCode::Char('c' | 'C')
                if key_event.modifiers == KeyModifiers::CONTROL =>
            {
                self.events.send(AppEvent::Quit)
            }
            // Up
            KeyCode::Up | KeyCode::Char('j') => {
                self.events.send(AppEvent::SelectPrevious)
            }
            // Down
            KeyCode::Down | KeyCode::Char('k') => {
                self.events.send(AppEvent::SelectNext)
            }
            // Connect/Disconnect
            KeyCode::Right => self.events.send(AppEvent::Connect),
            KeyCode::Left => self.events.send(AppEvent::Disconnect),
            _ => {}
        }
        Ok(())
    }

    /// Handles the tick event of the terminal.
    ///
    /// The tick event is where you can update the state of your application with any logic that
    /// needs to be updated at a fixed frame rate. E.g. polling a server, updating an animation.
    fn tick(&self) {}

    /// Set running to false to quit the application.
    fn quit(&mut self) {
        self.running = false;
    }

    /// Selects the next mount row.
    fn select_next(&mut self) {
        self.mounts_list.select_next(self.mounts.len());
    }

    /// Selects the previous mount in the list or the last one if no mount is selected.
    fn select_previous(&mut self) {
        self.mounts_list.select_previous(self.mounts.len());
    }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // TODO if editing render a different component.
        let mounts = self
            .mounts
            .iter()
            .map(|m| MountItem {
                name: &m.name,
                path: m.path.as_path(),
                storage_type: m.storage.kind(),
                is_connected: m.is_connected,
            })
            .collect::<Vec<_>>();
        let widget = MountsList {
            mounts: mounts.as_slice(),
        };
        StatefulWidget::render(&widget, area, buf, &mut self.mounts_list);
    }
}

impl From<Config> for MountConfig {
    fn from(config: Config) -> Self {
        Self {
            name: config.name,
            path: config.path,
            storage: config.storage,
            is_connected: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::App;
    use crate::domain::{
        Config, ConfigRepository, ConnectStorageError, Storage, StorageConfig,
    };
    use ratatui::{Terminal, backend::TestBackend};

    #[derive(serde::Serialize, serde::Deserialize)]
    struct TestStorageConfig;

    #[typetag::serde(name = "tui-test")]
    impl StorageConfig for TestStorageConfig {
        fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
            unreachable!("rendering must not connect storage")
        }

        fn kind(&self) -> &'static str {
            "local"
        }
    }

    struct TestConfigRepository {
        names: Vec<&'static str>,
    }

    impl ConfigRepository for TestConfigRepository {
        type Iter<'a> = std::vec::IntoIter<Config>;

        fn list(&self) -> Self::Iter<'_> {
            self.names
                .iter()
                .map(|name| Config {
                    name: (*name).to_owned(),
                    path: PathBuf::from("/tmp/mnt"),
                    storage: Box::new(TestStorageConfig),
                    driver: None,
                })
                .collect::<Vec<_>>()
                .into_iter()
        }
    }

    #[test]
    fn loads_mounts_from_config_repository() {
        let app = App::new(TestConfigRepository {
            names: vec!["First", "Second"],
        });

        assert_eq!(app.mounts.len(), 2);
        assert_eq!(app.mounts[0].name, "First");
        assert_eq!(app.mounts[0].storage.kind(), "local");
        assert!(!app.mounts[0].is_connected);
    }

    #[test]
    fn test_mounts_list() {
        let mut app = App::new(TestConfigRepository {
            names: vec![
                "Hello first",
                "Hello second",
                "Third",
                "Hello first",
                "Hello first",
            ],
        });
        app.mounts[1].is_connected = true;
        app.mounts[2].is_connected = true;
        let mut terminal = Terminal::new(TestBackend::new(80, 20))
            .expect("test terminal should be created");
        terminal
            .draw(|frame| frame.render_widget(&mut app, frame.area()))
            .expect("application should render");
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!(terminal.backend());
        });
    }
}
