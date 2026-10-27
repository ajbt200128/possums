# Intake response-read interruption: HTTP/2 mitigation

## Status and observed failure

Source/offline qualified against gateway baseline `b1aebe8b`; **not deployed or
installed**. This is a mitigation for an observed transport path, not proof that
the intake origin or an intermediary has been repaired.

The original failure carried `INTAKE_REQUEST_UNAVAILABLE`, operation
`intake_refresh`, stage `transport`, constraint `response_read_unknown`, observed
intake HTTP 200 and gateway HTTP 503. HTTP 200 established receipt of headers,
not completion or authenticity of the response body. No authenticated terminal
receipt was available for that failed turn; its billing outcome remains unknown.
A graceful Pi host stop did not establish completion of the requested work.

Authorized live native-intake probes reproduced this boundary repeatedly:
HTTP/1.1, chunked framing, a nonempty body and typed
`std::io::ErrorKind::UnexpectedEof` during body consumption. One interrupted body
was syntactically complete JSON, but transport completion was missing. Fresh
connections also failed, so connection reuse alone does not explain the issue.
The producing component and underlying reason for interruption remain unknown;
this evidence does not attribute the fault to an origin, proxy or TLS library.

## Minimal source change

- Enable reqwest 0.13.5's supported `http2` feature. The lockfile adds the already
  resolved `h2` dependency to reqwest; no package version changes.
- Advertise `h2`, then `http/1.1`, in intake's existing verified rustls config.
  Reqwest preserves ALPN from this preconfigured TLS object; enabling its feature
  alone would not make this client negotiate HTTP/2.
- Retain HTTP/1 fallback, certificate/hostname validation, HTTPS-only requests,
  disabled proxy/redirects/compression, deadlines and bounded response reads.
  Explicit no-retry behavior is unchanged. Incomplete HTTP/1 and HTTP/2 responses
  still fail closed, even if their delivered bytes contain valid JSON.

No auth, reservation, renewal, settlement, client wire contract or diagnostic
category changes. Authorization is revalidated at several sites, including after
reservation readiness; a refresh failure must not be described as universally
pre-reservation or as evidence of a refund. No inference replay or recovery RPC
has been added.

Cargo feature unification also enables HTTP/2 in other reqwest consumers using
default TLS configuration, including SDK public-verification fetches and the
optional EHBP proxy client. This is not an entirely intake-local dependency
change. The pinned Tinfoil inference transport supplies its own TLS config without
ALPN; that configuration's negotiation selection is unchanged. Independent
source review found no resulting trust/accounting defect, but this is not live
qualification of every SDK transport mode.

## Executed evidence

Pinned Rust 1.92.0 and Go 1.27.2; telemetry disabled during live probing. No
credentials, body contents, raw error chains or native development-history
references are included in this record.

### Operator-authorized live observations

- The installed verified Pi client completed verified login, 120 authenticated
  catalog refreshes and eight small gateway generations with settled receipts.
  These exercised the existing deployment, **not the candidate gateway**. They
  do not establish the outcome of the original failed request.
- Direct native-intake candidate probing completed an initial 500 refresh cycles.
  A subsequent same-process interleaved control verified negotiated HTTP versions:
  HTTP/1.1 encountered the interruption at attempt 461, while HTTP/2 completed
  1,000 refreshes. A separate HTTP/1 control completed 500, demonstrating the
  failure's intermittency rather than a deterministic request-count threshold.
- These native auth probes submitted no inference prompts or reservations. The
  separate verified gateway generations were real service inference, not
  synthetic acceptance fixtures. No paid calls were automatically replayed.

### Final-source offline checks

- `cargo test --locked --offline --all-targets --all-features`: **680 passed,
  zero failed, eight ignored**, across 27 test binaries. Ignored live/funded
  acceptance remains distinct from this run.
- `cargo clippy --locked --offline --all-targets --all-features -- -D warnings`:
  passed. Existing vendored unused-code warnings remain outside the project lint
  result.
- `python3 tests/intake_review/transport.py`: both actual Go/PocketBase/Rust HTTPS
  transport and renewal groups passed, archiving intake source
  `1946aa43f21f3ccd6ceaee7819aa53a5f8221f6f` rather than either dirty intake checkout.
- New offline regressions reject complete JSON without the final HTTP/1 chunk,
  require actual HTTP/2 ALPN negotiation, accept normal HTTP/2 completion and
  reject a delayed HTTP/2 stream reset after body bytes with observed HTTP 200.
  The HTTP/2 success/error cases each observe exactly one request. Existing
  HTTP/1 fallback, TLS rejection, response bounds and no-retry cases still pass.
- Removing `h2` from the advertised ALPN list made the negotiation regression fail;
  the candidate was restored before the final full test run.
- Independent read-only review found no blocking findings and separately passed
  intake and selected auth/accounting/inference/privacy/transport tests. Its
  broader feature-scope and release-evidence caveats are retained above.

Applicable policy: [data handling boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures) and
[synthetic testing](../PRIVACY.md#synthetic-testing-exception). No new logs,
telemetry, persistence, support export or diagnostic-triggered work was added.
Temporary live-probe modules and instrumentation are absent from the source diff.

## Remaining release limits

HTTP/1-only origins still use the susceptible path. Successful finite HTTP/2
probes do not prove that HTTP/2 cannot fail, nor identify or repair the component
that interrupted HTTP/1 delivery. Transport failures remain errors, not permission
to accept partial authorization/accounting evidence or retry a generation.

The gateway/client wire contract is unchanged; no shipped-client upgrade is
required by this change. Existing API/accounting/stream/privacy regressions pass,
but the candidate has not been exercised through a deployed gateway with old and
current clients. Exact-artifact CI, provenance/attestation qualification and a
separately authorized protected rollout plus verified end-to-end acceptance are
still required before claiming production resolution. This work authorizes no
installation, activation or deployment and does not resolve historical attestation
or unrelated live billing/privacy limitations.
