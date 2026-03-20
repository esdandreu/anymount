# TUI Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a complete TUI redesign with datacenter rack aesthetic, 3D row effects for connected/disconnected mounts, and a clean edit menu.

**Architecture:** Rewrite the TUI rendering to use custom row widgets with 3D effects instead of standard ratatui List widgets. Maintain the existing state management (AppState, EditSession) but update UI rendering completely. Use mouse hover tracking via crossterm events.

**Tech Stack:** ratatui 0.29, crossterm 0.28

---

## File Structure

- **Modify:** `crates/anymount/src/tui/tui.rs` - Complete UI rewrite
- **Modify:** `crates/anymount/src/tui/mod.rs` - No changes needed (exports)
- **Modify:** `crates/anymount/src/tui/error.rs` - Add any new error variants if needed

---

## Task 1: Define Color Constants and Row Types

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs:1-50`

- [ ] **Step 1: Add new color constants and row state enum**

Replace the existing constants around line 35-38:

```rust
const COLOR_BORDER: Color = Color::Blue;
const COLOR_HIGHLIGHT: Color = Color::Yellow;
const COLOR_CONTEXT: Color = Color::Cyan;
const COLOR_STATUS: Color = Color::Green;
```

With:

```rust
const COLOR_CONNECTED: Color = Color::Green;
const COLOR_DISCONNECTED: Color = Color::DarkGray;
const COLOR_SELECTED: Color = Color::White;
const COLOR_ROW_BG_NORMAL: Color = Color::Reset;
const COLOR_ROW_BG_HOVERED: Color = Color::Rgb(30, 40, 60);
const COLOR_ROW_BG_SELECTED: Color = Color::Rgb(45, 66, 99);
const COLOR_ROW_3D_SHADOW: Color = Color::DarkGray;
const COLOR_BUTTON: Color = Color::Cyan;
const COLOR_BUTTON_TEXT: Color = Color::Black;
```

Add new enum for row connection state (after ProviderEntry around line 44):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionState {
    Connected,
    Disconnected,
}
```

- [ ] **Step 2: Add RowState struct to track hover/focus per row**

Add after ConnectionState:

```rust
#[derive(Debug, Clone)]
struct RowState {
    is_hovered: bool,
    is_keyboard_focused: bool,
}
```

- [ ] **Step 3: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "feat(tui): add color constants and row state types for new design"
```

---

## Task 2: Update AppState for Mouse Hover Tracking

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs:54-129`

- [ ] **Step 1: Add hovered_index to AppState**

Modify AppState struct (around line 54):

```rust
#[derive(Debug, Clone)]
struct AppState {
    providers: Vec<ProviderEntry>,
    selected: usize,
    hovered: usize,
    status: String,
    mode: UiMode,
}
```

- [ ] **Step 2: Update load method**

Modify AppState::load (around line 62) to initialize hovered:

```rust
Ok(Self {
    providers,
    selected: 0,
    hovered: 0,
    status: "j↑ select ↓k  c connect  d disconnect  ↵ edit".to_owned(),
    mode: UiMode::Browse,
})
```

- [ ] **Step 3: Update refresh method to preserve hovered**

Modify AppState::refresh (around line 80):

```rust
fn refresh<U>(&mut self, use_case: &U) -> Result<()>
where
    U: ConfigUseCase,
{
    let selected_name = self.selected_name().map(ToOwned::to_owned);
    let refreshed = Self::load(use_case)?;
    self.providers = refreshed.providers;
    self.status = refreshed.status;
    self.hovered = 0;
    if let Some(name) = selected_name {
        if let Some(pos) = self
            .providers
            .iter()
            .position(|provider| provider.name == name)
        {
            self.selected = pos;
            return Ok(());
        }
    }
    self.selected = self.selected.min(self.providers.len().saturating_sub(1));
    Ok(())
}
```

- [ ] **Step 4: Add hovered row methods**

Add after select_prev method (around line 128):

```rust
fn hovered_name(&self) -> Option<&str> {
    self.providers
        .get(self.hovered)
        .map(|provider| provider.name.as_str())
}

fn hovered_provider(&self) -> Option<&ProviderEntry> {
    self.providers.get(self.hovered)
}

fn select_next(&mut self) {
    if self.providers.is_empty() {
        return;
    }
    self.hovered = (self.hovered + 1) % (self.providers.len() + 1); // +1 for Add row
    self.selected = self.hovered;
}

fn select_prev(&mut self) {
    if self.providers.is_empty() {
        return;
    }
    if self.hovered == 0 {
        self.hovered = self.providers.len(); // Jump to Add row
    } else {
        self.hovered -= 1;
    }
    self.selected = self.hovered;
}

fn is_add_row(&self) -> bool {
    self.hovered >= self.providers.len()
}
```

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "feat(tui): add hovered tracking and Add row support to AppState"
```

---

## Task 3: Create Rack Row Rendering

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs`

