# Private inference service

> **Historical plan:** This document preserves the original long-term product vision. It is not the implementation contract for Phase 0. For current Phase 0 requirements, follow [`AGENTS.md`](AGENTS.md), [`SPEC.md`](SPEC.md), and [`docs/phase0.md`](docs/phase0.md), in that order.

Threat model, architecture, and stack. Metered LLM inference over Tor, sold as prepaid credits at a fixed markup over cost, served from an attested enclave.

```
┌─────────────────────────────────────────────────────────┐
│  STORE — clearnet + onion mirror, NOT attested          │
│  Stripe Checkout · Monero top-ups                       │
│  accounts(account_id, units_left)                       │
│  Knows: which payment bought which account; sees every  │
│  spend event live                                       │
└────────────────────────┬────────────────────────────────┘
                         │ account number (16 digits)   ▲
                         ▼                              │ spend(account, n)
┌─────────────────────────────────────────────────────────┐
│  GATEWAY — onion, ATTESTED ENCLAVE                      │
│  Chat UI (no JS) · OpenAI-compatible API                │
│  Buffered turns, two flushes, markdown rendered         │
│  No database. No disk. No logs.                         │
│  Sees prompts in memory, never at rest                  │
│  Egress open (Tor needs it); config is public           │
└────────────────────────┬────────────────────────────────┘
                         │ prompt, over HPKE
                         ▼
┌─────────────────────────────────────────────────────────┐
│  TINFOIL — ATTESTED ENCLAVE                             │
│  AMD SEV-SNP · Sigstore-pinned code                     │
│  Model inference. Sees prompts. Cannot retain them.     │
└─────────────────────────────────────────────────────────┘
```

Prompts exist only in the two attested zones. The store never receives them, and holds no record that a request happened — only a balance that got smaller.

---

## What we claim

The claim is deliberately narrower than "anonymous." Everything below is either provable or plainly disclosed; nothing is implied.

**We do not log your prompts.**
Verifiable, not promised. The gateway runs a reproducibly-built image whose measurement is published; that image has no writable mounts, no syslog, and a publicly measured network policy. Tinfoil's own enclave is separately attested against a Sigstore-pinned release.

**We do not currently retain request history — and the component that could is not attested.**
One mutable row per account holding a balance. No timestamps, no per-request rows, no `last_used_at`. But every request sends `{account, units}` to the store, live, and the store is an ordinary server. Attestation covers the gateway, which holds prompts; it says nothing about the store, which sees the spend stream and holds the payment map. This claim is a policy on an unattested box, and a court can compel that box to start logging in a way users cannot detect. Say it at that width.

**We know which payment bought which account.**
Stated, not hidden. There are no blind signatures in this design. Fund an account with Monero and there is no identity for us to hold; fund it with a card and Stripe holds it regardless of what we do.

**An account number is a persistent pseudonym.**
Every request on one account is one identity. Tor gives a fresh circuit per session; the account number defeats that. Buying several small accounts and rotating them is the user's control, and the docs say so.

---

## Threat model

| Adversary | Gets | Does not get |
|---|---|---|
| Network observer | Tor circuit metadata | Prompts, responses, identity |
| Cloud host running the enclave | Ciphertext, resource usage | Memory contents (SEV-SNP) |
| Tinfoil | Our API key, token counts | Prompts, user IPs |
| Us, under subpoena | Payment↔account map, current balances; can be compelled to start logging the spend stream going forward | Prompts, response content, past history |
| Server seizure | Same as above, plus live in-flight requests | Anything at rest about usage |
| RCE in the gateway | Prompts and account numbers together in memory, spend API access, the onion key, the account HMAC key | History, payment records, balances (a forged account number has no store row and is worthless) |
| RCE in gateway *and* store breach | Card↔prompt, for requests in flight during the compromise | Anything before the compromise |
| Store database breach | Payment↔account map, balances | Prompts, usage patterns |
| Seized user device | Account number, anything saved locally | Server-side history (none exists) |

### Residual risks we accept and disclose

