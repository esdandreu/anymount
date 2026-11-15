use super::*;
use crate::domain::{AuthorizationError, HostSaveError, SaveStatus};
use crate::test_utils::fakeoauth::{
    FakeConfigurationPersistence, FakeOAuthRuntime,
};
use crate::test_utils::mocks::BytesWriter;
use futures::FutureExt;
use serde_json::json;

fn config() -> OneDriveConfig {
    serde_json::from_value(json!({
        "root":"", "endpoint":"https://graph.microsoft.com/v1.0",
        "client_id":"test-public-client", "access_token":"opaque-old-access",
        "refresh_token":"old-refresh", "token_expiry_buffer_secs":0
    }))
    .expect("legacy-compatible configuration")
}
fn session(fake: &FakeOAuthRuntime) -> OneDriveSession {
    OneDriveSession::with_runtime(config(), "record-one".into(), fake.runtime())
        .expect("session")
}
fn token(access: &str, refresh: Option<&str>) -> serde_json::Value {
    let mut value = json!({"access_token":access,"token_type":"Bearer","expires_in":3600,"scope":"Files.ReadWrite"});
    if let Some(refresh) = refresh {
        value["refresh_token"] = json!(refresh);
    }
    value
}
fn callback(begin: &crate::domain::RedirectAuthorization) -> String {
    let url =
        url::Url::parse(&begin.authorization_url).expect("authorization URL");
    let state = url
        .query_pairs()
        .find(|(name, _)| name == "state")
        .expect("state")
        .1
        .into_owned();
    format!(
        "https://app.example/callback?code=secret-authorization-code&state={state}"
    )
}
fn snapshot(session: &OneDriveSession) -> serde_json::Value {
    serde_json::to_value(
        session.pending_update().expect("pending update").config,
    )
    .expect("snapshot")
}

