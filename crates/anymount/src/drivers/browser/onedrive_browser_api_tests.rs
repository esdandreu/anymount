use super::*;
use crate::test_utils::fakeoauth::FakeOAuthRuntime;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_test::wasm_bindgen_test;

fn config_json(access_token: Option<&str>) -> String {
    json!({
        "type": "onedrive",
        "root": "",
        "endpoint": "https://graph.microsoft.com/v1.0",
        "client_id": "browser-public-client",
        "access_token": access_token,
    })
    .to_string()
}

fn parse_json(value: &str) -> Value {
    serde_json::from_str(value).expect("valid host JSON")
}

fn error_name(error: JsValue) -> String {
    error.unchecked_into::<js_sys::Error>().name().into()
}

fn ensure_temporary_storage() {
    let global = js_sys::global();
    let existing =
        js_sys::Reflect::get(&global, &JsValue::from_str("sessionStorage"))
            .expect("temporary storage property");
    if existing.is_null() || existing.is_undefined() {
        let storage = Function::new_no_args(
            "const entries = new Map(); return {
                getItem: key => entries.get(key) ?? null,
                setItem: (key, value) => entries.set(key, value),
                removeItem: key => entries.delete(key)
            };",
        )
        .call0(&JsValue::UNDEFINED)
        .expect("test storage");
        js_sys::Reflect::set(
            &global,
            &JsValue::from_str("sessionStorage"),
            &storage,
        )
        .expect("install temporary storage");
    }
}

fn fake_api(
    fake: &FakeOAuthRuntime,
    access_token: Option<&str>,
) -> OneDriveBrowserApi {
    ensure_temporary_storage();
    let config =
        serde_json::from_str(&config_json(access_token)).expect("config");
    let connector = OneDriveConnector::with_runtime(
        config,
        "saved-record".into(),
        fake.runtime(),
    )
    .expect("session");
    OneDriveBrowserApi::from_connector(connector)
}

fn callback(redirect: &Value) -> String {
    let url = url::Url::parse(
        redirect["authorization_url"]
            .as_str()
            .expect("authorization URL"),
    )
    .expect("URL");
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .expect("state")
        .1
        .into_owned();
    let mut callback =
        url::Url::parse("https://host.example/callback").expect("callback");
    callback
        .query_pairs_mut()
        .append_pair("state", &state)
        .append_pair("code", "code-sentinel");
    callback.into()
}

async fn sign_in(
    api: &OneDriveBrowserApi,
    fake: &FakeOAuthRuntime,
    token: &str,
) {
    let redirect = parse_json(
        &api.begin_redirect("https://host.example/callback")
            .await
            .expect("begin"),
    );
    fake.push_json(
        200,
        json!({
            "access_token": token,
            "token_type": "Bearer",
            "refresh_token": "refresh-sentinel",
            "expires_in": 3600,
            "scope": "Files.ReadWrite offline_access",
        }),
    );
    api.complete_redirect(&callback(&redirect))
        .await
        .expect("complete");
}

#[wasm_bindgen_test]
fn constructor_accepts_legacy_opaque_credentials_without_exposing_them() {
    let api = OneDriveBrowserApi::new(
        &config_json(Some("opaque-sentinel-token")),
        "saved-record".into(),
    )
    .expect("valid legacy config");
    api.connect().expect("opaque token connects");
    assert!(api.pending_update().expect("pending state").is_none());
    assert_eq!(
        parse_json(&api.save_status().expect("save status"))["state"],
        "clean"
    );
    let debug = format!("{api:?}");
    assert!(debug.contains("OneDriveBrowserApi"));
    assert!(!debug.contains("opaque-sentinel-token"));
    api.disconnect();
    assert_eq!(
        error_name(api.connected_storage().err().expect("disconnected")),
        "DisconnectedError"
    );
}