- [ ] **Step 1: Create RowStyle enum**

Add before draw_ui function (around line 1315):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowStyle {
    Normal,
    Hovered,      // Mouse hovered
    Keyboard,     // Keyboard focused (shown displaced)
    HoveredConnected,   // Mouse hovered + connected
    HoveredDisconnected, // Mouse hovered + disconnected
    KeyboardConnected,  // Keyboard focused + connected
    KeyboardDisconnected, // Keyboard focused + disconnected
}
```

- [ ] **Step 2: Create render_mount_row function**

First, add connection state checking. Add method to ProviderEntry (around line 41):

```rust
impl ProviderEntry {
    fn is_connected(&self) -> bool {
        crate::cli::provider_control::provider_daemon_ready(&self.name)
    }
}
```

Add after RowStyle:

```rust
fn render_mount_row(
    frame: &mut Frame,
    entry: &ProviderEntry,
    rect: Rect,
    style: RowStyle,
    show_buttons: bool,
) {
    let is_displaced = matches!(
        style,
        RowStyle::KeyboardDisconnected | RowStyle::KeyboardConnected
    ) || matches!(
        style,
        RowStyle::HoveredDisconnected | RowStyle::HoveredConnected
    );

    let bg_color = if matches!(style, RowStyle::Normal) {
        COLOR_ROW_BG_NORMAL
    } else {
        COLOR_ROW_BG_HOVERED
    };

    let is_connected = entry.is_connected();
    let status_icon = if is_connected { "●" } else { "○" };
    let status_color = if is_connected { COLOR_CONNECTED } else { COLOR_DISCONNECTED };

    let connection_indicator = if matches!(
        style,
        RowStyle::KeyboardDisconnected | RowStyle::KeyboardConnected
    ) {
        "⇅"
    } else {
        " "
    };

    // Calculate 3D effect dimensions
    let displacement = if is_displaced { 2 } else { 0 };
    let shadow_width = if is_displaced { 2 } else { 0 };

    // Shadow area (left side for 3D effect)
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

    // Main row background
    let row_rect = Rect::new(rect.x + displacement, rect.y, rect.width, rect.height);
    let row_block = Block::default()
        .bg(bg_color)
        .border_bottom(Borders::BOT)
        .border_left(Borders::LEFT)
        .border_right(Borders::RIGHT);
    frame.render_widget(row_block, row_rect);

    // Row content
    let content = format!(
        "{}{}  {:12}  {:25}  {:10}",
        connection_indicator,
        status_icon,
        entry.name,
        entry.config.path.display(),
        get_storage_type_label(&entry.config.storage),
    );
    let text_style = if is_connected {
        Style::default().fg(status_color)
    } else {
        Style::default().fg(COLOR_DISCONNECTED)
    };

    frame.render_widget(
        Paragraph::new(content).style(text_style).alignment(Alignment::Left),
        row_rect.inner(&ratatui::padding::Padding::new(1, 0, 1, 0)),
    );

    // Buttons (if hovered)
    if show_buttons {
        let buttons = if is_connected {
            "[ ⇐ ] [ ↵ ]"
        } else {
            "[ ⇒ ] [ ↵ ]"
        };
        frame.render_widget(
            Paragraph::new(buttons)
                .style(Style::default().fg(COLOR_BUTTON))
                .alignment(Alignment::Right),
            Rect::new(
                row_rect.x + 1,
                row_rect.y,
                row_rect.width.saturating_sub(2),
                row_rect.height,
            ),
        );
    }
}

