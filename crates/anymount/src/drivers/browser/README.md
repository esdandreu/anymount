# Browser workspace driver

The browser driver exposes a user-selected directory as a logical workspace.
It uses the File System Access API rather than creating an OS mount.

## States

- **Unmounted:** The workspace is not registered with the driver.
- **Mounted:** The driver holds a directory handle for the workspace.
  - **Connected:** Permission is granted and workspace file operations are enabled.
  - **Disconnected:** The handle remains registered but file operations are disabled.

The driver manages workspace connections and browser permissions. Permission
denials disconnect the workspace.

File writes use streaming sessions. Call `finish` to commit or await `abort`
to discard staged changes. Dropping a session does not guarantee cleanup.

## OneDrive OAuth host

`OneDriveBrowserApi` is a separate wasm-bindgen export. It uses the same
portable `OneDriveConnector` and `OneDriveStorage` as native hosts. OAuth consent
does not grant File System Access permission; neither API invokes the other.
Create one instance per host-owned saved configuration identity and reuse it
for authorization, connections, and reads. A new instance creates an independent
credential owner, even when its input credentials match another instance.
Hosts must prevent concurrent owners of the same saved record across tabs or
processes; there is no distributed coordination.

The constructor takes OneDrive configuration JSON and a stable host-owned
identity. Existing `root`, `endpoint`, token, and optional `client_id` fields
remain accepted. `connect()` uses the shared connector without opening UI or
saving credentials. `readDir(path)` resolves JSON entry records with
`name`, `is_dir`, and `size`. `readFile(path, offset, length)` resolves a
`Uint8Array`; `offset` is a JavaScript bigint and `length` is at most 65,536.
Paths are storage-relative. `disconnect()` releases only this connected handle,
not credentials or pending saves. Existing filesystem streaming writes are
unchanged.

The embedding host must supply a Microsoft public-client registration whose
account types, authority, scopes, and exact redirect URI match its deployment.
For browser code exchange, register the redirect as a **Single-page
application** URI with the correct origin. No client secret or Anymount
backend is used. The built-in client ID is not verified for arbitrary
self-hosted origins. PKCE does not bypass token endpoint or Graph CORS policy.
A provider endpoint that blocks required browser requests is unsupported;
another consent prompt cannot repair it. Device methods are exposed for
deployments supporting those endpoints, but browser device-flow CORS support
is not promised.

On explicit user sign-in, await `beginRedirect(redirectUri)` and parse its JSON.
The API stores the sensitive `continuation` in this tab's `sessionStorage`
before returning; navigate only after success. Never log the continuation,
append it to a URL, or put it in long-lived configuration. After navigation,
construct the API with the same configuration identity and call
`resumeRedirect(continuation)` with the stored record, then
`completeRedirect(window.location.href)`. The API validates the callback before
removing its record, consumes it before exchanging the code, and clears OAuth
query parameters from the current page's history. An invalid stale callback
does not delete the current attempt. Missing, expired, consumed, or mismatched
continuations require explicit sign-in. `cancel()` removes the stored attempt
without revoking already usable credentials or provider consent.

`BrowserDriverApi.mount_onedrive(name, path, authorizationHost)` uses that
host's connector. Workspaces share renewal; disconnecting one does not
invalidate another.

Successful sign-in and renewal update in-memory credentials immediately.
`pendingUpdate()` returns sensitive JSON `{generation, config, fields}`, or no
value; it is not a manual save acknowledgement. Independently call
`await api.savePending(async updateJson => { ... })`. Verify that `config.type`
matches the saved storage type. For each storage-relative path in `fields`, copy
the value from `config` or delete the field if the value is absent or null.
Leave unlisted fields and siblings untouched. Resolve only when the write
succeeds; throwing or rejecting reports `HostSaveError` without exposing
callback details. Saves run in order and catch up to newer generations.
Once started, the save task owns its callback until completion even if the
caller stops awaiting its promise; another save cannot overtake that write.
`saveStatus()` reports the browser host's clean, pending, saved, or failed state.
Retry `savePending`, not authorization, after a save failure. Reads neither
invoke nor wait for this callback. Hosts should schedule saving after sign-in
and after operations that may renew credentials.

Long-lived configuration contains credentials and must be treated as secret.
Reloading before a successful save can lose rotated credentials and require
sign-in; no transaction spans provider rotation and local storage.
OneDrive downloads use provider metadata's preauthenticated URL rather than
authenticated `/content` redirects. Such URLs are secrets, and bearer tokens
must not be attached to their download destinations.

Authorization errors have actionable JavaScript names such as
`TemporaryError`, `SetupError`, `SignInRequiredError`, `PermissionDeniedError`,
`DeclinedError`, `ExpiredError`, `CancelledError`, and `InvalidCallbackError`.
Do not treat every network failure or resource denial as a request to sign in.
