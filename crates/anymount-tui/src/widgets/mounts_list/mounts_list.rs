use std::path::Path;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect, Spacing},
    text::Span,
    widgets::{
        Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget,
    },
};

use super::add_mount::AddMount;

use super::mount_row::MountRow;

pub struct MountItem<'a> {
    pub name: &'a str,
    pub path: &'a Path,
    pub storage_type: &'a str,
    pub is_connected: bool,
}

pub struct MountsList<'a> {
    pub mounts: &'a [MountItem<'a>],
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct MountsListState {
    /// Item of the selected item in the list, if any. This is an index into the
    /// list of mounts.
    selected: Option<usize>,
    /// Index of the first visible item in the list. Previous items should be
    /// hidden by scrolling.
    offset: usize,
    /// Count of visibile items.
    visible_count: usize,
}

impl MountsListState {
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn select_next(&mut self, mount_count: usize) {
        let next = self
            .selected
            .map_or(0, |index| index.saturating_add(1).min(mount_count));
        if next >= self.offset.saturating_add(self.visible_count) {
            self.offset =
                next.saturating_sub(self.visible_count.saturating_sub(1));
        }
        self.selected = Some(next);
    }

    pub fn select_previous(&mut self, mount_count: usize) {
        let previous = self
            .selected
            .map_or(mount_count, |index| index.saturating_sub(1));
        if previous < self.offset {
            self.offset = previous;
        }
        self.selected = Some(previous);
    }

    /// Updates the count of visible items and updates. If there is a selected
    /// item and the visibility has changed, it also updates the offset to keep
    /// the selected item visible.
    pub fn update_visibility(&mut self, visible_row_count: usize) {
        if self.visible_count == visible_row_count {
            return;
        };

        self.visible_count = visible_row_count;
        let Some(selected) = self.selected else {
            return;
        };
        // Update offset if the selected item is not visible. Selected item must
        // be visible.
        if selected >= self.offset.saturating_add(self.visible_count) {
            self.offset =
                selected.saturating_sub(self.visible_count.saturating_sub(1));
        }
    }
}

impl StatefulWidget for &MountsList<'_> {
    type State = MountsListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let row_count = self.row_count();
        let visible_row_count = self.visible_row_count(row_count, area.height);
        let body_height = self.body_height(visible_row_count);
        state.update_visibility(visible_row_count);

        let [body_area, footer_area] = Layout::vertical([
            Constraint::Length(body_height),
            Constraint::Length(1),
        ])
        .flex(Flex::Start)
        .areas(area);

        let rows_area = if row_count > visible_row_count {
            let mut scrollbar_state =
                ScrollbarState::new(row_count - visible_row_count + 1)
                    .position(state.offset);
            self.render_scrollbar(body_area, buf, &mut scrollbar_state)
        } else {
            body_area
        };

        let rows = Layout::vertical(std::iter::repeat_n(
            Constraint::Length(MountsList::ROW_HEIGHT),
            visible_row_count,
        ))
        .spacing(Spacing::Overlap(1))
        .split(rows_area);

        for (visible_index, row) in rows.iter().enumerate() {
            let row_index = state.offset + visible_index;
            if let Some(item) = self.mounts.get(row_index) {
                MountRow {
                    index: row_index,
                    is_selected: state.selected == Some(row_index),
                    item,
                }
                .render(*row, buf);
            } else {
                AddMount {
                    is_selected: state.selected >= Some(self.mounts.len()),
                }
                .render(*row, buf);
            }
        }

        self.render_footer(footer_area, buf);
    }
}

