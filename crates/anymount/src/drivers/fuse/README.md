# macOS FUSE driver

The FUSE driver exposes storage through a native FUSE mount at a local path.
Mounting starts a FUSE session that serves filesystem requests.

## States

- **Unmounted:** No FUSE session is mounted at the configured path.
- **Mounted:** A FUSE session is mounted at the path and serves filesystem
  requests.
  - **Connected:** The FUSE session is active.
  - **Disconnected:** The FUSE session has stopped but the mount point still
    exists.