- **The enclave protects the less interesting box.** The attested gateway holds prompts; the unattested store holds the payment map and receives every spend event. For a subpoena, the second is the target, and nothing hardware-verifiable covers it.
- Account numbers correlate all of one user's sessions. Mitigation is user-side rotation, which only works if top-ups are cheap.
- A prepaid balance is cumulative, so the number encodes rough lifetime purchase history. Fixed denominations blunt this.
- Stripe charge timing and account creation timing are correlatable at the edges — and top-up cadence is a usage-intensity signal sitting in Stripe's records, where nothing we attest can reach it. Accepted in exchange for zero cost exposure.
- No-JS users cannot verify attestation in the browser. Verification is out-of-band, once, against the onion key.
- Gateway egress is `open`, because Tor relays cannot be expressed in a hostname allowlist. A compromised renderer can reach anywhere.
- The gateway holds two keys: the attested onion identity, and the HMAC key for locally validating account numbers. The latter is the same key that generates them. Forged numbers are harmless only because balance lives in the store.
- The userspace CUDA driver is closed. There is an unauditable blob in the trusted computing base.
- Attestation roots in AMD's signing key. A single vendor.
- Postgres WAL and backup snapshots can reconstruct what the schema deliberately does not store. Retention must be minimal.
- Arti runs in the gateway process. A Tor panic is a service outage, and a reboot rotates guards unless state persists, which weakens vanguard protection.
- The claims page must say which phase is live. Since the gateway is attested from phase 0, "we do not log prompts" is verifiable from the first deploy; what arrives later is Tor, payments, and the client tiers.

---

## Architecture

### Store — outside the boundary

Clearnet for Stripe Checkout, plus an onion mirror so Tor users can top up without leaving the network. Sells and credits; never sees a prompt.

```
/                    pricing — per unit, fixed markup over Tinfoil
/checkout            → Stripe Checkout (hosted, guest, one-time)
/webhook             Stripe → create account, credit units
/topup               existing account: units_left += n
/balance             account → units left
POST /spend          {account, units} → ok | exhausted
POST /check          {account} → ok | exhausted
```

`/spend` and `/check` are reached from the enclave over the public internet — Tinfoil Containers has no private inbound networking — so they are public endpoints authenticated with a shared secret, not internal ones.

Prepaid credits at cost plus a fixed percentage. Money arrives before compute is spent, so there is no usage distribution to model and no cap to tune — the one pricing model that never needs the usage data this design refuses to collect. Sell in a few fixed denominations so balances cluster.

```sql
accounts(account_id text primary key, units_left int)

charges(stripe_charge_id text primary key, account_id text, amount int)

-- account_id = HMAC(server_key, client_nonce)
```

Spend is one atomic statement:

```sql
UPDATE accounts
SET units_left = units_left - $2
WHERE account_id = $1 AND units_left >= $2
RETURNING units_left;
```

Zero rows means insufficient funds. The table holds no history — a request leaves no trace except a number being smaller. The `charges` table exists because disputes and refunds need it, and Stripe holds the same link regardless. Put nothing else in it.

Client-supplied entropy means the number cannot encode anything about the purchase. `log_statement = 'none'`, minimal WAL retention, no `updated_at` triggers.

### Gateway — inside the boundary

Onion-only. The single attested image, kept small enough that one reviewer can read it end to end — that review is the first link in the verification chain.

```
GET  /                        chat form, no JS
POST /chat                    buffered turn, two flushes
GET  /app                     single-file JS client (phase three)
GET  /attestation             SEV-SNP quote, onion key endorsed
POST /v1/chat/completions     OpenAI-compatible, Bearer = account number
```

No database, no disk, no session store. Ramdisk filesystem, read-only root, all Linux capabilities dropped — platform defaults on Tinfoil Containers rather than things we configure.

### Request lifecycle

Responses are buffered, not streamed per token. That makes markdown render correctly, reduces the gateway to a very small auditable thing, and collapses billing to one debit with the exact amount.

