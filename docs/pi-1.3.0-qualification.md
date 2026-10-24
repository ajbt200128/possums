# Pi 1.3.0 — source/build qualification

## Scope and exact artifact

This bounded pin bump qualifies current source on **Pi 1.3.0**. The build guard,
report and output use one version constant for all three peers: `pi-ai`,
`pi-coding-agent` and `pi-agent-core`. No provider runtime, trust, retry,
accounting, wire contract or host policy changes are included.

The normal scratch build at `/tmp/possums-pi-build-CkbVSr/package` passed
TypeScript checking and reports Node **24.13.0**, Pi **1.3.0**, TypeScript
**5.9.3**, esbuild **0.25.10**, Go **1.27.2**, Rust **1.92.0** and
`tor` **0.47.0** (Darwin arm64 native helper). Existing verifier/native artifacts
were reused through the normal build's source/artifact checks. Build dependencies
were confined to scratch; none were installed in the checkout.

Before the final complete check run, byte comparisons verified all copied current
Pi `.ts`/`.mts`/`.mjs`/`.json` build inputs and all **22** copied test files against
the checkout. All **165** bundled input hashes, **22** bundled project-source
files, **8** package artifact hashes and all three peer manifest hashes/versions
matched the report and current source. The final startup test adaptation was
copied into scratch before these comparisons. Only documentation changed after
this comparison; no rebuild or dependency fetch was needed for the final run.

Scratch extension SHA-256:
`d16b1eaf3af596bfc126088a75d832ce824b338f9f4202b67d48a2d2bb2c48ab`.
The extension bytes are unchanged from the earlier CVM-compatible artifact; the
new scratch report qualifies the build and peers as **1.3.0**.

## Offline acceptance and observed upstream changes

The complete `clients/pi/check.mjs` passed again using the exact scratch source,
Node 24.13.0 and Pi 1.3.0 peers: native Pi **162 groups**, discovery **62**,
packaged real Worker/WASM/Pi **18**, legacy release **194**, v3 **69**,
peer-key recovery **18**, and all remaining client, stream, thinking, cache,
Tor, diagnostics and admission suites. Fixtures use synthetic acquisition and
isolated temporary homes/caches, including actual SDK/RPC and loopback TLS;
positive synthetic crypto fixtures are not live trust acceptance.

Two upstream startup behavior changes required test-only adaptations in
`tests/phase02_pi.mjs`, informed by Pi 1.3.0 documentation and implementation:

1. Pi marks a run busy before `before_agent_start`. A custom message submitted
   during that hook joins the starting run rather than creating a nested run.
   The test requires exactly one dispatch containing both messages, one renewal
   and no extra credential. A later idle custom trigger starts one additional
   run with `messageType: custom`, without another renewal.
2. Stop during an awaited startup hook remembers cancellation before an active
   signal exists. `ctx.isIdle` is false; `abort()` waits for the run to become
   idle. The test starts abort, releases the held hook, then requires zero
   dispatches. Only a later deliberate prompt dispatches once.

These assertions describe observed upstream scheduling; they do not grant new
renewal or replay authority. Existing **BLOCKED** same-signal/native-origin
permission tests remain: nested extension work can be indistinguishable at the
authentication boundary and can consume existing renewal permission. This bump
does not resolve that ambiguity or claim native-origin-only authorization.

## Installation distinction and remaining limits

The [earlier CVM compatibility and installation record](pi-cvm-0.14.16-compatibility.md#subsequent-operator-authorized-local-installation)
remains unchanged. The operator already installed
`pi-cvm01416-d16b1eaf3af5-pi130` through the stable package link. That artifact's
original report describes **Pi 1.0.4 build qualification**, while its three
installation peer links select **Pi 1.3.0**. Do not reinterpret or rewrite that
past report as a 1.3.0 build. The new scratch report supplies separate full
source/build qualification, not a new installation event.

This packet did not inspect private runtime configuration, auth, history or
caches; modify installation or its stable link; restart processes; make public
requests; or send inference. Installation state is the separately supplied
operator/parent evidence, not a new installation inspection by this packet.
Activation in existing processes, positive live startup/authentication/catalog,
T3 rendering, Linux behavior, live billing and upstream retention remain
unqualified here. No push, merge, release or deployment follows from these tests.

Privacy scope: [data handling boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed interface failures](../PRIVACY.md#typed-interface-failures-and-token-aggregate-extension)
and [synthetic testing](../PRIVACY.md#synthetic-testing-exception).
Existing hostile-error, terminal-evidence, cancellation and no-replay checks
remain enabled. No telemetry, logger, support bundle, diagnostic persistence or
raw exception export was added. Pi/T3 native history persistence remains a
separate host boundary; no whole-process transience or upstream erasure is
claimed.
