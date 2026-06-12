use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Layout, Rect, Spacing},
    symbols::merge::MergeStrategy,
    widgets::{Block, Borders, Padding, Paragraph, Widget},
};

use super::mounts_list::MountItem;

pub struct MountRow<'a> {
    pub index: usize,
    pub is_selected: bool,
    pub item: &'a MountItem<'a>,
}

impl<'a> Widget for &MountRow<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.item.is_connected {
            self.render_connected(area, buf);
        } else {
            self.render_disconnected(area, buf);
        }
    }
}

impl<'a> MountRow<'a> {
    fn render_disconnected(&self, area: Rect, buf: &mut Buffer) {
        let [left, right] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(2)])
                .spacing(Spacing::Overlap(1))
                .areas(area);
        let block = Block::bordered()
            .padding(Padding::horizontal(1))
            .merge_borders(MergeStrategy::Exact);
        self.render_line(block.inner(left), buf);
        block.render(left, buf);
        let spacer = if self.index == 0 {
            Block::default()
                .borders(Borders::TOP | Borders::RIGHT)
                .merge_borders(MergeStrategy::Exact)
        } else {
            Block::default()
                .borders(Borders::RIGHT)
                .merge_borders(MergeStrategy::Exact)
        };
        spacer.render(right, buf);
    }

    fn render_connected(&self, area: Rect, buf: &mut Buffer) {
        let [_, right] =
            Layout::horizontal([Constraint::Length(1), Constraint::Fill(1)])
                .areas(area);
        let block = Block::bordered()
            .padding(Padding::horizontal(1))
            .merge_borders(MergeStrategy::Exact);
        self.render_line(block.inner(right), buf);
        block.render(right, buf);
    }

    fn render_line(&self, area: Rect, buf: &mut Buffer) {
        let [status, name, path, storage_type, buttons] = Layout::horizontal([
            Constraint::Length(2),
            Constraint::Fill(2),
            Constraint::Fill(3),
            Constraint::Fill(1),
            Constraint::Length(10),
        ])
        .spacing(Spacing::Space(1))
        .areas(area);

        Paragraph::new(if self.item.is_connected { "●" } else { "○" })
            .render(status, buf);
        Paragraph::new(self.item.name).render(name, buf);
        Paragraph::new(self.item.path.to_string_lossy()).render(path, buf);
        Paragraph::new(self.item.storage_type)
            .alignment(Alignment::Center)
            .render(storage_type, buf);
        if self.is_selected {
            Paragraph::new(if self.item.is_connected {
                "[ ⇐ ][ ↵ ]"
            } else {
                "[ ⇒ ][ ↵ ]"
            })
            .render(buttons, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{MountItem, MountRow};
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn test_connected_mount_row() {
        let app = MountRow {
            index: 0,
            is_selected: false,
            item: &MountItem {
                name: "test",
                path: Path::new("/test"),
                storage_type: "test",
                is_connected: true,
            },
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 3)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(&app, frame.area()))
            .unwrap();
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!(terminal.backend());
        });
    }

    #[test]
    fn test_top_disconnected_mount_row() {
        let app = MountRow {
            index: 0,
            is_selected: false,
            item: &MountItem {
                name: "test",
                path: Path::new("/test"),
                storage_type: "test",
                is_connected: false,
            },
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 3)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(&app, frame.area()))
            .unwrap();
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!(terminal.backend());
        });
    }

    #[test]
    fn test_disconnected_mount_row() {
        let app = MountRow {
            index: 1,
            is_selected: false,
            item: &MountItem {
                name: "test",
                path: Path::new("/test"),
                storage_type: "test",
                is_connected: false,
            },
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 3)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(&app, frame.area()))
            .unwrap();
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!(terminal.backend());
        });
    }

    #[test]
    fn test_connected_mount_row_selected() {
        let app = MountRow {
            index: 1,
            is_selected: true,
            item: &MountItem {
                name: "test",
                path: Path::new("/test"),
                storage_type: "test",
                is_connected: true,
            },
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 3)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(&app, frame.area()))
            .unwrap();
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!(terminal.backend());
        });
    }

    #[test]
    fn test_disconnected_mount_row_selected() {
        let app = MountRow {
            index: 1,
            is_selected: true,
            item: &MountItem {
                name: "test",
                path: Path::new("/test"),
                storage_type: "test",
                is_connected: false,
            },
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 3)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(&app, frame.area()))
            .unwrap();
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!(terminal.backend());
        });
    }
}