1. Account number arrives in a cookie (web) or the `Authorization` header (API). The gateway validates its HMAC locally, in constant time, before any I/O — so garbage floods cost microseconds and no network round trip.
2. The form carries a random idempotency token. A refresh during the placeholder re-POSTs the same token; the gateway recognises it and does not start a second generation. Without this, every impatient refresh is a double charge.
3. Gateway calls `check` against the store: any balance left, and under the in-flight cap of three to five.
4. First flush: document head, prior transcript, the prompt, a placeholder, and about 1KB of padding so the browser starts rendering. No stylesheet. An HTML comment every five seconds keeps the connection alive.
5. Gateway calls Tinfoil via the Rust SDK, which verifies SEV-SNP attestation and the Sigstore release before sending a byte.
6. Second flush: the rendered markdown, a style rule hiding the placeholder, then the form carrying updated history.
7. Debit the exact amount. A second form emitted *before* the response carries history up to the previous turn, so a broken circuit leaves the user able to retry rather than stranded.
8. On client disconnect, stop generating. Tor circuits drop constantly and generation nobody receives still costs money.

### Conversation state

The transcript lives in a hidden form field and is resubmitted each turn. The gateway holds nothing between turns — not in a database, not in memory. "We cannot retain your conversation" becomes structural rather than promised, and every replica stays interchangeable.

Cost is quadratic upload growth, which bites over Tor because upload is the slow direction. Mitigations: plain UTF-8 rather than base64, gzip the history, cap at N turns with a visible notice, and render prior turns with checkboxes so users can drop what they do not need. This is also exactly what the API already does with its `messages` array — same protocol, wearing a form.

Double-spend across replicas is handled by the row itself, with no coordination — the conditional `UPDATE` above is the whole mechanism. Postgres serializes concurrent writers on that row; different accounts never contend. Gateways are stateless, so scaling is N identical copies behind OnionBalance — wired by hand, since Tinfoil does not balance traffic between enclave instances.

Cost exposure is structurally zero: every unit consumed was paid for at a markup. Debiting after completion means overdraft is bounded by `max_tokens × per-replica cap × replicas`, and that overdraft is the only way an account consumes more than it paid for; cap `max_tokens` aggressively. The one unbounded cost is dropped circuits — Tinfoil paid for output never delivered and never debited — which the markup has to absorb.

### Denial of service

There is no client IP on the onion path, so IP rate limiting, geoblocking and upstream scrubbing are all unavailable, and the enclave cannot scale out under load.

- **Tor proof-of-work** is the main defence, and the only one that works at the introduction-circuit layer where flooding actually happens. Dormant under normal load; around 5–30ms per solve when active, up to roughly a minute under heavy attack. Enable intro-point rate limiting alongside it.
- **Locally verifiable account numbers** mean the most likely flood is rejected without touching the network.
- **Hard body-size cap**, because history-in-a-hidden-field makes a large POST an obvious target.
- **Intake is the softer target** — it has free pages. Serve them pre-rendered from memory, and gate Stripe session creation, since that is an outbound API call an attacker would otherwise get for free.

Slowloris is unfixable: real Tor clients are slow, so aggressive timeouts break legitimate users. Under sustained attack the honest failure mode is a static "under load" page rather than timeouts.

---

## Stack

Rust for both services. One toolchain, one dependency audit — which matters more here than SDK convenience, because the pitch is auditability.

