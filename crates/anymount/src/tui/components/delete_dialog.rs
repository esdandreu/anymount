use super::super::edit::UiMode;
use super::super::state::AppState;
use super::super::theme_layout::{COLOR_BUTTON, COLOR_BUTTON_TEXT, COLOR_ROW_BG_SELECTED};
use super::{AppAction, Component};
use crate::tui::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::prelude::Stylize;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Paragraph};

pub(crate) struct DeleteDialogComponent;

impl DeleteDialogComponent {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Component for DeleteDialogComponent {
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> Result<AppAction> {
        let name = if let UiMode::DeleteConfirm { ref name } = state.mode {
            name.clone()
        } else {
            return Ok(AppAction::Noop);
        };

        match key.code {
            KeyCode::Char('y') => Ok(AppAction::DeleteConfirmed(name)),
            KeyCode::Char('n') | KeyCode::Esc => Ok(AppAction::DeleteCanceled),
            _ => Ok(AppAction::Noop),
        }
    }

    fn render(&mut self, frame: &mut Frame, _area: Rect, state: &AppState) {
        let UiMode::DeleteConfirm { name } = &state.mode else {
            return;
        };

        let size = frame.area();
        let dialog_width = 50;
        let dialog_height = 5;
        let x = (size.width.saturating_sub(dialog_width)) / 2;
        let y = (size.height.saturating_sub(dialog_height)) / 2;

        let dialog_rect = Rect::new(x, y, dialog_width, dialog_height);
        let content = format!("  Delete '{}'?  [ y ]  [ n ]", name);

        let block = Block::default()
            .bg(COLOR_ROW_BG_SELECTED)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_BUTTON));

        frame.render_widget(
            Paragraph::new(content)
                .style(Style::default().fg(COLOR_BUTTON_TEXT))
                .block(block),
            dialog_rect,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DriverFileConfig;
    use crate::domain::driver::StorageConfig;
    use std::path::PathBuf;

    #[test]
    fn delete_confirm_key_maps_to_delete_confirmed_action() {
        let mut component = DeleteDialogComponent::new();
        let mut state = AppState {
            providers: vec![super::super::super::model::ProviderEntry {
                name: "a".to_owned(),
                config: DriverFileConfig {
                    path: PathBuf::from("/mnt/a"),
                    storage: StorageConfig::Local {
                        root: PathBuf::from("/data/a"),
                    },
                    telemetry: Default::default(),
                },
            }],
            selected: 0,
            hovered: 0,
            is_keyboard_mode: true,
            status: String::new(),
            mode: UiMode::DeleteConfirm {
                name: "alpha".to_owned(),
            },
        };

        let action = component
            .handle_key(KeyEvent::from(KeyCode::Char('y')), &mut state)
            .expect("delete key should map to action");

        assert!(matches!(action, AppAction::DeleteConfirmed(name) if name == "alpha"));
    }
}