#[wasm_bindgen_test]
fn constructor_rejects_invalid_configuration_without_echoing_input() {
    let error = OneDriveBrowserApi::new(
        "{secret-sentinel-invalid-json",
        "saved-record".into(),
    )
    .expect_err("invalid config");
    let error: js_sys::Error = error.unchecked_into();
    assert_eq!(error.name(), "SetupError");
    assert!(!String::from(error.message()).contains("secret-sentinel"));
}

#[wasm_bindgen_test]
async fn disconnected_and_invalid_reads_do_not_start_authorization() {
    let api = OneDriveBrowserApi::new(
        &config_json(Some("opaque-token")),
        "saved-record".into(),
    )
    .expect("valid config");
    assert_eq!(
        error_name(api.read_dir("").await.expect_err("disconnected")),
        "DisconnectedError"
    );
    api.connect().expect("connect");
    assert_eq!(
        error_name(api.read_dir("../outside").await.expect_err("invalid path")),
        "InvalidPathError"
    );
    assert_eq!(
        error_name(
            api.read_file("file", 0, 65_537)
                .await
                .expect_err("large range")
        ),
        "RangeError"
    );
    assert_eq!(
        error_name(
            api.read_file("file", u64::MAX, 1)
                .await
                .expect_err("overflow")
        ),
        "RangeError"
    );
    assert!(api.pending_update().expect("pending state").is_none());
}

#[wasm_bindgen_test]
fn authorization_errors_preserve_actionable_names() {
    for (error, name) in [
        (AuthorizationError::Temporary, "TemporaryError"),
        (AuthorizationError::Setup, "SetupError"),
        (AuthorizationError::SignInRequired, "SignInRequiredError"),
        (
            AuthorizationError::PermissionDenied,
            "PermissionDeniedError",
        ),
        (AuthorizationError::Declined, "DeclinedError"),
        (AuthorizationError::Expired, "ExpiredError"),
        (AuthorizationError::Cancelled, "CancelledError"),
        (AuthorizationError::InvalidCallback, "InvalidCallbackError"),
    ] {
        assert_eq!(error_name(authorization_error(error)), name);
    }
}

#[wasm_bindgen_test]
async fn redirect_survives_navigation_and_reads_share_the_authorized_provider()
{
    let fake = FakeOAuthRuntime::new(1_700_000_000);
    let before_navigation = fake_api(&fake, None);
    let redirect = parse_json(
        &before_navigation
            .begin_redirect("https://host.example/callback")
            .await
            .expect("begin redirect"),
    );
    let continuation = redirect["continuation"].as_str().expect("continuation");
    let callback_url = callback(&redirect);
    drop(before_navigation);

    let api = fake_api(&fake, None);
    api.resume_redirect(continuation)
        .expect("resume after navigation");
    fake.push_json(
        200,
        json!({
            "access_token": "authorized-sentinel",
            "refresh_token": "rotated-sentinel",
            "token_type": "Bearer",
            "expires_in": 3600,
            "scope": "Files.ReadWrite offline_access",
        }),
    );
    api.complete_redirect(&callback_url)
        .await
        .expect("complete redirect");
    assert_eq!(
        error_name(
            api.complete_redirect(&callback_url)
                .await
                .expect_err("replay")
        ),
        "InvalidCallbackError"
    );
    api.connect().expect("connect authorized session");
    fake.push_json(
        200,
        json!({
            "value": [{"name": "note.bin", "size": 3, "file": {}}],
        }),
    );
    let entries = parse_json(&api.read_dir("").await.expect("directory"));
    assert_eq!(entries[0]["name"], "note.bin");
    assert_eq!(entries[0]["size"], 3);
    assert_eq!(entries[0]["is_dir"], false);
    fake.push_json(200, json!({
        "@microsoft.graph.downloadUrl": "https://download.example/note?secret=download-sentinel",
    }));
    fake.push_body(206, vec![0, 255, 42]);
    assert_eq!(
        api.read_file("note.bin", 0, 3).await.expect("binary read"),
        [0, 255, 42]
    );
    let requests = fake.requests();
    assert_eq!(requests.len(), 4);
    assert!(requests[0].url.contains("/token"));
    assert!(requests[1].url.ends_with("/children"));
    assert!(requests[1].headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("authorization")
            && value == "Bearer authorized-sentinel"
    }));
    assert!(!requests[2].url.contains("/content"));
    assert!(requests[3].url.starts_with("https://download.example/"));
    assert!(
        !requests[3]
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization"))
    );
    let pending =
        parse_json(&api.pending_update().expect("snapshot").expect("unsaved"));
    assert_eq!(pending["config"]["type"], "onedrive");
    assert_eq!(pending["config"]["access_token"], "authorized-sentinel");
    assert_eq!(
        parse_json(&api.save_status().expect("status"))["state"],
        "pending"
    );
}

