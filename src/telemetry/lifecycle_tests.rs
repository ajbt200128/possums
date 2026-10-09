//! Real router/owner hooks with local synthetic inference, not an export fixture.
use super::*;
use crate::{
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::Auth,
    catalog::Model,
    inference::{
        stream::{FinishReason, StreamCompletion, StreamUsage},
        Inference, InferenceError, InferenceFailure, Message,
    },
    telemetry::hooks::Lease,
    web::{router, AppState},
};
use async_trait::async_trait;
use axum::{body::Body, http::Request};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use http_body_util::BodyExt;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::Notify;
use tower::ServiceExt;

fn metrics(mode: Deployment) -> Arc<AggregateMetrics> {
    let metrics = Arc::new(AggregateMetrics::new(
        mode,
        SystemClock {
            start: Instant::now(),
            fixed: Some(ClockReading {
                monotonic_ns: SECOND,
                wall_ns: 301 * SECOND,
            }),
        },
    ));
    // Owner integration only: bypass warmup, never release a partial fixture.
    metrics.state.lock().unwrap().requests.eligible = 300 * SECOND;
    metrics
}

#[derive(Clone, Copy)]
enum Failure {
    None,
    Preflight,
    Stream,
    Usage,
}
struct Provider {
    failure: Failure,
    entered: Notify,
    finish: Notify,
    startup_queued: Notify,
    startup_drained: std::sync::Mutex<Option<Arc<Notify>>>,
}
#[async_trait]
impl EvidenceVerifier for Provider {
    async fn verify(&self, _: &str, now: u64) -> Result<GatewayEvidence, EvidenceError> {
        Ok(GatewayEvidence {
            quote: serde_json::json!({}),
            issued_at_unix: now,
            release_digest: String::new(),
            endpoint_key_sha256: String::new(),
            freshness_expires_at_unix: now + 60,
        })
    }
}
#[async_trait]
impl Inference for Provider {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Ok(br#"{"object":"list","data":[{"id":"kimi-k3","type":"chat","context_window":20,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}}]}"#.to_vec())
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _: Arc<Lease>,
    ) -> Result<u64, InferenceError> {
        if matches!(self.failure, Failure::Preflight) {
            Err(InferenceFailure::TokenizerSendFailed.into())
        } else {
            Ok(1)
        }
    }
    async fn generate_stream(
        &self,
        model: &Model,
        history: &[Message],
        heavy: Arc<Lease>,
        delta: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<StreamUsage, InferenceError> {
        self.generate_completion_stream(model, history, heavy, delta)
            .await
            .map(|c| c.usage)
    }
    async fn generate_completion_stream(
        &self,
        _: &Model,
        _: &[Message],
        heavy: Arc<Lease>,
        delta: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<StreamCompletion, InferenceError> {
        heavy.dispatch_generation();
        self.startup_queued.notify_one();
        let ready = self.startup_drained.lock().unwrap().take();
        if let Some(ready) = ready {
            ready.notified().await;
        }
        delta("");
        delta("hostile-output-canary");
        delta("another fragment");
        self.entered.notify_one();
        self.finish.notified().await;
        if matches!(self.failure, Failure::Stream) {
            return Err(InferenceFailure::StreamUsageMissing.into());
        }
        Ok(StreamCompletion {
            usage: StreamUsage {
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: if matches!(self.failure, Failure::Usage) {
                    999
                } else {
                    2
                },
            },
            finish_reason: FinishReason::Stop,
        })
    }
    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        Err(InferenceError::Unavailable)
    }
}
struct Fixture {
    state: AppState,
    provider: Arc<Provider>,
    credential: String,
    csrf: String,
    token: String,
    api: bool,
}
impl Fixture {
    fn new(api: bool, failure: Failure, metrics: Option<Arc<AggregateMetrics>>) -> Self {
        let secret = URL_SAFE_NO_PAD.encode([8; 32]);
        let auth = Auth::from_json(&serde_json::json!([{"id":"private-account-canary","credential_sha256":URL_SAFE_NO_PAD.encode(Sha256::digest(secret.as_bytes())),"demo_microunits":100}]).to_string()).unwrap();
        let (credential, csrf, token) = if api {
            let (bearer, session) = auth
                .authenticate_api(&secret, &auth.issue_api_challenge().unwrap())
                .unwrap();
            let token = auth
                .issue_api_submission(&bearer, "kimi-k3", false)
                .unwrap();
            (bearer, session.csrf, token)
        } else {
            let (id, session) = auth
                .authenticate(&secret, &auth.issue_login_challenge().unwrap())
                .unwrap();
            let token = auth.issue_submission(&id).unwrap();
            (id, session.csrf, token)
        };
        let provider = Arc::new(Provider {
            failure,
            entered: Notify::new(),
            finish: Notify::new(),
            startup_queued: Notify::new(),
            startup_drained: std::sync::Mutex::new(None),
        });
        let state = AppState::new(auth, provider.clone(), "unused", provider.clone())
            .with_telemetry(metrics);
        Self {
            state,
            provider,
            credential,
            csrf,
            token,
            api,
        }
    }
    async fn send(&self) -> axum::response::Response {
        let request = if self.api {
            Request::post("/v1/chat/completions").header("authorization", format!("Bearer {}", self.credential)).header("content-type", "application/json")
                .body(Body::from(serde_json::json!({"model":"kimi-k3","stream":true,"submission":self.token,"messages":[{"role":"user","content":"hostile-prompt-canary"}]}).to_string())).unwrap()
        } else {
            Request::post("/chat").header("cookie", crate::auth::session_cookie(&self.credential)).header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!("csrf={}&token={}&model=kimi-k3&h000000=W10&history_manifest=1.000001.00000002&prompt=hostile-prompt-canary", self.csrf, self.token))).unwrap()
        };
        router(self.state.clone()).oneshot(request).await.unwrap()
    }
    async fn quiescent(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while self.state.available_lanes() != [4, 4, 4, 1, 1] {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("owner cleanup");
    }
}
async fn drain(mut body: Body) {
    while let Some(frame) = body.frame().await {
        drop(frame.unwrap());
    }
}
fn totals(metrics: &AggregateMetrics) -> (u64, u64, u64, u64) {
    let state = metrics.state.lock().unwrap();
    let t = &state.requests.active;
    (
        t.http_starts.iter().sum(),
        t.http_completed.iter().sum(),
        t.generation_starts.iter().sum(),
        t.generation_completed.iter().sum(),
    )
}

