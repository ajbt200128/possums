# README rewrite notes

Working notes for rewriting the repository's reader-facing documentation. Written
for the operator, not for publication: delete this file once the READMEs are
rewritten.

Load-bearing context: `AGENTS.md` and `SPEC.md` are authoritative for scope.
`docs/` holds dated evidence records and is deliberately written for review, not
for onboarding. Almost nothing in `docs/` should be linked from a README as
"here's how to use this" — it is evidence, and its tense is historical on purpose.

---

## 1. The three structural gaps

### 1.1 There is no root `README.md`

Not now, and never in git history. The repository's actual front door is
`AGENTS.md`, which is 30 KB of operator directives addressed to agents ("Current
intake gateway cutover authority…", "Persona — THE most important rule"). A
visitor who opens the repo has no idea what Possums is.

A skeleton exists at `README.md` in this branch. It is a section outline with
one-line placeholders; see §5 for what each section must contain.

### 1.2 Nobody can set the thing up from the repository

No page anywhere answers "how do I run this". Specifically missing:

- How to run the gateway at all. Nothing in the repo documents the eight required
  environment variables or the intake dependency (see §1.3).
- How to talk to the HTTP API. The route list lives only in `src/api.rs`, and the
  wire shapes only in type definitions.
- How to get a credential. The answer today is "the operator provisions intake
  accounts manually" — there is no signup, no payment path and no demo mode in the
  source. `POSSUMS_ACCOUNTS_JSON` is gone from `src/`; authentication is
  `Auth::from_intake_config(origin, key)` only.
- How to build and install the Pi client. It *is* documented, but in the middle of
  `clients/pi/README.md`, below roughly 200 lines of qualification status.
- How Tor mode works end to end, from "you need a SOCKS listener" to "these are
  the two flags".

### 1.3 The gateway is no longer a standalone service

Worth stating plainly in the README, because it changes what "local setup" means:
the gateway authenticates and bills through a **paired intake service**. Startup
reads `POSSUMS_INTAKE_ORIGIN` and `POSSUMS_INTAKE_LEDGER_KEY` and fails with
`configuration_missing` without them. There is no built-in demo credential path in
the current source.

Consequence for documentation: a visitor cannot run a gateway from a clean clone
without either a reachable intake instance or upstream test fixtures. Do not write
a README quickstart that pretends otherwise.

---

## 2. Per-file verdicts

| Path | Verdict | Audience |
| --- | --- | --- |
| `README.md` (new) | Skeleton written, needs authoring | Visitor, evaluator |
| `clients/pi/README.md` | Rewrite — correct but unreadable | Pi user, then client maintainer |
| `examples/phase01/README.md` | Rewrite — mostly historical | Contributor reviving the reference client |
| `docs/development.md` | Good; needs setup gaps filled | Contributor |
| `docs/tor-client-setup.md` | Right content, wrong shape | Pi user enabling Tor |
| `docs/pi-client.md` | Leave — superseded plan, correctly labelled | — |
| `vendor/tinfoil/README.md` | Leave — upstream, vendored | — |
| `vendor/tor-*-0.47.0/README.md` | Leave — upstream, vendored | — |
| `docs/*.md` (other ~95) | Leave — dated evidence records | — |

---

## 3. Per-file detail

### 3.1 `README.md` (new skeleton)

Author it in this order; the security story is what makes the rest make sense.

Keep the disclaimer short and factual. "Not deployed / not audited / unaudited
cryptography is out of scope" is honest. Do not invent a guarantee the code does
not make — the trust boundary list in §3.2 is the correct shape.

### 3.2 `clients/pi/README.md` — rewrite, 297 lines → target ≤80

This is the single highest-value file and currently the least readable. Every
statement in it is true and carefully scoped; the problem is that fifteen
successive project packets were appended in chronological order, so the reader
meets "Historical legacy-v2 public evidence cache (not used by v3)" before ever
learning what the client is for.

Sections that must survive the rewrite, with the contradiction resolved:

1. **What it is** — a Pi extension that connects Pi to a Possums gateway.
2. **What it verifies and what it does not** — the four-word summary is: *it
   verifies the backend, not your machine*. Then the detail.
3. **Install** — see §4.1. Note explicitly that `build.sh` requires a pinned Pi
   **1.3.0** release directory and **Node 24.13.0**; it fails loudly otherwise.
4. **First use** — `/login`, pick Possums, paste the recovery credential. Public
   evidence and key binding are verified *before* the credential is requested.
5. **Failure handling** — this is the section users actually need, and it is
   currently a 12-bullet error taxonomy placed after the install instructions.
   Keep the codes; add one sentence of policy first: *non-trust failures stop the
   turn with a warning and never retry inference; backend identity failures are
   fatal; a deliberate new turn may incur another charge.*
6. **Tor mode** — see §4.3.
7. **Privacy boundary** — Pi saves plaintext sessions. `--no-session` disables the
   session file only. Never `/share` or `/export` a sensitive session.

**The contradiction to resolve:** the page contains both of these, 200 lines apart.

- "The operator-approved client failure policy supersedes the historical
  retry/terminal-error … descriptions below."
- A whole section headed "Native steering and retry source candidate (historical;
  superseded)" which then describes native retry behaviour in detail.

