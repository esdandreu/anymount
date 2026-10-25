export function supportsBrowserWorkspace() {
  return (
    globalThis.isSecureContext &&
    "showDirectoryPicker" in globalThis &&
    "indexedDB" in globalThis
  );
}

export class DirectoryHandleStore {
  async get(name) {
    const db = await this.open();
    return this.transaction(db, "readonly", (store) => store.get(name));
  }

  async put(name, handle) {
    const db = await this.open();
    return this.transaction(db, "readwrite", (store) =>
      store.put(handle, name),
    );
  }

  open() {
    return new Promise((resolve, reject) => {
      const request = indexedDB.open("anymount-browser", 1);
      request.onupgradeneeded = () => {
        request.result.createObjectStore("directory-handles");
      };
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  }

  transaction(db, mode, operation) {
    return new Promise((resolve, reject) => {
      const transaction = db.transaction("directory-handles", mode);
      const request = operation(transaction.objectStore("directory-handles"));
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
      transaction.onabort = () => reject(transaction.error);
    });
  }
}