#[tokio::test]
async fn web_and_api_terminal_hooks_preserve_accounting_and_duplicate_disposition() {
    for api in [false, true] {
        for (failure, terminal, balance) in [
            (Failure::None, GenerationTerminal::Success, 97),
            (Failure::Preflight, GenerationTerminal::Transport, 100),
            (Failure::Stream, GenerationTerminal::TerminalUsage, 100),
            (Failure::Usage, GenerationTerminal::Settlement, 100),
        ] {
            let metrics = metrics(Deployment::IsolatedSynthetic);
            let f = Fixture::new(api, failure, Some(metrics.clone()));
            let gate = (!matches!(failure, Failure::Preflight)).then(|| Arc::new(Notify::new()));
            *f.provider.startup_drained.lock().unwrap() = gate.clone();
            let response = f.send().await;
            if !matches!(failure, Failure::Preflight) {
                assert_eq!(response.status(), 200);
            }
            if gate.is_some() {
                // Deliberately backlog startup: yielding alone cannot guarantee a drain.
                f.provider.startup_queued.notified().await;
            }
            let consumer = tokio::spawn(async move {
                let mut body = response.into_body();
                let marker = if api {
                    &b"\"role\":\"assistant\""[..]
                } else {
                    &b"</pre><pre aria-label=\"Assistant\">"[..]
                };
                let mut gate = gate;
                while let Some(frame) = body.frame().await {
                    let frame = frame.unwrap();
                    let at_startup = frame.into_data().ok().is_some_and(|data| {
                        data.windows(marker.len()).any(|window| window == marker)
                    });
                    if at_startup {
                        if let Some(gate) = gate.take() {
                            gate.notify_one();
                        }
                    }
                }
                assert!(gate.is_none(), "startup marker missing");
            });
            if !matches!(failure, Failure::Preflight) {
                tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    f.provider.entered.notified(),
                )
                .await
                .expect("fixture startup drain and output");
                assert_eq!(totals(&metrics), (1, 0, 1, 0));
                assert_eq!(f.state.available_lanes(), [3, 3, 4, 1, 1]);
                f.provider.finish.notify_one();
            }
            consumer.await.unwrap();
            f.quiescent().await;
            assert_eq!(
                f.state.accounting.available("private-account-canary"),
                Some(balance)
            );
            assert_eq!(totals(&metrics), (1, 1, 1, 1));
            let duplicate = f.send().await;
            duplicate.into_body().collect().await.unwrap();
            f.quiescent().await;
            assert_eq!(totals(&metrics), (2, 2, 1, 1));
            let state = metrics.state.lock().unwrap();
            let t = &state.requests.active;
            let e = if api {
                Endpoint::ChatApi
            } else {
                Endpoint::ChatWeb
            };
            let g = generation_index(e, QualifiedModel(0)).unwrap();
            assert_eq!(t.generation_completed[g * 12 + terminal as usize], 1);
            assert_eq!(
                t.dispositions
                    .iter()
                    .map(|d| d[Disposition::NewGeneration as usize])
                    .sum::<u64>(),
                1
            );
            assert_eq!(
                t.dispositions
                    .iter()
                    .map(|d| d[Disposition::Duplicate as usize])
                    .sum::<u64>(),
                1
            );
            assert_eq!(t.rejected.iter().sum::<u64>(), 0); // post-reserve failure is not rejection
            let output = t
                .first_output
                .iter()
                .map(|h| h.count().unwrap())
                .sum::<u64>();
            assert_eq!(output, u64::from(!matches!(failure, Failure::Preflight)));
            assert_eq!(
                t.delivery[g * 3 + DeliveryTerminal::Completed as usize],
                output,
                "api={api} terminal={terminal:?} delivery={:?}",
                t.delivery
            );
            assert_eq!(t.contributors[Lane::Heavy as usize], 2);
            assert_eq!(t.contributors[Lane::Ingress as usize], 2);
            assert_eq!(t.contributors[Lane::Generation as usize], 2); // duplicate briefly acquires the real slot
            let formatted = format!("{t:?}");
            for forbidden in [
                "hostile-output-canary",
                "hostile-prompt-canary",
                "private-account-canary",
                &f.credential,
                &f.token,
            ] {
                assert!(!formatted.contains(forbidden));
            }
        }
    }
}