Both are true of different artifacts: the *installed* package still uses the older
retry policy, while the *source candidate* uses warning-only graceful stops. A
rewrite must state which applies to the artifact the reader is holding. Suggested
framing: "Installed builds do X. Current source does Y. They differ on purpose;
see `docs/pi-client-failure-policy.md`."

**Contradiction to resolve, second:** "not installed" appears in roughly ten
section headings, while §"Explicit-submit expiry renewal" says "The client was
locally installed after integrated qualification and review". Establish one
current state for the artifact under discussion and say it once, up front.

**Stale-in-source warning:** the old `possums_tools_unsupported` text says
"for a text-only model, use `/possums-text-only`". That command is a runtime
extension registration, not present in the current `clients/pi/index.ts`. Treat
that step as historical until someone confirms it still exists.

**What to move out:** every "source candidate; not installed/deployed" qualifier.
The rewrite should describe the shipped client and link to
`docs/` for packet history. Keeping all of it is why the file is unreadable.

### 3.3 `examples/phase01/README.md` — rewrite

Currently opens "This is a historical API-client qualification record, **not
current deployment instructions**", then documents an approval that expired on
**2026-10-11**, then spends most of its length on source-candidate notes for a
different artifact.

Two options; pick one:

- **Retire it.** Replace the body with a five-line note: what the directory is,
  that the fixtures are historical, and that `docs/phase01.md` holds the evidence.
- **Rewrite as a fixture guide.** If the `ReferenceClient` is still useful to
  revivers, document how to actually build and run it — see §4.2 — and keep the
  expiry caveat to one sentence.

Do not leave it in the current state: an expired dated approval presented as a
client flow will mislead anyone who tries to follow it.

### 3.4 `docs/development.md` — good, needs two additions

Offline commands are documented correctly and the `devenv` / flake split is clear.
Missing:

- The Python test suite. `check.yml` runs five `python3 -m unittest` selections
  that are not mentioned here; run them locally with
  `python3 -m unittest discover -s tests -p 'test_release*.py'` (and the
  `test_intake_cutover.py`, `test_image_health.py`, `test_image_tor.py`,
  `test_release_cache.py` variants).
- The client-side build chain: `scripts/build-v3-verifier.sh` and
  `scripts/build-tor-native.sh` are prerequisites for `clients/pi/build.sh` and are
  not mentioned anywhere in the development docs.

### 3.5 `docs/tor-client-setup.md` — restructure

The technical content is correct and verified against source: `--possums-tor-onion`
locks the route, `--possums-tor-proxy` defaults to `127.0.0.1:19050` and must be a
numeric loopback address, proxy-only credentials come from
`POSSUMS_TOR_PROXY_USERNAME` / `POSSUMS_TOR_PROXY_PASSWORD` and must be provided
together, and Tor cannot be combined with `--possums-manifest`.

