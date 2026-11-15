# Browser workspace driver

The browser driver exposes a user-selected directory as a logical workspace.
It uses the File System Access API rather than creating an OS mount.

## States

- **Unmounted:** The workspace is not registered with the driver.
- **Mounted:** The driver holds a directory handle for the workspace.
  - **Connected:** Permission is granted and workspace file operations are enabled.
  - **Disconnected:** The handle remains registered but file operations are disabled.

The driver owns workspace connection state and browser permission checks.
The built-in browser storage configuration is selected by its `kind()`; other
storage configurations are connected through their normal storage interface.

Writes open an incremental session with at most one pending 64 KiB browser
write. Flush waits for that write, and close seals input without committing.
Only explicit `finish` closes the browser stream and commits the replacement.
Callers must await `abort` after failure to clean up staged output; abort
remains available after permission denial or workspace disconnection.
Permission errors during writes, finalization, and cleanup disconnect the
workspace. Dropping a session cannot await cleanup, and canceling finish after
browser close has started cannot guarantee rollback.
