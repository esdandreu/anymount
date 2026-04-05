use ratatui::layout::Rect;
use ratatui::style::Color;

pub(crate) const COLOR_CONNECTED: Color = Color::Green;
pub(crate) const COLOR_DISCONNECTED: Color = Color::DarkGray;
pub(crate) const COLOR_ROW_BG_NORMAL: Color = Color::Reset;
pub(crate) const COLOR_ROW_BG_HOVERED: Color = Color::Rgb(30, 40, 60);
pub(crate) const COLOR_ROW_BG_SELECTED: Color = Color::Rgb(45, 66, 99);
pub(crate) const COLOR_ROW_3D_SHADOW: Color = Color::DarkGray;
pub(crate) const COLOR_BUTTON: Color = Color::Cyan;
pub(crate) const COLOR_BUTTON_TEXT: Color = Color::Black;
pub(crate) const MIN_TERMINAL_WIDTH: u16 = 64;
pub(crate) const MIN_TERMINAL_HEIGHT: u16 = 8;
pub(crate) const MIN_NAME_WIDTH: u16 = 8;
pub(crate) const STORAGE_TYPE_WIDTH: u16 = 10;
pub(crate) const MIN_PATH_WIDTH: u16 = 8;
pub(crate) const BUTTONS_WIDTH: u16 = 13;
pub(crate) const STATUS_WIDTH: u16 = 2;
pub(crate) const COLUMN_GAP_WIDTH: u16 = 2;

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy)]
pub(crate) struct MountRowLayout {
    pub(crate) show_name: bool,
    pub(crate) show_path: bool,
    pub(crate) show_storage_type: bool,
    pub(crate) show_buttons: bool,
    pub(crate) preferred_path_width: u16,
    pub(crate) path_width: u16,
}

pub(crate) fn is_supported_size(area: Rect) -> bool {
    area.width >= MIN_TERMINAL_WIDTH && area.height >= MIN_TERMINAL_HEIGHT
}

pub(crate) fn compute_mount_row_layout(
    available_width: u16,
    show_path: bool,
    show_storage_type: bool,
    show_buttons: bool,
) -> MountRowLayout {
    let preferred_path_width = 25;
    let mut remaining = available_width;

    remaining = remaining.saturating_sub(STATUS_WIDTH);
    remaining = remaining.saturating_sub(MIN_NAME_WIDTH);

    let show_buttons = show_buttons && remaining >= BUTTONS_WIDTH + COLUMN_GAP_WIDTH;
    if show_buttons {
        remaining = remaining.saturating_sub(BUTTONS_WIDTH + COLUMN_GAP_WIDTH);
    }

    let mut show_storage_type = show_storage_type;
    if show_storage_type {
        let storage_budget = STORAGE_TYPE_WIDTH + COLUMN_GAP_WIDTH;
        if remaining >= storage_budget + MIN_PATH_WIDTH + COLUMN_GAP_WIDTH {
            remaining = remaining.saturating_sub(storage_budget);
        } else {
            show_storage_type = false;
        }
    }

    let mut path_width = 0;
    let mut show_path = show_path;
    if show_path {
        let path_budget = remaining.saturating_sub(COLUMN_GAP_WIDTH);
        if path_budget >= MIN_PATH_WIDTH {
            path_width = path_budget.min(preferred_path_width);
        } else {
            show_path = false;
        }
    }

    MountRowLayout {
        show_name: true,
        show_path,
        show_storage_type,
        show_buttons,
        preferred_path_width,
        path_width,
    }
}

pub(crate) fn unsupported_size_message(area: Rect) -> String {
    format!(
        "Terminal size not supported. Current: {}x{}, required: {}x{}.",
        area.width, area.height, MIN_TERMINAL_WIDTH, MIN_TERMINAL_HEIGHT
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn terminal_size_support_rejects_smaller_than_72x20() {
        assert!(!is_supported_size(Rect::new(0, 0, 71, 20)));
        assert!(!is_supported_size(Rect::new(0, 0, 72, 19)));
        assert!(is_supported_size(Rect::new(0, 0, 72, 20)));
    }

    #[test]
    fn row_layout_drops_path_before_storage_type() {
        let layout = compute_mount_row_layout(80, true, true, true);

        assert!(layout.show_name);
        assert!(layout.show_buttons);
        assert!(layout.path_width <= layout.preferred_path_width);
    }

    #[test]
    fn row_layout_can_remove_path_and_storage_type_but_keeps_name_and_buttons() {
        let layout = compute_mount_row_layout(32, true, true, true);

        assert!(layout.show_name);
        assert!(layout.show_buttons);
        assert!(!layout.show_path || !layout.show_storage_type);
    }

    #[test]
    fn unsupported_size_message_mentions_minimum() {
        let message = unsupported_size_message(Rect::new(0, 0, 70, 18));

        assert!(message.contains("72x20"));
        assert!(message.contains("70x18"));
    }
}
