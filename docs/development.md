# Development with devenv

The existing `flake.nix` still owns packages, images and CI checks. Its default
`nix develop` shell is unchanged. `devenv.nix` adds a more comfortable local shell
without changing the gateway, deployment or release configuration.

## Enter the environment

With Nix and devenv 2.x installed, run from the repository root:

`devenv shell`

For automatic activation with your existing direnv shell hook, review `.envrc`
and approve it:

`direnv allow`

No global shell configuration is changed by this repository.

The shell includes Rust from `rust-toolchain.toml` (currently 1.88.0, including
Clippy and rustfmt), Go 1.27.2, Node 24/npm for independent clients, Git and ripgrep. Go cannot silently
download a different compiler (`GOTOOLCHAIN=local`). `devenv.lock` pins the shell
inputs; `devenv.yaml` uses the existing `flake.lock` revisions for `nixpkgs`,
`nixpkgs-unstable` and `rust-overlay`. Rust uses the same stable package set as
the build flake; Go and Node use the pinned unstable set. Go applies the same
1.27.2 source/version override as `flake.nix` for the attestation SDK; keep those
overrides aligned too. The shell does not provision `gopls`; editor language
servers remain separately managed. When deliberately updating those flake inputs, update
the matching YAML revisions and run
`devenv update` too. The build flake's pins remain unchanged.

## Everyday commands

Inside the shell:

- `possums-check` — root-package formatting check and all-target strict Clippy.
- `possums-test` — locked Rust tests followed by Go helper tests. Funded Rust
  tests remain ignored; this command does not opt into paid inference.
- `possums-test-go` — only the Go helper tests, with read-only module resolution.

`devenv test` runs `possums-check` and `possums-test`. Commands
anchor themselves to the repository root even when invoked from a subdirectory.
For a one-shot command outside the shell:

`devenv shell possums-check`

Client dependencies remain explicit and local to each client, for example:

`devenv shell npm --prefix clients/pi ci`

`devenv shell npm --prefix clients/pi run build`

## Incremental builds and worktrees

Use Cargo inside `devenv shell` for the edit/test loop. For example:

`devenv shell cargo test --locked --test privacy`

The shell defaults `CARGO_TARGET_DIR` to
`${XDG_CACHE_HOME:-$HOME/.cache}/possums/target/<Rust host>/<Rust version>`.
Compatible worktrees reuse dependency compilation without sharing their source
files or credentials. Cargo still fingerprints source changes; simultaneous
builds using the same target directory may wait on Cargo's build lock. The first
build in this new directory is cold. `cargo clean` clears shared artifacts for
all worktrees in that compiler/host namespace. An explicit `CARGO_TARGET_DIR`
is preserved if you prefer an isolated directory. The build flake does not use
this mutable cache.

Run full reproducible checks/images at checkpoints, rather than after every edit:

`nix flake check --print-build-logs`

`nix build .#gateway-image --print-build-logs`

Images require Linux. Linux binary-cache outputs do not accelerate native macOS
builds; Magic Nix Cache is CI-only. We do not add Cachix or linker/profile changes.

Build the isolated API startup fixture with `nix build .#api-smoke-fixture`.
On Linux, `nix build .#gateway-smoke-image` packages it for the read-only-root,
runtime-secret startup check. These synthetic fixtures do not verify production
attestation or inference. See [CI and release builds](ci-builds.md); historical
browser passes are not API-only qualification.

## Secrets and local state

Entering the shell does not install npm dependencies, start the gateway, install
Git hooks or load `.env`. Existing application/test-specific secret handling is
unchanged. Do not put credentials in `devenv.nix`, `devenv.yaml`, shell commands
or `devenv.local.nix`: Nix outputs and task caches are not secret storage.
`.devenv/`, `.direnv/` and optional `devenv.local.nix` are Git-ignored.

No exporter, tracing, support bundle or telemetry process is added. The applicable
privacy boundaries remain [data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data)
and [synthetic testing](../PRIVACY.md#synthetic-testing-exception). Local test
passes are not evidence of deployed privacy controls or provider billing.
