{ pkgs, inputs, ... }:
let
  unstablePkgs = import inputs.nixpkgs-unstable {
    system = pkgs.stdenv.hostPlatform.system;
  };
  # Match flake.nix: the pinned attestation SDK requires Go 1.27.2.
  attestationGo = unstablePkgs.go_1_27.overrideAttrs (finalAttrs: _: {
    version = "1.27.2";
    src = unstablePkgs.fetchurl {
      url = "https://go.dev/dl/go${finalAttrs.version}.src.tar.gz";
      hash = "sha256-A0ldorpkiU1A9cSZLklFT6eLUGkGBP+Stq//UIG3bmI=";
    };
  });
in
{
  packages = [ pkgs.git pkgs.ripgrep ];

  languages.rust = {
    enable = true;
    toolchainFile = ./rust-toolchain.toml;
  };
  languages.go = {
    enable = true;
    package = attestationGo;
    lsp.enable = false;
  };
  languages.javascript = {
    enable = true;
    package = unstablePkgs.nodejs_24;
    npm.enable = true;
    npm.install.enable = false;
  };
  env.GOTOOLCHAIN = "local";

  # Credentials must stay out of Nix evaluation, store paths and task caches.
  dotenv.enable = false;
  dotenv.disableHint = true;

  scripts.possums-check = {
    description = "Check Rust formatting and run strict Clippy";
    exec = ''
      set -euo pipefail
      cd "$DEVENV_ROOT"
      cargo fmt --package possums -- --check
      cargo clippy --locked --all-targets --all-features -- -D warnings
    '';
  };
  scripts.possums-test = {
    description = "Run local Rust and Go tests (funded Rust tests stay ignored)";
    exec = ''
      set -euo pipefail
      cd "$DEVENV_ROOT"
      cargo test --locked --all-targets --all-features
      possums-test-go
    '';
  };
  scripts.possums-test-go = {
    description = "Test the attestation helper";
    exec = ''
      cd "$DEVENV_ROOT/attestation-helper"
      exec go test -mod=readonly ./...
    '';
  };
  scripts.possums-browser-setup = {
    description = "Install locked browser-test dependencies and Chromium";
    exec = ''
      set -euo pipefail
      cd "$DEVENV_ROOT"
      npm ci
      npx --no-install playwright install chromium
    '';
  };
  scripts.possums-test-browser = {
    description = "Build and test the synthetic no-JavaScript browser fixture";
    exec = ''
      set -euo pipefail
      cd "$DEVENV_ROOT"
      cargo build --locked --example browser_fixture
      npm run test:browser
    '';
  };

  enterShell = ''
    # Worktrees share incremental artifacts, but not across compiler/host changes.
    if [ -z "''${CARGO_TARGET_DIR:-}" ]; then
      rust_version=$(rustc --version | cut -d ' ' -f 2)
      rust_host=$(rustc -vV | awk '/^host:/ { print $2 }')
      export CARGO_TARGET_DIR="''${XDG_CACHE_HOME:-$HOME/.cache}/possums/target/$rust_host/$rust_version"
    fi
    echo "Possums: Rust 1.88 / Go 1.27 / Node 24"
    echo "  possums-check          formatting + Clippy"
    echo "  possums-test           local Rust + Go tests"
    echo "  possums-browser-setup  install browser dependencies (once)"
    echo "  possums-test-browser   synthetic browser acceptance"
  '';

  enterTest = ''
    possums-check
    possums-test
  '';
}
