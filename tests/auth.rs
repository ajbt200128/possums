use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use possums::auth::{session_cookie, Auth, AuthError};
use sha2::{Digest, Sha256};

fn fixture() -> (Auth, String) {
    let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes()));
    let auth = Auth::from_json(&format!(
        r#"[{{"id":"demo","credential_sha256":"{hash}","demo_microunits":1000}}]"#
    ))
    .unwrap();
    (auth, credential)
}

#[test]
fn accepts_high_entropy_credential_and_sets_hardened_cookie() {
    let (auth, credential) = fixture();
    let challenge = auth.issue_login_challenge().unwrap();
    let (id, session) = auth.authenticate(&credential, &challenge).unwrap();
    assert_eq!(auth.session(&id).unwrap().account_id, "demo");
    assert!(Auth::verify_csrf(&session, &session.csrf));
    let cookie = session_cookie(&id);
    assert!(cookie.contains("Secure"));
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Strict"));
}

#[test]
fn rejects_short_oversized_or_wrong_credentials() {
    let (auth, _) = fixture();
    let challenge = auth.issue_login_challenge().unwrap();
    assert_eq!(
        auth.authenticate("short", &challenge).unwrap_err(),
        AuthError::Invalid
    );
    let challenge = auth.issue_login_challenge().unwrap();
    assert_eq!(
        auth.authenticate(&"A".repeat(1_000_000), &challenge)
            .unwrap_err(),
        AuthError::Invalid
    );
    let challenge = auth.issue_login_challenge().unwrap();
    assert_eq!(
        auth.authenticate(&URL_SAFE_NO_PAD.encode([9_u8; 32]), &challenge)
            .unwrap_err(),
        AuthError::Invalid
    );
}

#[test]
fn rejects_replayed_login_and_forged_cross_session_or_altered_model_tokens() {
    let (auth, credential) = fixture();
    let challenge = auth.issue_login_challenge().unwrap();
    let (first_id, first) = auth.authenticate(&credential, &challenge).unwrap();
    assert_eq!(
        auth.authenticate(&credential, &challenge).unwrap_err(),
        AuthError::Invalid
    );

    let token = auth.issue_submission(&first_id).unwrap();
    assert!(auth
        .bind_submission(&first_id, &first.account_id, &token, "model-a")
        .is_ok());
    assert_eq!(
        auth.bind_submission(&first_id, &first.account_id, &token, "model-b")
            .unwrap_err(),
        AuthError::Invalid
    );
    assert_eq!(
        auth.bind_submission(&first_id, &first.account_id, "forged", "model-a")
            .unwrap_err(),
        AuthError::Invalid
    );
    let next_token = auth.issue_submission(&first_id).unwrap();
    assert_eq!(
        auth.bind_submission(&first_id, &first.account_id, &next_token, "model-b")
            .unwrap_err(),
        AuthError::Invalid
    );
    assert_eq!(
        auth.session(&first_id).unwrap().selected_model.as_deref(),
        Some("model-a")
    );

    let challenge = auth.issue_login_challenge().unwrap();
    let (second_id, second) = auth.authenticate(&credential, &challenge).unwrap();
    assert_eq!(
        auth.bind_submission(&second_id, &second.account_id, &token, "model-a")
            .unwrap_err(),
        AuthError::Invalid
    );
}
