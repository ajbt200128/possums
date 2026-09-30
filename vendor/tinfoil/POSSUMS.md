# Local SDK integration

Source: https://github.com/tinfoilsh/tinfoil-rs at
`34157e497a747c191852d52af39cfbdb8dbd9eb7` (Apache-2.0).
`src/`, `assets/`, `tests/`, Cargo.toml, Cargo.lock, LICENSE and README.md
are imported byte-for-byte before the local bounded-evidence patch. Git records
the pristine import separately for review. Cargo.lock in this directory pins
the SDK's standalone test dependencies; the repository root Cargo.lock pins
production dependencies. No dependency-cache edits are required.

The local patch adds a pre-clone bounded evidence export. Its direct path
borrows immutable verified state; its proxy path holds the channel read lock
across validation and export. Existing attestation verification, release
provenance, TLS binding, refresh publication and revocation remain unchanged.
The original unbounded SDK APIs remain for upstream compatibility but must not
be used by the gateway's request-owned evidence path.

The shared pinned-client builder (including origin-bound requests) also disables
the HTTP/1 idle pool.
Hyper's pooled checkout can otherwise finish speculative connections in the
background after a request returns, without retaining that request's resource
lease. Zero idle capacity bypasses that path; it does not weaken certificate
pinning or origin checks, but costs a fresh TLS handshake per request. This is
source-level lifetime reasoning, not a whole-process RSS or live-provider proof.
