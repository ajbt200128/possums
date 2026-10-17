use super::*;
use std::sync::atomic::Ordering::SeqCst;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const STAGES: [Stage; 8] = [
    Stage::Configuration,
    Stage::Accounts,
    Stage::Signal,
    Stage::TorPreparation,
    Stage::Connect,
    Stage::Bind,
    Stage::TorBootstrap,
    Stage::TorLaunch,
];
const HOSTILE: &str = "HOSTILE_SECRET_URL_ACCOUNT_ONION_PANIC";

fn off() -> Config {
    Config::from_lookup(|_| None)
}
fn records(outcome: Outcome) -> Vec<Observation> {
    vec![
        Observation {
            stage: Stage::Configuration,
            outcome: None,
            time: 10,
        },
        Observation {
            stage: Stage::Bind,
            outcome: Some(outcome),
            time: 20,
        },
    ]
}
fn text(value: &AnyValue) -> &str {
    let Some(Value::StringValue(value)) = &value.value else {
        panic!("not text");
    };
    value
}
fn expected_resource() -> Resource {
    Resource {
        attributes: [
            ("service.name", "possums-gateway"),
            ("deployment.environment.name", "production"),
            ("possums.gateway.slot", "gateway-01"),
        ]
        .into_iter()
        .map(|(key, value)| KeyValue {
            key: key.into(),
            value: Some(AnyValue {
                value: Some(Value::StringValue(value.into())),
            }),
        })
        .collect(),
        dropped_attributes_count: 0,
    }
}