#[wasm_bindgen_test]
async fn redirect_cancel_and_mismatched_callbacks_do_not_exchange_codes() {
    let fake = FakeOAuthRuntime::new(1_700_000_000);
    let api = fake_api(&fake, None);
    let redirect = parse_json(
        &api.begin_redirect("https://host.example/callback")
            .await
            .expect("begin"),
    );
    let wrong_callback =
        "https://host.example/callback?state=wrong&code=secret-code";
    assert_eq!(
        error_name(
            api.complete_redirect(wrong_callback)
                .await
                .expect_err("state mismatch")
        ),
        "InvalidCallbackError"
    );
    api.cancel().expect("cancel");
    let error = error_name(
        api.complete_redirect(&callback(&redirect))
            .await
            .expect_err("cancelled callback"),
    );
    assert!(error == "CancelledError" || error == "InvalidCallbackError");
    assert!(fake.requests().is_empty());
    assert!(api.pending_update().expect("pending").is_none());
}

#[wasm_bindgen_test]
async fn independent_save_failure_keeps_credentials_usable_and_retryable() {
    let fake = FakeOAuthRuntime::new(1_700_000_000);
    let api = fake_api(&fake, None);
    sign_in(&api, &fake, "usable-token").await;
    api.connect().expect("connect");
    let failing = Function::new_no_args(
        "return Promise.reject(new Error('secret-host-failure'))",
    );
    let error = api.save_pending(failing).await.expect_err("save fails");
    let error: js_sys::Error = error.unchecked_into();
    assert_eq!(error.name(), "HostSaveError");
    assert!(!String::from(error.message()).contains("secret-host-failure"));
    assert_eq!(
        parse_json(&api.save_status().expect("status"))["state"],
        "failed"
    );
    fake.push_json(200, json!({"value": []}));
    assert_eq!(
        parse_json(&api.read_dir("").await.expect("read despite save failure")),
        json!([])
    );
    let saved = Rc::new(RefCell::new(Vec::new()));
    let captured = saved.clone();
    let callback =
        Closure::<dyn FnMut(String) -> JsValue>::new(move |encoded: String| {
            captured.borrow_mut().push(parse_json(&encoded));
            Promise::resolve(&JsValue::UNDEFINED).into()
        });
    api.save_pending(callback.as_ref().unchecked_ref::<Function>().clone())
        .await
        .expect("retry save");
    assert_eq!(saved.borrow().len(), 1);
    assert_eq!(saved.borrow()[0]["config"]["access_token"], "usable-token");
    assert!(api.pending_update().expect("pending").is_none());
    assert_eq!(
        parse_json(&api.save_status().expect("status"))["state"],
        "saved"
    );
    assert_eq!(
        fake.requests()
            .iter()
            .filter(|request| request.url.contains("/token"))
            .count(),
        1
    );
}

