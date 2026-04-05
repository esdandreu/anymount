use super::super::edit::{EditSession, UiMode};
use super::super::model::ProviderEntry;
use super::super::state::{AppState, suggest_new_provider_name};
use super::super::theme_layout::{
    COLOR_BUTTON, COLOR_CONNECTED, COLOR_DISCONNECTED, COLOR_ROW_3D_SHADOW, COLOR_ROW_BG_HOVERED,
    COLOR_ROW_BG_NORMAL, MIN_NAME_WIDTH, STORAGE_TYPE_WIDTH, compute_mount_row_layout,
};
use super::{AppAction, Component};
use crate::tui::Result;
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::prelude::Stylize;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Paragraph};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowStyle {
    Normal,
    Disconnected,
    HoveredConnected,
    HoveredDisconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowAction {
    Connect,
    Disconnect,
    Edit,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy)]
pub(crate) struct MountRowRenderModel {
    pub(crate) left_offset: u16,
    pub(crate) shadow_width: u16,
    pub(crate) row_rect: Rect,
}

pub(crate) struct ProviderListComponent;

impl ProviderListComponent {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn handle_browse_key(&mut self, key: KeyEvent, state: &mut AppState) -> AppAction {
        match key.code {
            KeyCode::Char('q') => AppAction::Quit,
            KeyCode::Esc => AppAction::Noop,
            KeyCode::Down | KeyCode::Char('j') => {
                state.is_keyboard_mode = true;
                state.select_next();
                AppAction::Noop
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.is_keyboard_mode = true;
                state.select_prev();
                AppAction::Noop
            }
            KeyCode::Char('r') => AppAction::Refresh,
            KeyCode::Char('e') | KeyCode::Enter => {
                if state.is_add_row() {
                    AppAction::OpenAdd
                } else {
                    AppAction::OpenEditSelected
                }
            }
            KeyCode::Char('d') => AppAction::DisconnectSelected,
            KeyCode::Char('c') => AppAction::ConnectSelected,
            _ => AppAction::Noop,
        }
    }
}

