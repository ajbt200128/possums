use super::*;
use crate::{failures::InterfaceError, telemetry::errors::ErrorTotals};

fn populated() -> RequestTables {
    let mut t = RequestTables {
        ticks: 300,
        extension_qualification: [true; 2],
        ..Default::default()
    };
    for error in InterfaceError::ALL {
        t.errors.observe(error);
    }
    for (_, model) in MODELS {
        t.tokens.observe(model, 100, 20, 120, Some(0));
    }
    t
}
fn exact_extensions() -> WirePoints {
    let mut out = WirePoints::new();
    for error in InterfaceError::ALL {
        add_exact(
            &mut out,
            "possums.interface.errors",
            "{error}",
            &[("error_type", error.label())],
            1,
            W,
        );
    }
    for (model, _) in MODELS {
        for (name, value) in [
            ("possums.tokens.input", 100),
            ("possums.tokens.output", 20),
            ("possums.tokens.cached", 0),
        ] {
            add_exact(
                &mut out,
                name,
                "{token}",
                &[("possums.model", model)],
                value,
                W,
            );
        }
    }
    out
}

#[tokio::test]
async fn full_closed_extension_wire_and_runtime_conversion() {
    if !clean_child("telemetry::export::materialize::tests::extensions::full_closed_extension_wire_and_runtime_conversion") { return; }
    let t = populated();
    let expected = exact_extensions();
    assert_eq!(expected.len(), 63);
    assert_eq!(points(&wire(request(&t, W).unwrap()).await), expected);
    // Exercise the production encoder as well as the supported exporter above.
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let client = super::super::super::Client::new(listener.local_addr().unwrap()).unwrap();
    let metrics = request(&t, W).unwrap();
    let peer = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let length = capture_header(&mut stream, true).await;
        assert!(length <= super::super::super::transport::MAX_OUTBOUND);
        let mut bytes = vec![0; length];
        stream.read_exact(&mut bytes).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        bytes
    };
    let (result, bytes) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(
            super::super::super::encode_and_send(&client, &metrics),
            peer
        )
    })
    .await
    .unwrap();
    assert!(result.is_ok());
    client.cancel().await;
    assert_eq!(points(&bytes), expected);
    for sentinel in [
        "synthetic-key-canary",
        "credential",
        "account_id",
        "cache_scope",
        "request_id",
        "https://",
    ] {
        assert!(!bytes
            .windows(sentinel.len())
            .any(|b| b == sentinel.as_bytes()));
    }
}

#[tokio::test]
async fn independent_withholding_and_population_wire() {
    if !clean_child("telemetry::export::materialize::tests::extensions::independent_withholding_and_population_wire") { return; }
    for variant in 0..9 {
        let mut t = populated();
        let mut exact = exact_extensions();
        match variant {
            0 => {
                t.valid = false;
            }
            1 => {
                t.http_completed[0] = 1;
            } // unmatched old lifecycle
            2 => {
                t.http_starts[0] = i64::MAX as u64 + 1;
            }
            3 => {
                t.rejected[0] = 1;
            } // invalid old rejection pair
            4 => {
                t.tokens.observe(QualifiedModel(1), 1, 0, 1, None);
                exact.retain(|(n, _), _| n != "possums.tokens.cached");
                for ((n, a), (_, value, _, _)) in &mut exact {
                    if n == "possums.tokens.input" && a[0].1 == "glm-5-3" {
                        *value += 1;
                    }
                }
            }
            5 => {
                t.tokens.observe(QualifiedModel(0), 1, 1, 3, Some(0));
                exact.retain(|(n, _), _| !n.starts_with("possums.tokens."));
            }
            6 => {
                t.errors = ErrorTotals::with_counts([i64::MAX as u64; 54]);
                t.errors.observe(InterfaceError::BodyLimit);
                exact.retain(|(n, _), _| n != "possums.interface.errors");
            }
            7 => {
                t.tokens = Default::default();
                t.tokens.observe(QualifiedModel(0), 0, 0, 0, Some(0));
                exact.retain(|(n, _), _| !n.starts_with("possums.tokens."));
                for name in [
                    "possums.tokens.input",
                    "possums.tokens.output",
                    "possums.tokens.cached",
                ] {
                    add_exact(
                        &mut exact,
                        name,
                        "{token}",
                        &[("possums.model", "kimi-k3")],
                        0,
                        W,
                    );
                }
            }
            _ => {
                t.tokens.observe(QualifiedModel(2), 1, 0, 1, Some(2));
                exact.retain(|(n, _), _| !n.starts_with("possums.tokens."));
            }
        }
        assert_eq!(
            points(&wire(request(&t, W).unwrap()).await),
            exact,
            "variant {variant}"
        );
    }
    for (gates, count) in [([true, false], 54), ([false, true], 9)] {
        let mut t = populated();
        t.extension_qualification = gates;
        assert_eq!(points(&wire(request(&t, W).unwrap()).await).len(), count);
    }
    let mut t = populated();
    t.extension_qualification = [false; 2];
    assert!(request(&t, W).is_none());
    t.http_starts[0] = 1;
    let p = points(&wire(request(&t, W).unwrap()).await);
    assert_eq!(p.len(), 1);
    assert_eq!(p.keys().next().unwrap().0, "possums.http.requests");
    let mut t = cohort(10, false, 0);
    t.extension_qualification = [true; 2];
    t.errors = ErrorTotals::with_counts([i64::MAX as u64; 54]);
    t.errors.observe(InterfaceError::BodyLimit);
    t.tokens.observe(QualifiedModel(0), 1, 1, 3, Some(0));
    assert_eq!(
        points(&wire(request(&t, W).unwrap()).await),
        expected(10, false, 0)
    );
    let empty = RequestTables {
        ticks: 300,
        extension_qualification: [true; 2],
        ..Default::default()
    };
    assert!(request(&empty, W).is_none());
    for defect in 0..3 {
        let mut t = populated();
        match defect {
            0 => t.window_valid = false,
            1 => t.ticks = 299,
            _ => t.ticks = 301,
        }
        assert!(request(&t, W).is_none());
    }
}

