use super::super::edit::UiMode;
use super::super::state::AppState;
use super::super::theme_layout::COLOR_ROW_BG_NORMAL;
use super::Component;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::prelude::Stylize;
use ratatui::widgets::{Block, Borders, Paragraph};

pub(crate) struct FooterComponent;

impl FooterComponent {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Component for FooterComponent {
    fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .bg(COLOR_ROW_BG_NORMAL)
            .borders(Borders::TOP);

        frame.render_widget(block, area);
        let legend = match state.mode {
            UiMode::Browse | UiMode::DeleteConfirm { .. } => browse_footer_text(),
            UiMode::Edit(_) => "",
        };
        let legend_area = Rect::new(area.x, area.y, area.width, 1);
        frame.render_widget(Paragraph::new(legend), legend_area);

        if area.height > 1 && !state.status.is_empty() {
            let status_area = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
            frame.render_widget(Paragraph::new(state.status.clone()), status_area);
        }
    }
}

pub(crate) fn browse_footer_text() -> &'static str {
    "j/k/↑/↓ select  c connect  d disconnect  ↵ edit  r refresh  q quit"
}

pub(crate) fn edit_footer_text(is_new: bool) -> String {
    let save_label = if is_new { "Create" } else { "Save" };
    format!("[ d Disc. ] [ x ] [ c {save_label} ] [ q Quit ]")
}