#[tokio::test]
async fn body_disconnect_does_not_finish_generation_or_release_its_permits() {
    for api in [false, true] {
        let metrics = metrics(Deployment::IsolatedSynthetic);
        let f = Fixture::new(api, Failure::None, Some(metrics.clone()));
        let mut body = f.send().await.into_body();
        f.provider.entered.notified().await;
        let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
        let retained = frame.slice(..1);
        drop(frame);
        drop(body);
        assert_eq!(totals(&metrics), (1, 1, 1, 0));
        assert_eq!(f.state.available_lanes(), [3, 3, 4, 1, 1]);
        f.provider.finish.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while f.state.available_lanes()[0] != 4 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(totals(&metrics), (1, 1, 1, 1));
        assert_eq!(f.state.available_lanes()[1], 3); // dequeued slice is still the real heavy owner
        assert_eq!(
            f.state.accounting.available("private-account-canary"),
            Some(97)
        );
        drop(retained);
        f.quiescent().await;
        let state = metrics.state.lock().unwrap();
        let t = &state.requests.active;
        let e = if api {
            Endpoint::ChatApi
        } else {
            Endpoint::ChatWeb
        };
        assert_eq!(
            t.http_completed[http_index(e, Status::Success, HttpTerminal::Unknown)],
            1
        );
        assert_eq!(
            t.delivery[generation_index(e, QualifiedModel(0)).unwrap() * 3
                + DeliveryTerminal::Interrupted as usize],
            1
        );
        assert_eq!(
            state
                .records
                .iter()
                .filter(|r| r.kind == Kind::Lease && !r.terminal)
                .count(),
            0
        );
    }
}

