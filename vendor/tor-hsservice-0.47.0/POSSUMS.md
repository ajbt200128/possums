# Local Arti SDK integration

Source: https://gitlab.torproject.org/tpo/core/arti.git/ at `ce8bc6e0998bd5a4efdf06dd62dce53c98ea1087`,
observed release tag `arti-v2.7.0` (MIT OR Apache-2.0).
Crate source path: `crates/tor-hsservice`; version 0.47.0.
Pristine crates.io archive SHA-256: `55809560d9295dcc2ee77002c5a4275d1f5f5a54203de7cd6acb52215f5344bd`.
The archive's `.cargo_vcs_info.json` records this commit/path; its
`Cargo.toml.orig` matches the canonical upstream manifest byte-for-byte.

The published crate archive omits the full root licenses. `LICENSE-MIT` and
`LICENSE-APACHE` here are copied VERBATIM from that exact upstream commit using
the Tor Project GitLab HTTPS API. Content SHA-256 and Git blob IDs were checked
against API metadata and the commit tree. LICENSE-MIT includes the upstream
copyright notice. No COPYRIGHT/NOTICE file was present at the repository root;
no notice or copyright attribution was invented. This is not a tag-signature
verification claim.

- `LICENSE-MIT`: SHA-256 `e40081d18f17b6783fcb554ae42f6fa3548988e4d705a3e4541e2ef240293040`; Git blob `46f0bc9287a5da523702aaf1265f16422e7c9d65`. Source: https://gitlab.torproject.org/api/v4/projects/tpo%2Fcore%2Farti/repository/files/LICENSE-MIT/raw?ref=ce8bc6e0998bd5a4efdf06dd62dce53c98ea1087
- `LICENSE-APACHE`: SHA-256 `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30`; Git blob `d645695673349e3947e8e5ae42332d0ac3164cd7`. Source: https://gitlab.torproject.org/api/v4/projects/tpo%2Fcore%2Farti/repository/files/LICENSE-APACHE/raw?ref=ce8bc6e0998bd5a4efdf06dd62dce53c98ea1087

Local changes: Descriptor upload uses the reviewed response-clock opt-out; upload_all skips an empty period with continue rather than returning. Removed unused internal-prelude send_request import. Existing offline publisher fixtures/test-only visibility/includes are local changes, not upstream tests.
Pinned Rust 1.92.0 rustfmt formats only the touched SDK lib/reactor and the two
local fixture files. No dependency version, feature, HTTP-400 retry, health,
circuit-budget or trust policy change is intended. Keep the pristine import
and local runtime/test/mechanical/license deltas separately reviewable.

The root Cargo.lock is the gateway graph; this crate's original Cargo.lock is
its standalone SDK dev graph. Temporary standalone verification manifest/lock
adjustments and private build expressions must not be shipped.

Retain these licenses and this provenance with repo/vendor source redistribution.
No extra root Nix build-source allowlist is needed for these noncompiler files;
a filtered Cargo build source is not a complete source-redistribution bundle.
This note does not qualify a release image or all third-party notice obligations.