The problem is shape: a user who wants Tor gets 220 lines of adjudication and wire
format. Split it:

1. **User guide (new, ~25 lines)** — needs a SOCKS listener, the two flags, the
   env pair, what fails and why. Link out for the rest.
2. **Keep this file** as the wire/protocol record for the endorsement response:
   95-byte payload, 32-byte challenge, 32-byte onion identity, closed error
   envelope, the size bounds. That detail is genuinely useful and does not belong
   in a user guide.

Add to the user guide, because nothing says it today: *selecting Tor does not
start Tor.* The client speaks to an existing listener; it never launches a daemon.


### 3.7 `AGENTS.md` — not a README, but blocks the reader

Not in scope for this pass, and not something to edit casually: it is the
authority file and its dated authorizations are load-bearing. Two observations
worth acting on separately:

- It is 30 KB, mixes live authority with historical authority, and opens with
  intake cutover mechanics rather than what the project is.
- It contains agent instructions ("Persona — THE most important rule") that a
  human reading the repository does not need.

Consider splitting into `AGENTS.md` (agent directives) and a separate
`docs/authority-log.md`. Do not do this without deciding which dated paragraphs
are still live — that is an operator judgment, not an editorial one.

---

## 4. Setup command reference

Everything below was read from the current source. Commands are inline per repo
style; copy them as-is.

### 4.1 Pi client (the verified path)

Prerequisites: Nix with flakes, Node **24.13.0**, Go **1.27.2** (for the verifier),
and a pinned Pi **1.3.0** installation.

`nix develop --offline`

`nix flake check --print-build-logs` (optional; confirms the tree builds)

`POSSUMS_PI_ROOT="$HOME/.pi/agent/install/releases/1.3.0" sh clients/pi/build.sh`

The script requires that variable, requires Node to report exactly `v24.13.0`,
requires `node_modules/@earendil-works/{pi-ai,pi-coding-agent,pi-agent-core}` to
exist under `$POSSUMS_PI_ROOT`, and builds the verifier and native Tor connector
from source unless you supply `POSSUMS_VERIFIER_BUILD` / `POSSUMS_TOR_NATIVE_BUILD`.
It prints a scratch path; the package ends up at `<scratch>/package`.

Register it:

`pi install /absolute/path/to/<scratch>/package`

Or load it for one session without installing:

`pi --no-session --no-extensions --no-tools -e /absolute/path/to/<scratch>/package/extension.mjs`

Then `/login` → choose Possums → paste the recovery credential.

Clean-agent alternative, if you do not want Pi's own extensions in the loop:

`pi --no-session --no-tools --tools read,bash,edit,write,grep,find,ls --provider possums --model <model-id>`

Both the build and the load path need a full Pi restart to take effect; `/reload`
was insufficient in the operator's session.

### 4.2 Reference client (`examples/phase01`)

`nix develop --offline --command npm --prefix examples/phase01 ci --ignore-scripts`

`PHASE01_OUT=/absolute/scratch/path nix develop --offline --command npm --prefix examples/phase01 run build`

`examples/phase01/build.mjs` refuses to run unless `PHASE01_OUT` is absolute, the
cwd is not the primary checkout, Node is `v24.13.0` and esbuild is `0.25.10`. It
emits `channel-node.mjs` and `channel-browser.mjs` plus a hash report.

Note the approval caveat: `ReferenceClient.verified(...)` checks the installed
approval, and the current one is dated **2026-10-11**. Anyone following this today
will hit an expiry rejection. Say so in the rewrite, or retire §3.3.

### 4.3 Tor mode

1. Provide a SOCKS listener. Tor Browser on port 9150 is the common case; a
   system Tor on 9050 also works. The client starts nothing itself.
2. Know the onion address you intend to reach.
3. Launch Pi with both flags:

`pi --possums-tor-onion <56-char-v3-onion> --possums-tor-proxy 127.0.0.1:19050`