#[test]
fn official_protobuf_is_exact_and_has_no_automatic_metadata_or_ids() {
    let observations = records(Outcome::Failed(Failure::BindUnknown));
    let bytes = logs(&observations).encode_to_vec();
    let message = ExportLogsServiceRequest::decode(bytes.as_slice()).unwrap();
    assert_eq!(message.resource_logs.len(), 1);
    let resource = &message.resource_logs[0];
    assert_eq!(resource.resource, Some(expected_resource()));
    assert_eq!(resource.schema_url, "");
    assert_eq!(resource.scope_logs.len(), 1);
    let scope = &resource.scope_logs[0];
    assert_eq!(
        scope.scope,
        Some(InstrumentationScope {
            name: "possums.telemetry".into(),
            ..Default::default()
        })
    );
    assert_eq!(scope.schema_url, "");
    assert_eq!(scope.log_records.len(), 2);
    for (i, record) in scope.log_records.iter().enumerate() {
        let expected_body = if i == 0 {
            "event=possums.startup stage=configuration outcome=entered code=startup_stage_entered constraint=initialization_only action=wait_for_terminal_initialization_outcome"
        } else {
            "event=possums.startup stage=bind outcome=failed code=startup_bind_unknown constraint=tcp_listener_bind_cause_unknown action=check_approved_listener_configuration_offline cause=unknown"
        };
        assert_eq!(text(record.body.as_ref().unwrap()), expected_body);
        let attrs: Vec<_> = record
            .attributes
            .iter()
            .map(|a| (a.key.as_str(), text(a.value.as_ref().unwrap())))
            .collect();
        assert_eq!(
            attrs,
            if i == 0 {
                vec![
                    ("stage", "configuration"),
                    ("outcome", "entered"),
                    ("code", "startup_stage_entered"),
                    ("constraint", "initialization_only"),
                    ("action", "wait_for_terminal_initialization_outcome"),
                ]
            } else {
                vec![
                    ("stage", "bind"),
                    ("outcome", "failed"),
                    ("code", "startup_bind_unknown"),
                    ("constraint", "tcp_listener_bind_cause_unknown"),
                    ("action", "check_approved_listener_configuration_offline"),
                ]
            }
        );
        // Equality with generated defaults pins every field, not just an ID scan.
        assert_eq!(
            *record,
            LogRecord {
                time_unix_nano: if i == 0 { 10 } else { 20 },
                observed_time_unix_nano: if i == 0 { 10 } else { 20 },
                severity_number: if i == 0 {
                    SeverityNumber::Info as i32
                } else {
                    SeverityNumber::Error as i32
                },
                severity_text: if i == 0 { "INFO" } else { "ERROR" }.into(),
                body: record.body.clone(),
                attributes: record.attributes.clone(),
                event_name: "possums.startup".into(),
                ..Default::default()
            }
        );
    }
    assert_eq!(bytes, message.encode_to_vec());

    let bytes = metrics(&observations).encode_to_vec();
    let message = ExportMetricsServiceRequest::decode(bytes.as_slice()).unwrap();
    assert_eq!(
        message,
        ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: Some(expected_resource()),
                scope_metrics: vec![ScopeMetrics {
                    scope: Some(InstrumentationScope {
                        name: "possums.telemetry".into(),
                        ..Default::default()
                    }),
                    metrics: vec![Metric {
                        name: "possums.startup.transitions".into(),
                        unit: "{transition}".into(),
                        data: Some(Data::Sum(Sum {
                            aggregation_temporality: AggregationTemporality::Delta as i32,
                            is_monotonic: true,
                            data_points: [("configuration", "entered"), ("bind", "failed")]
                                .into_iter()
                                .map(|(stage, outcome)| NumberDataPoint {
                                    attributes: vec![
                                        attribute("stage", stage),
                                        attribute("outcome", outcome)
                                    ],
                                    start_time_unix_nano: 10,
                                    time_unix_nano: 20,
                                    value: Some(number_data_point::Value::AsInt(1)),
                                    ..Default::default()
                                })
                                .collect(),
                        })),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }]
        }
    );
    assert_eq!(bytes, message.encode_to_vec());
}

#[tokio::test]
async fn finite_stages_terminal_once_and_off_keeps_platform_observations() {
    for outcome in [
        Outcome::InitializationComplete,
        Outcome::Interrupted,
        Outcome::Failed(Failure::AccountsInvalid),
    ] {
        let mut startup = Startup::new(off());
        assert!(startup.active());
        for _ in 0..100 {
            for stage in STAGES {
                startup.enter(stage);
            }
        }
        let state = startup.state.as_ref().unwrap();
        assert_eq!(state.records.len(), MAX_RECORDS - 1);
        assert_eq!(state.seen, u8::MAX);
        assert_eq!(state.current, Stage::TorLaunch);
        for (record, stage) in state.records.iter().zip(STAGES) {
            assert_eq!(record.stage, stage);
            assert_eq!(record.outcome, None);
        }
        startup.finish(outcome).await;
        assert!(!startup.active());
        assert!(startup.state.is_none());
        startup.enter(Stage::Configuration);
        startup.finish(Outcome::Failed(Failure::BindUnknown)).await;
        assert!(startup.state.is_none());
    }
}

#[test]
fn known_failures_and_stage_specific_unknowns_keep_safe_evidence() {
    let failures = [
        Failure::ConfigurationMissing,
        Failure::ConfigurationEncodingInvalid,
        Failure::AccountsInvalid,
        Failure::SignalUnknown,
        Failure::BindUnknown,
        Failure::Connect(InferenceFailure::VerificationFailed),
        Failure::Connect(InferenceFailure::CatalogFailed),
        Failure::Connect(InferenceFailure::TokenizerSendDeadline),
        Failure::Connect(InferenceFailure::TokenizerConnectFailed),
        Failure::Tor(TorStartupFailure::Configuration),
        Failure::Tor(TorStartupFailure::FeatureUnavailable),
        Failure::Tor(TorStartupFailure::GrantRead),
        Failure::Tor(TorStartupFailure::GrantInvalid),
        Failure::Tor(TorStartupFailure::GrantMismatch),
        Failure::Tor(TorStartupFailure::IdentityFileInvalid),
        Failure::Tor(TorStartupFailure::IdentitySecretInvalid),
        Failure::Tor(TorStartupFailure::State),
        Failure::Tor(TorStartupFailure::Client),
        Failure::Tor(TorStartupFailure::Bootstrap),
    ];
    for failure in failures {
        let record = Observation::new(Stage::Connect, Some(Outcome::Failed(failure)));
        let body = record.to_string();
        for field in [
            "event=possums.startup",
            "stage=",
            "outcome=failed",
            "code=",
            "constraint=",
            "action=",
        ] {
            assert!(body.contains(field));
        }
        assert!(!body.contains(HOSTILE));
        assert!(body.len() < 2048);
        let message = logs(&[record]);
        assert_eq!(
            text(
                message.resource_logs[0].scope_logs[0].log_records[0]
                    .body
                    .as_ref()
                    .unwrap()
            ),
            body
        );
    }
    assert!(Failure::Connect(InferenceFailure::TokenizerSendDeadline)
        .to_string()
        .contains("cause=TokenizerSendDeadline"));
    assert!(Failure::Connect(InferenceFailure::TokenizerConnectFailed)
        .to_string()
        .contains("cause=TokenizerConnectFailed"));
    for failure in [Failure::SignalUnknown, Failure::BindUnknown] {
        assert!(failure.to_string().contains("cause=unknown"));
    }
    for outcome in [Outcome::InitializationComplete, Outcome::Interrupted] {
        let record = Observation::new(Stage::Bind, Some(outcome));
        assert!(!record.to_string().contains("outcome=failed"));
        if outcome == Outcome::InitializationComplete {
            assert!(record.to_string().contains("not_verified_serving"));
        }
    }
}

#[cfg(feature = "tor")]
#[test]
fn nested_tor_codes_are_preserved_without_appraisal_status() {
    use crate::{server::ServerFailure, tor_ingress};
    let cases = [
        (
            Failure::Tor(TorStartupFailure::Identity(
                tor_ingress::Failure::IdentityMismatch,
            )),
            "code=tor_identity_mismatch",
        ),
        (
            Failure::TorLaunch(tor_ingress::ServeFailure {
                source: Some(tor_ingress::Failure::Launch),
                shared: Some(ServerFailure::ShutdownIntegrity),
                reporting: Some(tor_ingress::ReportingFailure {
                    original: tor_ingress::Failure::Disabled,
                }),
            }),
            "code=tor_reporter_unknown",
        ),
    ];
    for (failure, code) in cases {
        let record = Observation::new(Stage::TorLaunch, Some(Outcome::Failed(failure)));
        let body = record.to_string();
        assert!(body.contains(code));
        assert!(body.len() < 2048);
        assert!(!body.contains(HOSTILE));
        let bytes = logs(&[record]).encode_to_vec();
        assert!(bytes
            .windows(code.len())
            .any(|window| window == code.as_bytes()));
        // Status never becomes a startup metric dimension.
        assert!(!String::from_utf8_lossy(&metrics(&[record]).encode_to_vec()).contains(code));
    }
    assert!(!Failure::Tor(TorStartupFailure::Bootstrap)
        .to_string()
        .contains("status="));
}

async fn capture<S: AsyncRead + Unpin>(stream: &mut S, path: &str) -> Vec<u8> {
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        assert!(header.len() < 4096);
        header.push(stream.read_u8().await.unwrap());
    }
    let header = String::from_utf8(header).unwrap();
    assert!(header.starts_with(&format!("POST {path} HTTP/1.1\r\n")));
    let mut length = None;
    for line in header.split("\r\n").skip(1).filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(": ").unwrap();
        match name {
            "host" => assert_eq!(value, "api.honeycomb.io"),
            "content-type" => assert_eq!(value, "application/x-protobuf"),
            "connection" => assert_eq!(value, "close"),
            "x-honeycomb-team" => assert_eq!(value, "synthetic-key-canary"),
            "content-length" => length = Some(value.parse::<usize>().unwrap()),
            _ => panic!("unexpected header"), // includes dataset/user-agent/tracing
        }
    }
    assert!(header.contains("x-honeycomb-team: synthetic-key-canary\r\n"));
    let mut bytes = vec![0; length.unwrap()];
    assert!(bytes.len() < 32 * 1024);
    stream.read_exact(&mut bytes).await.unwrap();
    bytes
}

