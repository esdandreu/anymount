use std::path::PathBuf;

use crate::domain::driver::StorageConfig;

use super::event::{AppEvent, Event, EventHandler};

use super::components::{MountItem, MountsList, MountsListState};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    DefaultTerminal,
    buffer::Buffer,
    layout::Rect,
    widgets::{StatefulWidget, Widget},
};

#[derive(Debug)]
pub struct MountConfig {
    pub name: String,
    /// Local mount path.
    pub path: PathBuf,
    /// Storage backend configuration.
    pub storage: StorageConfig,
    /// Whether the mount is currently connected.
    pub is_connected: bool,
}

/// Application.
#[derive(Debug)]
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
            mounts: vec![
                MountConfig {
                    name: "Hello first".to_string(),
                    path: PathBuf::from("/tmp/mnt"),
                    storage: StorageConfig::Local {
                        root: PathBuf::from("/users/desktop"),
                    },
                    is_connected: false,
                },
                MountConfig {
                    name: "Hello second".to_string(),
                    path: PathBuf::from("/tmp/mnt"),
                    storage: StorageConfig::Local {
                        root: PathBuf::from("/users/desktop"),
                    },
                    is_connected: true,
                },
                MountConfig {
                    name: "Hello second".to_string(),
                    path: PathBuf::from("/tmp/mnt"),
                    storage: StorageConfig::Local {
                        root: PathBuf::from("/users/desktop"),
                    },
                    is_connected: true,
                },
                MountConfig {
                    name: "Hello second".to_string(),
                    path: PathBuf::from("/tmp/mnt"),
                    storage: StorageConfig::Local {
                        root: PathBuf::from("/users/desktop"),
                    },
                    is_connected: true,
                },
            ],
            mounts_list: MountsListState::default(),
        }
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new() -> Self {
        Self::default()
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
            KeyCode::Up => self.events.send(AppEvent::SelectPrevious),
            KeyCode::Char('j') => self.events.send(AppEvent::SelectPrevious),
            // Down
            KeyCode::Down => self.events.send(AppEvent::SelectNext),
            KeyCode::Char('k') => self.events.send(AppEvent::SelectNext),
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
                storage_type: m.storage.label(),
                is_connected: m.is_connected,
            })
            .collect::<Vec<_>>();
        let widget = MountsList {
            mounts: mounts.as_slice(),
        };
        StatefulWidget::render(&widget, area, buf, &mut self.mounts_list);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        App, EventHandler, MountConfig, MountsListState, StorageConfig,
    };
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn test_mounts_list() {
        let mut app = App {
            running: true,
            events: EventHandler::new(),
            mounts: vec![
                MountConfig {
                    name: "Hello first".to_string(),
                    path: PathBuf::from("/tmp/mnt"),
                    storage: StorageConfig::Local {
                        root: PathBuf::from("/users/desktop"),
                    },
                    is_connected: false,
                },
                MountConfig {
                    name: "Hello second".to_string(),
                    path: PathBuf::from("/tmp/mnt"),
                    storage: StorageConfig::Local {
                        root: PathBuf::from("/users/desktop"),
                    },
                    is_connected: true,
                },
                MountConfig {
                    name: "Third".to_string(),
                    path: PathBuf::from("/tmp/mnt"),
                    storage: StorageConfig::Local {
                        root: PathBuf::from("/users/desktop"),
                    },
                    is_connected: true,
                },
                MountConfig {
                    name: "Hello first".to_string(),
                    path: PathBuf::from("/tmp/mnt"),
                    storage: StorageConfig::Local {
                        root: PathBuf::from("/users/desktop"),
                    },
                    is_connected: false,
                },
                MountConfig {
                    name: "Hello first".to_string(),
                    path: PathBuf::from("/tmp/mnt"),
                    storage: StorageConfig::Local {
                        root: PathBuf::from("/users/desktop"),
                    },
                    is_connected: false,
                },
            ],
            mounts_list: MountsListState::default(),
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(&mut app, frame.area()))
            .unwrap();
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!(terminal.backend());
        });
    }
}
