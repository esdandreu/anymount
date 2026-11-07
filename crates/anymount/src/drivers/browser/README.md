# Browser workspace driver

The browser driver exposes a user-selected directory as a logical workspace.
It uses the File System Access API rather than creating an OS mount.

## States

- **Unmounted:** The workspace is not registered with the driver.
- **Mounted:** The driver holds a directory handle for the workspace.
  - **Connected:** Permission is granted and workspace file operations are enabled.
  - **Disconnected:** The handle remains registered but file operations are disabled.