#[tokio::test]
async fn absent_off_and_lost_telemetry_never_gate_generation() {
    for api in [false, true] {
        for mode in [
            None,
            Some(Deployment::Off),
            Some(Deployment::NonIsolated),
            Some(Deployment::IsolatedSynthetic),
        ] {
            let metrics = mode.map(metrics);
            let f = Fixture::new(api, Failure::None, metrics.clone());
            let response = f.send().await;
            f.provider.entered.notified().await;
            // Also simulate telemetry health failure while the actual inference lives.
            if let Some(m) = &metrics {
                m.off();
            }
            drop(response);
            f.provider.finish.notify_one();
            f.quiescent().await;
            assert_eq!(
                f.state.accounting.available("private-account-canary"),
                Some(97)
            );
            if let Some(m) = metrics {
                assert_eq!(totals(&m), (0, 0, 0, 0));
                assert!(m.request().is_none());
            }
        }
    }
}

#[test]
fn owned_conversion_moves_the_key_and_armed_drop_remains_unknown() {
    use crate::{
        accounting::{Accounting, ReserveResult},
        catalog::Quote,
        generation_owner::ReservedGeneration,
    };
    let metrics = metrics(Deployment::IsolatedSynthetic);
    let (http, context) = hooks::HttpObservation::new(Some(&metrics), Endpoint::ChatApi);
    let ledger = Arc::new(Accounting::new([("synthetic".into(), 100)]));
    let quote = Quote {
        model: Model {
            id: "kimi-k3".into(),
            context_tokens: 10,
            max_output_tokens: 9,
            input_microunits_per_million_tokens: 1,
            output_microunits_per_million_tokens: 1,
        },
        input_tokens: 1,
        reserved_microunits: 10,
    };
    assert_eq!(
        ledger
            .reserve(
                "synthetic",
                [1; 32],
                [2; 32],
                quote,
                Instant::now() + std::time::Duration::from_secs(60)
            )
            .unwrap(),
        ReserveResult::Reserved
    );
    let slots = Arc::new(tokio::sync::Semaphore::new(1));
    let heavy = Arc::new(tokio::sync::Semaphore::new(1));
    let owner = ReservedGeneration::new(
        ledger.clone(),
        [1; 32],
        slots.clone().try_acquire_owned().unwrap(),
        Lease::from(heavy.clone().try_acquire_owned().unwrap()),
    )
    .observed(context.generation(QualifiedModel(0)), QualifiedModel(0));
    assert_eq!(totals(&metrics), (1, 0, 1, 0));
    drop(owner);
    drop(http);
    assert_eq!(ledger.available("synthetic"), Some(100));
    assert_eq!(slots.available_permits(), 1);
    assert_eq!(heavy.available_permits(), 1);
    assert_eq!(totals(&metrics), (1, 1, 1, 1));
    let state = metrics.state.lock().unwrap();
    assert_eq!(
        state.requests.active.generation_completed[3 * 12 + GenerationTerminal::Unknown as usize],
        1
    );
}