#[tokio::test]
async fn actual_finish_uses_normal_routing_logs_then_metrics_once() {
    let (listener, client, acceptor) =
        super::super::runtime::tests::fixture(true, "api.honeycomb.io").await;
    let evidence = client.evidence();
    let peer = async {
        let mut bodies = Vec::new();
        for path in ["/v1/logs", "/v1/metrics"] {
            let (io, _) = listener.accept().await.unwrap();
            let mut io = acceptor.accept(io).await.unwrap();
            bodies.push(capture(&mut io, path).await);
            io.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        }
        bodies
    };
    let mut startup = Startup::new(off());
    startup.client = Some(client);
    for stage in STAGES {
        startup.enter(stage);
        startup.enter(stage);
    }
    let (_, bodies) = tokio::join!(startup.finish(Outcome::InitializationComplete), peer);
    assert!(!startup.active());
    assert_eq!(evidence.connections.load(SeqCst), 2);
    assert_eq!(evidence.live_io.load(SeqCst), 0);
    let logs = ExportLogsServiceRequest::decode(bodies[0].as_slice()).unwrap();
    let records = &logs.resource_logs[0].scope_logs[0].log_records;
    assert_eq!(records.len(), MAX_RECORDS);
    assert!(text(records.last().unwrap().body.as_ref().unwrap())
        .contains("outcome=initialization_complete"));
    let metrics = ExportMetricsServiceRequest::decode(bodies[1].as_slice()).unwrap();
    let Some(Data::Sum(sum)) = &metrics.resource_metrics[0].scope_metrics[0].metrics[0].data else {
        panic!("not sum");
    };
    assert_eq!(sum.data_points.len(), MAX_RECORDS);
    assert!(sum
        .data_points
        .iter()
        .all(|p| p.value == Some(number_data_point::Value::AsInt(1))
            && p.attributes.len() == 2
            && p.exemplars.is_empty()));
    let cells: std::collections::HashSet<_> = sum
        .data_points
        .iter()
        .map(|p| {
            p.attributes
                .iter()
                .map(|a| text(a.value.as_ref().unwrap()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(cells.len(), MAX_RECORDS);
    startup.enter(Stage::Connect);
    startup
        .finish(Outcome::Failed(Failure::SignalUnknown))
        .await;
    assert_eq!(evidence.connections.load(SeqCst), 2);
}

async fn response_result(signal: Signal, status: u16, body: Vec<u8>) -> Result<(), Delivery> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let client = Client::new(listener.local_addr().unwrap())
        .unwrap()
        .for_signal(signal);
    let peer = async {
        let (mut io, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(io.read_u8().await.unwrap());
        }
        let header = String::from_utf8(header).unwrap();
        let length: usize = header
            .lines()
            .find_map(|line| line.strip_prefix("content-length: "))
            .unwrap()
            .parse()
            .unwrap();
        let mut bytes = vec![0; length];
        io.read_exact(&mut bytes).await.unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(HOSTILE));
        let response = format!(
            "HTTP/1.1 {status} Synthetic\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        io.write_all(response.as_bytes()).await.unwrap();
        let _ = io.write_all(&body).await;
    };
    let observations = records(Outcome::Interrupted);
    let deadline = tokio::time::Instant::now() + BUDGET;
    let (result, _) = tokio::join!(send(&client, signal, &observations, deadline), peer);
    assert_eq!(client.evidence().live_io.load(SeqCst), 0);
    result
}

#[tokio::test]
async fn official_otlp_responses_reject_partial_warnings_and_hostile_bodies() {
    use opentelemetry_proto::tonic::collector::{
        logs::v1::ExportLogsPartialSuccess, metrics::v1::ExportMetricsPartialSuccess,
    };
    for signal in [Signal::Logs, Signal::Metrics] {
        assert!(response_result(signal, 200, vec![]).await.is_ok());
        assert!(response_result(signal, 200, vec![10, 0]).await.is_ok()); // empty partial_success
        for rejected in [0, 1, -1] {
            let error_message = if rejected == 0 {
                HOSTILE.into()
            } else {
                String::new()
            };
            let body = match signal {
                Signal::Logs => ExportLogsServiceResponse {
                    partial_success: Some(ExportLogsPartialSuccess {
                        rejected_log_records: rejected,
                        error_message,
                    }),
                }
                .encode_to_vec(),
                Signal::Metrics => ExportMetricsServiceResponse {
                    partial_success: Some(ExportMetricsPartialSuccess {
                        rejected_data_points: rejected,
                        error_message,
                    }),
                }
                .encode_to_vec(),
            };
            let error = response_result(signal, 200, body).await.err().unwrap();
            assert_eq!(error.constraint, "otlp_partial_success_or_warning");
            assert!(!error.to_string().contains(HOSTILE));
        }
        for status in [200, 201, 204, 302, 401, 429, 500] {
            let error = response_result(signal, status, HOSTILE.as_bytes().to_vec())
                .await
                .err()
                .unwrap();
            assert_eq!(
                error.constraint,
                if status == 200 {
                    "otlp_response_invalid"
                } else {
                    "http_response_rejected"
                }
            );
            assert!(!error.to_string().contains(HOSTILE));
            assert!(error
                .to_string()
                .contains("original startup outcome unchanged"));
        }
        assert!(
            response_result(signal, 200, vec![0; transport::RESPONSE_LIMIT + 1])
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn malformed_config_and_off_do_no_transport_work_or_content_encoding() {
    for disabled in [None, Some("true"), Some(HOSTILE)] {
        let config = Config::from_lookup(|key| match key {
            "OTEL_SDK_DISABLED" => disabled.map(String::from),
            "OTEL_EXPORTER_OTLP_ENDPOINT" => Some(HOSTILE.into()),
            _ => Some(format!("x-honeycomb-team={HOSTILE}")),
        });
        assert!(!config.enabled());
        assert!(!format!("{config:?}").contains(HOSTILE));
        let mut startup = Startup::new(config);
        startup.enter(Stage::Configuration);
        startup
            .finish(Outcome::Failed(Failure::ConfigurationEncodingInvalid))
            .await;
        assert!(!startup.active());
    }
    let valid = Config::from_lookup(|key| match key {
        "OTEL_EXPORTER_OTLP_ENDPOINT" => Some("https://api.honeycomb.io".into()),
        "OTEL_EXPORTER_OTLP_HEADERS" => Some(format!("x-honeycomb-team={HOSTILE}")),
        _ => None,
    });
    assert!(valid.enabled());
    assert!(!format!("{valid:?}").contains(HOSTILE));
    let client = valid.into_client().unwrap().unwrap(); // constructs TLS only; no dialing
    assert!(!format!("{client:?}").contains(HOSTILE));
    assert_eq!(client.endpoint(), "https://api.honeycomb.io/v1/metrics");
    assert_eq!(
        client.clone().for_signal(Signal::Logs).endpoint(),
        "https://api.honeycomb.io/v1/logs"
    );
    assert_eq!(client.evidence().connecting.load(SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn expired_shared_deadline_never_opens_another_connection() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let client = Client::new(listener.local_addr().unwrap()).unwrap();
    let failure = send(
        &client,
        Signal::Metrics,
        &records(Outcome::Interrupted),
        tokio::time::Instant::now(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(failure.constraint, "shared_delivery_deadline");
    assert_eq!(client.evidence().connecting.load(SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn shared_deadline_and_cancelled_finish_drop_owned_work_and_never_reopen() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let mut client = Client::new(listener.local_addr().unwrap()).unwrap();
    client.stall_connect = true;
    let evidence = client.evidence();
    let start = tokio::time::Instant::now();
    let failure = deliver(client.clone(), &records(Outcome::Interrupted))
        .await
        .err()
        .unwrap();
    assert_eq!(tokio::time::Instant::now() - start, BUDGET);
    assert_eq!(failure.constraint, "signal_delivery_deadline");
    assert_eq!(evidence.connecting.load(SeqCst), 2);
    assert_eq!(evidence.live_io.load(SeqCst), 0);

    for (index, outcome) in [
        Outcome::Failed(Failure::AccountsInvalid),
        Outcome::InitializationComplete,
    ]
    .into_iter()
    .enumerate()
    {
        let mut startup = Startup::new(off());
        startup.client = Some(client.fresh());
        let mut finish = Box::pin(startup.finish(outcome));
        assert!(futures_util::poll!(&mut finish).is_pending());
        // A later shutdown/cancellation cannot retroactively change a terminal
        // local-initialization snapshot already emitted before the first await.
        drop(finish);
        assert!(!startup.active());
        startup.enter(Stage::Connect);
        startup.finish(Outcome::Interrupted).await;
        assert_eq!(evidence.connecting.load(SeqCst), index + 3);
        assert_eq!(evidence.live_io.load(SeqCst), 0);
    }
}

#[tokio::test]
async fn second_signal_gets_its_own_budget_and_preserves_first_failure() {
    for rejected in [false, true] {
        let (listener, client, acceptor) =
            super::super::runtime::tests::fixture(true, "api.honeycomb.io").await;
        let evidence = client.evidence();
        let peer = async {
            let (io, _) = listener.accept().await.unwrap();
            let mut io = acceptor.accept(io).await.unwrap();
            capture(&mut io, "/v1/logs").await;
            tokio::time::sleep(Duration::from_millis(450)).await;
            let status = if rejected { 500 } else { 200 };
            let body = if rejected { HOSTILE } else { "" };
            let header = format!("HTTP/1.1 {status} Synthetic\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            io.write_all(header.as_bytes()).await.unwrap();
            drop(io);
            let (io, _) = listener.accept().await.unwrap();
            let mut io = acceptor.accept(io).await.unwrap();
            let payload = capture(&mut io, "/v1/metrics").await;
            assert!(!String::from_utf8_lossy(&payload).contains(HOSTILE));
            // No response: the signal deadline must dispose the real TLS stream.
            assert!(io.read_u8().await.is_err());
        };
        let observations = records(Outcome::Failed(Failure::BindUnknown));
        let start = tokio::time::Instant::now();
        let (result, ()) = tokio::join!(deliver(client, &observations), peer);
        assert!(start.elapsed() < BUDGET + Duration::from_millis(350));
        let failure = result.err().unwrap();
        if rejected {
            assert!(matches!(failure.signal, Signal::Logs));
            assert_eq!(failure.constraint, "http_response_rejected");
        } else {
            assert!(matches!(failure.signal, Signal::Metrics));
            assert_eq!(failure.constraint, "signal_delivery_deadline");
        }
        assert!(!failure.to_string().contains(HOSTILE));
        assert_eq!(evidence.connecting.load(SeqCst), 2);
        assert_eq!(evidence.live_io.load(SeqCst), 0);
        assert!(observations[1].to_string().contains("startup_bind_unknown"));
    }
}

#[tokio::test]
async fn logs_timeout_does_not_starve_metrics_of_its_only_attempt() {
    let (listener, client, acceptor) =
        super::super::runtime::tests::fixture(true, "api.honeycomb.io").await;
    let evidence = client.evidence();
    let peer = async {
        let (io, _) = listener.accept().await.unwrap();
        let mut io = acceptor.accept(io).await.unwrap();
        capture(&mut io, "/v1/logs").await;
        // Withhold the log response through its deadline. The owned TLS stream
        // must close before the independent metrics attempt is allowed to start.
        assert!(io.read_u8().await.is_err());
        drop(io);
        let next = tokio::time::timeout(Duration::from_millis(500), listener.accept()).await;
        let Ok(Ok((io, _))) = next else {
            return false;
        };
        let mut io = acceptor.accept(io).await.unwrap();
        let body = capture(&mut io, "/v1/metrics").await;
        ExportMetricsServiceRequest::decode(body.as_slice()).unwrap();
        io.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        true
    };
    let observations = records(Outcome::Failed(Failure::BindUnknown));
    let (result, metrics_attempted) = tokio::time::timeout(Duration::from_secs(4), async {
        tokio::join!(deliver(client, &observations), peer)
    })
    .await
    .unwrap();
    assert!(
        metrics_attempted,
        "logs exhausted the only metrics delivery opportunity"
    );
    let failure = result.err().unwrap();
    assert!(matches!(failure.signal, Signal::Logs));
    assert_eq!(failure.constraint, "signal_delivery_deadline");
    assert_eq!(evidence.connecting.load(SeqCst), 2);
    assert_eq!(evidence.live_io.load(SeqCst), 0);
    assert!(observations[1].to_string().contains("startup_bind_unknown"));
}

#[tokio::test]
async fn two_individually_timely_tls_signals_both_complete() {
    let (listener, client, acceptor) =
        super::super::runtime::tests::fixture(true, "api.honeycomb.io").await;
    let evidence = client.evidence();
    let peer = async {
        for path in ["/v1/logs", "/v1/metrics"] {
            let (io, _) = listener.accept().await.unwrap();
            let mut io = acceptor.accept(io).await.unwrap();
            capture(&mut io, path).await;
            tokio::time::sleep(Duration::from_millis(600)).await;
            let _ = io
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
        }
    };
    let observations = records(Outcome::Interrupted);
    let (result, ()) = tokio::time::timeout(Duration::from_secs(4), async {
        tokio::join!(deliver(client, &observations), peer)
    })
    .await
    .unwrap();
    assert!(
        result.is_ok(),
        "individually timely signals lost their delivery opportunity"
    );
    assert_eq!(evidence.connections.load(SeqCst), 2);
    assert_eq!(evidence.live_io.load(SeqCst), 0);
}

#[tokio::test]
async fn outage_does_not_replace_failure_or_retry_either_signal() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let client = Client::new(listener.local_addr().unwrap()).unwrap();
    let evidence = client.evidence();
    drop(listener);
    let failure = deliver(client, &records(Outcome::Failed(Failure::BindUnknown)))
        .await
        .err()
        .unwrap();
    assert_eq!(failure.constraint, "transport_cause_unknown");
    assert_eq!(evidence.connecting.load(SeqCst), 2);
    assert_eq!(evidence.live_io.load(SeqCst), 0);
}
