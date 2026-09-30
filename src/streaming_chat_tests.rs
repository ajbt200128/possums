//! Synthetic trait-object composition tests, NOT authenticated provider/network.
use super::*;
use crate::{
    accounting::{Accounting, ReserveResult},
    auth::AuthError,
    catalog::Quote,
    inference::{
        stream::{ProtocolParser, StreamUsage},
        stream_support as wire, Inference,
    },
};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use http_body_util::BodyExt;
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::{Notify, Semaphore};

const PROMPT: &str = "private-composer-prompt-canary <script>&";
const DELTA: &str = "<script>alert(1)</script>&\"'";

fn quote() -> Quote {
    Quote {
        model: Model {
            id: "synthetic-model".into(),
            context_tokens: 100,
            max_output_tokens: 10,
            input_microunits_per_million_tokens: 2_000_000,
            output_microunits_per_million_tokens: 3_000_000,
        },
        input_tokens: 5,
        reserved_microunits: 52,
    }
}

struct Fixture {
    auth: Arc<Auth>,
    ledger: Arc<Accounting>,
    session_id: String,
    csrf: String,
    conversation: ConversationId,
    token: String,
    heavy: Arc<Semaphore>,
    generations: Arc<Semaphore>,
}

impl Fixture {
    fn new() -> Self {
        let credential = URL_SAFE_NO_PAD.encode([7; 32]);
        let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes()));
        let auth = Arc::new(
            Auth::from_json(&format!(
                r#"[{{"id":"a","credential_sha256":"{hash}","demo_microunits":1000}}]"#
            ))
            .unwrap()
            .with_submission_capacity(2),
        );
        let (session_id, session) = auth
            .authenticate(&credential, &auth.issue_login_challenge().unwrap())
            .unwrap();
        let token = auth.issue_submission(&session_id).unwrap();
        Self {
            ledger: Arc::new(Accounting::new(auth.account_budgets())),
            auth,
            session_id,
            csrf: session.csrf,
            conversation: session.conversation,
            token,
            heavy: Arc::new(Semaphore::new(1)),
            generations: Arc::new(Semaphore::new(1)),
        }
    }

    fn accept(&self) -> (ReservedGeneration, Arc<OwnedSemaphorePermit>, AcceptedChat) {
        let heavy = Arc::new(self.heavy.clone().try_acquire_owned().unwrap());
        let generation = self.generations.clone().try_acquire_owned().unwrap();
        let admission = self
            .auth
            .admit_submission(
                &self.ledger,
                &self.session_id,
                &self.csrf,
                &self.token,
                quote(),
            )
            .unwrap();
        assert_eq!(admission.result, ReserveResult::Reserved);
        (
            ReservedGeneration::new(
                self.ledger.clone(),
                admission.submission.id,
                generation,
                heavy.clone(),
            ),
            heavy,
            AcceptedChat {
                model: quote().model,
                history: vec![
                    Message {
                        role: "user".into(),
                        content: "prior <img>".into(),
                    },
                    Message {
                        role: "assistant".into(),
                        content: "prior & answer".into(),
                    },
                ],
                prompt: PROMPT.into(),
                session_id: self.session_id.clone(),
                csrf: self.csrf.clone(),
                conversation: self.conversation,
                resource_hooks: None,
            },
        )
    }

    fn compose(&self, mock: Arc<Mock>) -> (DeliveryBody, Observer) {
        let (owner, heavy, input) = self.accept();
        compose(owner, heavy, input, self.auth.clone(), mock)
    }

    fn balance(&self, expected: u64) {
        assert_eq!(self.ledger.available("a"), Some(expected));
    }

    fn no_next_token(&self) {
        // The accepted token occupies one of two slots. Exactly one new probe
        // must fit: this detects hidden token issuance, not just missing HTML.
        assert!(self
            .auth
            .issue_submission_for(&self.session_id, self.conversation, Some(&quote().model.id))
            .is_ok());
        assert_eq!(
            self.auth.issue_submission(&self.session_id),
            Err(AuthError::Capacity)
        );
    }
}

#[derive(Clone, Copy)]
enum TerminalResult {
    Success,
    InvalidUsage,
    MissingUsage,
    UpstreamError,
}

struct Mock {
    started: Notify,
    delta_sent: Notify,
    delta_gate: Semaphore,
    finish_gate: Semaphore,
    calls: AtomicUsize,
    consumed_terminal: AtomicBool,
    result: TerminalResult,
}

