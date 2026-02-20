use qbz_nix_lib::{api::QobuzClient, credentials};

// Live tests against Qobuz require:
// - Explicit opt-in via env var
// - Local credentials available via QBZ credential store (keyring or fallback file)
//
// These tests must never print credentials or tokens.

fn live_tests_enabled() -> bool {
    matches!(
        std::env::var("QBZ_RUN_QOBUZ_LIVE_TESTS").as_deref(),
        Ok("1")
    )
}

#[tokio::test]
#[ignore]
async fn qobuz_login_and_favorites_smoke() {
    if !live_tests_enabled() {
        return;
    }

    let Some(creds) = credentials::load_qobuz_credentials().expect("Failed to load credentials")
    else {
        return;
    };

    let client = QobuzClient::default();
    client.init().await.expect("Failed to init client");

    client
        .login(&creds.email, &creds.password)
        .await
        .expect("Login failed");

    // Auth + signature protected endpoint; should succeed even if empty.
    let _ = client
        .get_favorites("tracks", 1, 0)
        .await
        .expect("get_favorites failed");
}