impl Component for ProviderListComponent {
    fn handle_mouse(
        &mut self,
        mouse: MouseEvent,
        area: Rect,
        state: &mut AppState,
    ) -> Result<AppAction> {
        if matches!(state.mode, UiMode::Edit(_)) {
            return Ok(AppAction::Noop);
        }

        let list_area = Rect::new(0, 0, area.width, area.height.saturating_sub(2));
        match mouse.kind {
            MouseEventKind::Moved => {
                state.is_keyboard_mode = false;
                let row = (mouse.row as usize).saturating_sub(list_area.y as usize);
                if row <= state.providers.len() {
                    state.hovered = row;
                }
                Ok(AppAction::Noop)
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let row = (mouse.row as usize).saturating_sub(list_area.y as usize);
                if row < state.providers.len() {
                    state.hovered = row;
                    state.selected = row;
                    if let Some(p) = state.selected_provider().cloned() {
                        let is_connected = p.is_connected();
                        let style = if is_connected {
                            RowStyle::HoveredConnected
                        } else {
                            RowStyle::HoveredDisconnected
                        };
                        let rect = Rect::new(list_area.x, mouse.row, list_area.width, 1);
                        let model = mount_row_render_model(rect.width, style);
                        let row_rect = Rect::new(
                            rect.x + model.left_offset,
                            rect.y,
                            model.row_rect.width,
                            rect.height,
                        );
                        return Ok(
                            match hit_test_row_action(row_rect, mouse.column, is_connected) {
                                Some(RowAction::Connect) => AppAction::ConnectSelected,
                                Some(RowAction::Disconnect) => AppAction::DisconnectSelected,
                                Some(RowAction::Edit) | None => {
                                    state.mode = UiMode::Edit(EditSession::new_for_edit(&p));
                                    AppAction::Noop
                                }
                            },
                        );
                    }
                } else if row == state.providers.len() {
                    let default_name = suggest_new_provider_name(state);
                    state.mode = UiMode::Edit(EditSession::new_for_add(default_name));
                }
                Ok(AppAction::Noop)
            }
            _ => Ok(AppAction::Noop),
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let row_height = 1;
        let mut y = area.y;

        for (i, entry) in state.providers.iter().enumerate() {
            let is_hovered = i == state.hovered;
            let is_connected = entry.is_connected();

            let style = if is_hovered {
                if is_connected {
                    RowStyle::HoveredConnected
                } else {
                    RowStyle::HoveredDisconnected
                }
            } else if is_connected {
                RowStyle::Normal
            } else {
                RowStyle::Disconnected
            };

            let rect = Rect::new(area.x, y, area.width, row_height);
            render_mount_row(
                frame,
                entry,
                rect,
                style,
                is_hovered,
                state.is_keyboard_mode,
            );
            y += row_height;
        }

        let add_rect = Rect::new(area.x, y, area.width, row_height);
        render_add_row(frame, add_rect, state.is_add_row());
    }
}

fn mount_row_render_model(available_width: u16, style: RowStyle) -> MountRowRenderModel {
    let left_offset = match style {
        RowStyle::Normal => 0,
        RowStyle::Disconnected => 1,
        RowStyle::HoveredConnected | RowStyle::HoveredDisconnected => 2,
    };
    let row_rect = bounded_row_rect(Rect::new(0, 0, available_width, 1), left_offset);

    MountRowRenderModel {
        left_offset,
        shadow_width: left_offset,
        row_rect,
    }
}

fn bounded_row_rect(rect: Rect, left_offset: u16) -> Rect {
    Rect::new(
        rect.x,
        rect.y,
        rect.width.saturating_sub(left_offset),
        rect.height,
    )
}

fn row_button_labels(is_connected: bool) -> (&'static str, &'static str) {
    if is_connected {
        ("[ ⇐ ]", "[ ↵ ]")
    } else {
        ("[ ⇒ ]", "[ ↵ ]")
    }
}

fn row_action_areas(row_rect: Rect, is_connected: bool) -> Option<(Rect, Rect)> {
    let inner_width = row_rect.width.saturating_sub(2);
    let inner_x = row_rect.x + 1;
    let (primary_label, edit_label) = row_button_labels(is_connected);
    let total_width = primary_label.chars().count() as u16 + 1 + edit_label.chars().count() as u16;
    if inner_width < total_width {
        return None;
    }

    let start_x = inner_x + inner_width - total_width;
    let primary_rect = Rect::new(
        start_x,
        row_rect.y,
        primary_label.chars().count() as u16,
        row_rect.height,
    );
    let edit_rect = Rect::new(
        start_x + primary_rect.width + 1,
        row_rect.y,
        edit_label.chars().count() as u16,
        row_rect.height,
    );
    Some((primary_rect, edit_rect))
}

fn hit_test_row_action(row_rect: Rect, column: u16, is_connected: bool) -> Option<RowAction> {
    let (primary_rect, edit_rect) = row_action_areas(row_rect, is_connected)?;
    if column >= primary_rect.x && column < primary_rect.x + primary_rect.width {
        return Some(if is_connected {
            RowAction::Disconnect
        } else {
            RowAction::Connect
        });
    }
    if column >= edit_rect.x && column < edit_rect.x + edit_rect.width {
        return Some(RowAction::Edit);
    }
    None
}

fn truncate_cell(value: &str, width: u16) -> String {
    let width = width as usize;
    if width == 0 {
        return String::new();
    }

    let len = value.chars().count();
    if len <= width {
        return format!("{value:<width$}");
    }
    if width == 1 {
        return "…".to_owned();
    }

    let mut result: String = value.chars().take(width - 1).collect();
    result.push('…');
    result
}

fn format_mount_row_text(
    entry: &ProviderEntry,
    available_width: u16,
    show_buttons: bool,
) -> String {
    let layout = compute_mount_row_layout(available_width, true, true, show_buttons);
    let mut segments = vec![truncate_cell(&entry.name, MIN_NAME_WIDTH)];

    if layout.show_path {
        segments.push(truncate_cell(
            &entry.config.path.display().to_string(),
            layout.path_width,
        ));
    }

    if layout.show_storage_type {
        segments.push(truncate_cell(
            get_storage_type_label(entry),
            STORAGE_TYPE_WIDTH,
        ));
    }

    let mut line = segments.join("  ");
    if layout.show_buttons {
        if !line.is_empty() {
            line.push_str("  ");
        }
        line.push_str("[ ⇒ ] [ ↵ ]");
    }
    line
}

fn render_mount_row(
    frame: &mut Frame,
    entry: &ProviderEntry,
    rect: Rect,
    style: RowStyle,
    show_buttons: bool,
    is_keyboard_mode: bool,
) {
    let is_hovered = matches!(
        style,
        RowStyle::HoveredConnected | RowStyle::HoveredDisconnected
    );
    let is_connected = entry.is_connected();
    let model = mount_row_render_model(rect.width, style);

    let bg_color = if is_hovered {
        COLOR_ROW_BG_HOVERED
    } else {
        COLOR_ROW_BG_NORMAL
    };

    let status_icon = if is_connected { "●" } else { "○" };
    let status_color = if is_connected {
        COLOR_CONNECTED
    } else {
        COLOR_DISCONNECTED
    };

    if model.shadow_width > 0 {
        let shadow_rect = Rect::new(rect.x, rect.y, model.shadow_width, rect.height);
        frame.render_widget(
            Paragraph::new(" ".repeat(model.shadow_width as usize))
                .style(Style::default().bg(COLOR_ROW_3D_SHADOW)),
            shadow_rect,
        );
    }

    let row_rect = Rect::new(
        rect.x + model.left_offset,
        rect.y,
        model.row_rect.width,
        rect.height,
    );
    let row_block = Block::default()
        .bg(bg_color)
        .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT);
    frame.render_widget(row_block, row_rect);