#[wasm_bindgen_test]
async fn delayed_saves_do_not_gate_reads_and_follow_the_latest_generation() {
    let fake = FakeOAuthRuntime::new(1_700_000_000);
    let api = fake_api(&fake, None);
    sign_in(&api, &fake, "first-token").await;
    api.connect().expect("connect");
    let resolve = Rc::new(RefCell::new(None::<Function>));
    let resolve_slot = resolve.clone();
    let deferred = Promise::new(&mut move |resolve, _reject| {
        *resolve_slot.borrow_mut() = Some(resolve);
    });
    let saves = Rc::new(RefCell::new(Vec::new()));
    let captured = saves.clone();
    let callback =
        Closure::<dyn FnMut(String) -> JsValue>::new(move |encoded: String| {
            let mut saved = captured.borrow_mut();
            saved.push(parse_json(&encoded));
            if saved.len() == 1 {
                deferred.clone().into()
            } else {
                Promise::resolve(&JsValue::UNDEFINED).into()
            }
        });
    let save =
        api.save_pending(callback.as_ref().unchecked_ref::<Function>().clone());
    futures::pin_mut!(save);
    assert!(futures::poll!(save.as_mut()).is_pending());
    JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
        .await
        .expect("start save task");
    assert_eq!(saves.borrow().len(), 1);
    fake.push_json(200, json!({"value": []}));
    api.read_dir("").await.expect("read while saving");

    sign_in(&api, &fake, "newer-token").await;
    let newest = parse_json(
        &api.pending_update()
            .expect("snapshot")
            .expect("newest pending"),
    );
    assert_eq!(newest["config"]["access_token"], "newer-token");
    assert_eq!(
        parse_json(&api.save_status().expect("status")),
        json!({"state":"pending", "generation": newest["generation"]}),
    );
    let other_calls = Rc::new(Cell::new(0));
    let calls = other_calls.clone();
    let other_callback =
        Closure::<dyn FnMut(String) -> JsValue>::new(move |_| {
            calls.set(calls.get() + 1);
            Promise::resolve(&JsValue::UNDEFINED).into()
        });
    let other_save = api.save_pending(
        other_callback.as_ref().unchecked_ref::<Function>().clone(),
    );
    futures::pin_mut!(other_save);
    assert!(futures::poll!(other_save.as_mut()).is_pending());
    JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
        .await
        .expect("start queued save task");
    assert_eq!(other_calls.get(), 0);
    fake.push_json(200, json!({"value": []}));
    api.read_dir("")
        .await
        .expect("read latest credentials while save blocked");
    resolve
        .borrow()
        .as_ref()
        .expect("resolver")
        .call0(&JsValue::UNDEFINED)
        .expect("resolve save");
    save.await.expect("save latest generation");
    other_save
        .await
        .expect("concurrent save observes clean state");
    let saved = saves.borrow();
    assert_eq!(saved.len(), 2);
    assert!(saved[0]["generation"].as_u64() < saved[1]["generation"].as_u64());
    assert_eq!(saved[1]["config"]["access_token"], "newer-token");
    assert_eq!(other_calls.get(), 0);
    assert!(api.pending_update().expect("pending").is_none());
    assert_eq!(
        parse_json(&api.save_status().expect("status")),
        json!({"state":"saved", "generation": newest["generation"]}),
    );
}

#[wasm_bindgen_test]
async fn browser_storage_errors_preserve_temporary_and_permission_outcomes() {
    let fake = FakeOAuthRuntime::new(1_700_000_000);
    let api = fake_api(&fake, Some("opaque-token"));
    api.connect().expect("connect");
    fake.push_json(503, json!({"error": {"message": "secret-response"}}));
    assert_eq!(
        error_name(api.read_dir("").await.expect_err("temporary")),
        "TemporaryError"
    );
    fake.push_json(403, json!({"error": {"code": "accessDenied"}}));
    assert_eq!(
        error_name(api.read_dir("").await.expect_err("permission")),
        "PermissionDeniedError"
    );
    assert_eq!(fake.requests().len(), 2);
    assert!(api.pending_update().expect("pending").is_none());
}

