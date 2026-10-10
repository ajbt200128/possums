# Possums-only diagnostic improvement — selected scope

2026-10-10. The operator narrowed the diagnostic work to **Possums client changes only**, without modifying Pi or T3. This supersedes the proposed host implementation phases in `docs/end-to-end-diagnostics-plan.md`. **Planning only; implementation/installation are not claimed.** GitHub authentication remains deferred.

## Goal

Preserve a known Possums connection failure through Pi's existing assistant-error path, where T3 already presents provider failures, instead of allowing a secondary model-selection error to obscure it.

The reproduced case is release-discovery HTTP 403, `rate_limited`, resulting in an empty Pi catalog and failed `set_model`. Do not assume that cause for every missing model.

This is scoped transparency, not end-to-end coverage. Extension-load failures, process loss before reporting, T3-owned startup failures and discarded host notifications remain external limits.

## Proposed approach

Separate **discoverable model metadata** from **authorization to infer** using supported Pi provider APIs:

1. Retain only previously authenticated model metadata for selection when startup verification/catalog restoration fails. Clearly distinguish last-known metadata from a live authenticated catalog; do not fabricate IDs or advertise stale prices as current quotes.
2. Retain the current session's primary closed diagnostic, separately from selection metadata.
3. If Pi can select that known model, its existing auth/stream entry point returns the preserved diagnostic through the ordinary assistant-error path when usable verified state is unavailable.
4. No prompt transmission, submission/reservation or inference occurs unless current trust, authentication and catalog validation succeed. The live catalog supplies request limits, quote and model-bound authorization; discovery metadata supplies none of these.
5. Accepted recovery clears the current failure. Session/account/provider replacement and late asynchronous results cannot leak another scope's failure or restore revoked authorization.

Qualification must establish whether Pi 1.0.4 and 1.1.0 supported APIs permit this distinction. If selection still fails before the provider can report, document that host blocker; do not substitute a model, create placeholder models, fake authentication, write directly to RPC stdout or patch installed host bundles.

Without any previously authenticated metadata (first use, empty cache, deleted credentials), a selection failure may remain unavoidable. Do not promise coverage there. Reconcile the product's live authenticated catalog requirement explicitly: last-known discovery entries are not current supported-model authority.

## Small work packets

### 1. Prove the presentation path offline

Read `PRIVACY.md`, the existing diagnostic verification record and applicable Pi provider APIs before changes. Use actual SDK/RPC fixtures to reproduce both the current hidden-startup chain and a selected-provider closed error. Test the actual T3 presentation where separately authorized, without changing T3. Formatter/factory smoke tests are insufficient.

Likely files: `tests/phase02_pi.mjs`, `tests/phase02_diagnostics.mjs`, `clients/pi/{index,provider,diagnostics}.ts`. Reuse existing failure carriers and locally authored messages.

### 2. Make the smallest client change

Separate discovery metadata, usable authorization and diagnostic ownership. Determine the minimal public metadata retention needed; use existing SDK stores where they fit. No new credential store, diagnostic journal or telemetry. Preserve existing auth checks, trust deadlines, stream validation, accounting and native retry classification.

Known setup failures must stay terminal under the existing retry policy; exposing them during a turn is not permission to replay work or retry evidence acquisition. Do not alter renewal or recovery policy owned by the blue/green thread.

### 3. Qualify and document

Test startup verification/provenance/key-binding, auth and catalog failures; unavailable/removed models; first use without metadata; revoked credentials; account/session replacement; late results; successful recovery; interrupted diagnostic delivery; native retries/abort; and post-receipt display failures.

Use hostile synthetic sentinels, including valid-looking diagnostic prefixes, URLs, credentials, ANSI/HTML and nested exceptions. Preserve closed code/stage/constraint/observed status/action and evidence-supported billing outcome. Raw exceptions and request data must not escape.

Assert no inference, reservation, replay or automatic renewal from diagnostic/status cases. Verify zero changes to retry budgets and settlement/refund behavior. Inspect generated and installed artifacts only after separate authorization, keeping previous packages for rollback.

## Privacy and accounting

Apply `PRIVACY.md` sections Mandatory reference and change control, Data handling boundaries, Telemetry: permitted signals and forbidden data, and Required evidence.

Ordinary assistant errors may enter native Pi/T3 saved history. Disclose that boundary; this plan does not authorize a new persistence channel or claim whole-app transience. A safe diagnostic does not justify retaining raw notifications, exceptions, headers or content.

A setup attempt proven not dispatched may say it sent no inference. Otherwise billing remains unknown unless existing authenticated accounting evidence establishes settlement/refund. A later presentation failure must preserve an observed receipt.

## Coordination

`docs/pi-public-evidence-cache-plan.md` covers cross-process public GitHub evidence acquisition caching separately. That cache may reduce startup failures but does not authorize inference. The blue/green thread owns recovery eligibility; do not change it here.

The broader Sol draft remains historical design context. No Pi/T3 host contract, orchestration persistence, renderer or logging changes belong to this selected scope.
