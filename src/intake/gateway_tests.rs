//! Synthetic gateway acceptance over the disposable, verified Go/PocketBase TLS
//! fixture. No model transport or telemetry exporter is constructed. See
//! PRIVACY.md: Data handling boundaries and Synthetic testing exception.
use super::{
    protocol::Intake,
    transport::{IntakeClient, Request as IntakeRequest},
};
use crate::{
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::{Auth, CacheScope},
    catalog::{Model, ReasoningOptions},
    inference::{
        forward::{Evidence, ForwardSink},
        stream::{FinishReason, StreamCompletion, StreamUsage},
        tools::ToolInvocation,
        Inference, InferenceError, InferenceFailure, Message,
    },
    server::{router, AppState},
    telemetry::hooks::Lease,
};
use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
    response::Response,
    Router,
};
use serde_json::{json, Value};
use std::{
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::Notify;
use tower::ServiceExt;

#[path = "renewal_gateway_tests.rs"]
mod renewal_gateway_tests;

// The Go fixture creates actual native records and passes only their synthetic
// IDs. No production alias mapping or alternate account identity is introduced.
pub(super) fn fixture_account(name: &str) -> String {
    let id = std::env::var(format!("GATEWAY_INTAKE_FIXTURE_{name}")).unwrap();
    assert_eq!(id.len(), 15);
    assert!(id
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()));
    id
}

const BODY_BYTES: usize = 16 * 1024;
// Reservation covers both operational input and output maxima, not their
// post-tokenization context allowance (20 + 20 tokens, then 30% markup).
const RESERVATION: u64 = 52;

#[derive(Clone, Copy)]
enum Case {
    Complete,
    UsageFailure,
    NoUsageFailure,
    BodyDrop,
    Logout,
    Reset,
    RevokeBeforeTokens,
    RevokeAfterTokens,
    PreflightLogout,
    PreflightReset,
}
impl Case {
    fn charge(self) -> u64 {
        match self {
            Self::Complete => 7,
            Self::NoUsageFailure => 0,
            _ => 4,
        }
    }
}

struct Synthetic {
    case: Case,
    tokenizations: AtomicUsize,
    generations: AtomicUsize,
    captured: Notify,
    release: Notify,
    retired: Notify,
}
// This also fires when the gateway cancels the blocked inference future. Body
// loss must retire upstream without needing the synthetic release notification.
struct Retired<'a>(&'a Notify);
impl Drop for Retired<'_> {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

#[async_trait]
impl EvidenceVerifier for Synthetic {
    async fn verify(&self, _: &str, now: u64) -> Result<GatewayEvidence, EvidenceError> {
        Ok(GatewayEvidence {
            quote: json!({}),
            issued_at_unix: now,
            release_digest: String::new(),
            endpoint_key_sha256: String::new(),
            freshness_expires_at_unix: now + 60,
        })
    }
}
#[async_trait]
impl Inference for Synthetic {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Ok(json!({"object":"list","data":[{
            "id":"m","type":"chat","context_window":20,
            "endpoints":["/v1/chat/completions"],
            "pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}
        }]})
        .to_string()
        .into_bytes())
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _: Arc<Lease>,
    ) -> Result<u64, InferenceError> {
        panic!("synthetic acceptance uses count_request_tokens")
    }
    async fn count_request_tokens(
        &self,
        _: &str,
        _: (&[Message], Option<&ToolInvocation>, &ReasoningOptions),
        _: Arc<Lease>,
    ) -> Result<u64, InferenceError> {
        self.tokenizations.fetch_add(1, Ordering::SeqCst);
        if matches!(self.case, Case::PreflightLogout | Case::PreflightReset) {
            self.captured.notify_one();
            self.release.notified().await;
        }
        Ok(2)
    }
    async fn generate_forwarded_request(
        &self,
        model: &Model,
        _: (&[Message], Option<&ToolInvocation>, &ReasoningOptions),
        _: &CacheScope,
        heavy: Arc<Lease>,
        sink: &mut dyn ForwardSink,
    ) -> Result<StreamCompletion, InferenceError> {
        let _retired = Retired(&self.retired);
        self.generations.fetch_add(1, Ordering::SeqCst);
        assert_eq!(model.id, "m");
        assert_eq!(model.max_output_tokens, 18);
        heavy.dispatch_generation();
        if !matches!(self.case, Case::NoUsageFailure) {
            sink.evidence(Evidence::sse(StreamUsage {
                input_tokens: 2,
                output_tokens: 1,
                total_tokens: 3,
            }));
        }
        sink.event(
            json!({"choices":[{"index":0,"delta":{"content":"synthetic"},"finish_reason":null}]}),
        )
        .await?;
        self.captured.notify_one();
        self.release.notified().await;
        if !matches!(self.case, Case::Complete) {
            return Err(InferenceFailure::StreamTransportFailed.into());
        }
        let usage = StreamUsage {
            input_tokens: 2,
            output_tokens: 3,
            total_tokens: 5,
        };
        sink.event(
            json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}}),
        )
        .await?;
        let mut evidence = Evidence::sse(usage);
        evidence.source = "final_sse";
        sink.evidence(evidence);
        Ok(StreamCompletion {
            usage,
            finish_reason: FinishReason::Stop,
        })
    }
    fn verification_document(&self) -> Result<Value, InferenceError> {
        Err(InferenceError::Unavailable)
    }
}