impl Mock {
    fn new(result: TerminalResult) -> Arc<Self> {
        Arc::new(Self {
            started: Notify::new(),
            delta_sent: Notify::new(),
            delta_gate: Semaphore::new(0),
            finish_gate: Semaphore::new(0),
            calls: AtomicUsize::new(0),
            consumed_terminal: AtomicBool::new(false),
            result,
        })
    }
}

#[async_trait]
impl Inference for Mock {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        panic!("unexpected catalog call")
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _heavy: std::sync::Arc<tokio::sync::OwnedSemaphorePermit>,
    ) -> Result<u64, InferenceError> {
        panic!("unexpected preflight call")
    }

    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        Err(InferenceError::Unavailable)
    }

    async fn generate_stream(
        &self,
        model: &Model,
        messages: &[Message],
        _heavy: std::sync::Arc<tokio::sync::OwnedSemaphorePermit>,
        on_delta: &mut (dyn for<'delta> FnMut(&'delta str) + Send),
    ) -> Result<StreamUsage, InferenceError> {
        assert_eq!(model, &quote().model); // No output/model reduction.
        assert!(messages
            .last()
            .is_some_and(|message| message.role == "user" && message.content == PROMPT));
        assert_eq!(self.calls.fetch_add(1, Ordering::SeqCst), 0);
        self.started.notify_one();
        self.delta_gate.acquire().await.unwrap().forget();
        let mut parser = ProtocolParser::default();
        parser
            .feed(
                &wire::event(wire::choice(Some(DELTA), None)),
                &mut *on_delta,
            )
            .map_err(|_| InferenceError::InvalidResponse)?;
        self.delta_sent.notify_one();
        self.finish_gate.acquire().await.unwrap().forget();
        self.consumed_terminal.store(true, Ordering::SeqCst);
        let mut terminal = wire::event(wire::choice(None, Some("stop")));
        match self.result {
            TerminalResult::Success => terminal.extend(wire::event(wire::usage(5, 4))),
            TerminalResult::InvalidUsage => terminal.extend(wire::event(serde_json::json!({"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":4,"total_tokens":10}}))),
            TerminalResult::MissingUsage => {},
            TerminalResult::UpstreamError => terminal.extend(wire::event(serde_json::json!({"error":{"message":"fixture failure"}}))),
        }
        terminal.extend(b"data: [DONE]\n\n");
        parser
            .feed(&terminal, on_delta)
            .map_err(|_| InferenceError::InvalidResponse)?;
        parser.eof().map_err(|_| InferenceError::InvalidResponse)
    }
}

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .unwrap()
}

async fn frame(body: &mut DeliveryBody) -> Option<String> {
    let frame = bounded(body.frame()).await?.unwrap().into_data().unwrap();
    assert!(frame.len() <= LIMITS.chunk_bytes as usize);
    let usage = body.usage();
    assert!(
        usage.high_bytes <= LIMITS.payload_bytes as usize && usage.high_frames <= LIMITS.frames
    );
    Some(std::str::from_utf8(&frame).unwrap().to_owned())
}

async fn through(body: &mut DeliveryBody, html: &mut String, marker: &str) {
    while !html.contains(marker) {
        html.push_str(&frame(body).await.expect("synthetic body ended early"));
    }
}

async fn prefix(body: &mut DeliveryBody, mock: &Mock) -> String {
    let mut html = String::new();
    through(body, &mut html, "<pre aria-label=\"Assistant\">").await;
    bounded(mock.started.notified()).await;
    html
}