| Concern | Choice | Why |
|---|---|---|
| Both services | Rust | One language; `axum` + `tokio` |
| Inference client | Tinfoil Rust SDK | Verifies SEV-SNP and the Sigstore release before sending a byte |
| Own attestation | `sev` / `sev-snp-utils` | Fetches and parses the guest report from inside the CVM |
| Markdown | `pulldown-cmark` | Buffered response, so one render at the end |
| Templates | `maud` or `askama` | Two flushes, no incremental templating needed |
| Onion | Arti, in process | Onion key generated by measured code; vanguards and PoW built in |
| Database | Postgres + `sqlx` | One row per account; streaming replica for failover, no sharding |
| Jobs | `apalis` | Postgres backend with NOTIFY, heartbeats, orphan re-enqueue |
| Payments | Stripe Checkout, hosted | Card data never touches our infrastructure; SAQ A. Hand-rolled webhook HMAC rather than `async-stripe` |
| Crypto rail | `monero-oxide` + `monerod` | View-key scanning in process; no `monero-wallet-rpc` |
| Build | One Nix flake: `crane` + `dockerTools` | Both images are derivations; toolchain, crates, `monerod` and Tor all pinned by `flake.lock` |
| Enclave hosting | Tinfoil Containers, from phase 0 | Attested from the first deploy — there is no unattested phase |

### Attestation coverage

Everything the gateway needs inside the enclave has a Rust path, and in the SEV-SNP case Rust is where the reference tooling lives — `virtee/sev` and `snpguest` are AMD-adjacent. Tinfoil Containers is self-serve for CPU workloads; only GPU access is gated.

- **Prove what we are.** `AttestationReport::request()` reads `/dev/sev-guest` and returns the signed report including the measurement. Served at `/attestation`.
- **Bind the onion key.** Tinfoil's `attested-keys` generates Ed25519 key pairs at boot and endorses the public key in the attestation — and a v3 onion address *is* an Ed25519 public key. So the address-to-measurement binding is a platform feature, not something hand-rolled into `report_data`. Keys land as read-only PEM mounts granted to one container.
- **Key lifetime.** A container restart keeps the keys; a CVM reboot rotates them. A stable address across releases therefore needs an encrypted persistent volume unlocked by the operator on each new CVM — attended deploys, and attended unplanned reboots. The alternative is rotating the address per release and publishing rotations to the transparency log.
- **Verify Tinfoil.** Their Rust SDK does SEV-SNP plus Sigstore checking and refuses to send if verification fails.
- **Public config.** `tinfoil-config.yml` must live in a public repo; Tinfoil measures it per tag and publishes to Sigstore. Free transparency infrastructure — resource shape, egress policy, and secret names are public by construction, while the image itself may stay private, pinned by digest.
- **Reproducibility.** The target is the OCI digest pinned in `tinfoil-config.yml`, not just the binary — layer tarballs, mtimes and config JSON all byte-identical. `dockerTools.buildLayeredImage` makes the image a Nix derivation with epoch-zero timestamps and every input pinned, so an auditor's whole procedure is `nix build .#gateway-image` and compare. No Docker daemon, no BuildKit version as an undeclared input. Tinfoil's own model is provenance (Sigstore attests that GitHub Actions built it); independent reproduction is the stronger claim and it is ours to deliver.

**Egress is a known downgrade.** Container networks take an egress policy of closed, allowlist, or open — but allowlist entries must be hostnames, not IP addresses, and Tor guard relays are IPs from a rotating consensus. Running Arti in the enclave therefore requires `egress: open`, giving up the allowlist that would otherwise stop a compromised renderer reaching somewhere loggable. The policy is visible in the measured public config, so this is disclosed rather than hidden. Worth testing whether a self-run Tor bridge at a stable hostname lets Arti keep a tight allowlist.

---

## Operational risks

- **New Identity clears the cookie.** Tor users hit it reflexively. Expect "I lost my account" from people who still have the number. Make re-entering it one field on the front page.
- **Turn fifteen is slow.** Quadratic history upload over Tor's slow direction. Users will not know why. The truncation notice and the per-turn checkboxes are what make this survivable.
- **Arti onion-service PoW in production** has few operators at any scale. Expect to debug it live, and keep C Tor as a fallback path in the image.
- **Dropped circuits are a cost centre**, not an edge case — see above.

---

## Access tiers

- **No-JS web, both networks.** Canonical. Works at Tor Browser's Safest setting. Verified out of band, once, against the onion key. Clearnet is the same code, just faster.
- **Static client at `/app`.** One self-contained HTML file served from the gateway, so it sits inside the measurement — one artifact, one hash, one review. Conversations stay on the device. Phase three.
- **CLI and SDK.** Full attestation verification on every connection. The only tier where the guarantee is checked automatically.