#[tokio::test]
async fn signed_integer_limits_and_across_partition_overflow_wire() {
    if !clean_child("telemetry::export::materialize::tests::extensions::signed_integer_limits_and_across_partition_overflow_wire") { return; }
    for output in [false, true] {
        let mut t = RequestTables {
            ticks: 300,
            extension_qualification: [true; 2],
            ..Default::default()
        };
        let max = i64::MAX as u64;
        let mut counts = [0; 54];
        counts[InterfaceError::BodyLimit as usize] = max;
        t.errors = ErrorTotals::with_counts(counts);
        let (input, out) = if output { (0, max) } else { (max, 0) };
        t.tokens
            .observe(QualifiedModel(0), input, out, max, Some(input));
        let p = points(&wire(request(&t, W).unwrap()).await);
        assert_eq!(p.len(), 4);
        assert_eq!(
            p[&(
                "possums.interface.errors".into(),
                vec![("error_type".into(), "body_limit".into())]
            )]
                .1,
            i64::MAX
        );
        for (name, value) in [
            ("possums.tokens.input", input),
            ("possums.tokens.output", out),
            ("possums.tokens.cached", input),
        ] {
            assert_eq!(
                p[&(
                    name.into(),
                    vec![("possums.model".into(), "kimi-k3".into())]
                )]
                    .1,
                value as i64
            );
        }
        // Across-type overflow withholds errors independently of valid tokens.
        t.errors.observe(InterfaceError::HeaderLimit);
        assert_eq!(points(&wire(request(&t, W).unwrap()).await).len(), 3);
        // Across-model overflow withholds all token points, not a partial subtotal.
        t.tokens.observe(
            QualifiedModel(1),
            u64::from(!output),
            u64::from(output),
            1,
            Some(0),
        );
        assert!(request(&t, W).is_none());
    }
    for value in [i64::MAX as u64 + 1, u64::MAX] {
        let mut t = populated();
        t.tokens.observe(QualifiedModel(0), value, 0, value, None);
        let p = points(&wire(request(&t, W).unwrap()).await);
        assert_eq!(p.len(), 54);
        assert!(p.keys().all(|(n, _)| n == "possums.interface.errors"));
    }
    // Pin the actual supported 0.29 conversion's dangerous fallback, not a duplicate parser.
    assert_eq!(Value::from(i64::MAX as u64), Value::AsInt(i64::MAX));
    assert_eq!(Value::from(i64::MAX as u64 + 1), Value::AsInt(0));
}
