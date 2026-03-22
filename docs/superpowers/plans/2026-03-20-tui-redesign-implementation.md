# TUI Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the TUI match the redesigned rack-style spec, including
`80x24` support, width-safe rendering, real row button interactions, and edit
screen behavior that matches the written design.

**Architecture:** Keep the current TUI state machine in
`crates/anymount/src/tui/tui.rs`, but introduce explicit layout helpers for
minimum-size checks, row width budgeting, truncation, and mouse hit-testing.
Drive the rewrite with tests around pure layout and input logic first, then
swap the rendering code to use those helpers so the UI cannot render outside
the frame.

**Tech Stack:** Rust, ratatui 0.29, crossterm 0.28, cargo test, mise tasks

---

## File Structure

- **Modify:** `crates/anymount/src/tui/tui.rs` - add layout helpers, render
  guardrails, button hit-testing, and spec-compliant input handling
- **Modify:** `docs/superpowers/specs/2026-03-20-tui-redesign-design.md` - no
  further changes expected during implementation unless a new ambiguity is
  found

Keep the implementation in `tui.rs` unless a helper extraction becomes
necessary to keep functions readable. Do not split files preemptively.

---

## Task 1: Lock In Minimum Terminal Size And Width Budget Rules

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs`
- Test: `crates/anymount/src/tui/tui.rs`

- [ ] **Step 1: Write failing tests for size support and row column priority**

Add pure tests for:

```rust
#[test]
fn terminal_size_support_rejects_smaller_than_80x24() {
    assert!(!is_supported_size(Rect::new(0, 0, 79, 24)));
    assert!(!is_supported_size(Rect::new(0, 0, 80, 23)));
    assert!(is_supported_size(Rect::new(0, 0, 80, 24)));
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
```

- [ ] **Step 2: Run the targeted tests and confirm they fail**

Run:

```bash
cargo test tui::tui::tests::terminal_size_support_rejects_smaller_than_80x24
```

Expected: FAIL because `is_supported_size` and `compute_mount_row_layout` do
not exist yet.

- [ ] **Step 3: Add minimal width-budgeting helpers**

Implement small pure helpers in `tui.rs`:

```rust
const MIN_TERMINAL_WIDTH: u16 = 80;
const MIN_TERMINAL_HEIGHT: u16 = 24;

fn is_supported_size(area: Rect) -> bool {
    area.width >= MIN_TERMINAL_WIDTH && area.height >= MIN_TERMINAL_HEIGHT
}

struct MountRowLayout {
    show_name: bool,
    show_path: bool,
    show_storage_type: bool,
    show_buttons: bool,
    preferred_path_width: u16,
    path_width: u16,
}
```

Add width allocation logic that:
- always reserves buttons and name
- shrinks path first
- removes path before storage type
- removes storage type only after path is exhausted

- [ ] **Step 4: Re-run the targeted tests**

Run:

```bash
cargo test tui::tui::tests::terminal_size_support_rejects_smaller_than_80x24
cargo test tui::tui::tests::row_layout_drops_path_before_storage_type
cargo test tui::tui::tests::row_layout_can_remove_path_and_storage_type_but_keeps_name_and_buttons
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "test(tui): add terminal size and row layout guards"
```

---

## Task 2: Make Rack Rows Width-Safe And Spec-Compliant

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs`
- Test: `crates/anymount/src/tui/tui.rs`

- [ ] **Step 1: Write failing tests for truncation and left displacement**

Add tests for pure formatting helpers:

```rust
#[test]
fn render_model_uses_left_displacement_for_hovered_rows() {
    let model = mount_row_render_model(80, RowStyle::HoveredConnected, true);
    assert!(model.left_offset > 0);
    assert_eq!(model.right_overflow, 0);
}

#[test]
fn row_text_truncates_path_before_name() {
    let line = format_mount_row_text(&sample_provider(), 40, true);
    assert!(line.contains("backup"));
}
```

- [ ] **Step 2: Run the targeted tests and confirm they fail**

Run:

```bash
cargo test tui::tui::tests::render_model_uses_left_displacement_for_hovered_rows
```

Expected: FAIL because the helpers do not exist yet.

- [ ] **Step 3: Replace direct row formatting with bounded helpers**

Implement helpers that:
- compute the visible row rect after left displacement
- reduce width when displacement is applied instead of keeping the original
  width
- clip all draw areas to the frame
- format columns using the width budget from Task 1
- truncate with ellipsis where needed

Update `render_mount_row` and `render_add_row` to use these helpers and remove
the current `rect.x + displacement, rect.width` overflow behavior.

- [ ] **Step 4: Add a focused regression test for the current crash cause**

Add a test that exercises the helper with an `80`-column area and hovered row
state and asserts that the computed drawing rect remains within the frame:

```rust
#[test]
fn hovered_row_rect_stays_within_frame_width() {
    let rect = bounded_row_rect(Rect::new(0, 0, 80, 1), 2);
    assert!(rect.x + rect.width <= 80);
}
```

- [ ] **Step 5: Run all TUI unit tests**

Run:

```bash
cargo test tui::tui::tests:: -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "feat(tui): make rack row rendering width-safe"
```

---

## Task 3: Add Unsupported-Size Screen And Stable Footer

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs`
- Test: `crates/anymount/src/tui/tui.rs`

- [ ] **Step 1: Write failing tests for footer text and unsupported-size mode**

Add tests for pure text helpers:

```rust
#[test]
fn browse_footer_contains_full_shortcut_legend() {
    let footer = browse_footer_text();
    assert!(footer.contains("j"));
    assert!(footer.contains("r"));
    assert!(footer.contains("q"));
}

#[test]
fn unsupported_size_message_mentions_minimum() {
    let message = unsupported_size_message();
    assert!(message.contains("80x24"));
}
```

- [ ] **Step 2: Run the targeted tests and confirm they fail**

Run:

```bash
cargo test tui::tui::tests::browse_footer_contains_full_shortcut_legend
```

Expected: FAIL because the helpers do not exist yet.

- [ ] **Step 3: Add explicit unsupported-size rendering**

Update `draw_ui` to:
- check `is_supported_size(frame.area())` first
- render a dedicated unsupported-size screen when too small
- skip normal main/edit rendering in that state

Add stable footer helpers so browse and edit modes have deterministic shortcut
legends. Keep status feedback separate from the legend.

- [ ] **Step 4: Re-run the targeted tests**

Run:

```bash
cargo test tui::tui::tests::browse_footer_contains_full_shortcut_legend
cargo test tui::tui::tests::unsupported_size_message_mentions_minimum
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "feat(tui): add minimum terminal guard and stable footer"
```

---

## Task 4: Make Main-Menu Buttons Real Mouse Targets

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs`
- Test: `crates/anymount/src/tui/tui.rs`

- [ ] **Step 1: Write failing tests for button hit-testing**

Add a small hit-test enum and tests:

```rust
#[test]
fn hit_test_returns_edit_when_click_is_inside_edit_button() {
    let target = hit_test_row_action(sample_row_geometry(), 72);
    assert_eq!(target, Some(RowAction::Edit));
}

#[test]
fn hit_test_returns_connect_or_disconnect_for_primary_action_button() {
    let target = hit_test_row_action(sample_row_geometry(), 64);
    assert_eq!(target, Some(RowAction::Disconnect));
}
```

- [ ] **Step 2: Run the targeted tests and confirm they fail**

Run:

```bash
cargo test tui::tui::tests::hit_test_returns_edit_when_click_is_inside_edit_button
```

Expected: FAIL because hit-testing does not exist yet.

- [ ] **Step 3: Implement row button geometry and mouse dispatch**

Add pure helpers for:
- visible button labels
- button rectangles inside a row
- click-to-action mapping

Update `handle_mouse_event` so left-click:
- edits only when the edit button is clicked
- connects or disconnects when the primary action button is clicked
- selects the row when the click lands elsewhere in the row
- adds a new mount when the add-row button is clicked

- [ ] **Step 4: Run focused tests plus a connect/disconnect behavior test**

Run:

```bash
cargo test tui::tui::tests::hit_test_returns_edit_when_click_is_inside_edit_button
cargo test tui::tui::tests::hit_test_returns_connect_or_disconnect_for_primary_action_button
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "feat(tui): add real mouse hit-testing for row buttons"
```

---

## Task 5: Align Browse-Key Behavior With The Spec

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs`
- Test: `crates/anymount/src/tui/tui.rs`

- [ ] **Step 1: Write failing tests for quit and add-row behavior**

Add tests around `handle_browse_key`:

```rust
#[test]
fn browse_q_quits() {
    assert!(handle_browse_key(KeyCode::Char('q'), &cd, &mut state).unwrap());
}

#[test]
fn browse_escape_does_not_quit() {
    assert!(!handle_browse_key(KeyCode::Esc, &cd, &mut state).unwrap());
}
```

- [ ] **Step 2: Run the targeted tests and confirm they fail**

Run:

```bash
cargo test tui::tui::tests::browse_escape_does_not_quit
```

Expected: FAIL because `Esc` currently quits browse mode.

- [ ] **Step 3: Update browse-mode key handling**

Make `handle_browse_key` match the spec:
- `q` quits
- `Esc` does nothing in browse mode
- `j/k` and arrows move selection
- `Enter` edits selected row or opens add row
- `c`, `d`, `r` continue to work

- [ ] **Step 4: Re-run the targeted tests**

Run:

```bash
cargo test tui::tui::tests::browse_q_quits
cargo test tui::tui::tests::browse_escape_does_not_quit
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "fix(tui): align browse mode shortcuts with spec"
```

---

## Task 6: Make Edit Fields Directly Editable

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs`
- Test: `crates/anymount/src/tui/tui.rs`

- [ ] **Step 1: Replace the old edit-entry test with spec-compliant failing tests**

Remove or rewrite the current test that asserts Enter is required before
typing. Add tests like:

```rust
#[test]
fn typing_on_active_edit_field_updates_value_without_enter() {
    let mut session = EditSession::new_for_add("provider-1".to_owned());
    session.selected_field = EditField::Name;

    let _ = handle_edit_key(KeyCode::Char('x'), &cd, &mut session).unwrap();
    assert_eq!(session.draft.name, "provider-1x");
}

#[test]
fn edit_q_quits_tui() {
    assert_eq!(handle_edit_key(KeyCode::Char('q'), &cd, &mut session).unwrap(),
        EditAction::Quit);
}
```

- [ ] **Step 2: Run the targeted tests and confirm they fail**

Run:

```bash
cargo test tui::tui::tests::typing_on_active_edit_field_updates_value_without_enter
```

Expected: FAIL because edit mode still requires `Enter` before typing and
there is no `Quit` edit action.

- [ ] **Step 3: Simplify edit input modes**

Refactor `EditMode` and `handle_edit_key` so:
- text fields accept typing immediately while selected
- `Backspace` edits immediately
- `Tab` performs path completion for path fields, otherwise moves next
- `Esc` cancels the edit session
- `q` returns a new `EditAction::Quit`

Keep any special handling needed for storage type separate from free-text
fields.

- [ ] **Step 4: Re-run the targeted tests**

Run:

```bash
cargo test tui::tui::tests::typing_on_active_edit_field_updates_value_without_enter
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "feat(tui): make edit fields directly editable"
```

---

## Task 7: Replace Hidden Storage-Type Choice With Visible Inline Behavior

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs`
- Test: `crates/anymount/src/tui/tui.rs`

- [ ] **Step 1: Write failing tests for storage-type interaction**

Add tests such as:

```rust
#[test]
fn typing_o_on_storage_type_selects_onedrive() {
    let mut session = EditSession::new_for_add("provider-1".to_owned());
    session.selected_field = EditField::StorageType;

    let _ = handle_edit_key(KeyCode::Char('o'), &cd, &mut session).unwrap();
    assert_eq!(session.draft.storage_type, ProviderType::OneDrive);
}

#[test]
fn storage_type_field_remains_visible_without_modal_choice_state() {
    assert_eq!(storage_type_display(ProviderType::Local), "local");
}
```

- [ ] **Step 2: Run the targeted tests and confirm they fail**

Run:

```bash
cargo test tui::tui::tests::typing_o_on_storage_type_selects_onedrive
```

Expected: FAIL until the modal chooser path is removed.

- [ ] **Step 3: Remove hidden chooser mode and render visible value**

Update the edit flow so `storage.type`:
- always shows its current value inline
- responds to `l` / `o`
- can cycle with arrows or `j/k` when the field is selected if that fits the
  implementation cleanly
- continues preserving local and OneDrive field values when switching

- [ ] **Step 4: Re-run the targeted tests**

Run:

```bash
cargo test tui::tui::tests::typing_o_on_storage_type_selects_onedrive
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "feat(tui): make storage type selection visible and inline"
```

---

## Task 8: Update Edit Rendering And Bottom Action Bar

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs`
- Test: `crates/anymount/src/tui/tui.rs`

- [ ] **Step 1: Write failing tests for edit footer text and labels**

Add tests for helper output:

```rust
#[test]
fn edit_footer_contains_quit_and_save_shortcuts() {
    let footer = edit_footer_text(false);
    assert!(footer.contains("q"));
    assert!(footer.contains("c Save"));
}

#[test]
fn edit_field_labels_match_spec_name_field() {
    assert_eq!(EditField::Name.label(), "name");
}
```

- [ ] **Step 2: Run the targeted tests and confirm they fail**

Run:

```bash
cargo test tui::tui::tests::edit_field_labels_match_spec_name_field
```

Expected: FAIL because the label is currently `provider.name`.

- [ ] **Step 3: Update edit rendering helpers**

Adjust `draw_edit_menu` and related helpers so:
- labels match the spec
- active field highlighting stays single-line
- the bottom action bar is width-budgeted
- save button text switches between `Save` and `Create`
- footer legend includes `q`

- [ ] **Step 4: Re-run the targeted tests**

Run:

```bash
cargo test tui::tui::tests::edit_footer_contains_quit_and_save_shortcuts
cargo test tui::tui::tests::edit_field_labels_match_spec_name_field
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "feat(tui): align edit screen labels and actions with spec"
```

---

## Task 9: Verify Real TUI Launch At 80x24 And Full Test Suite

**Files:**
- Modify: `crates/anymount/src/tui/tui.rs` if verification reveals issues

- [ ] **Step 1: Run focused unit tests for the TUI module**

Use the project task runner where possible. Run:

```bash
cargo test tui::tui::tests:: -- --nocapture
```

Expected: PASS.

- [ ] **Step 2: Run the full test suite**

Run:

```bash
mise run test
```

Expected: PASS.

- [ ] **Step 3: Verify a real TUI launch in an `80x24` terminal**

Seed a temporary config and launch the TUI:

```bash
XDG_CONFIG_HOME=/tmp/anymount-tui-plan mise run anymount -- config add backup --path /mnt/backup local /data/backup
XDG_CONFIG_HOME=/tmp/anymount-tui-plan COLUMNS=80 LINES=24 target/release/anymount-cli
```

Expected:
- no panic on startup
- rack rows render within bounds
- `q` exits cleanly

- [ ] **Step 4: If verification exposed any issue, add a regression test and
  fix it**

Keep the regression small and local to `tui.rs`.

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/tui/tui.rs
git commit -m "test(tui): verify redesigned tui at 80x24"
```

