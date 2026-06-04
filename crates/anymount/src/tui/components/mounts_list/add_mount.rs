use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect, Spacing},
    symbols::merge::MergeStrategy,
    widgets::{Block, Padding, Paragraph, Widget},
};

pub struct AddMount {
    pub is_selected: bool,
}

impl Widget for &AddMount {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [_, right] =
            Layout::horizontal([Constraint::Length(1), Constraint::Fill(1)])
                .areas(area);
        let block = Block::bordered()
            .padding(Padding::horizontal(1))
            .merge_borders(MergeStrategy::Exact);
        if self.is_selected {
            self.render_line(block.inner(right), buf);
        }
        block.render(right, buf);
    }
}

impl AddMount {
    fn render_line(&self, area: Rect, buf: &mut Buffer) {
        let [status, name, buttons] = Layout::horizontal([
            Constraint::Length(2),
            Constraint::Fill(1),
            Constraint::Length(5),
        ])
        .spacing(Spacing::Space(1))
        .areas(area);

        Paragraph::new("+").render(status, buf);
        Paragraph::new("Add").render(name, buf);
        Paragraph::new("[ ↵ ]").render(buttons, buf);
    }
}
