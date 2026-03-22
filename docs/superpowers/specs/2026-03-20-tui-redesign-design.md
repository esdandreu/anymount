# TUI Redesign Specification

## Overview

Complete rework of the terminal user interface with a datacenter storage rack aesthetic.

## Scope Clarifications

- This redesign is a strict compliance pass against this spec plus the
  clarifications below.
- `q` quits the TUI from both the main menu and the edit menu.
- `Esc` returns from the edit menu to the main menu and discards unsaved
  changes.
- The TUI must support `80x24` terminals as a first-class minimum size.
- When the terminal is smaller than the supported minimum, render a dedicated
  "Terminal size not supported" screen instead of attempting partial layout.
- Main-menu rows must stay single-line.
- Row content should compress horizontally before truncating.
- Truncation priority for main-menu rows:
  1. Shrink and then remove the path column first.
  2. Remove the storage type column next if space is still insufficient.
  3. Preserve the name column and action buttons.

## Main Menu

### Visual Design

The main menu presents mounts as physical rack units:

```
 ┌────────────────────────────────────────────────────────┐                
 │ ●  backup       /mnt/backup      local                 │                
┌┴───────────────────────────────────────────────────────┬┤                
│ ○  backup       /mnt/backup      local                 ││                
└┬───────────────────────────────────────────────────────┴┤                
 │ ●  backup       /mnt/backup      local      [ ⇐ ][ ↵ ] │ Mouse Hover    
┌┴───────────────────────────────────────────────────────┬┤                
⇅ ○  backup       /mnt/backup      local      [ ⇒ ][ ↵ ] ││ Keyboard Hover 
├────────────────────────────────────────────────────────┤│                
│ ○  backup       /mnt/backup      local                 ││                
└┬───────────────────────────────────────────────────────┴┤                
 │                                                        │                
 └────────────────────────────────────────────────────────┘                
   j↑ select ↓k       ⇐ disconnect connect ⇒      info ↵
```

### Layout Rules

1. **No top border or title** — the first mount row is the top of the rack
2. **Each row is a rack unit** — separated by horizontal borders
3. **Connected rows** — displayed flush with the rack (standard position)
4. **Disconnected rows** — displayed displaced left with 3D depth effect:
   - Left border extended to create depth shadow
   - Right border offset to create pulled-out appearance
   - Row appears to hang off the left side of the rack
5. **Width safety** — all rack rows must fit within the visible frame without
   drawing past the terminal buffer

### Row Content

Each mount row displays:
- **Status icon**: `●` (green, connected) or `○` (gray, disconnected)
- **Name**: mount identifier
- **Mount path**: where the mount is exposed (e.g., `/mnt/backup`)
- **Type**: storage backend type (`local`, `onedrive`)

### Row States

#### Normal (not hovered)
- Connected: no action buttons shown
- Disconnected: no action buttons shown

#### Mouse Hovered
```
│ ●  name       /mnt/path    type      [ ⇐ ][ ↵ ] │ Mouse Hover
```
- Row displaced left (3D effect)
- Two buttons visible:
  - `[ ⇐ ]` — Disconnect button (for connected mounts)
  - `[ ↵ ]` — Edit button
- Buttons are interactive and respond to mouse clicks individually

#### Keyboard Focused
```
⇅ ○  name       /mnt/path    type      [ ⇒ ][ ↵ ] ││ Keyboard Hover
```
- Row displaced left (3D effect)
- Connection direction indicator: `⇅`
- Two buttons visible:
  - `[ ⇒ ]` — Connect button (for disconnected mounts)
  - `[ ↵ ]` — Edit button
- Buttons remain visible while keyboard-focused even without mouse hover

#### Add Row
```
│ +                                              [ ↵ Add]│
```
- Always at bottom of list
- Hovered: shows `[ ↵ Add]` button
- Pressing Enter on this row creates a new mount

### Status Indicators

| Icon | Color | Meaning |
|------|-------|---------|
| `●`  | Green | Mount is connected |
| `○`  | Gray  | Mount is disconnected |
| `⇅`  | White | Currently keyboard-focused |

### Footer

```
j↑ select ↓k       c connect   d disconnect      ↵ edit
```

The footer is a stable shortcut legend, not a transient status message line.
If status feedback is shown, it must not replace the shortcut legend.

| Key | Action |
|-----|--------|
| `j` / `↓` | Select next row |
| `k` / `↑` | Select previous row |
| `c` | Connect selected mount |
| `d` | Disconnect selected mount |
| `↵` | Edit selected mount / Add new mount |
| `r` | Refresh list |
| `q` | Quit |

## Edit Menu

### Visual Design

```
┌────────────────────────────────────────────────────────┐
│                                                        │
│  name             backup█                              │
│  path             /mnt/backup                          │
│  storage.type     local                                │
│  storage.root     /data/backup                         │
│                                                        │
│                              [ d Disc. ] [ x ] [ c Save ] │
└────────────────────────────────────────────────────────┘
```

Buttons in bottom-right:
- `[ d Disc. ]` — Disconnect mount
- `[ x ]` — Delete mount
- `[ c Save ]` — Save changes

Or for new mounts (not yet saved):
```
│                              [ d Disc. ] [ x ] [ c Create ] │
```

### Layout Rules