The proxy flag is optional and defaults to `127.0.0.1:19050`. The host must be
`127.x.x.x` or `::1`; remote resolution happens at the SOCKS layer.

If the listener requires authentication, set both variables in the environment
(never on the command line):

`POSSUMS_TOR_PROXY_USERNAME=… POSSUMS_TOR_PROXY_PASSWORD=…`

Tor cannot be combined with `--possums-manifest`; that combination fails closed as
`tor_selection_invalid`.

### 4.4 Raw HTTP API without client-side verification — the degraded path

This is the "other harness" path: plain HTTPS, a bearer token, no attestation
verification and no EHBP body encryption. Document it with the warning in §5.4.

The protocol is six calls — this is the only accurate description of it anywhere:

1. `GET /attestation` — public, credential-free. Describe it in the README as
   informational only; it does **not** authorize anything on its own.
2. `GET /v1/auth/challenge` → `{"challenge": "…", "expires_in": 600}`.
3. `POST /v1/sessions` with `{"challenge": "…", "credential": "…"}` →
   `{"token": "…", "token_type": "Bearer", "expires_in": 43200}`.
4. `GET /v1/models` with `Authorization: Bearer …` — returns the live authenticated
   catalog. Note that all numeric fields (`context_tokens`, `max_output_tokens`,
   the microunit prices, `maximum_reservation_microunits`) are **decimal strings**,
   deliberately, so JS consumers cannot lose precision.
5. `POST /v1/submissions` with `{"model": "…", "new_conversation": false}` → a
   server-issued submission token. Every chat needs a fresh one, and it is
   model-bound.
6. `POST /v1/chat/completions` with `{"model", "stream": true, "submission",
   "messages": […]}`, optionally `tools`, `tool_choice`, `reasoning_effort`,
   `chat_template_kwargs`, `stream_options`.

Plus `DELETE /v1/sessions/current` for logout, and `GET /healthz` for readiness
(204 ready / 503 not ready; credential-free, outside admission and observation).

Three things the README must state, because they surprise OpenAI-SDK users:

- The request schema is `deny_unknown_fields`. Fields an OpenAI SDK habitually
  sends will be rejected.
- `submission` is a required field with no OpenAI equivalent.
- The stream is SSE that ends with an authenticated receipt; the final charge is
  authoritative, so a client that treats the connection close as the only terminal
  event cannot report billing. Billing is **unknown** without a receipt, and an
  interrupted stream is not a refund.

### 4.5 Running the gateway itself

Document honestly: this is not currently possible from a clean clone without an
intake instance. What the binary needs, from `src/main.rs`:

`POSSUMS_INTAKE_ORIGIN` and `POSSUMS_INTAKE_LEDGER_KEY` (authentication and the
ledger — required, no in-process account list any more)

`POSSUMS_TINFOIL_HOST`, `POSSUMS_TINFOIL_REPOSITORY`, `TINFOIL_API_KEY` (upstream
inference)

`POSSUMS_GATEWAY_ATTESTATION_SOCKET`, `POSSUMS_GATEWAY_REPOSITORY` (self-attestation)

`POSSUMS_BIND` (optional; defaults to `127.0.0.1:8080`)

Tor additionally wants `POSSUMS_TOR_ENABLED`, and, when enabled, the local identity
and expected-onion material. Telemetry is off unless `OTEL_EXPORTER_OTLP_*` is
configured, and `OTEL_SDK_DISABLED=true` is the switch.

If a local path is expected to exist, the README needs a fixture or a documented
intake stub. That is a code decision, not a doc decision — flag it, do not
paper over it.

---

## 5. What the root README must say

### 5.1 What this is

Two sentences: a gateway that proxies to attested Tinfoil enclaves and bills per
request; a Pi client that verifies the gateway before sending anything. State the
current status plainly (source, gated releases, not audited).

### 5.2 Security guarantees — what is actually proven

State each guarantee with its mechanism in the same sentence. Verified against
source in this pass:

- **Measured backend identity.** The gateway runs in a CVM; clients verify a
  signed release manifest, the enclave measurement and the serving key before
  transmitting a credential or prompt.