#[test]
fn configuration_binding_and_debug_are_safe() {
    let fake = FakeOAuthRuntime::new(1000);
    let session = session(&fake);
    let rendered = format!("{:?} {:?}", config(), session);
    assert!(!rendered.contains("opaque-old-access"));
    assert!(!rendered.contains("old-refresh"));
    let mut malicious = config();
    malicious.endpoint = "https://evil.example/v1.0".into();
    assert!(matches!(
        OneDriveSession::with_runtime(malicious, "copy".into(), fake.runtime()),
        Err(AuthorizationError::Setup)
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn redirect_survives_navigation_and_replaces_the_whole_grant_once() {
    let fake = FakeOAuthRuntime::new(1000);
    let first = session(&fake);
    let begin = first
        .begin_redirect("https://app.example/callback")
        .await
        .expect("begin");
    let url = url::Url::parse(&begin.authorization_url).expect("URL");
    assert!(
        url.query_pairs()
            .any(|(key, value)| key == "code_challenge_method"
                && value == "S256")
    );
    let verifier =
        serde_json::from_str::<serde_json::Value>(&begin.continuation)
            .expect("continuation")["verifier"]
            .as_str()
            .expect("verifier")
            .to_owned();
    assert!(!begin.authorization_url.contains(&verifier));
    assert!(!format!("{begin:?}").contains(&verifier));
    let resumed = session(&fake);
    resumed
        .resume_redirect(&begin.continuation)
        .expect("resume after page navigation");
    fake.push_json(200, token("new-opaque-access", None));
    resumed
        .complete_redirect(&callback(&begin))
        .await
        .expect("complete");
    let update = snapshot(&resumed);
    assert_eq!(update["access_token"], "new-opaque-access");
    assert!(update["refresh_token"].is_null());
    assert_eq!(update["expires_at"], 4600);
    assert_eq!(update["scopes"], json!(["Files.ReadWrite"]));
    assert_eq!(update["binding"]["client_id"], "test-public-client");
    assert_eq!(
        resumed.complete_redirect(&callback(&begin)).await,
        Err(AuthorizationError::InvalidCallback)
    );
    assert_eq!(
        resumed.resume_redirect(&begin.continuation),
        Err(AuthorizationError::InvalidCallback)
    );
    assert_eq!(fake.requests().len(), 1);
    let form =
        String::from_utf8(fake.requests()[0].body.clone()).expect("form");
    assert!(form.contains("code_verifier="));
    assert!(!form.contains("client_secret"));
}

#[tokio::test(flavor = "current_thread")]
async fn callbacks_reject_mismatch_expiry_cancel_supersession_and_wrong_identity()
 {
    let fake = FakeOAuthRuntime::new(1000);
    let owner = session(&fake);
    let begin = owner
        .begin_redirect("https://app.example/callback")
        .await
        .expect("begin");
    assert_eq!(
        owner
            .complete_redirect(
                "https://app.example/callback?code=secret&state=wrong"
            )
            .await,
        Err(AuthorizationError::InvalidCallback)
    );
    let copy = OneDriveSession::with_runtime(
        config(),
        "other-record".into(),
        fake.runtime(),
    )
    .expect("copy");
    assert_eq!(
        copy.resume_redirect(&begin.continuation),
        Err(AuthorizationError::InvalidCallback)
    );
    owner.cancel();
    assert_eq!(
        owner.complete_redirect(&callback(&begin)).await,
        Err(AuthorizationError::InvalidCallback)
    );
    assert_eq!(
        owner.resume_redirect(&begin.continuation),
        Err(AuthorizationError::InvalidCallback)
    );
    let old = owner
        .begin_redirect("https://app.example/callback")
        .await
        .expect("begin");
    let latest = owner
        .begin_redirect("https://app.example/callback")
        .await
        .expect("supersede");
    assert_eq!(
        owner.complete_redirect(&callback(&old)).await,
        Err(AuthorizationError::InvalidCallback)
    );
    fake.advance(601);
    assert_eq!(
        owner.complete_redirect(&callback(&latest)).await,
        Err(AuthorizationError::Expired)
    );
    let new_page = session(&fake);
    assert_eq!(
        new_page.resume_redirect(&latest.continuation),
        Err(AuthorizationError::Expired)
    );
    assert!(fake.requests().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn device_polling_uses_provider_instructions_slowdown_and_typed_outcomes()
{
    for (error, expected) in [
        ("access_denied", AuthorizationError::Declined),
        ("expired_token", AuthorizationError::Expired),
    ] {
        let fake = FakeOAuthRuntime::new(1000);
        let owner = session(&fake);
        fake.push_json(200, json!({"device_code":"secret-device", "user_code":"SHOW-USER",
            "verification_uri":"https://microsoft.com/devicelogin", "expires_in":600, "interval":2}));
        let instructions = owner.begin_device().await.expect("device begin");
        assert_eq!(
            instructions.verification_uri,
            "https://microsoft.com/devicelogin"
        );
        assert_eq!(instructions.user_code, "SHOW-USER");
        assert!(instructions.verification_uri_complete.is_none());
        assert!(!format!("{instructions:?}").contains("SHOW-USER"));
        fake.push_json(400, json!({"error":"authorization_pending"}));
        fake.push_json(400, json!({"error":"slow_down"}));
        fake.push_json(
            400,
            json!({"error":error,"error_description":"secret-raw-response"}),
        );
        assert_eq!(owner.poll_device().await, Err(expected));
        assert!(fake.sleeps().ends_with(&[
            std::time::Duration::from_secs(2),
            std::time::Duration::from_secs(7)
        ]));
        assert!(owner.pending_update().is_none());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn device_poll_can_be_cancelled_without_clearing_existing_credentials() {
    let fake = FakeOAuthRuntime::new(1000);
    let owner = session(&fake);
    fake.push_json(200, json!({"device_code":"secret-device", "user_code":"USER",
        "verification_uri":"https://microsoft.com/devicelogin", "expires_in":600, "interval":2}));
    owner.begin_device().await.expect("begin");
    let _response = fake.defer_response();
    let mut poll = Box::pin(owner.poll_device());
    assert!(poll.as_mut().now_or_never().is_none());
    owner.cancel();
    assert_eq!(poll.await, Err(AuthorizationError::Cancelled));
    assert_eq!(
        &*owner
            .request_credentials(None)
            .await
            .expect("existing credentials")
            .token,
        "opaque-old-access"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn opaque_unknown_expiry_access_only_credentials_need_no_refresh() {
    let fake = FakeOAuthRuntime::new(1000);
    let mut config = config();
    config.refresh_token = None;
    let owner =
        OneDriveSession::with_runtime(config, "legacy".into(), fake.runtime())
            .expect("session");
    for _ in 0..3 {
        assert_eq!(
            &*owner
                .request_credentials(None)
                .await
                .expect("opaque access")
                .token,
            "opaque-old-access"
        );
    }
    assert!(fake.requests().is_empty());
    assert_eq!(
        owner.request_credentials(Some(0)).await.err(),
        Some(AuthorizationError::SignInRequired)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn multiple_connections_share_one_refresh_and_preserve_omitted_refresh_token()
 {
    let fake = FakeOAuthRuntime::new(1000);
    let mut config = config();
    config.expires_at = Some(999);
    let owner =
        OneDriveSession::with_runtime(config, "record".into(), fake.runtime())
            .expect("session");
    let first = owner.connect().expect("first connection");
    let second = owner.connect().expect("second connection");
    fake.push_json(200, token("rotated-access", None));
    fake.push_json(200, json!({"value":[]}));
    fake.push_json(200, json!({"value":[]}));
    let (left, right) = futures::join!(
        first.read_dir(StoragePath::root()),
        second.read_dir(StoragePath::root())
    );
    assert!(left.is_ok() && right.is_ok());
    assert_eq!(
        fake.requests()
            .iter()
            .filter(|request| request.method == "POST")
            .count(),
        1
    );
    assert_eq!(snapshot(&owner)["refresh_token"], "old-refresh");
    assert_eq!(snapshot(&owner)["credential_generation"], 1);
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_refresh_failure_is_shared_and_not_replayed() {
    for (response, expected) in [
        (
            json!({"error":"temporarily_unavailable"}),
            AuthorizationError::Temporary,
        ),
        (
            json!({"error":"invalid_grant"}),
            AuthorizationError::SignInRequired,
        ),
        (json!({"error":"invalid_client"}), AuthorizationError::Setup),
    ] {
        let fake = FakeOAuthRuntime::new(1000);
        let mut config = config();
        config.expires_at = Some(999);
        let owner = OneDriveSession::with_runtime(
            config,
            "record".into(),
            fake.runtime(),
        )
        .expect("session");
        fake.push_json(400, response);
        let (left, right) = futures::join!(
            owner.request_credentials(None),
            owner.request_credentials(None)
        );
        assert_eq!(left.err(), Some(expected));
        assert_eq!(right.err(), Some(expected));
        assert_eq!(owner.request_credentials(None).await.err(), Some(expected));
        assert_eq!(fake.requests().len(), 1);
        assert!(owner.pending_update().is_none());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_waiter_does_not_lose_completed_rotation_or_other_connection()
{
    let fake = FakeOAuthRuntime::new(1000);
    let mut config = config();
    config.expires_at = Some(999);
    let owner =
        OneDriveSession::with_runtime(config, "record".into(), fake.runtime())
            .expect("session");
    let response = fake.defer_response();
    let mut waiter = Box::pin(owner.request_credentials(None));
    assert!(waiter.as_mut().now_or_never().is_none());
    drop(waiter);
    tokio::task::yield_now().await;
    response
        .send(
            oauth2::http::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(
                    serde_json::to_vec(&token(
                        "new-access",
                        Some("replacement-refresh"),
                    ))
                    .expect("response"),
                )
                .expect("response"),
        )
        .expect("refresh still owns response");
    let credential =
        owner.request_credentials(None).await.expect("other waiter");
    assert_eq!(&*credential.token, "new-access");
    assert_eq!(snapshot(&owner)["refresh_token"], "replacement-refresh");
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn usable_credentials_ignore_delayed_failed_saves_and_retry_the_latest_generation()
 {
    let fake = FakeOAuthRuntime::new(1000);
    let owner = session(&fake);
    fake.push_json(200, token("first-access", Some("first-refresh")));
    owner
        .request_credentials(Some(0))
        .await
        .expect("first rotation");
    let store = FakeConfigurationPersistence::default();
    let (release, gate) = futures::channel::oneshot::channel();
    *store.gate.lock().expect("gate") = Some(gate);
    let mut saving = Box::pin(owner.save_pending(&store));
    assert!(saving.as_mut().now_or_never().is_none());
    assert_eq!(
        &*owner
            .request_credentials(None)
            .await
            .expect("usable during save")
            .token,
        "first-access"
    );
    fake.push_json(200, token("second-access", Some("second-refresh")));
    owner
        .request_credentials(Some(1))
        .await
        .expect("newer rotation");
    *store.fail.lock().expect("fail") = true;
    release.send(()).expect("save waiting");
    assert_eq!(saving.await, Err(HostSaveError));
    assert_eq!(owner.save_status(), SaveStatus::Failed { generation: 2 });
    assert_eq!(
        &*owner
            .request_credentials(None)
            .await
            .expect("usable after save failure")
            .token,
        "second-access"
    );
    *store.fail.lock().expect("fail") = false;
    owner
        .save_pending(&store)
        .await
        .expect("retry latest, no refresh");
    assert_eq!(*store.received.lock().expect("received"), vec![1, 2]);
    assert_eq!(
        store.saved.lock().expect("saved")[0]["access_token"],
        "second-access"
    );
    assert!(owner.pending_update().is_none());
    assert_eq!(fake.requests().len(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn serialized_saves_catch_up_and_copied_configurations_remain_independent()
 {
    let fake = FakeOAuthRuntime::new(1000);
    let owner = session(&fake);
    let copy = session(&fake);
    fake.push_json(200, token("one", Some("one-refresh")));
    owner.request_credentials(Some(0)).await.expect("rotate");
    assert_eq!(
        &*copy
            .request_credentials(None)
            .await
            .expect("independent config copy")
            .token,
        "opaque-old-access"
    );
    let store = FakeConfigurationPersistence::default();
    let (release, gate) = futures::channel::oneshot::channel();
    *store.gate.lock().expect("gate") = Some(gate);
    let mut old_save = Box::pin(owner.save_pending(&store));
    assert!(old_save.as_mut().now_or_never().is_none());
    fake.push_json(200, token("two", Some("two-refresh")));
    owner
        .request_credentials(Some(1))
        .await
        .expect("rotate again");
    let mut newer_save = Box::pin(owner.save_pending(&store));
    assert!(newer_save.as_mut().now_or_never().is_none());
    release.send(()).expect("save pending");
    let (older, newer) = futures::join!(old_save, newer_save);
    older.expect("older catches up");
    newer.expect("newer sees saved");
    assert_eq!(*store.received.lock().expect("received"), vec![1, 2]);
    assert_eq!(
        store.saved.lock().expect("saved").last().expect("saved")["access_token"],
        "two"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn safe_read_rejection_is_bounded_and_permission_is_not_sign_in() {
    let fake = FakeOAuthRuntime::new(1000);
    let owner = session(&fake);
    let storage = owner.connect().expect("connect");
    fake.push_json(401, json!({"error":{"code":"InvalidAuthenticationToken"}}));
    fake.push_json(200, token("new", Some("new-refresh")));
    fake.push_json(401, json!({"error":{"code":"InvalidAuthenticationToken"}}));
    assert!(matches!(
        storage.read_dir(StoragePath::root()).await,
        Err(ReadDirError::Authorization(
            AuthorizationError::SignInRequired
        ))
    ));
    assert_eq!(fake.requests().len(), 3);
    assert!(matches!(
        storage.read_dir(StoragePath::root()).await,
        Err(ReadDirError::Authorization(
            AuthorizationError::SignInRequired
        ))
    ));
    assert_eq!(
        fake.requests().len(),
        3,
        "rejected generation stays rejected"
    );
    let storage = session(&fake).connect().expect("independent usable grant");
    fake.push_json(
        403,
        json!({"error":{"message":"secret-provider-response"}}),
    );
    assert!(matches!(
        storage.read_dir(StoragePath::root()).await,
        Err(ReadDirError::PermissionDenied)
    ));
    assert_eq!(fake.requests().len(), 4);
    fake.push_error(AuthorizationError::Temporary);
    let error = storage
        .read_dir(StoragePath::root())
        .await
        .err()
        .expect("temporary error");
    assert!(matches!(
        error,
        ReadDirError::Authorization(AuthorizationError::Temporary)
    ));
    assert!(!format!("{error:?}").contains("secret-provider-response"));
}

#[tokio::test(flavor = "current_thread")]
async fn browser_compatible_downloads_use_metadata_and_never_forward_bearer_tokens()
 {
    let fake = FakeOAuthRuntime::new(1000);
    let owner = session(&fake);
    let storage = owner.connect().expect("connect");
    fake.push_json(200, json!({"@microsoft.graph.downloadUrl":"https://download.example/secret?ticket=secret-ticket"}));
    fake.push_body(206, b"abc".to_vec());
    let mut writer = BytesWriter::default();
    assert_eq!(
        storage
            .read_file_at(
                StoragePath::try_from(PathBuf::from("file")).expect("path"),
                &mut writer,
                4..7
            )
            .await
            .expect("read"),
        3
    );
    let requests = fake.requests();
    assert!(!requests[0].url.ends_with("/content"));
    assert!(
        requests[0]
            .headers
            .iter()
            .any(|(name, _)| name == "authorization")
    );
    assert!(
        !requests[1]
            .headers
            .iter()
            .any(|(name, _)| name == "authorization")
    );
    assert!(
        requests[1]
            .headers
            .iter()
            .any(|(name, value)| name == "range" && value == "bytes=4-6")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn failed_recovery_never_reuses_the_rejected_unexpired_token() {
    let fake = FakeOAuthRuntime::new(1000);
    let owner = session(&fake);
    let storage = owner.connect().expect("connection");
    fake.push_json(401, json!({"error":{"code":"InvalidAuthenticationToken"}}));
    fake.push_json(400, json!({"error":"invalid_grant"}));
    for _ in 0..2 {
        assert!(matches!(
            storage.read_dir(StoragePath::root()).await,
            Err(ReadDirError::Authorization(
                AuthorizationError::SignInRequired
            ))
        ));
    }
    assert_eq!(fake.requests().len(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn temporary_renewal_failure_recovers_only_after_host_retry() {
    let fake = FakeOAuthRuntime::new(1000);
    let owner = session(&fake);
    let storage = owner.connect().expect("connection");
    fake.push_json(401, json!({}));
    fake.push_json(503, json!({"error":"temporarily_unavailable"}));
    for _ in 0..2 {
        assert!(matches!(
            storage.read_dir(StoragePath::root()).await,
            Err(ReadDirError::Authorization(AuthorizationError::Temporary))
        ));
    }
    assert_eq!(fake.requests().len(), 2);
    assert!(crate::domain::Authorization::retry_renewal(&owner));
    fake.push_json(200, token("recovered-access", Some("recovered-refresh")));
    fake.push_json(200, json!({"value":[]}));
    storage
        .read_dir(StoragePath::root())
        .await
        .expect("recovered read");
    assert_eq!(snapshot(&owner)["refresh_token"], "recovered-refresh");
    assert_eq!(fake.requests().len(), 4);
    assert!(!owner.retry_renewal());
}