#[wasm_bindgen_test]
async fn multiple_browser_mounts_share_authorized_credentials_without_saving() {
    let fake = FakeOAuthRuntime::new(1_700_000_000);
    let api = fake_api(&fake, None);
    sign_in(&api, &fake, "shared-access").await;
    let driver = crate::drivers::browser::BrowserDriverApi::new();
    driver
        .mount_onedrive("first".into(), "first".into(), &api)
        .expect("first mount");
    driver
        .mount_onedrive("second".into(), "second".into(), &api)
        .expect("second mount");
    driver
        .disconnect_workspace("first".into())
        .await
        .expect("disconnect one");
    fake.push_json(
        200,
        json!({
            "@microsoft.graph.downloadUrl": "https://download.example/note"
        }),
    );
    fake.push_body(206, b"shared".to_vec());
    assert_eq!(
        driver
            .read_text("second".into(), "note".into())
            .await
            .expect("other mount reads without save"),
        "shared"
    );
    assert!(api.pending_update().expect("pending update").is_some());
    assert!(
        fake.requests()[1]
            .headers
            .iter()
            .any(|(name, value)| name == "authorization"
                && value == "Bearer shared-access")
    );
}

#[wasm_bindgen_test]
async fn expired_redirect_continuations_cannot_be_restored() {
    let fake = FakeOAuthRuntime::new(1_700_000_000);
    let api = fake_api(&fake, None);
    let redirect = parse_json(
        &api.begin_redirect("https://host.example/callback")
            .await
            .expect("begin"),
    );
    let expiry = redirect["expires_at"].as_u64().expect("expiry");
    fake.advance(expiry - 1_700_000_000 + 1);
    let restored = fake_api(&fake, None);
    let error = restored
        .resume_redirect(
            redirect["continuation"].as_str().expect("continuation"),
        )
        .expect_err("expired continuation");
    assert_eq!(error_name(error), "ExpiredError");
    assert!(fake.requests().is_empty());
}

#[wasm_bindgen_test]
async fn dropping_a_save_waiter_does_not_release_the_serialized_host_write() {
    let fake = FakeOAuthRuntime::new(1_700_000_000);
    let api = fake_api(&fake, None);
    sign_in(&api, &fake, "first-token").await;
    let resolver = Rc::new(RefCell::new(None::<Function>));
    let resolver_slot = resolver.clone();
    let deferred = Promise::new(&mut move |resolve, _reject| {
        *resolver_slot.borrow_mut() = Some(resolve);
    });
    let snapshots = Rc::new(RefCell::new(Vec::new()));
    let captured = snapshots.clone();
    let callback =
        Closure::<dyn FnMut(String) -> JsValue>::new(move |encoded: String| {
            let mut snapshots = captured.borrow_mut();
            snapshots.push(parse_json(&encoded));
            if snapshots.len() == 1 {
                deferred.clone().into()
            } else {
                Promise::resolve(&JsValue::UNDEFINED).into()
            }
        });
    let mut save = Box::pin(
        api.save_pending(callback.as_ref().unchecked_ref::<Function>().clone()),
    );
    assert!(futures::poll!(save.as_mut()).is_pending());
    drop(save);
    JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
        .await
        .expect("start detached save");
    assert_eq!(snapshots.borrow().len(), 1);
    sign_in(&api, &fake, "newer-token").await;
    let overtaking_calls = Rc::new(Cell::new(0));
    let calls = overtaking_calls.clone();
    let later_callback =
        Closure::<dyn FnMut(String) -> JsValue>::new(move |_| {
            calls.set(calls.get() + 1);
            Promise::resolve(&JsValue::UNDEFINED).into()
        });
    let later_save = api.save_pending(
        later_callback.as_ref().unchecked_ref::<Function>().clone(),
    );
    futures::pin_mut!(later_save);
    assert!(futures::poll!(later_save.as_mut()).is_pending());
    JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
        .await
        .expect("queue later save");
    assert_eq!(overtaking_calls.get(), 0);
    resolver
        .borrow()
        .as_ref()
        .expect("resolver")
        .call0(&JsValue::UNDEFINED)
        .expect("resolve first write");
    later_save.await.expect("serialized save completes");
    assert_eq!(snapshots.borrow().len(), 2);
    assert_eq!(
        snapshots.borrow()[1]["config"]["access_token"],
        "newer-token"
    );
    assert_eq!(overtaking_calls.get(), 0);
    assert_eq!(
        parse_json(&api.save_status().expect("status"))["state"],
        "saved"
    );
}