    let keyboard_indicator = if is_keyboard_mode && is_hovered {
        "⇅"
    } else {
        " "
    };
    let content_width = row_rect.width.saturating_sub(2);
    let button_width = if show_buttons {
        row_action_areas(row_rect, is_connected)
            .map(|(primary_rect, edit_rect)| primary_rect.width + edit_rect.width + 1)
            .unwrap_or(0)
    } else {
        0
    };
    let content_padding = if show_buttons && button_width > 0 {
        2
    } else {
        0
    };
    let content = format_mount_row_text(
        entry,
        content_width.saturating_sub(button_width + content_padding),
        false,
    );
    let content = if content_width > 0 {
        let prefix = format!("{keyboard_indicator}{status_icon}  ");
        let line = format!("{prefix}{content}");
        truncate_cell(&line, content_width)
    } else {
        String::new()
    };
    let text_style = Style::default().fg(status_color);

    frame.render_widget(
        Paragraph::new(content).style(text_style),
        Rect::new(
            row_rect.x + 1,
            row_rect.y,
            row_rect.width.saturating_sub(2),
            row_rect.height,
        ),
    );

    if show_buttons {
        if let Some((primary_rect, edit_rect)) = row_action_areas(row_rect, is_connected) {
            let (primary_label, edit_label) = row_button_labels(is_connected);
            frame.render_widget(
                Paragraph::new(primary_label).style(Style::default().fg(COLOR_BUTTON)),
                primary_rect,
            );
            frame.render_widget(
                Paragraph::new(edit_label).style(Style::default().fg(COLOR_BUTTON)),
                edit_rect,
            );
        }
    }
}

fn get_storage_type_label(entry: &ProviderEntry) -> &'static str {
    match &entry.config.storage {
        crate::domain::driver::StorageConfig::Local { .. } => "local",
        crate::domain::driver::StorageConfig::OneDrive { .. } => "onedrive",
    }
}

fn render_add_row(frame: &mut Frame, rect: Rect, is_hovered: bool) {
    let bg_color = if is_hovered {
        COLOR_ROW_BG_HOVERED
    } else {
        COLOR_ROW_BG_NORMAL
    };

    let displacement = if is_hovered { 2 } else { 0 };
    let shadow_width = if is_hovered { 2 } else { 0 };

    if shadow_width > 0 {
        let shadow_rect = Rect::new(
            rect.x.saturating_sub(shadow_width),
            rect.y,
            shadow_width,
            rect.height,
        );
        frame.render_widget(
            Paragraph::new(" ".repeat(shadow_width as usize))
                .style(Style::default().bg(COLOR_ROW_3D_SHADOW)),
            shadow_rect,
        );
    }

    let row_rect = Rect::new(rect.x + displacement, rect.y, rect.width, rect.height);
    let row_block = Block::default()
        .bg(bg_color)
        .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT);
    frame.render_widget(row_block, row_rect);

    let content = if is_hovered {
        "+                                                  [ ↵ Add ]"
    } else {
        "+"
    };

    frame.render_widget(
        Paragraph::new(content).style(Style::default().fg(COLOR_BUTTON)),
        Rect::new(
            row_rect.x + 1,
            row_rect.y,
            row_rect.width.saturating_sub(2),
            row_rect.height,
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DriverFileConfig;
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
    fn browse_q_maps_to_quit_action() {
        let mut component = ProviderListComponent::new();
        let mut state = AppState {
            providers: vec![local_provider("a")],
            selected: 0,
            hovered: 0,
            is_keyboard_mode: true,
            status: String::new(),
            mode: UiMode::Browse,
        };

        let action = component.handle_browse_key(KeyEvent::from(KeyCode::Char('q')), &mut state);

        assert!(matches!(action, AppAction::Quit));
    }

    #[test]
    fn browse_down_selects_next_row() {
        let mut component = ProviderListComponent::new();
        let mut state = AppState {
            providers: vec![local_provider("a"), local_provider("b")],
            selected: 0,
            hovered: 0,
            is_keyboard_mode: false,
            status: String::new(),
            mode: UiMode::Browse,
        };

        let action = component.handle_browse_key(KeyEvent::from(KeyCode::Down), &mut state);

        assert!(matches!(action, AppAction::Noop));
        assert_eq!(state.selected, 1);
        assert_eq!(state.hovered, 1);
        assert!(state.is_keyboard_mode);
    }
}