async fn bounded<F: Future>(work: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(5), work)
        .await
        .expect("synthetic gateway acceptance deadline")
}
async fn send(app: &Router, request: Request<Body>, status: StatusCode) -> Response {
    let response = bounded(app.clone().oneshot(request)).await.unwrap();
    assert_eq!(response.status(), status);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    response
}
async fn bytes(response: Response) -> axum::body::Bytes {
    bounded(to_bytes(response.into_body(), BODY_BYTES))
        .await
        .unwrap()
}
async fn control(app: &Router, request: Request<Body>, status: StatusCode) -> Value {
    let bytes = bytes(send(app, request, status).await).await;
    if status == StatusCode::NO_CONTENT {
        assert!(bytes.is_empty());
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    }
}
fn post(path: &str, bearer: Option<&str>, body: Value) -> Request<Body> {
    let mut request = Request::post(path).header(header::CONTENT_TYPE, "application/json");
    if let Some(bearer) = bearer {
        request = request.header(header::AUTHORIZATION, format!("Bearer {bearer}"));
    }
    request.body(Body::from(body.to_string())).unwrap()
}
async fn login(app: &Router) -> String {
    login_with_credential(app, &"A".repeat(43)).await
}
async fn login_with_credential(app: &Router, credential: &str) -> String {
    let challenge = control(
        app,
        Request::get("/v1/auth/challenge")
            .body(Body::empty())
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    let login = control(
        app,
        post(
            "/v1/sessions",
            None,
            json!({"challenge":challenge["challenge"],"credential":credential}),
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(login["token_type"], "Bearer");
    login["token"].as_str().unwrap().to_owned()
}
async fn submission(app: &Router, bearer: &str, reset: bool) -> String {
    control(
        app,
        post(
            "/v1/submissions",
            Some(bearer),
            json!({"model":"m","new_conversation":reset}),
        ),
        StatusCode::OK,
    )
    .await["submission"]
        .as_str()
        .unwrap()
        .to_owned()
}
fn chat(bearer: &str, submission: &str) -> Request<Body> {
    post(
        "/v1/chat/completions",
        Some(bearer),
        json!({
            "model":"m","stream":true,"submission":submission,
            "messages":[{"role":"user","content":"synthetic request"}]
        }),
    )
}
async fn duplicate(app: &Router, bearer: &str, submission: &str, outcome: &str) {
    let error = control(app, chat(bearer, submission), StatusCode::CONFLICT).await;
    assert_eq!(error["error"]["code"], "duplicate_request");
    assert_eq!(error["error"]["outcome"], outcome);
}
async fn balance(client: &IntakeClient, available: u64, pending: usize) -> Value {
    // Only the TEST's private intake client reads accounting. No gateway balance
    // API is added or used, and native login material/tokens stay internal.
    account_balance(client, &fixture_account("ACCOUNT"), available, pending).await
}
async fn account_balance(
    client: &IntakeClient,
    account: &str,
    available: u64,
    pending: usize,
) -> Value {
    let response = bounded(client.send(IntakeRequest::Balance(account)))
        .await
        .unwrap();
    assert_eq!(response.status.as_u16(), 200);
    let snapshot = response.decode::<Value>().unwrap();
    assert_eq!(snapshot["account_balance"][0]["account_id"], account);
    assert_eq!(snapshot["available"], available.to_string());
    assert_eq!(
        snapshot["pending_reservations"].as_array().unwrap().len(),
        pending
    );
    snapshot
}
fn receipt(bytes: &[u8], case: Case) {
    let text = std::str::from_utf8(bytes).unwrap();
    assert!(text.ends_with("data: [DONE]\n\n"));
    assert_eq!(text.matches("data: [DONE]").count(), 1);
    let mut receipts = 0;
    let mut errors = 0;
    let mut finishes = 0;
    for data in text.split("\n\n").filter(|frame| !frame.is_empty()) {
        let data = data.strip_prefix("data: ").unwrap();
        if data == "[DONE]" {
            assert_eq!(receipts, 1); // An authoritative receipt must precede DONE.
            continue;
        }
        let event: Value = serde_json::from_str(data).unwrap();
        if event["choices"][0]["finish_reason"] == "stop" {
            finishes += 1;
        }
        if event.get("error").is_some() {
            errors += 1;
            assert_eq!(event["error"]["billing"], "unknown");
        }
        if let Some(receipt) = event.get("possums") {
            receipts += 1;
            let complete = matches!(case, Case::Complete);
            assert_eq!(
                receipt["status"],
                if complete { "complete" } else { "incomplete" }
            );
            assert_eq!(
                receipt["outcome"],
                if case.charge() == 0 {
                    "refunded"
                } else {
                    "settled"
                }
            );
            assert_eq!(receipt["reserved_microunits"], RESERVATION.to_string());
            assert_eq!(receipt["charged_microunits"], case.charge().to_string());
            assert_eq!(
                receipt["refunded_microunits"],
                (RESERVATION - case.charge()).to_string()
            );
            if matches!(case, Case::NoUsageFailure) {
                assert_eq!(receipt["evidence"], "none");
                assert!(receipt["usage"].is_null());
            } else {
                assert_eq!(
                    receipt["evidence"],
                    if complete { "final_sse" } else { "running_sse" }
                );
                assert_eq!(
                    receipt["usage"],
                    json!({"prompt_tokens":2,
                    "completion_tokens":if complete {3} else {1},
                    "total_tokens":if complete {5} else {3}})
                );
            }
        }
    }
    assert_eq!(receipts, 1);
    assert_eq!(finishes, usize::from(matches!(case, Case::Complete)));
    assert_eq!(errors, usize::from(!matches!(case, Case::Complete)));
}

async fn preflight_authority_changes(intake: Arc<Intake>, client: &IntakeClient, available: u64) {
    let primary = fixture_account("ACCOUNT");
    for case in [
        Case::RevokeBeforeTokens,
        Case::RevokeAfterTokens,
        Case::PreflightLogout,
        Case::PreflightReset,
    ] {
        let (account, prefix, expected_tokens) = match case {
            Case::RevokeBeforeTokens => (fixture_account("RESERVE_ACCOUNT"), "B", 0),
            Case::RevokeAfterTokens => (fixture_account("TOKENIZER_ACCOUNT"), "C", 1),
            _ => (primary.clone(), "A", 1),
        };
        let account_available = if account == primary { available } else { 1000 };
        let credential = format!("{}A", prefix.repeat(42));
        let provider = Arc::new(Synthetic {
            case,
            tokenizations: AtomicUsize::new(0),
            generations: AtomicUsize::new(0),
            captured: Notify::new(),
            release: Notify::new(),
            retired: Notify::new(),
        });
        let state = AppState::new(
            Auth::from_intake(intake.clone()),
            provider.clone(),
            "unused-synthetic-evidence",
            provider.clone(),
        );
        let app = router(state.clone());
        let bearer = login_with_credential(&app, &credential).await;
        let token = submission(&app, &bearer, false).await;
        let request = chat(&bearer, &token);
        let serving = app.clone();
        let pending = tokio::spawn(async move { serving.oneshot(request).await.unwrap() });
        let mut held = None;
        if matches!(case, Case::PreflightLogout | Case::PreflightReset) {
            // Actual owned hold and blocked tokenizer, not an unpolled mock.
            bounded(provider.captured.notified()).await;
            held = Some(balance(client, available - RESERVATION, 1).await);
            if matches!(case, Case::PreflightLogout) {
                control(
                    &app,
                    Request::delete("/v1/sessions/current")
                        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                        .body(Body::empty())
                        .unwrap(),
                    StatusCode::NO_CONTENT,
                )
                .await;
            } else {
                let _new_submission = submission(&app, &bearer, true).await;
            }
            provider.release.notify_one();
        }
        let response = bounded(pending).await.unwrap();
        // Native password revocation keeps its 401 projection; local post-await
        // session/submission loss uses the existing admission-invalid 400.
        let (status, code) = if matches!(case, Case::PreflightLogout | Case::PreflightReset) {
            (StatusCode::BAD_REQUEST, "invalid_request")
        } else {
            (StatusCode::UNAUTHORIZED, "unauthorized")
        };
        assert_eq!(response.status(), status);
        let body = bytes(response).await;
        let error: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(error["error"]["code"], code);
        assert!(!std::str::from_utf8(&body).unwrap().contains(&credential));
        drop(body); // The retained DATA bytes still own their response lease.
        state.lifecycle.quiesce();
        assert_eq!(bounded(state.lifecycle.wait_drained()).await, Ok(()));
        account_balance(client, &account, account_available, 0).await;
        balance(client, available, 0).await;
        if let Some(held) = held {
            let id = held["pending_reservations"][0]["reservation_id"]
                .as_str()
                .unwrap();
            let terminal = bounded(client.send(IntakeRequest::Lookup(id)))
                .await
                .unwrap();
            assert_eq!(terminal.status.as_u16(), 200);
            let terminal = terminal.decode::<Value>().unwrap();
            assert_eq!(terminal["reservation"]["state"], "refunded");
            assert_eq!(terminal["reservation"]["charged_amount"], "0");
        }
        assert_eq!(
            provider.tokenizations.load(Ordering::SeqCst),
            expected_tokens
        );
        assert_eq!(provider.generations.load(Ordering::SeqCst), 0);
        assert_eq!(state.active_lanes(), [0, 0, 0]);
    }
}

pub(super) async fn exercise(intake: Arc<Intake>, client: &IntakeClient) {
    let mut available = 889;
    balance(client, available, 0).await;
    for case in [
        Case::Complete,
        Case::UsageFailure,
        Case::NoUsageFailure,
        Case::BodyDrop,
        Case::Logout,
        Case::Reset,
    ] {
        let provider = Arc::new(Synthetic {
            case,
            tokenizations: AtomicUsize::new(0),
            generations: AtomicUsize::new(0),
            captured: Notify::new(),
            release: Notify::new(),
            retired: Notify::new(),
        });
        let state = AppState::new(
            Auth::from_intake(intake.clone()),
            provider.clone(),
            "unused-synthetic-evidence",
            provider.clone(),
        );
        assert!(state.accounting.is_remote());
        let app = router(state.clone());
        let bearer = login(&app).await;
        control(
            &app,
            Request::get("/v1/balance")
                .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                .body(Body::empty())
                .unwrap(),
            StatusCode::NOT_FOUND,
        )
        .await;
        let token = submission(&app, &bearer, false).await;
        let response = send(&app, chat(&bearer, &token), StatusCode::OK).await;
        bounded(provider.captured.notified()).await;
        let held = balance(client, available - RESERVATION, 1).await;
        duplicate(&app, &bearer, &token, "in_flight").await;
        assert!(held == balance(client, available - RESERVATION, 1).await);

        match case {
            Case::BodyDrop => {
                drop(response);
                // No release: this notification proves cancellation of the same
                // blocked inference, not a successful completion or replay.
                bounded(provider.retired.notified()).await;
            }
            _ => {
                if matches!(case, Case::Logout | Case::Reset) {
                    if matches!(case, Case::Logout) {
                        control(
                            &app,
                            Request::delete("/v1/sessions/current")
                                .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                                .body(Body::empty())
                                .unwrap(),
                            StatusCode::NO_CONTENT,
                        )
                        .await;
                    } else {
                        let _unused_new_token = submission(&app, &bearer, true).await;
                    }
                    // Logout invalidates bearer authority; reset retains the
                    // bearer but invalidates the old submission at admission.
                    let (status, code) = if matches!(case, Case::Logout) {
                        (StatusCode::UNAUTHORIZED, "unauthorized")
                    } else {
                        (StatusCode::BAD_REQUEST, "invalid_request")
                    };
                    let refused = control(&app, chat(&bearer, &token), status).await;
                    assert_eq!(refused["error"]["code"], code);
                    assert!(held == balance(client, available - RESERVATION, 1).await);
                }
                provider.release.notify_one();
                receipt(&bytes(response).await, case);
                bounded(provider.retired.notified()).await;
                if !matches!(case, Case::Logout | Case::Reset) {
                    duplicate(
                        &app,
                        &bearer,
                        &token,
                        if case.charge() == 0 {
                            "refunded"
                        } else {
                            "settled"
                        },
                    )
                    .await;
                }
            }
        }
        // Drain proves that the detached reserve/settle actor has retired; a
        // response-body EOF or upstream notification alone cannot establish it.
        state.lifecycle.quiesce();
        assert_eq!(bounded(state.lifecycle.wait_drained()).await, Ok(()));
        available -= case.charge();
        balance(client, available, 0).await;
        let id = held["pending_reservations"][0]["reservation_id"]
            .as_str()
            .unwrap();
        let terminal = bounded(client.send(IntakeRequest::Lookup(id)))
            .await
            .unwrap();
        assert_eq!(terminal.status.as_u16(), 200);
        let terminal = terminal.decode::<Value>().unwrap();
        assert_eq!(terminal["reservation"]["maximum"], RESERVATION.to_string());
        assert_eq!(
            terminal["reservation"]["state"],
            if case.charge() == 0 {
                "refunded"
            } else {
                "settled"
            }
        );
        assert_eq!(
            terminal["reservation"]["charged_amount"],
            case.charge().to_string()
        );
        assert_eq!(provider.tokenizations.load(Ordering::SeqCst), 1);
        assert_eq!(provider.generations.load(Ordering::SeqCst), 1);
        assert_eq!(state.active_lanes(), [0, 0, 0]);
    }
    assert_eq!(available, 866);
    preflight_authority_changes(intake, client, available).await;
}