impl MountsList<'_> {
    const FOOTER_HEIGHT: u16 = 1;
    const ROW_HEIGHT: u16 = 3;
    const ROW_OVERLAP: u16 = 1;
    const ROW_STEP: u16 = Self::ROW_HEIGHT - Self::ROW_OVERLAP;

    fn row_count(&self) -> usize {
        self.mounts.len().saturating_add(1)
    }

    fn visible_row_count(&self, row_count: usize, area_height: u16) -> usize {
        let max_body_height = area_height.saturating_sub(Self::FOOTER_HEIGHT);
        if max_body_height < Self::ROW_HEIGHT {
            return 0;
        }
        let max_visible_row_count = 1 + max_body_height
            .saturating_sub(Self::ROW_HEIGHT)
            / Self::ROW_STEP;
        row_count.min(max_visible_row_count as usize)
    }

    fn body_height(&self, visible_row_count: usize) -> u16 {
        (visible_row_count as u16)
            .saturating_sub(1)
            .saturating_mul(Self::ROW_STEP)
            .saturating_add(Self::ROW_HEIGHT)
    }

    fn render_scrollbar(
        &self,
        area: Rect,
        buf: &mut Buffer,
        state: &mut ScrollbarState,
    ) -> Rect {
        let [rows_area, scrollbar_area] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(1)])
                .areas(area);
        Scrollbar::new(ScrollbarOrientation::VerticalRight).render(
            scrollbar_area,
            buf,
            state,
        );
        rows_area
    }

    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        let left_span = Span::raw("j↑ select ↓k");
        let center_span = Span::raw("⇐ disconnect connect ⇒");
        let right_span = Span::raw("info ↵");
        let [left_area, center_area, right_area] = Layout::horizontal([
            Constraint::Length(left_span.width() as u16),
            Constraint::Length(center_span.width() as u16),
            Constraint::Length(right_span.width() as u16),
        ])
        .flex(Flex::SpaceAround)
        .areas(area);
        left_span.render(left_area, buf);
        center_span.render(center_area, buf);
        right_span.render(right_area, buf);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{MountItem, MountsList, MountsListState};
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn state_select_next() {
        let mut state = MountsListState::default();
        let mount_count = 2;

        // Select next when no item is selected should select the first item.
        assert_eq!(state.selected(), None);
        state.select_next(mount_count);
        assert_eq!(state.selected(), Some(0));

        // Select next when an item is selected should increment the selected
        // index.
        state.select_next(mount_count);
        assert_eq!(state.selected(), Some(1));
        state.select_next(mount_count);
        assert_eq!(state.selected(), Some(2));

        // Select next when the selected index is the last item does nothing.
        state.select_next(mount_count);
        assert_eq!(state.selected(), Some(2));
    }

    #[test]
    fn state_select_previous() {
        let mut state = MountsListState::default();
        let mount_count = 2;

        // Select previous when no item is selected should wrap around to the
        // last item.
        assert_eq!(state.selected(), None);
        state.select_previous(mount_count);
        assert_eq!(state.selected(), Some(2));

        // Select previous when an item is selected should decrement the
        // selected index.
        state.select_previous(mount_count);
        assert_eq!(state.selected(), Some(1));
        state.select_previous(mount_count);
        assert_eq!(state.selected(), Some(0));

        // Select previous when the selected index is 0 does nothing.
        state.select_previous(mount_count);
        assert_eq!(state.selected(), Some(0));
    }

    #[test]
    fn test_mounts_list() {
        let mounts = [
            MountItem {
                name: "Connected Mount",
                path: Path::new("/mnt/connected"),
                storage_type: "local",
                is_connected: true,
            },
            MountItem {
                name: "Disconnected Mount",
                path: Path::new("/mnt/disconnected"),
                storage_type: "local",
                is_connected: false,
            },
        ];
        let app = MountsList { mounts: &mounts };
        let mut state = MountsListState::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 10))
            .expect("test terminal should be created");
        terminal
            .draw(|f| f.render_stateful_widget(&app, f.area(), &mut state))
            .expect("mount list should render");
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!(terminal.backend());
        });
    }

    fn scrollable_mounts() -> [MountItem<'static>; 4] {
        [
            MountItem {
                name: "First",
                path: Path::new("/mnt/first"),
                storage_type: "local",
                is_connected: true,
            },
            MountItem {
                name: "Second",
                path: Path::new("/mnt/second"),
                storage_type: "local",
                is_connected: false,
            },
            MountItem {
                name: "Third",
                path: Path::new("/mnt/third"),
                storage_type: "local",
                is_connected: false,
            },
            MountItem {
                name: "Fourth",
                path: Path::new("/mnt/fourth"),
                storage_type: "local",
                is_connected: false,
            },
        ]
    }

    #[test]
    fn test_mounts_list_scroll_unselected() {
        let mounts = scrollable_mounts();
        let app = MountsList { mounts: &mounts };
        let mut state = MountsListState::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 11))
            .expect("test terminal should be created");
        terminal
            .draw(|f| f.render_stateful_widget(&app, f.area(), &mut state))
            .expect("mount list should render");
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!(terminal.backend());
        });
    }

    #[test]
    fn test_mounts_list_scroll_first() {
        let mounts = scrollable_mounts();
        let app = MountsList { mounts: &mounts };
        let mut state = MountsListState::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 11))
            .expect("test terminal should be created");
        // Select the first item.
        state.select_next(mounts.len());
        terminal
            .draw(|f| f.render_stateful_widget(&app, f.area(), &mut state))
            .expect("mount list should render");
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!(terminal.backend());
        });
    }

    #[test]
    fn test_mounts_list_scroll_last() {
        let mounts = scrollable_mounts();
        let app = MountsList { mounts: &mounts };
        let mut state = MountsListState::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 11))
            .expect("test terminal should be created");
        // Select the last item.
        state.select_previous(mounts.len());
        terminal
            .draw(|f| f.render_stateful_widget(&app, f.area(), &mut state))
            .expect("mount list should render");
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!(terminal.backend());
        });
    }

    #[test]
    fn test_mounts_list_scroll_resize() {
        let mounts = scrollable_mounts();
        let app = MountsList { mounts: &mounts };
        let mut state = MountsListState::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 11))
            .expect("test terminal should be created");
        // Select the last item.
        state.select_previous(mounts.len());
        terminal
            .draw(|f| f.render_stateful_widget(&app, f.area(), &mut state))
            .expect("mount list should render");
        assert_eq!(state.offset, 1);
        // Resize and re-draw, the number of visible items should decrease and
        // the offset should be updated accordingly.
        terminal.backend_mut().resize(80, 8);
        terminal
            .draw(|f| f.render_stateful_widget(&app, f.area(), &mut state))
            .expect("resized mount list should render");
        assert_eq!(state.offset, 2);
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!(terminal.backend());
        });
    }
}