fn get_storage_type_label(storage: &StorageConfig) -> &'static str {
    match storage {
        StorageConfig::Local { .. } => "local",
        StorageConfig::OneDrive { .. } => "onedrive",
    }
}
```

- [ ] **Step 3: Create render_add_row function**

```rust
fn render_add_row(
    frame: &mut Frame,
    rect: Rect,
    is_hovered: bool,
) {
    let bg_color = if is_hovered {
        COLOR_ROW_BG_HOVERED
    } else {
        COLOR_ROW_BG_NORMAL
    };

    let row_block = Block::default()
        .bg(bg_color)
        .border_bottom(Borders::BOT)
        .border_left(Borders::LEFT)
        .border_right(Borders::RIGHT);

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
    frame.render_widget(row_block, row_rect);

    let content = if is_hovered {
        "+                                                  [ ↵ Add ]"
    } else {
        "+"
    };

    frame.render_widget(
        Paragraph::new(content)
            .style(Style::default().fg(COLOR_BUTTON))
            .alignment(Alignment::Left),
        row_rect.inner(&ratatui::padding::Padding::new(1, 0, 1, 0)),
    );
}
```

- [ ] **Step 4: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "feat(tui): add rack row rendering functions with 3D effect"
```

---

## Task 4: Rewrite Main Menu Rendering

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs:1318-1420`

- [ ] **Step 1: Rewrite draw_ui for main menu**

Replace the entire draw_ui function content with the new rack-style layout:

```rust
fn draw_ui(frame: &mut Frame, cd: &ConfigDir, state: &AppState) {
    let area = frame.area();
    let (list_area, footer_area) = if matches!(state.mode, UiMode::Edit(_)) {
        (
            Rect::new(0, 0, area.width, area.height.saturating_sub(4)),
            Rect::new(0, area.height.saturating_sub(4), area.width, 4),
        )
    } else {
        (
            Rect::new(0, 0, area.width, area.height.saturating_sub(2)),
            Rect::new(0, area.height.saturating_sub(2), area.width, 2),
        )
    };

    match &state.mode {
        UiMode::Browse | UiMode::ConfirmDelete => {
            draw_main_menu(frame, list_area, state);
        }
        UiMode::Edit(session) => {
            draw_main_menu(frame, list_area, state);
            draw_edit_menu(frame, session);
        }
    }

    draw_footer(frame, footer_area, state);
}

fn draw_main_menu(frame: &mut Frame, area: Rect, state: &AppState) {
    let row_height = 1;
    let mut y = area.y;

    for (i, entry) in state.providers.iter().enumerate() {
        let is_hovered = i == state.hovered;
        let is_selected = i == state.selected;

        let style = if is_hovered {
            RowStyle::Hovered // simplified for now
        } else {
            RowStyle::Normal
        };

        let rect = Rect::new(area.x, y, area.width, row_height);
        render_mount_row(frame, entry, rect, style, is_hovered);
        y += row_height;
    }

    // Add row
    let add_rect = Rect::new(area.x, y, area.width, row_height);
    render_add_row(frame, add_rect, state.is_add_row());
}