#[tokio::test]
async fn http_empty_error_and_drop_are_distinct_from_handler_return() {
    for terminal in [
        HttpTerminal::Eof,
        HttpTerminal::Error,
        HttpTerminal::Unknown,
    ] {
        let metrics = metrics(Deployment::IsolatedSynthetic);
        let (observation, _) = hooks::HttpObservation::new(Some(&metrics), Endpoint::Other);
        let body = match terminal {
            HttpTerminal::Eof => Body::empty(),
            HttpTerminal::Error => Body::from_stream(futures_util::stream::once(async {
                Err::<axum::body::Bytes, _>(std::io::Error::other(
                    "synthetic error must not enter metrics",
                ))
            })),
            HttpTerminal::Unknown => Body::from("unconsumed"),
        };
        let mut response = axum::response::Response::new(body);
        *response.status_mut() = axum::http::StatusCode::IM_A_TEAPOT;
        let response = crate::web::observed_response(response, None, observation);
        if terminal != HttpTerminal::Eof {
            assert_eq!(totals(&metrics).1, 0);
        }
        if terminal == HttpTerminal::Unknown {
            drop(response);
        } else {
            let _ = response.into_body().collect().await;
        }
        let state = metrics.state.lock().unwrap();
        assert_eq!(
            state.requests.active.http_completed
                [http_index(Endpoint::Other, Status::ClientError, terminal)],
            1
        );
        assert_eq!(
            state.requests.active.generation_starts.iter().sum::<u64>(),
            0
        );
    }
}

