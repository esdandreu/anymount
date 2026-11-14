# Browser workspace POC

This static example uses the Rust `Driver` and `Mount` traits through the
`anymount` WebAssembly adapter. Its browser-facing storage port uses the
File System Access API for the selected directory. IndexedDB only remembers the
directory handle; application data stays in `anymount-note.txt` in that
directory.

## Run

Build the WebAssembly package and start the static file server:

```sh
mise run examples:browser:build
mise run examples:browser:serve
```

Open `http://localhost:4173`. Choose a workspace, write a note, and save it.
Reload the page and load the note from the same directory. The browser can ask
you to reconnect if permission did not persist.

Run the adapter tests with:

```sh
mise run examples:browser:test
```

## Compatibility and limits

The directory picker requires a secure context and a user gesture. The example
feature-detects it and is intended for current Chromium browsers; support varies
across browsers. `localhost` is treated as a secure context for local testing.
The browser remains responsible for permission prompts and users can revoke
access. Directory permission is browser access control, not multi-user
authorization.

IndexedDB remembers the directory handle for this site on this device. It does
not share the handle or data between devices. A user may select a folder that is
already synchronized by another provider, but synchronization is outside this
POC. There are no app accounts, authentication service, or application backend.

This driver does not provide FUSE, Windows Cloud Filter, arbitrary filesystem
access, cloud-provider access, or encryption. The existing native `Storage`
interface remains synchronous for native filesystem callbacks. Browser file
operations use the shared asynchronous byte-storage port and browser promises.
The shared `Driver` and `Mount` traits allow non-`Send` implementations and
futures, including single-threaded browser handles. The browser `Mount` represents a
logical user workspace; the native `Mount` implementations continue to expose
operating-system mount points.