fn draw_footer(frame: &mut Frame, area: Rect, state: &AppState) {
    let content = state.status.clone();
    let block = Block::default()
        .bg(COLOR_ROW_BG_NORMAL)
        .borders(Borders::TOP);

    frame.render_widget(Paragraph::new(content), area);
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "feat(tui): rewrite main menu with rack-style rows"
```

---

## Task 5: Implement Mouse Hover Detection

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs:946-1111`

- [ ] **Step 1: Update handle_mouse_event**

Replace the current mouse handling with hover detection:

```rust
fn handle_mouse_event(
    terminal: &mut DefaultTerminal,
    state: &mut AppState,
    mouse: MouseEvent,
) -> Result<()> {
    if matches!(state.mode, UiMode::Edit(_)) {
        return Ok(());
    }

    let size = terminal.size().map_err(|source| Error::Terminal {
        operation: "read terminal size",
        source,
    })?;
    let list_area = Rect::new(0, 0, size.width, size.height.saturating_sub(2));

    match mouse.kind {
        MouseEventKind::Moved(_, _) | MouseEventKind::Dragging(_, _) => {
            let row = (mouse.row as usize).saturating_sub(list_area.y as usize);
            if row <= state.providers.len() {
                state.hovered = row;
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let row = (mouse.row as usize).saturating_sub(list_area.y as usize);
            if row < state.providers.len() {
                state.hovered = row;
                state.selected = row;
                // Trigger edit
                let provider = state.selected_provider().cloned();
                if let Some(p) = provider {
                    state.mode = UiMode::Edit(EditSession::new_for_edit(&p));
                }
            } else if row == state.providers.len() {
                // Add row
                let default_name = suggest_new_provider_name(state);
                state.mode = UiMode::Edit(EditSession::new_for_add(default_name));
            }
        }
        _ => {}
    }

    Ok(())
}
```

- [ ] **Step 2: Update handle_browse_key for c/d shortcuts**

Modify handle_browse_key around line 1113 to add c/d handling:

```rust
fn handle_browse_key(code: KeyCode, cd: &ConfigDir, state: &mut AppState) -> Result<bool> {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => Ok(true),
        KeyCode::Down | KeyCode::Char('j') => {
            state.select_next();
            Ok(false)
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.select_prev();
            Ok(false)
        }
        KeyCode::Char('c') => {
            // Connect
            match connect_selected_provider_for_config(cd, state) {
                Ok(Some(name)) => state.status = format!("Connected '{name}'"),
                Ok(None) => state.status = "No mount selected".to_owned(),
                Err(e) => state.status = format!("Connect failed: {e}"),
            }
            Ok(false)
        }
        KeyCode::Char('d') => {
            // Disconnect
            match disconnect_selected_provider(cd, state) {
                Ok(Some(name)) => state.status = format!("Disconnected '{name}'"),
                Ok(None) => state.status = "No mount selected".to_owned(),
                Err(e) => state.status = format!("Disconnect failed: {e}"),
            }
            Ok(false)
        }
        KeyCode::Char('r') => {
            match refresh_state(cd, state) {
                Ok(()) => state.status = "Refreshed mount list".to_owned(),
                Err(e) => state.status = format!("Refresh failed: {e}"),
            }
            Ok(false)
        }
        KeyCode::Char('a') => {
            let default_name = suggest_new_provider_name(state);
            state.mode = UiMode::Edit(EditSession::new_for_add(default_name));
            state.status = "Adding new mount".to_owned();
            Ok(false)
        }
        KeyCode::Char('e') | KeyCode::Enter => {
            if state.is_add_row() {
                let default_name = suggest_new_provider_name(state);
                state.mode = UiMode::Edit(EditSession::new_for_add(default_name));
                state.status = "Adding new mount".to_owned();
            } else if let Some(provider) = state.selected_provider() {
                state.mode = UiMode::Edit(EditSession::new_for_edit(provider));
                state.status = "Editing mount".to_owned();
            } else {
                state.status = "No mount selected".to_owned();
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}
```

- [ ] **Step 3: Add disconnect function**

Add after connect_all_providers (around line 1584):

```rust
fn disconnect_selected_provider<U>(use_case: &U, state: &AppState) -> Result<Option<String>>
where
    U: ConnectUseCase,
{
    let Some(name) = state.selected_name() else {
        return Ok(None);
    };
    let name = name.to_owned();
    run_disconnect(use_case, Some(name.clone()))?;
    Ok(Some(name))
}

fn run_disconnect<U>(use_case: &U, name: Option<String>) -> Result<()>
where
    U: ConnectUseCase,
{
    if let Some(name) = name {
        use_case.disconnect_name(&name).map_err(Error::from)
    } else {
        Ok(())
    }
}
```

- [ ] **Step 4: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "feat(tui): implement mouse hover and connect/disconnect shortcuts"
```

---

## Task 6: Rewrite Edit Menu Rendering

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs`

- [ ] **Step 1: Add draw_edit_menu function**

Add after draw_footer:

```rust
fn draw_edit_menu(frame: &mut Frame, session: &EditSession) {
    let size = frame.area();
    let edit_area = Rect::new(0, 0, size.width, size.height.saturating_sub(2));
    let button_area = Rect::new(0, size.height.saturating_sub(2), size.width, 2);

    // Draw fields
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

        let bg = if is_active { COLOR_ROW_BG_SELECTED } else { COLOR_ROW_BG_NORMAL };
        let cursor = if is_active { "█" } else { "" };
        let content = format!("  {:25}  {}{}", field.label(), shown, cursor);

        let rect = Rect::new(edit_area.x, y, edit_area.width, 1);
        let block = Block::default().bg(bg);
        frame.render_widget(Paragraph::new(content), rect);
        y += 1;
    }

    // Draw buttons in bottom-right per spec
    // Buttons: [ d Disc. ] [ x ] [ c Save ] or [ c Create ] for new mounts
    let is_new = session.original_name.is_none();
    let save_label = if is_new { "Create" } else { "Save" };

    let button_text = format!(
        "{:>width$}[ d Disc. ] [ x ] [ c {} ]",
        "",
        save_label,
        width = edit_area.width.saturating_sub(45)
    );

    let block = Block::default()
        .bg(COLOR_ROW_BG_NORMAL)
        .border_top(Borders::TOP);
    frame.render_widget(Paragraph::new(button_text).style(Style::default().fg(COLOR_BUTTON)), button_area);
}
```

- [ ] **Step 2: Update help_lines for edit mode**

Modify help_lines around line 1422:

```rust
fn help_lines(state: &AppState) -> Vec<Line<'static>> {
    match &state.mode {
        UiMode::Browse => vec![
            Line::from("j↑ select ↓k  c connect  d disconnect  ↵ edit"),
        ],
        UiMode::Edit(ref session) => {
            vec![Line::from(
                "j↑ ⇓↓ select  type to edit  Tab complete  Esc back  c save  d disconnect  x delete",
            )]
        }
        UiMode::ConfirmDelete => vec![
            Line::from("Delete confirmation: y confirm  n/Esc cancel"),
        ],
    }
}
```

- [ ] **Step 3: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "feat(tui): rewrite edit menu with clean field list"
```

---

## Task 7: Add Delete Confirmation Dialog

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs`

- [ ] **Step 1: Add DeleteConfirm variant to UiMode with name tracking**

```rust
#[derive(Debug, Clone)]
enum UiMode {
    Browse,
    Edit(EditSession),
    DeleteConfirm { name: String },
}
```

- [ ] **Step 2: Add draw_delete_dialog function**

Add after draw_edit_menu:

```rust
fn draw_delete_dialog(frame: &mut Frame, name: &str) {
    let size = frame.area();
    let dialog_width = 50;
    let dialog_height = 5;
    let x = (size.width.saturating_sub(dialog_width)) / 2;
    let y = (size.height.saturating_sub(dialog_height)) / 2;

    let dialog_rect = Rect::new(x, y, dialog_width, dialog_height);

    let content = format!(
        "  Delete '{}'?  [ y ]  [ n ]",
        name
    );

    let block = Block::default()
        .bg(COLOR_ROW_BG_SELECTED)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLOR_BUTTON));

    frame.render_widget(Clear, dialog_rect);
    frame.render_widget(
        Paragraph::new(content)
            .style(Style::default().fg(COLOR_BUTTON_TEXT))
            .block(block),
        dialog_rect,
    );
}
```

Note: Import `Clear` widget from ratatui::widgets.

- [ ] **Step 3: Update handle_edit_key for x key**

Add to handle_edit_key in EditMode::Navigate:

```rust
KeyCode::Char('x') => {
    let name = session.draft.name.clone();
    state.mode = UiMode::DeleteConfirm { name };
    Ok(EditAction::Continue)
}
```

- [ ] **Step 4: Add handle_delete_confirm_key**

```rust
fn handle_delete_confirm_key(code: KeyCode, cd: &ConfigDir, state: &mut AppState) -> Result<bool> {
    let name = if let UiMode::DeleteConfirm { ref name } = state.mode {
        name.clone()
    } else {
        return Ok(false);
    };

    match code {
        KeyCode::Char('y') => {
            remove_provider(cd, &name)?;
            state.mode = UiMode::Browse;
            refresh_state(cd, state)?;
            state.status = format!("Deleted '{}'", name);
            Ok(false)
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            state.mode = UiMode::Browse;
            state.status = "Delete canceled".to_owned();
            Ok(false)
        }
        _ => Ok(false),
    }
}
```

- [ ] **Step 5: Update run_loop to handle DeleteConfirm mode**

Modify run_loop:

```rust
let should_quit = match state.mode {
    UiMode::Browse => handle_browse_key(key.code, cd, state)?,
    UiMode::DeleteConfirm { .. } => handle_delete_confirm_key(key.code, cd, state)?,
    UiMode::Edit(_) => {
        let action = {
            let UiMode::Edit(session) = &mut state.mode else {
                unreachable!()
            };
            handle_edit_key(key.code, cd, session)?
        };
        match action {
            EditAction::Continue => {}
            EditAction::Cancel => {
                state.mode = UiMode::Browse;
                state.status = "Edit canceled".to_owned();
            }
            EditAction::Saved(name) => {
                state.mode = UiMode::Browse;
                refresh_state(cd, state)?;
                state.status = format!("Saved mount '{}'", name);
            }
            EditAction::Deleted => {
                state.mode = UiMode::Browse;
            }
            EditAction::Message(message) => {
                state.status = message;
            }
        }
        false
    }
};
```

- [ ] **Step 6: Add Delete action to EditAction enum**

```rust
enum EditAction {
    Continue,
    Cancel,
    Saved(String),
    Deleted,
    Message(String),
}
```

- [ ] **Step 7: Update handle_edit_key for x and d**

In handle_edit_key EditMode::Navigate:

```rust
KeyCode::Char('x') => {
    Ok(EditAction::Deleted)
}
KeyCode::Char('d') => {
    // Disconnect - trigger from edit menu
    // Similar to c connect, but disconnect
    Ok(EditAction::Message("Use c to connect, d to disconnect in main menu".to_owned()))
}
```

- [ ] **Step 8: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "feat(tui): add delete confirmation dialog"
```

---

## Task 8: Update Tests for New Behavior

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs:1764-2199`

- [ ] **Step 1: Update existing tests for new AppState structure**

Update tests that create AppState directly to include `hovered` field:

- [ ] **Step 2: Add tests for new hover behavior**

```rust
#[test]
fn select_next_wraps_to_start() {
    let mut state = AppState {
        providers: vec![local_provider("a"), local_provider("b")],
        selected: 1,
        hovered: 1,
        status: String::new(),
        mode: UiMode::Browse,
    };

    state.select_next();

    assert_eq!(state.selected, 0);
    assert_eq!(state.hovered, 0);
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test -p anymount tui
```

- [ ] **Step 4: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "test(tui): update and add tests for new TUI behavior"
```

---

## Task 9: Add OneDrive OAuth Authentication

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs`

- [ ] **Step 1: Update handle_edit_key for OneDrive auth keys**

In handle_edit_key EditMode::Navigate, add:

```rust
_ if is_onedrive_auth_key(code) => {
    let message = authenticate_onedrive_in_terminal(&mut session.draft)?;
    Ok(EditAction::Message(message))
}
```

Note: `is_onedrive_auth_key` and `authenticate_onedrive_in_terminal` already exist in the codebase.

- [ ] **Step 2: Update edit menu footer text**

Modify help_lines to include OneDrive auth hint when editing OneDrive mount:

```rust
UiMode::Edit(ref session) => {
    let mut line = String::from("j↑ ⇓↓ select  type to edit  Tab complete  Esc back  c save  d disconnect  x delete");
    if matches!(session.draft.storage_type, ProviderType::OneDrive) {
        line.push_str("  l/o OneDrive auth");
    }
    vec![Line::from(line)]
}
```

- [ ] **Step 3: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "feat(tui): update OneDrive auth hints in edit menu"
```

---

## Task 10: Add Path Completion for Path Fields

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs`

- [ ] **Step 1: Ensure Tab completion works in text input mode**

The `complete_selected_path` function already exists and correctly handles path fields (`EditField::Path` and `EditField::LocalRoot`). Ensure it's called in EditMode::TextInput:

```rust
KeyCode::Tab => {
    if let Some(message) = session.complete_selected_path()? {
        Ok(EditAction::Message(message))
    } else {
        Ok(EditAction::Continue)
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "feat(tui): ensure path completion works in edit menu"
```

---

## Task 11: Integration Testing

- [ ] **Step 1: Build and run TUI**

```bash
cargo build -p anymount
cargo run -p anymount -- tui
```

- [ ] **Step 2: Manual testing checklist**
- [ ] Main menu displays with rack aesthetic
- [ ] Rows show connected (●) / disconnected (○) status
- [ ] Hovering shows 3D displacement effect
- [ ] Keyboard navigation works (j/k)
- [ ] c key connects, d key disconnects
- [ ] Enter opens edit menu
- [ ] Edit menu shows fields with active highlighting
- [ ] Navigation in edit menu works
- [ ] Save (c) works
- [ ] Delete (x) shows confirmation dialog
- [ ] Escape returns to main menu
- [ ] OneDrive auth (l/o) works when editing OneDrive mounts
- [ ] Path completion (Tab) works for path fields

- [ ] **Step 3: Run all tests**

```bash
cargo test -p anymount
```

- [ ] **Step 4: Commit final changes**

```bash
git add -A
git commit -m "feat(tui): complete TUI redesign implementation"
```

---

## Summary

| Task | Description |
|------|-------------|
| 1 | Define colors and row state types |
| 2 | Update AppState for hover tracking |
| 3 | Create rack row rendering functions |
| 4 | Rewrite main menu rendering |
| 5 | Implement mouse hover detection |
| 6 | Rewrite edit menu rendering |
| 7 | Add delete confirmation dialog |
| 8 | Update tests |
| 9 | Add OneDrive OAuth authentication |
| 10 | Add path completion for path fields |
| 11 | Integration testing |