#[tokio::test]
async fn pre_reserve_rejections_and_control_leases_use_closed_labels() {
    let metrics = metrics(Deployment::IsolatedSynthetic);
    let f = Fixture::new(true, Failure::None, Some(metrics.clone()));
    let response = router(f.state.clone())
        .oneshot(
            Request::post("/v1/chat/completions")
                .header("content-type", "application/json")
                .body(Body::from("hostile-body-canary"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    drain(response.into_body()).await;
    assert_eq!(totals(&metrics), (1, 1, 0, 0));
    let control = router(f.state.clone())
        .oneshot(
            Request::get("/claims?hostile-query-canary")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(f.state.available_lanes()[4], 0);
    let rejected = router(f.state.clone())
        .oneshot(Request::get("/claims").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(rejected.status(), 503);
    drain(rejected.into_body()).await;
    drop(control);
    f.quiescent().await;
    let state = metrics.state.lock().unwrap();
    let t = &state.requests.active;
    assert_eq!(t.rejected.iter().sum::<u64>(), 2);
    assert_eq!(
        t.rejected[(Endpoint::ChatApi as usize * 5 + AdmissionModel::Unknown.index()) * 14
            + Rejection::Auth as usize],
        1
    );
    assert_eq!(
        t.rejected[(Endpoint::Claims as usize * 5 + AdmissionModel::NotApplicable.index()) * 14
            + Rejection::RequestCapacity as usize],
        1
    );
    assert_eq!(t.contributors[Lane::Control as usize], 1);
    assert_eq!(
        t.dispositions
            .iter()
            .map(|d| d[Disposition::PreReservationRejected as usize])
            .sum::<u64>(),
        2
    );
}

#[tokio::test]
async fn exhausted_observation_pool_cannot_reject_or_cancel_a_generation() {
    let metrics = metrics(Deployment::IsolatedSynthetic);
    let held: Vec<_> = (0..POOL)
        .map(|_| metrics.http(Endpoint::Home).into_owned(&metrics))
        .collect();
    let f = Fixture::new(true, Failure::None, Some(metrics.clone()));
    let response = f.send().await;
    f.provider.entered.notified().await;
    assert_eq!(metrics.epoch.load(Ordering::SeqCst) % 2, 0);
    drop(response);
    f.provider.finish.notify_one();
    f.quiescent().await;
    assert_eq!(
        f.state.accounting.available("private-account-canary"),
        Some(97)
    );
    drop(held);
    assert!(metrics.request().is_none());
}

#[tokio::test]
async fn production_router_window_reaches_local_tls_only_when_releasable() {
    use opentelemetry_proto::tonic::{
        collector::metrics::v1::ExportMetricsServiceRequest,
        metrics::v1::{metric::Data, number_data_point::Value},
    };
    use prost::Message;
    use tokio::io::AsyncWriteExt;

    fn at(metrics: &mut Arc<AggregateMetrics>, wall_ns: u64) {
        Arc::get_mut(metrics).unwrap().clock.fixed = Some(ClockReading {
            monotonic_ns: wall_ns - 125 * SECOND,
            wall_ns,
        });
    }

    for n in [9, 10] {
        let mut metrics = Arc::new(AggregateMetrics::new(
            Deployment::Production,
            SystemClock {
                start: Instant::now(),
                fixed: Some(ClockReading {
                    monotonic_ns: 0,
                    wall_ns: 125 * SECOND,
                }),
            },
        ));
        for second in 125..=300 {
            at(&mut metrics, second * SECOND + SECOND / 2);
            metrics.poll();
        }
        at(&mut metrics, 301 * SECOND);
        {
            let f = Fixture::new(false, Failure::None, Some(metrics.clone()));
            for _ in 0..n {
                let response = router(f.state.clone())
                    .oneshot(
                        Request::get("/claims?hostile-query-canary")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
                drain(response.into_body()).await;
            }
            f.quiescent().await;
        }
        assert_eq!(totals(&metrics), (n, n, 0, 0));
        // Complete all 300 actual fake-clock polls, without bypassing warmup.
        // These instantaneous control lifetimes end between occupancy samples.
        for second in 301..600 {
            at(&mut metrics, second * SECOND + SECOND / 2);
            metrics.poll();
        }
        at(&mut metrics, 600 * SECOND);
        metrics.poll();
        if n == 9 {
            assert!(metrics.take_window().is_none());
            continue;
        }
        let permit = metrics
            .take_window()
            .expect("eligible production request window");
        assert_eq!(permit.window.start_ns, 300 * SECOND);
        assert_eq!(permit.window.end_ns, 600 * SECOND);
        let handoff::Table::Request(table) = permit.table.as_ref().unwrap() else {
            panic!("unexpected infrastructure window");
        };
        assert_eq!(table.http_started(Endpoint::Claims), Some(10));
        assert_eq!(
            table.http_terminal(Endpoint::Claims, Status::Success, HttpTerminal::Eof),
            Some(10)
        );
        assert_eq!(table.contributors[Lane::Control as usize], 10);

        let (listener, client, acceptor) = runtime::tests::fixture(true, "api.honeycomb.io").await;
        let receive = async {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = acceptor.accept(stream).await.unwrap();
            let bytes = runtime::tests::capture(&mut stream).await;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
            bytes
        };
        let (sent, bytes) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            tokio::join!(
                export::handoff::send(permit, client, Default::default()),
                receive
            )
        })
        .await
        .unwrap();
        assert!(sent.is_ok());
        for sentinel in [
            b"hostile-query-canary".as_slice(),
            b"private-account-canary",
            b"synthetic-key-canary",
        ] {
            assert!(!bytes.windows(sentinel.len()).any(|value| value == sentinel));
        }
        let decoded = ExportMetricsServiceRequest::decode(bytes.as_slice()).unwrap();
        let wire = &decoded.resource_metrics[0].scope_metrics[0].metrics;
        let mut names: Vec<_> = wire.iter().map(|metric| metric.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "possums.admission.occupancy.bucket",
                "possums.http.completed",
                "possums.http.duration.bucket",
                "possums.http.requests",
            ]
        );
        for metric in wire {
            let Some(Data::Sum(sum)) = &metric.data else {
                panic!("expected delta count encoding");
            };
            let total: i64 = sum
                .data_points
                .iter()
                .map(|point| {
                    assert_eq!(point.start_time_unix_nano, 300 * SECOND);
                    assert_eq!(point.time_unix_nano, 600 * SECOND);
                    let Some(Value::AsInt(value)) = point.value else {
                        panic!("expected integer count");
                    };
                    value
                })
                .sum();
            assert_eq!(
                total,
                if metric.name == "possums.admission.occupancy.bucket" {
                    300
                } else {
                    10
                }
            );
        }
        assert!(metrics.take_window().is_none());
    }
}
