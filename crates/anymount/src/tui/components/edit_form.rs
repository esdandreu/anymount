use super::super::edit::{EditField, EditMode, EditSession, is_onedrive_auth_key};
use super::super::state::AppState;
use super::super::theme_layout::{COLOR_BUTTON, COLOR_ROW_BG_NORMAL, COLOR_ROW_BG_SELECTED};
use super::footer::edit_footer_text;
use super::{AppAction, Component};
use crate::cli::commands::config::ProviderType;
use crate::tui::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::prelude::Stylize;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Paragraph};

pub(crate) struct EditFormComponent;

impl EditFormComponent {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Component for EditFormComponent {
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> Result<AppAction> {
        let super::super::edit::UiMode::Edit(session) = &mut state.mode else {
            return Ok(AppAction::Noop);
        };

        match session.mode {
            EditMode::Navigate => match key.code {
                KeyCode::Esc => Ok(AppAction::CancelEdit),
                KeyCode::Char('q') => Ok(AppAction::Quit),
                KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                    session.select_prev();
                    Ok(AppAction::Noop)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    session.select_next();
                    Ok(AppAction::Noop)
                }
                KeyCode::Tab => {
                    if let Some(message) = session.complete_selected_path()? {
                        Ok(AppAction::SetStatus(message))
                    } else {
                        session.select_next();
                        Ok(AppAction::Noop)
                    }
                }
                KeyCode::Enter => {
                    if matches!(session.selected_field(), EditField::StorageType) {
                        session.mode = EditMode::StorageTypeChoice {
                            index: session.storage_choice_index(),
                        };
                    } else {
                        session.mode = EditMode::TextInput;
                    }
                    Ok(AppAction::Noop)
                }
                KeyCode::Char('c') => Ok(AppAction::SaveEdit),
                KeyCode::Char('x') => Ok(AppAction::DeleteFromEdit),
                KeyCode::Char('d') => Ok(AppAction::DisconnectByName(session.draft.name.clone())),
                KeyCode::Char('l') | KeyCode::Char('L')
                    if matches!(session.selected_field(), EditField::StorageType) =>
                {
                    session.draft.storage_type = ProviderType::Local;
                    session.ensure_selected_visible();
                    Ok(AppAction::Noop)
                }
                KeyCode::Char('o') | KeyCode::Char('O')
                    if matches!(session.selected_field(), EditField::StorageType) =>
                {
                    session.draft.storage_type = ProviderType::OneDrive;
                    session.ensure_selected_visible();
                    Ok(AppAction::Noop)
                }
                _ if is_onedrive_auth_key(key.code) => Ok(AppAction::AuthenticateOneDrive),
                KeyCode::Backspace => {
                    session.backspace();
                    Ok(AppAction::Noop)
                }
                KeyCode::Char(c) => {
                    session.append_char(c);
                    Ok(AppAction::Noop)
                }
                _ => Ok(AppAction::Noop),
            },
            EditMode::TextInput => match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    session.mode = EditMode::Navigate;
                    Ok(AppAction::Noop)
                }
                KeyCode::Backspace => {
                    session.backspace();
                    Ok(AppAction::Noop)
                }
                KeyCode::Delete => {
                    session.clear_selected();
                    Ok(AppAction::Noop)
                }
                KeyCode::Tab => {
                    if let Some(message) = session.complete_selected_path()? {
                        Ok(AppAction::SetStatus(message))
                    } else {
                        Ok(AppAction::Noop)
                    }
                }
                KeyCode::Char(c) => {
                    session.append_char(c);
                    Ok(AppAction::Noop)
                }
                _ => Ok(AppAction::Noop),
            },
            EditMode::StorageTypeChoice { mut index } => match key.code {
                KeyCode::Esc => {
                    session.mode = EditMode::Navigate;
                    Ok(AppAction::Noop)
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if index == 0 {
                        index = EditSession::storage_choices().len() - 1;
                    } else {
                        index -= 1;
                    }
                    session.mode = EditMode::StorageTypeChoice { index };
                    Ok(AppAction::Noop)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    index = (index + 1) % EditSession::storage_choices().len();
                    session.mode = EditMode::StorageTypeChoice { index };
                    Ok(AppAction::Noop)
                }
                KeyCode::Enter => {
                    session.draft.storage_type = EditSession::storage_choices()[index];
                    session.ensure_selected_visible();
                    session.mode = EditMode::Navigate;
                    Ok(AppAction::Noop)
                }
                _ => Ok(AppAction::Noop),
            },
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let super::super::edit::UiMode::Edit(session) = &state.mode else {
            return;
        };

        let edit_area = area;
        let visible_fields = session.draft.visible_fields();
        let mut y = edit_area.y + 1;

        for field in &visible_fields {
            let is_active = *field == session.selected_field();
            let value = session.draft.field_value(*field);
            let shown = if value.is_empty() {
                "<unset>".to_owned()
            } else {
                value
            };

            let bg = if is_active {
                COLOR_ROW_BG_SELECTED
            } else {
                COLOR_ROW_BG_NORMAL
            };
            let cursor = if is_active { "█" } else { "" };
            let content = format!("  {:25}  {}{}", field.label(), shown, cursor);

            let rect = Rect::new(edit_area.x, y, edit_area.width, 1);
            let block = Block::default().bg(bg);
            frame.render_widget(block, rect);
            frame.render_widget(Paragraph::new(content), rect);
            y += 1;
        }

        let is_new = session.original_name.is_none();
        let nav_text = "[ ⇑ ] [ ⇓ ]";
        let action_text = edit_footer_text(is_new);
        let button_text = format!("{nav_text}  {action_text}");
        let button_area = Rect::new(
            area.x,
            area.y + area.height.saturating_sub(2),
            area.width,
            2,
        );

        let block = Block::default()
            .bg(COLOR_ROW_BG_NORMAL)
            .borders(Borders::TOP);
        frame.render_widget(block, button_area);
        frame.render_widget(
            Paragraph::new(button_text)
                .style(Style::default().fg(COLOR_BUTTON))
                .alignment(ratatui::layout::Alignment::Right),
            button_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DriverFileConfig;
    use crate::domain::driver::StorageConfig;
    use crate::tui::edit::UiMode;
    use crate::tui::model::ProviderEntry;
    use std::path::PathBuf;

    fn provider() -> ProviderEntry {
        ProviderEntry {
            name: "alpha".to_owned(),
            config: DriverFileConfig {
                path: PathBuf::from("/mnt/alpha"),
                storage: StorageConfig::Local {
                    root: PathBuf::from("/data/alpha"),
                },
                telemetry: Default::default(),
            },
        }
    }

    #[test]
    fn edit_q_maps_to_quit_action() {
        let mut component = EditFormComponent::new();
        let mut state = AppState {
            providers: vec![provider()],
            selected: 0,
            hovered: 0,
            is_keyboard_mode: true,
            status: String::new(),
            mode: UiMode::Edit(EditSession::new_for_edit(&provider())),
        };

        let action = component
            .handle_key(KeyEvent::from(KeyCode::Char('q')), &mut state)
            .expect("edit key handling should succeed");

        assert!(matches!(action, AppAction::Quit));
    }
}