Nothing in the design requires JavaScript. Stripe Checkout is hosted, so its JS runs on Stripe's domain, not ours; Monero invoices are server-rendered SVG. Requiring JS would also not help with denial of service, because intro-circuit flooding happens below HTTP entirely.

---

## Styling

**Gateway: bare HTML.** No stylesheet, no classes, default browser rendering, a viewport meta tag and semantic elements. Every byte in the gateway is in the measurement and in the audit, and a stylesheet is the least defensible thing to make a reviewer read. It also renders identically at every Tor Browser security level.

**Intake: minimal htmx and Tailwind, vendored.** Both built into the image by Nix — never loaded from a CDN, which would be an external origin on the onion mirror and a tracking vector. htmx is progressive enhancement only: every form must work as a plain POST with JavaScript disabled, because the onion mirror is used at Safest. Tailwind compiled at build time to one small CSS file.

---

## Phases

Each phase has an acceptance criterion. Nothing starts the next until it is met.

**Phase 0 — Gateway, attested, clearnet.** Rust gateway on Tinfoil Containers with a hardcoded account, calling Tinfoil inference, buffered turns over HTTPS.
*Acceptance:* deployed, and a stranger can verify it — `nix build` reproduces the digest in `tinfoil-config.yml`, and `/attestation` returns a quote whose measurement matches. This front-loads the biggest unknowns — attested keys, open egress, the Containers workflow — into the first week rather than the last.

**Phase 1 — Gateway over Tor.** Arti in process, onion key from `attested-keys`, proof-of-work and intro-point rate limiting on.
*Acceptance:* reachable at the onion address from Tor Browser at Safest, and the published address-to-measurement binding checks out.

**Phase 2 — Intake, database, daemons.** Store on clearnet with Stripe Checkout and hand-rolled webhook HMAC, `accounts` and `charges`, `/check` and `/spend`, `monerod` with view-key scanning, concurrency cap.
*Acceptance:* a stranger can buy credits with a card or Monero and spend them at the gateway.

**Phase 3 — Intake over Tor.** Onion mirror for the store.
*Acceptance:* top up from Tor Browser at Safest without touching clearnet via Monero; card path documented as clearnet-only.

**Later.** Static client at `/app`, CLI with full attestation, OnionBalance replicas, audit.

Phase 2 is a working paid product. Because phase 0 is already attested, there is no "plain VM" period where the no-log claim is a policy.

---

## Open questions

- **Attended deploys, or a rotating address?** Attended unlock gives a stable onion address but puts an operator on the hook for every unplanned CVM reboot, not just releases. Rotating per release costs nothing operationally but makes verification harder for users. Also check whether the unlock path can avoid `cvm_admin: true`, which grants administration of the whole enclave and would sit awkwardly in the measurement of a service selling minimality.
- **Does the bridge trick work?** If Arti can use a self-run bridge at a stable hostname as a fixed entry, the egress allowlist survives. Worth an afternoon before conceding `egress: open`.
- **Smallest denomination.** The highest-leverage privacy decision left. Rotation is the only session-unlinkability control users have, and it only exists if a small account is cheap. If the minimum is twenty dollars, nobody rotates and the advice is theatre.
- **Audit scope.** Get the gateway image and the accounts schema audited, and scope the public claim to exactly what was tested. Mullvad's audits covered VPN infrastructure while the payment backend was never in scope; the claims did not carry that qualification.
- **Chargeback exposure.** Credits consumed before a dispute cannot be clawed back from an account we cannot identify. Small denominations bound the loss per incident.

---

*Crypto handles who; enclaves handle what. This design spends its enclave budget on the second and answers the first with disclosure, the way Mullvad does. The chain runs: someone reads the source, a reproducible build yields a measurement, attestation binds that measurement to the running instance, and the onion key binds the address to that instance. Three of those four links are mathematics. The first is not, and no hardware fixes it.*