#[tokio::test]
async fn streams_escaped_deltas_before_terminal_but_token_only_after_settlement() {
    let f = Fixture::new();
    let mock = Mock::new(TerminalResult::Success);
    let (mut body, mut observer) = f.compose(mock.clone());
    let mut html = prefix(&mut body, &mock).await;
    f.balance(948);
    assert!(observer.try_recv().is_err());
    assert!(!html.contains("name=token") && !html.contains(">Send<"));
    mock.delta_gate.add_permits(1);
    bounded(mock.delta_sent.notified()).await;
    through(
        &mut body,
        &mut html,
        "&lt;script&gt;alert(1)&lt;/script&gt;&amp;&quot;&#39;",
    )
    .await;
    assert!(!mock.consumed_terminal.load(Ordering::SeqCst));
    f.balance(948);
    mock.finish_gate.add_permits(1);
    while let Some(part) = frame(&mut body).await {
        if part.contains("name=token") {
            f.balance(971);
        }
        html.push_str(&part);
    }
    assert_eq!(
        bounded(observer).await.unwrap(),
        Ok(Outcome::Settled { charged: 29 })
    );
    assert!(html.contains("prior &lt;img&gt;") && html.contains("prior &amp; answer"));
    assert!(!html.contains("<script") && !html.contains("<img") && !html.contains("<script src"));
    assert!(html.contains(">Send</button>") && html.contains("name=history_manifest"));
    let token = html
        .split("name=token value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let mut wrong_model = quote();
    wrong_model.model.id = "another-model".into();
    assert!(f
        .auth
        .admit_submission(&f.ledger, &f.session_id, &f.csrf, token, wrong_model)
        .is_err());
    let next = f
        .auth
        .admit_submission(&f.ledger, &f.session_id, &f.csrf, token, quote())
        .unwrap();
    assert_eq!(next.submission.conversation, f.conversation);
    assert_eq!(next.result, ReserveResult::Reserved);
    f.ledger.finish(next.submission.id, None).unwrap();
    f.balance(971);
    assert_eq!(f.generations.available_permits(), 1);
    assert_eq!(f.heavy.available_permits(), 0); // Body still owns the shared lane.
    drop(body);
    assert_eq!(f.heavy.available_permits(), 1);
}

#[tokio::test]
async fn no_token_is_minted_while_adapter_terminal_is_held() {
    let f = Fixture::new();
    let mock = Mock::new(TerminalResult::Success);
    let (mut body, observer) = f.compose(mock.clone());
    let mut html = prefix(&mut body, &mock).await;
    mock.delta_gate.add_permits(1);
    bounded(mock.delta_sent.notified()).await;
    through(&mut body, &mut html, "alert(1)").await;
    f.balance(948);
    f.no_next_token(); // Probe consumes remaining token capacity before terminal.
    mock.finish_gate.add_permits(1);
    while let Some(part) = frame(&mut body).await {
        html.push_str(&part);
    }
    assert_eq!(
        bounded(observer).await.unwrap(),
        Ok(Outcome::Settled { charged: 29 })
    );
    assert!(!html.contains("name=token") && !html.contains(">Send<"));
}

#[tokio::test]
async fn startup_or_streaming_body_and_observer_loss_still_consumes_and_settles() {
    for during_startup in [true, false] {
        let f = Fixture::new();
        let mock = Mock::new(TerminalResult::Success);
        let (mut body, observer) = f.compose(mock.clone());
        if !during_startup {
            prefix(&mut body, &mock).await;
        }
        drop((body, observer));
        if during_startup {
            bounded(mock.started.notified()).await;
        }
        mock.delta_gate.add_permits(1);
        bounded(mock.delta_sent.notified()).await;
        f.balance(948);
        assert_eq!(f.generations.available_permits(), 0);
        assert_eq!(f.heavy.available_permits(), 0);
        mock.finish_gate.add_permits(1);
        let all = bounded(f.generations.clone().acquire_owned())
            .await
            .unwrap();
        assert!(mock.consumed_terminal.load(Ordering::SeqCst));
        assert_eq!(mock.calls.load(Ordering::SeqCst), 1);
        f.balance(971);
        f.no_next_token();
        assert_eq!(f.heavy.available_permits(), 1);
        drop(all);
    }
}

#[tokio::test]
async fn invalid_or_missing_usage_and_upstream_failure_refund_without_continuation() {
    for result in [
        TerminalResult::InvalidUsage,
        TerminalResult::MissingUsage,
        TerminalResult::UpstreamError,
    ] {
        let f = Fixture::new();
        let mock = Mock::new(result);
        let (mut body, observer) = f.compose(mock.clone());
        let mut html = prefix(&mut body, &mock).await;
        mock.delta_gate.add_permits(1);
        bounded(mock.delta_sent.notified()).await;
        through(&mut body, &mut html, "alert(1)").await;
        mock.finish_gate.add_permits(1);
        while let Some(part) = frame(&mut body).await {
            html.push_str(&part);
        }
        assert_eq!(bounded(observer).await.unwrap(), Ok(Outcome::Refunded));
        f.balance(1000);
        f.no_next_token();
        assert!(!html.contains("name=token") && !html.contains(">Send<"));
        assert!(html.contains("Generation failed; no continuation is available."));
        assert!(!html.contains("Conversation changed"));
        assert_eq!(mock.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn reset_during_generation_cannot_mint_for_the_new_conversation() {
    let f = Fixture::new();
    let mock = Mock::new(TerminalResult::Success);
    let (mut body, observer) = f.compose(mock.clone());
    let mut html = prefix(&mut body, &mock).await;
    f.auth.new_chat(&f.session_id, &f.csrf).unwrap();
    mock.delta_gate.add_permits(1);
    bounded(mock.delta_sent.notified()).await;
    through(&mut body, &mut html, "alert(1)").await;
    mock.finish_gate.add_permits(1);
    while let Some(part) = frame(&mut body).await {
        html.push_str(&part);
    }
    assert_eq!(
        bounded(observer).await.unwrap(),
        Ok(Outcome::Settled { charged: 29 })
    );
    f.balance(971);
    assert!(!html.contains("name=token") && !html.contains(">Send<"));
    // Reset removed old tokens: both slots still fit, proving no stale issuance.
    assert!(f.auth.issue_submission(&f.session_id).is_ok());
    assert!(f.auth.issue_submission(&f.session_id).is_ok());
}

#[tokio::test]
async fn cumulative_startup_timeout_detaches_output_not_accepted_inference() {
    let f = Fixture::new();
    let mock = Mock::new(TerminalResult::Success);
    let (owner, heavy, mut input) = f.accept();
    input.history[0].content = "x".repeat(32 * 1024);
    let (body, observer) = compose_with_startup(
        owner,
        heavy,
        input,
        f.auth.clone(),
        mock.clone(),
        Duration::from_millis(20),
        start,
    );
    // No body drain: startup fills its eight frames, then its single deadline
    // expires. Inference must start anyway, without waiting on the client.
    bounded(mock.started.notified()).await;
    assert_eq!(body.usage().high_frames, LIMITS.frames);
    assert!(body.usage().high_bytes <= LIMITS.payload_bytes as usize);
    drop(body);
    f.balance(948);
    mock.delta_gate.add_permits(1);
    mock.finish_gate.add_permits(1);
    assert_eq!(
        bounded(observer).await.unwrap(),
        Ok(Outcome::Settled { charged: 29 })
    );
    f.balance(971);
    f.no_next_token();
}

#[tokio::test]
async fn invalid_startup_refunds_without_sending_prompt() {
    let f = Fixture::new();
    let mock = Mock::new(TerminalResult::Success);
    let (owner, heavy, mut input) = f.accept();
    input.csrf.clear();
    let (mut body, observer) = compose(owner, heavy, input, f.auth.clone(), mock.clone());
    assert!(frame(&mut body).await.is_none());
    assert_eq!(bounded(observer).await.unwrap(), Ok(Outcome::Refunded));
    assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
    f.balance(1000);
    f.no_next_token();
}

#[test]
fn runtime_cancellation_keeps_detached_blocking_input_heavy_leased() {
    let f = Fixture::new();
    let mock = Mock::new(TerminalResult::Success);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (owner, heavy, input) = f.accept();
    let (entered_tx, entered) = oneshot::channel();
    let (release_tx, release) = std::sync::mpsc::channel();
    let observer = {
        let _entered = runtime.enter();
        let (body, observer) = compose_with_startup(
            owner,
            heavy,
            input,
            f.auth.clone(),
            mock.clone(),
            STARTUP_TIMEOUT,
            move |mut job| {
                assert!(job.tx.send_blocking(b"detached").is_err());
                entered_tx.send(()).unwrap();
                release.recv_timeout(Duration::from_secs(5)).unwrap();
                start(job)
            },
        );
        drop(body); // No poll of the async worker until block_on below.
        observer
    };
    runtime.block_on(bounded(entered)).unwrap();
    runtime.shutdown_timeout(Duration::ZERO);
    drop(observer);
    f.balance(1000);
    assert_eq!(f.generations.available_permits(), 1);
    assert_eq!(f.heavy.available_permits(), 0); // Still held by blocking input!
    release_tx.send(()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while f.heavy.available_permits() == 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(f.heavy.available_permits(), 1);
    assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn blocking_panic_refunds_without_prompt_logs_with_service_hook() {
    const CHILD: &str = "POSSUMS_COMPOSER_PANIC_CHILD";
    if std::env::var_os(CHILD).is_some() {
        // Same prerequisite as main; isolate the global hook from other tests.
        std::panic::set_hook(Box::new(|_| {}));
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let f = Fixture::new();
                let mock = Mock::new(TerminalResult::Success);
                let (owner, heavy, input) = f.accept();
                let (mut body, observer) = compose_with_startup(
                    owner,
                    heavy,
                    input,
                    f.auth.clone(),
                    mock.clone(),
                    STARTUP_TIMEOUT,
                    |job| {
                        std::panic::panic_any(job.input.prompt.clone());
                    },
                );
                assert_eq!(bounded(observer).await.unwrap(), Ok(Outcome::Refunded));
                assert!(frame(&mut body).await.is_none());
                assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
                f.balance(1000);
                f.no_next_token();
            });
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "streaming_chat::tests::blocking_panic_refunds_without_prompt_logs_with_service_hook",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(PROMPT));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(PROMPT));
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}
