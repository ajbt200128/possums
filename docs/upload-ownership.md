# Phase 0 serialized upload ownership repair

> **Historical Web upload evidence:** `/chat`, Web rendering and browser acceptance below describe the earlier browser artifact. The API-only target uses its own bounded ingress and needs new exact fixture/allocation evidence; this record is not that proof.

Scope: local changes following `b38e3f9` (which includes `ba41dd5`). This is a
synthetic ownership regression record, not a release or a 512-MiB proof. Existing
user-edited policy/verification documents are intentionally unchanged.

## Invariant and phase boundary

The same `Arc<OwnedSemaphorePermit>` admitted by `/chat` travels through detached
preflight to `Inference::count_tokens`, and through composition to
`Inference::generate_stream`. Both production serializers still borrow the
messages and use the bounded JSON writer. Before entering reqwest, the resulting
`Vec` is placed inside `Bytes::from_owner` together with that lease. This retains
the entire allocation, including spare capacity; the serialized length alone is
not the allocation bound. DATA clones/slices and Hyper's background write queue
therefore retain admission after an early response, error, timeout, or worker
return. An outer Body lease alone would not do so.

The outer reqwest body remains streaming/non-cloneable: attaching admission does
not permit prompt replay. Four global heavy lanes, three concurrent reservations
per account, refunds, and delivery-independent terminal settlement are unchanged.
A refunded request can continue holding a heavy lane until its queued upload is
actually destroyed; refund/response completion is not authority to release it.

A successful tokenizer response cannot advance to generation serialization until
the tokenizer allocation is destroyed. The upload owner's field drop order is
`Vec`, lease, then a oneshot sender; sender destruction supplies the release
receipt. The receiver does not own the upload. The wait is inside the route's
existing 30-second preflight timeout (and the adapter's response deadline).
Timeout fails preflight closed; dropping the waiter does not detach the lease
from any surviving DATA. Thus tokenizer backing cannot overlap the next
serializer's old/new allocation growth peak. This receipt proves memory release,
not that the peer read the entire upload, nor successful delivery or billing.

Public inference calls require admission. Only the private fixed-input funded
probe passes `None`; it is not a route and was not run. The ignored production
adapter canary supplies its own synthetic permit, not a route admission.

## Regression and verification

`src/inference/upload_tests.rs` replaces the exploratory untracked
`tests/upload_lifetime.rs`. It calls the actual production factories and tokenizer
response/release path, rather than a generic observed body:

- Near-16-MiB tokenizer and generation requests receive an early HTTP/1 413 from
  a small-receive-buffer loopback peer that holds its socket without draining.
  After response rejection and worker return, admission stays held until cleanup.
- A valid early tokenizer response cannot reach generation serialization while
  upload backing remains queued. Generation begins only with the tokenizer's
  owner gone.
- A stalled successful response times out under an enclosing preflight-style
  deadline; its queued upload still holds admission until peer cleanup.
- DATA clones/slices keep both admission and the release receipt pending.

The combined resource fixture now passes the actual admitted lease into both
production serializers and waits for tokenizer backing release. Its source
messages remain borrowed; its authentication and upstreams remain synthetic.

Local macOS/aarch64 verification:

- `nix develop -c cargo fmt --package possums -- --check`: passed.
- `nix develop -c cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `nix develop -c cargo test --all-targets --all-features`: 218 passed, two funded
  tests ignored; no live funds used.
- `nix develop -c npm run test:browser`: progressive browser suite passed.
- `nix develop -c cargo test --lib web::resource_streaming_tests::combined_resource_gate -- --exact --test-threads=1 --nocapture`:
  exactly one test passed, 82 filtered; peak requested live allocation
  **119,000,481 bytes** for these synthetic phases. Nix used an expired cached
  dependency ref after a DNS lookup failed; the test itself completed normally.
- The same exact one-test command with `--release` passed again after the
  disclosure merge (`2a3c73c`), with **118,997,451 bytes** peak requested live
  allocation (82 filtered). Optimization and this synthetic sample are not
  worst-case, allocator-overhead, or RSS evidence.
- `git diff --check`: passed. The existing vendored SDK dead-code warning remains;
  no vendor source changes were made.

Two independent read-only reviews found no concrete upload-lifetime or fixture
wiring defect. One could not execute Rust tests in its own environment; the
full local suite and one-test fixture above were run separately in this checkout.

The synthetic peak is neither a universal bound nor RSS. The 104-MiB
per-heavy/512-MiB scoped target remains unproved; the user has explicitly
accepted that availability risk for Phase 0 rather than requiring its analytic
proof as a release gate. This repair still closes queued-upload ownership and
successive-serializer overlap gaps. It does not establish a whole-process memory
bound, SDK/TLS/helper RSS, authenticated live-provider behavior, or release
readiness.
