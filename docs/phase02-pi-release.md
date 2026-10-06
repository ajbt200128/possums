# Minimal Pi integration release

## Scope

Pi 0.99.2 uses the existing verified Possums provider: memory-only login/catalog,
SDK transcript and tool events, caller-owned execution checks, and matching-result
continuation. The Pi adapter has no credit reservation or spending-budget engine.
Reservations and final-usage settlement remain gateway behavior. No dependency,
public signup, payment, replica, or unrelated user-document change is included.

Only exact `kimi-k3` gets the OpenAI-functions wire profile. Existing live named
(F), auto and none (H) cases passed the verified SDK transport, final usage and
EOF path. H's required case had valid usage/EOF but ended at the diagnostic output
limit; this is not proof of a complete multiple-call round. Historical reports
remain `qualified:false` under their broader test criteria. This release decision
accepts the narrower wire evidence plus actual-SDK/local accounting and caller
fixtures; it does not retrospectively mark those reports fully qualified.

The user explicitly requested this minimum integration rather than another
standalone qualification matrix. Multiple-call execution, real Pi continuation,
and balance reconciliation remain live acceptance items until observed. A paid
partial response never authorizes execution or automatic regeneration.

## Removed test-only prerequisite

The private qualification harness is not part of this release. Its I check
subtracted the selected maximum output from the context and required a positive
remainder. The catalog schema has no separate output limit and represents the
maximum as the context window, so that private check necessarily rejected before
prompt transmission. The gateway instead tokenizes after reservation and uses
the remaining context. This does not change gateway credit checks, rates or
settlement, and does not justify silently exceeding a test spending ceiling.

## Release verification

Release/source/image/config/workflow/invocation pins, independent Linux builds,
real hardware/TLS/HPKE bindings, and the updated client approval must be verified
before the changed client sends credentials or prompts. The old v0.0.9 approval
is not authority for changed code. No fake evidence, fixture capability or
measurement-only provenance bypass is allowed.

Evidence will be recorded after publication and deployment; neither this file
nor local tests assert a completed rollout or full Phase 0.2 acceptance.