#[wasm_bindgen_test]
async fn device_host_surface_preserves_provider_instructions_and_activates_credentials()
 {
    let fake = FakeOAuthRuntime::new(1_700_000_000);
    let api = fake_api(&fake, None);
    fake.push_json(
        200,
        json!({
            "device_code": "private-device-sentinel",
            "user_code": "DISPLAY-CODE",
            "verification_uri": "https://microsoft.com/devicelogin",
            "expires_in": 900,
            "interval": 5,
        }),
    );
    let instructions =
        parse_json(&api.begin_device().await.expect("device instructions"));
    assert_eq!(
        instructions["verification_uri"],
        "https://microsoft.com/devicelogin"
    );
    assert_eq!(instructions["user_code"], "DISPLAY-CODE");
    assert!(instructions["verification_uri_complete"].is_null());
    assert!(instructions.get("device_code").is_none());
    fake.push_json(
        200,
        json!({
            "access_token": "device-access-token",
            "token_type": "Bearer",
            "expires_in": 3600,
        }),
    );
    api.poll_device().await.expect("device authorization");
    api.connect().expect("connect authorized device grant");
    let pending =
        parse_json(&api.pending_update().expect("snapshot").expect("pending"));
    assert_eq!(pending["config"]["access_token"], "device-access-token");
    assert!(pending["config"]["refresh_token"].is_null());

    let save = Function::new_with_args("raw", "
        const saved = {type: 'onedrive', root: 'keep-root', refresh_token: 'old',
                       binding: {future: 'keep-extension'}};
        const {config, fields} = JSON.parse(raw);
        if (config.type !== saved.type) throw new Error('provider mismatch');
        for (const path of fields) {
            let target = saved;
            let source = config;
            for (const key of path.slice(0, -1)) {
                source = source?.[key];
                target = target[key] ??= {};
            }
            const key = path.at(-1);
            if (source?.[key] == null) delete target[key];
            else target[key] = source[key];
        }
        if (saved.access_token !== 'device-access-token' ||
            'refresh_token' in saved ||
            saved.root !== 'keep-root' ||
            saved.binding.future !== 'keep-extension' ||
            saved.binding.provider !== 'onedrive') {
            throw new Error('incorrect persisted fields');
        }
    ");
    api.save_pending(save)
        .await
        .expect("save provider-owned fields");
}

#[wasm_bindgen_test]
async fn stale_callback_preserves_the_current_persisted_attempt() {
    let fake = FakeOAuthRuntime::new(1_700_000_000);
    let api = fake_api(&fake, None);
    let old = parse_json(
        &api.begin_redirect("https://host.example/callback")
            .await
            .expect("old attempt"),
    );
    let current = parse_json(
        &api.begin_redirect("https://host.example/callback")
            .await
            .expect("current attempt"),
    );
    assert_eq!(
        error_name(
            api.complete_redirect(&callback(&old))
                .await
                .expect_err("stale callback")
        ),
        "InvalidCallbackError"
    );
    drop(api);
    let restored = fake_api(&fake, None);
    restored
        .resume_redirect(current["continuation"].as_str().expect("record"))
        .expect("current record was not deleted");
    fake.push_json(
        200,
        json!({
            "access_token": "current-grant", "token_type": "Bearer"
        }),
    );
    restored
        .complete_redirect(&callback(&current))
        .await
        .expect("current callback");
    assert_eq!(fake.requests().len(), 1);
    let reloaded = fake_api(&fake, None);
    assert!(
        reloaded
            .resume_redirect(current["continuation"].as_str().expect("record"))
            .is_err(),
        "consumption survives page reload"
    );
}
