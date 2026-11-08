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
      const transaction = {
        objectStore() {
          return {
            get(name) {
              const request = {};
              queueMicrotask(() => {
                request.result = handles.get(name);
                request.onsuccess?.();
                transaction.oncomplete?.();
              });
              return request;
            },
            put(handle, name) {
              const request = {};
              queueMicrotask(() => {
                handles.set(name, handle);
                request.result = name;
                request.onsuccess?.();
                transaction.oncomplete?.();
              });
              return request;
            },
          };
        },
      };
      return transaction;
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

function installAbortingIndexedDb(error) {
  globalThis.indexedDB = {
    open() {
      const request = {};
      queueMicrotask(() => {
        request.result = {
          transaction() {
            const transaction = {
              error,
              objectStore() {
                return {
                  put() {
                    const request = {};
                    queueMicrotask(() => {
                      request.onsuccess?.();
                      transaction.onabort?.();
                    });
                    return request;
                  },
                };
              },
            };
            return transaction;
          },
        };
        request.onsuccess?.();
      });
      return request;
    },
  };
}

test("handle writes reject if the transaction aborts after the request", async () => {
  installAbortingIndexedDb(new Error("commit failed"));
  await assert.rejects(
    new DirectoryHandleStore().put("workspace", {}),
    /commit failed/,
  );
});

test("handle writes report an abort without a transaction error", async () => {
  installAbortingIndexedDb(null);
  await assert.rejects(
    new DirectoryHandleStore().put("workspace", {}),
    /Handle store transaction aborted/,
  );
});