1. **No row separators** — clean vertical list
2. **No column separators** — field/value spacing is self-explanatory
3. **No header** — content is intuitive
4. **Single-line fields** — each visible field remains one line high
5. **Width safety** — the form must fit within the supported terminal size
   without drawing outside the frame

### Field Display

Each field shows:
- Field name (left-aligned, fixed width for alignment)
- Field value (right-aligned after field name)

When a value is empty: show `<unset>` placeholder

### Select Controls

Similar to main menu, the edit menu has select controls:
```
│                              [ ⇑ ] [ ⇓ ] [ x ] [ c Save ] │
```

| Button | Key | Action |
|--------|-----|--------|
| `[ ⇑ ]` | `k` / `↑` | Select previous field |
| `[ ⇓ ]` | `j` / `↓` | Select next field |
| `[ x ]` | `x` | Delete mount |
| `[ c Save ]` | `c` | Save changes |

### Active Field

The active field (currently being edited):
- Has **highlighted background color**
- Shows **text cursor** (`█`) at end of current value
- Can be typed into directly without pressing `Enter` first

### Navigation

| Key | Action |
|-----|--------|
| `j` / `↓` / `Tab` | Next field |
| `k` / `↑` / `Shift-Tab` | Previous field |
| `Enter` | Confirm edit / Enter text input mode |
| `Esc` | Exit edit mode (cancel) |
| `c` | Save changes |
| `d` | Disconnect mount |
| `x` | Delete mount |
| `Tab` | Path completion (for path fields) |
| `q` | Quit |

`Tab` is context-sensitive:
- For `path` and storage root fields while editing text, it performs path
  completion.
- Otherwise it moves to the next field.

### Editable Fields

| Field | Description | Path Completion |
|-------|-------------|-----------------|
| `name` | Provider filename | No |
| `path` | Mount path | Yes |
| `storage.type` | Storage backend | No (cycle choices) |
| `storage.local.root` | Local root directory | Yes |
| `storage.onedrive.root` | OneDrive root path | No |
| `storage.onedrive.endpoint` | Graph API endpoint | No |
| `storage.onedrive.access_token` | Access token | No |
| `storage.onedrive.refresh_token` | Refresh token | No |
| `storage.onedrive.client_id` | OAuth client ID | No |
| `storage.onedrive.token_expiry_buffer_secs` | Token refresh buffer | No |

### Storage Type Switching

When `storage.type` is changed:
- Show/hide relevant fields (local vs onedrive)
- Preserve existing field values when switching back
- Show the current choice directly in the field; avoid hidden chooser state

### Path Completion

For `path` and `storage.*.root` fields:
- `Tab` triggers filesystem path completion
- Completes to longest common prefix
- Shows available matches

### OneDrive Authentication

When editing OneDrive mounts:
- `l` or `o` key triggers OAuth flow
- Suspends TUI, opens browser for auth
- Populates `refresh_token` on success
- Shows status message: "OneDrive authentication completed"

### Delete Confirmation

When delete is triggered:
```
┌────────────────────────────────────────────────────────┐
│                                                        │
│  name             backup█                              │
│  path             /mnt/backup                          │
│  ...                                                 │
│                                                        │
│  ┌────────────────────────────────────────────────┐   │
│  │  Delete 'backup'? [ y ] [ n ]                  │   │
│  └────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────┘
```

| Key | Action |
|-----|--------|
| `y` | Confirm delete |
| `n` / `Esc` | Cancel |

## Navigation Flow

```
Main Menu ──[Enter]──> Edit Menu
    │                        │
    │                        │
    └──[Esc]─────────────────┘
```

- **Enter** on selected mount: open Edit Menu for that mount
- **Enter** on `+` row: open Edit Menu with empty template
- **Escape** in Edit Menu: return to Main Menu (discard changes)
- **Escape** in Edit Menu after saving: return to Main Menu

## Color Scheme

| Element | Color |
|---------|-------|
| Connected icon (`●`) | Green |
| Disconnected icon (`○`) | Gray |
| Selected/focus indicator (`⇅`) | White |
| Active edit field background | Highlighted (implementation-specific) |
| Text cursor (`█`) | Accent color |
| Buttons | Accent color |

## Implementation Notes

- Uses `ratatui` framework (existing dependency)
- Mouse events for hover detection and button clicks
- Keyboard events for navigation and shortcuts
- Raw terminal mode for full control
- Suspend/resume TUI for blocking operations (OAuth, connect)
- Layout calculations must clip or reduce content before render so narrow
  layouts cannot panic

## Keyboard Shortcuts Summary

### Main Menu
| Key | Action |
|-----|--------|
| `j` / `↓` | Next row |
| `k` / `↑` | Previous row |
| `c` | Connect |
| `d` | Disconnect |
| `Enter` | Edit / Add |
| `r` | Refresh list |
| `q` | Quit |

### Edit Menu
| Key | Action |
|-----|--------|
| `j` / `↓` / `Tab` | Next field |
| `k` / `↑` / `Shift-Tab` | Previous field |
| `Enter` | Confirm / Edit field |
| `Esc` | Cancel / Exit |
| `c` | Save |
| `d` | Disconnect |
| `x` | Delete |
| `Tab` | Path completion |
| `Backspace` | Delete char |
| `l` / `o` | OneDrive auth |
| `q` | Quit |
