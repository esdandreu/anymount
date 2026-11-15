use js_sys::Function;
use wasm_bindgen::JsValue;

pub fn handle() -> JsValue {
    Function::new_no_args(r#"
        const h = {
            chunks: [], writes: 0, reads: 0, closes: 0, aborts: 0,
            committed: null, source: new Uint8Array([0, 1, 2, 3, 4, 5, 6])
        };
        const fail = name => { if (name) throw new DOMException('denied', name); };
        h.queryPermission = async () => 'granted';
        h.getFileHandle = async () => {
            fail(h.rejectGetFileHandle);
            if (h.holdStart) await new Promise(resolve => h.releaseStart = resolve);
            return {
                getFile: async () => ({
                    slice: (start, end) => ({ arrayBuffer: async () => {
                        h.reads++;
                        fail(h.rejectRead);
                        return h.source.slice(start, end).buffer;
                    } })
                }),
                createWritable: async () => {
                    fail(h.rejectCreateWritable);
                    return {
                        write: async bytes => {
                            h.writes++;
                            const copy = Array.from(bytes);
                            if (h.holdWrite) await new Promise(resolve => h.releaseWrite = resolve);
                            fail(h.rejectWrite);
                            h.chunks.push(copy);
                        },
                        close: async () => {
                            h.closes++;
                            fail(h.rejectClose);
                            if (h.holdClose) await new Promise(resolve => h.releaseClose = resolve);
                            h.committed = h.chunks.flat();
                        },
                        abort: async () => {
                            h.aborts++;
                            fail(h.rejectAbort);
                            if (h.holdAbort) await new Promise(resolve => h.releaseAbort = resolve);
                            h.chunks = [];
                        }
                    };
                }
            };
        };
        return h;
    "#).call0(&JsValue::NULL).expect("create browser fake")
}
