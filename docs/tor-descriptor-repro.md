# Developer Tor descriptor reproduction aid

**Best-effort source tooling, not a new public run or production-cause proof.**
One program, `scripts/tor-descriptor-repro.py`, contains host/publisher/controller
modes. One optional Linux flake target builds the existing
`tor_runtime::enabled::tests::local_e2e::operator_fresh_descriptor_generation`
libtest seam with `--locked --features tor --no-run`, unpatched Arti and unchanged
locks/features. It is not wired into default, checks, CI, release or dependency
images. There is no separate test program, qualification mode or continuing checklist.
The publisher-only Rust seam has no restart/TLS/client/identity-export paths.
Its test-only empty auth and deny/count bridge hook cannot change production
compiled behavior or dial the production shim. No production startup refactor is needed.

## Build, load, explicitly select one future run

Prerequisites: Python 3.12, native Docker CLI with a Linux arm64 daemon, pinned
Nix inputs and an explicitly available Linux arm64 builder. No implicit remote
builder or host installation. On that builder, build from the complete candidate:

`nix build --no-write-lock-file --builders '' path:.#packages.aarch64-linux.tor-descriptor-repro-image --out-link result-tor-descriptor-repro`

`path:.` includes untracked source without staging; it must include the existing
Rust seam. Build never executes the ignored public test. Load manually:

`docker load --input result-tor-descriptor-repro`

`IMAGE_ID=$(docker image inspect --format '{{.Id}}' possums-tor-descriptor-repro:local)`

**Source tooling is not authority to run public Tor.** After separate explicit
selection of one scratch attempt, choose a fresh operator-owned directory path:

`mkdir -m 700 "$ATTEMPT_DIR"`

`python3.12 -B scripts/tor-descriptor-repro.py --run --attempt-dir "$ATTEMPT_DIR" --image "$IMAGE_ID"`

These are usage steps, not recurring validation requirements. The runner never
pulls/builds; both roles use the supplied locally present immutable image ID.
Without `--run` there is no Docker work. The exclusive, fsynced `CONSUMED` marker
precedes Docker access. **Never delete it to rerun.** Unknown cleanup retains
`ACTIVE` and its local random ownership token; stop and reconcile only its exact
labelled resources, never broad-clean Docker or automatically retry.

## What the run does and does not establish

A fresh owned bridge/publisher uses UID/GID 65532+NSS, two CPUs, read-only root,
512 MiB memory-only `/tmp`, pids 256, no new privileges, core zero, all capabilities
dropped, no host ports/mounts and Docker logging disabled. The separate controller
shares only that publisher's network namespace with NET_ADMIN as its sole added
capability. After private prepared/bootstrapped/ready milestones, four tc commands
(each <=1.5s; output discarded) install root prio `1:`/three bands/16 zero priomap,
netem `30:` on `1:3` with 12000ms delay, priority-1 IPv4 TCP u32 total-length
match at offset 2 (`0x0000/0xfc00`) to `1:1`, then remaining IPv4 TCP to `1:3`.
Small packets stay fast; total length >=1024 is delayed. No reset/tuning phase.

One D at local-ready +660s, then Q/drain; no Running latch/status polling, extra
D, clients or replay. Rust lifetime stays 880s; container owner budgets 830s,
host work 850s and cleanup/reap under 900s. Late startup stops rather than extends.
Exact-ID+label removal and successful validated empty listings establish owned
Docker absence; unknown cleanup is not absence. Primary failures survive cleanup.
Host/daemon loss or uninterruptible OS operations can defeat delivery/cleanup.

Only known milestones and a small closed verdict reach stdout. SDK/libtest
stdout/stderr are discarded. Rust validates the rich vocabulary before private
IPC; Python bounds/strictly decodes it and emits only target-membership booleans,
recognized state/problem/readiness/outcomes/coverage and one-snapshot verdict.
`node_limit` remains disclosed: match means the required conjunction, not full
coverage, production-state equivalence or production cause. Tor randomness means
future matches are not guaranteed. No diagnostic archive is automatic; manually
retain only the small closed verdict if needed. Scratch identities remain in
container memory-only storage; Docker/Pi/T3 persistence is a separate boundary,
and positive cleanup does not prove erasure.

Policy: [data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden data](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[request-only exception](../PRIVACY.md#request-only-readiness-diagnostics-exception),
[synthetic limits](../PRIVACY.md#synthetic-testing-exception),
[diagnostic contract](tor-request-readiness-diagnostics.md).
[Historical r11 acceptance](file:///tmp/possums-tor-r11-parent-acceptance.md) stays
separate, untouched and unnecessary to execute this tool. The new recipe is not
that historical ELF. Bounded source/syntax inspection is not build/runtime
qualification: no full Linux rebuild/container/public run occurred for this
trimmed candidate. Rebuild/live portability is unqualified, not an ongoing check gate.
No production probe, inference, keys/custody/config change or deployment follows.
