import assert from "node:assert/strict";
import test from "node:test";

import {
  DirectoryHandleStore,
  supportsBrowserWorkspace,
} from "./driver.mjs";

function installIndexedDbMock() {
  const handles = new Map();
  const database = {
    createObjectStore() {},
    transaction() {
      return {
        objectStore() {
          return {
            get(name) {
              const request = {};
              queueMicrotask(() => {
                request.result = handles.get(name);
                request.onsuccess?.();
              });
              return request;
            },
            put(handle, name) {
              const request = {};
              queueMicrotask(() => {
                handles.set(name, handle);
                request.result = name;
                request.onsuccess?.();
              });
              return request;
            },
          };
        },
      };
    },
  };
  globalThis.indexedDB = {
    open() {
      const request = {};
      queueMicrotask(() => {
        request.result = database;
        request.onupgradeneeded?.();
        request.onsuccess?.();
      });
      return request;
    },
  };
}

test("directory handles persist through the site handle store", async () => {
  installIndexedDbMock();
  const store = new DirectoryHandleStore();
  const handle = { name: "chosen folder" };

  await store.put("workspace", handle);

  assert.equal(await new DirectoryHandleStore().get("workspace"), handle);
});

test("browser workspace feature detection requires a secure supported API", () => {
  const previous = {
    secure: globalThis.isSecureContext,
    picker: globalThis.showDirectoryPicker,
    indexedDB: globalThis.indexedDB,
  };
  Object.defineProperty(globalThis, "isSecureContext", {
    configurable: true,
    value: true,
  });
  globalThis.showDirectoryPicker = () => {};
  globalThis.indexedDB = {};

  assert.equal(supportsBrowserWorkspace(), true);

  Object.defineProperty(globalThis, "isSecureContext", {
    configurable: true,
    value: false,
  });
  assert.equal(supportsBrowserWorkspace(), false);

  Object.defineProperty(globalThis, "isSecureContext", {
    configurable: true,
    value: previous.secure,
  });
  if (previous.picker === undefined) {
    delete globalThis.showDirectoryPicker;
  } else {
    globalThis.showDirectoryPicker = previous.picker;
  }
  if (previous.indexedDB === undefined) {
    delete globalThis.indexedDB;
  } else {
    globalThis.indexedDB = previous.indexedDB;
  }
});
