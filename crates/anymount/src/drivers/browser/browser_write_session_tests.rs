use super::super::BrowserWorkspace;
use super::*;
use crate::domain::Storage;
use futures::{FutureExt, io::AsyncWriteExt};
use js_sys::{Function, Reflect};
use wasm_bindgen_test::wasm_bindgen_test;

fn handle() -> JsValue {
    Function::new_no_args(r#"
        const h = { chunks: [], writes: 0, closes: 0, aborts: 0, committed: null };
        const fail = name => { if (name) throw new DOMException('denied', name); };
        h.queryPermission = async () => 'granted';
        h.getFileHandle = async () => ({ createWritable: async () => ({
            write: async bytes => {
                h.writes++;
                fail(h.rejectWrite);
                const copy = Array.from(bytes);
                if (h.holdWrite) await new Promise(resolve => h.releaseWrite = resolve);
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
        }) });
        return h;
    "#).call0(&JsValue::NULL).expect("create browser fake")
}

fn get(handle: &JsValue, key: &str) -> JsValue {
    Reflect::get(handle, &JsValue::from_str(key)).expect("fake field")
}

fn set(handle: &JsValue, key: &str, value: JsValue) {
    Reflect::set(handle, &JsValue::from_str(key), &value)
        .expect("set fake field");
}

fn release(handle: &JsValue, key: &str) {
    get(handle, key)
        .dyn_into::<Function>()
        .expect("release function")
        .call0(handle)
        .expect("release promise");
}

fn path() -> crate::domain::StoragePath {
    crate::domain::StoragePath::try_from(std::path::PathBuf::from("file.bin"))
        .expect("path")
}

#[wasm_bindgen_test]
async fn browser_writes_are_bounded_and_pending_writes_accept_no_extra_input() {
    let handle = handle();
    set(&handle, "holdWrite", JsValue::TRUE);
    let storage = FileSystemAccessStorage::new(handle.clone());
    let mut session = storage.open_write(path()).await.expect("open");
    let input = vec![42; WRITE_CHUNK_SIZE * 3];
    assert_eq!(
        session.write(&input).await.expect("first partial write"),
        WRITE_CHUNK_SIZE
    );
    assert_eq!(get(&handle, "writes").as_f64(), Some(1.0));
    assert!(session.write(b"canceled").now_or_never().is_none());
    assert_eq!(get(&handle, "writes").as_f64(), Some(1.0));
    set(&handle, "holdWrite", JsValue::FALSE);
    release(&handle, "releaseWrite");
    assert_eq!(session.write(b"x").await.expect("different next input"), 1);
    session
        .write_all(&input[WRITE_CHUNK_SIZE..])
        .await
        .expect("remaining input");
    session.flush().await.expect("drain promises");
    assert!(get(&handle, "committed").is_null());
    let chunks = Array::from(&get(&handle, "chunks"));
    assert!(
        chunks
            .iter()
            .all(|chunk| Array::from(&chunk).length() as usize
                <= WRITE_CHUNK_SIZE)
    );
    session.close().await.expect("seal input");
    assert_eq!(get(&handle, "closes").as_f64(), Some(0.0));
    assert!(session.write(b"late").await.is_err());
    session.finish().await.expect("commit");
    let output = Uint8Array::new(&get(&handle, "committed")).to_vec();
    let mut expected = input[..WRITE_CHUNK_SIZE].to_vec();
    expected.push(b'x');
    expected.extend_from_slice(&input[WRITE_CHUNK_SIZE..]);
    assert_eq!(output, expected);
}

#[wasm_bindgen_test]
async fn finish_waits_for_close_and_resumes_without_duplicate_commit() {
    let handle = handle();
    set(&handle, "holdClose", JsValue::TRUE);
    let storage = BrowserWorkspace::new(
        Rc::new(FileSystemAccessStorage::new(handle.clone())),
        Some(handle.clone()),
    );
    storage.connect(false).await.expect("connect");
    let mut session = storage.open_write(path()).await.expect("open");
    session.write_all(b"content").await.expect("write");
    session.flush().await.expect("flush");
    assert!(session.finish().now_or_never().is_none());
    assert_eq!(get(&handle, "closes").as_f64(), Some(1.0));
    assert!(get(&handle, "committed").is_null());
    assert!(session.abort().await.is_err());
    release(&handle, "releaseClose");
    session.finish().await.expect("resume finish");
    session.finish().await.expect("repeat finish");
    assert_eq!(get(&handle, "closes").as_f64(), Some(1.0));
    assert_eq!(
        Uint8Array::new(&get(&handle, "committed")).to_vec(),
        b"content"
    );
}

#[wasm_bindgen_test]
async fn abort_waits_for_cleanup_and_drop_does_not_close() {
    let handle = handle();
    set(&handle, "holdAbort", JsValue::TRUE);
    let storage = FileSystemAccessStorage::new(handle.clone());
    let mut session = storage.open_write(path()).await.expect("open");
    session.write_all(b"discard").await.expect("write");
    session.flush().await.expect("flush");
    assert!(session.abort().now_or_never().is_none());
    assert!(session.finish().await.is_err());
    assert_eq!(get(&handle, "aborts").as_f64(), Some(1.0));
    release(&handle, "releaseAbort");
    session.abort().await.expect("resume abort");
    session.abort().await.expect("repeat abort");
    assert_eq!(get(&handle, "aborts").as_f64(), Some(1.0));
    assert!(session.write(b"late").await.is_err());
    drop(session);
    let mut dropped = storage.open_write(path()).await.expect("open");
    dropped.write_all(b"uncommitted").await.expect("write");
    drop(dropped);
    assert_eq!(get(&handle, "closes").as_f64(), Some(0.0));
    assert!(get(&handle, "committed").is_null());
}

#[wasm_bindgen_test]
async fn empty_file_finishes_without_a_write() {
    let handle = handle();
    let storage = FileSystemAccessStorage::new(handle.clone());
    let mut session = storage.open_write(path()).await.expect("open");
    assert_eq!(session.write(&[]).await.expect("empty write"), 0);
    session.finish().await.expect("finish empty file");
    assert_eq!(get(&handle, "writes").as_f64(), Some(0.0));
    assert_eq!(Array::from(&get(&handle, "committed")).length(), 0);
}

#[wasm_bindgen_test]
async fn permission_denials_disconnect_at_write_finish_and_abort() {
    for phase in ["rejectWrite", "rejectClose", "rejectAbort"] {
        let handle = handle();
        let workspace = BrowserWorkspace::new(
            Rc::new(FileSystemAccessStorage::new(handle.clone())),
            Some(handle.clone()),
        );
        workspace.connect(false).await.expect("connect");
        let mut session = workspace.open_write(path()).await.expect("open");
        set(&handle, phase, JsValue::from_str("NotAllowedError"));
        match phase {
            "rejectWrite" => {
                session
                    .write_all(b"data")
                    .await
                    .expect("accept buffered input");
                assert_eq!(
                    session.flush().await.expect_err("write denial").kind(),
                    io::ErrorKind::PermissionDenied
                );
                assert!(matches!(
                    session.finish().await,
                    Err(WriteFileError::PermissionDenied)
                ));
                assert_eq!(get(&handle, "closes").as_f64(), Some(0.0));
            }
            "rejectClose" => assert!(matches!(
                session.finish().await,
                Err(WriteFileError::PermissionDenied)
            )),
            _ => assert!(matches!(
                session.abort().await,
                Err(WriteFileError::PermissionDenied)
            )),
        }
        assert!(!workspace.is_connected());
        assert!(get(&handle, "committed").is_null());
        set(&handle, "rejectAbort", JsValue::UNDEFINED);
        session.abort().await.expect("cleanup while disconnected");
    }
}

#[wasm_bindgen_test]
async fn disconnection_blocks_existing_sessions_and_pending_finish() {
    let handle = handle();
    set(&handle, "holdWrite", JsValue::TRUE);
    let workspace = BrowserWorkspace::new(
        Rc::new(FileSystemAccessStorage::new(handle.clone())),
        Some(handle.clone()),
    );
    workspace.connect(false).await.expect("connect");
    let mut session = workspace.open_write(path()).await.expect("open");
    session.write_all(b"data").await.expect("accept write");
    assert!(session.finish().now_or_never().is_none());
    workspace.disconnect();
    release(&handle, "releaseWrite");
    assert!(matches!(
        session.finish().await,
        Err(WriteFileError::Disconnected)
    ));
    assert_eq!(
        session
            .write(b"late")
            .await
            .expect_err("disconnected")
            .kind(),
        io::ErrorKind::NotConnected
    );
    workspace.connect(false).await.expect("reconnect");
    assert!(session.finish().await.is_err());
    session.abort().await.expect("abort");
    assert_eq!(get(&handle, "closes").as_f64(), Some(0.0));
    assert!(get(&handle, "committed").is_null());
}

#[wasm_bindgen_test]
async fn synchronous_browser_failures_are_latched_without_committing() {
    let handle = handle();
    let storage = FileSystemAccessStorage::new(handle.clone());
    let mut session = storage.open_write(path()).await.expect("open");
    // A missing method throws before any promise can be created.
    let writable = Function::new_no_args("return { abort: async () => {} }")
        .call0(&JsValue::NULL)
        .expect("malformed writable");
    let mut malformed = BrowserWriteSession::new(&storage, writable);
    assert!(malformed.write_all(b"data").await.is_err());
    assert!(malformed.finish().await.is_err());
    malformed.abort().await.expect("abort malformed writer");
    set(&handle, "rejectClose", JsValue::from_str("OperationError"));
    assert!(session.finish().await.is_err());
    assert!(session.finish().await.is_err());
    session.abort().await.expect("abort failed commit");
    assert_eq!(get(&handle, "closes").as_f64(), Some(1.0));
    assert!(get(&handle, "committed").is_null());
}

#[wasm_bindgen_test]
async fn close_waits_for_pending_output_without_committing() {
    let handle = handle();
    set(&handle, "holdWrite", JsValue::TRUE);
    let storage = BrowserWorkspace::new(
        Rc::new(FileSystemAccessStorage::new(handle.clone())),
        Some(handle.clone()),
    );
    storage.connect(false).await.expect("connect");
    let mut session = storage.open_write(path()).await.expect("open");
    session.write_all(b"data").await.expect("accept input");
    assert!(session.close().now_or_never().is_none());
    assert!(session.write(b"late").await.is_err());
    assert_eq!(get(&handle, "closes").as_f64(), Some(0.0));
    release(&handle, "releaseWrite");
    session.close().await.expect("resume close");
    assert!(get(&handle, "committed").is_null());
    session.finish().await.expect("finish sealed session");
    assert_eq!(
        Uint8Array::new(&get(&handle, "committed")).to_vec(),
        b"data"
    );
}

#[wasm_bindgen_test]
async fn text_helper_aborts_on_write_or_commit_failure_and_preserves_permission_name()
 {
    for phase in ["rejectWrite", "rejectClose"] {
        let handle = handle();
        let api = super::super::BrowserDriverApi::new();
        assert!(
            api.restore_workspace(
                "workspace".into(),
                "workspace".into(),
                handle.clone(),
            )
            .await
            .expect("restore workspace")
        );
        set(&handle, phase, JsValue::from_str("NotAllowedError"));
        let error = api
            .write_text("workspace".into(), "file.txt".into(), "content".into())
            .await
            .expect_err("write failure")
            .dyn_into::<js_sys::Error>()
            .expect("JavaScript error");
        assert_eq!(error.name(), "NotAllowedError");
        assert!(
            !api.is_connected("workspace".into())
                .expect("connection state")
        );
        assert_eq!(get(&handle, "aborts").as_f64(), Some(1.0));
        assert!(get(&handle, "committed").is_null());
    }
}
