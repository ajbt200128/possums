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
    let (id, session) = auth.authenticate(&credential).unwrap();
    assert_eq!(auth.session(&id).unwrap().account_id, "demo");
    assert!(Auth::verify_csrf(&session, &session.csrf));
    let cookie = session_cookie(&id);
    assert!(cookie.contains("Secure"));
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Strict"));
}

#[test]
fn rejects_short_or_wrong_credentials() {
    let (auth, _) = fixture();
    assert_eq!(auth.authenticate("short").unwrap_err(), AuthError::Invalid);
    assert_eq!(
        auth.authenticate(&URL_SAFE_NO_PAD.encode([9_u8; 32]))
            .unwrap_err(),
        AuthError::Invalid
    );
}