- **Endpoint-key binding.** The client proves the peer holds the endorsed SPKI,
  with native hostname and date validation, *before any HTTP header or body is
  sent*.
- **Encrypted bodies.** EHBP over the bound channel.
- **Freshness.** Nonce-bound `GET /attestation/tor?nonce=…`; a 32-byte fresh
  challenge is signed together with the onion identity.
- **Fail-closed accounting.** Maximum cost is reserved before prompt bytes go
  upstream; unaffordable requests are rejected, never downgraded or substituted.
- **No prompt persistence.** Requests and responses are never stored or exported;
  observability is aggregate metrics only.

### 5.3 Trust boundaries — what is explicitly *not* proven

This list is an asset; it is why the project is credible. Do not soften it.

- Attestation proves a measured code identity. It does not prove the operator
  cannot retain data under runtime compromise.
- The ledger/intake store is **outside** the attested boundary and sees
  reservation/settlement events.
- The gateway and Tinfoil see prompt plaintext in memory.
- Pi, its extensions and the host OS are outside the measurement, and Pi saves
  plaintext sessions by default.
- Honeycomb (telemetry) is a third party outside the attested boundary.
- Residual risks, stated as such: open egress, payment metadata, timing
  correlation, account reuse, backups/WAL, upstream caches and dependency
  compromise.

### 5.4 The degraded path

Give the raw-API path its own subsection, headed so nobody mistakes it for the
recommended one. The framing you chose:

> You get: catalog, reservations, receipts, streaming, billing.
> You don't get: enclave measurement, release provenance, endpoint-key binding,
> EHBP body encryption.
> Use this only if you accept trusting the operator's TLS alone.

Then §4.4's six calls.

### 5.5 Everything else

- **Tor mode** — link the new user guide (§3.5), one paragraph in the README.
- **Development** — link `docs/development.md`, list the three offline commands.
- **Privacy** — link `PRIVACY.md`, state the aggregate-only claim and explicitly
  do *not* claim "no telemetry" or "no logs whatsoever".
- **Status / limits** — not audited, no public signup, no payment path yet, no
  stable public release contract.

---

## 6. Fixed in this branch

- 13 pages carried a banner declaring this branch an inactive HTTPS-only backup
  with Tor removed. Tor is restored and enabled in `Cargo.toml` (optional `tor`
  feature), `flake.nix`, `tinfoil-config.yml` and `src/tor_ingress`. Each banner
  now states what is still true for that specific page — several describe removed
  spikes, a removed helper, or a superseded two-endpoint design — and points at
  `docs/tor-main-restoration.md` for current scope. Historical results untouched.
- `devenv.nix` reported "Rust 1.88" in its shell banner while `rust-toolchain.toml`
  pins 1.92.0; `docs/development.md` had the same stale number.
- `docs/tor-development.md` presented `devenv shell possums-tor` as a working
  command. The script does not exist in `devenv.nix`; the page was deleted.

## 7. Found and deliberately not fixed

- **29 broken relative links**, almost all in `docs/verification.md` and
  `docs/telemetry-schema.md`, pointing at browser-era files removed by the API-only
  cutover (`src/web.rs`, `src/render.rs`, `src/streaming_chat.rs`,
  `src/web/resource_gate.rs`). Also `tor-arti-spike.md` → the absent `spikes/` tree,
  and two `file:///tmp/…` links in `tor-descriptor-repro.md`. Leaving historical
  evidence links broken is defensible; fixing them means rewriting evidence, which
  is a different task with its own review.
- **2 broken anchors** — `docs/pi-client.md:5` and `docs/verification.md:862`.
- `AGENTS.md` structure (§3.7) — needs operator judgment on which dated
  authorizations are still live.

## 8. Answered

1. The reference client is not kept as a usable entry point. `examples/phase01`
   became `clients/pi/core` — it is the client library, not a sample.
2. `possums-tor` is deleted, not restored.
3. The gateway is not expected to run standalone yet. The root README says so.
4. The Pi README describes the current source.
