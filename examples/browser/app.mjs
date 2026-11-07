import init, { BrowserDriverApi } from "./pkg/anymount_browser.js";
import {
  DirectoryHandleStore,
  supportsBrowserWorkspace,
} from "./driver.mjs";

const status = document.querySelector("#status");
const workspaceButton = document.querySelector("#workspace");
const reconnectButton = document.querySelector("#reconnect");
const loadButton = document.querySelector("#load");
const saveButton = document.querySelector("#save");
const content = document.querySelector("#content");
await init();
const driver = new BrowserDriverApi();
const handleStore = new DirectoryHandleStore();
let mount;

function browserMount() {
  return {
    name: "workspace",
    isConnected: () => driver.is_connected("workspace"),
    readText: () => driver.read_text("workspace", "anymount-note.txt"),
    writeText: (value) =>
      driver.write_text("workspace", "anymount-note.txt", value),
    connect: () => driver.connect_workspace("workspace", true),
  };
}

function setWorkspace(nextMount) {
  mount = nextMount;
  const connected = mount?.isConnected() ?? false;
  status.textContent = connected
    ? `Connected to ${mount.name}.`
    : "Workspace permission is needed.";
  reconnectButton.hidden = !mount || connected;
  loadButton.disabled = !connected;
  saveButton.disabled = !connected;
}

workspaceButton.addEventListener("click", async () => {
  try {
    await driver.mount_workspace("workspace", "workspace");
    await handleStore.put(
      "workspace",
      driver.directory_handle("workspace"),
    );
    setWorkspace(browserMount());
    await loadNote();
  } catch (error) {
    status.textContent = error.message;
  }
});

reconnectButton.addEventListener("click", async () => {
  try {
    if (await mount.connect()) {
      setWorkspace(mount);
      await loadNote();
    }
  } catch (error) {
    status.textContent = error.message;
  }
});

loadButton.addEventListener("click", loadNote);

saveButton.addEventListener("click", async () => {
  try {
    await mount.writeText(content.value);
    status.textContent = "Saved to the selected workspace.";
  } catch (error) {
    status.textContent = error.message;
  }
});

async function loadNote() {
  try {
    content.value = await mount.readText();
    status.textContent = "Loaded anymount-note.txt from the workspace.";
  } catch (error) {
    if (error.name === "NotFoundError") {
      content.value = "";
      status.textContent = "No saved note yet. Write something and save.";
      return;
    }
    status.textContent = error.message;
  }
}

async function restoreWorkspace() {
  if (!supportsBrowserWorkspace()) {
    status.textContent =
      "This browser does not support the required File System Access APIs.";
    workspaceButton.disabled = true;
    return;
  }
  try {
    const handle = await handleStore.get("workspace");
    if (!handle) {
      status.textContent = "Choose a workspace directory to get started.";
      return;
    }
    const connected = await driver.restore_workspace(
      "workspace",
      "workspace",
      handle,
    );
    const restored = browserMount();
    setWorkspace(restored);
    if (connected) {
      await loadNote();
    }
  } catch (error) {
    status.textContent = error.message;
  }
}

restoreWorkspace();
