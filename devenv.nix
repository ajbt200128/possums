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
  packages = [ pkgs.git pkgs.ripgrep pkgs.tor ];

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
  # Optional foreground client; never starts on shell entry or edits Tor Browser.
  scripts.possums-tor = {
    description = "Start an opt-in foreground Tor SOCKS client on 127.0.0.1:19050";
    exec = ''
      set -euo pipefail
      umask 077
      fail() {
        echo "code=tor_development_$1 stage=$2 constraint=$3 action=check_local_configuration_and_owned_state; no client launched" >&2
        exit 2
      }
      config="''${POSSUMS_TOR_CONFIG:-}"
      if [ "$#" -ne 0 ]; then
        case "$1" in
          -h|--help)
            echo "Usage: possums-tor [--config PATH]; otherwise uses private persistent state and loopback port 19050."
            exit 0 ;;
          --config)
            [ "$#" -eq 2 ] && [ -n "$2" ] || fail configuration configuration expected_arguments
            config=$2 ;;
          *) fail configuration configuration expected_arguments ;;
        esac
      fi
      if [ -n "$config" ]; then
        # The owner's configuration is passed through, never rewritten.
        exec ${pkgs.tor}/bin/tor -f "$config"
      fi
      state="''${XDG_STATE_HOME:-$HOME/.local/state}/possums-tor-client"
      case "$state" in /*) ;; *) fail configuration configuration absolute_state_directory ;; esac
      for directory in "$state" "$state/data"; do
        [ ! -L "$directory" ] || fail configuration state owned_directory_not_symlink
        if [ -e "$directory" ]; then
          [ -d "$directory" ] && [ -O "$directory" ] || fail configuration state owned_directory
        fi
        mkdir -p "$directory" 2>/dev/null && chmod 0700 "$directory" 2>/dev/null \
          || fail state_unknown state protected_directory_cause_unknown
      done
      # argv avoids serializing owner paths into a generated torrc. No system
      # defaults, service keys, automatic bootstrap or circuit-isolation claim.
      exec ${pkgs.tor}/bin/tor --defaults-torrc /dev/null -f /dev/null \
        --ClientOnly 1 --SafeSocks 1 --SocksPort 127.0.0.1:19050 \
        --DataDirectory "$state/data" --Log "err file /dev/null" >/dev/null 2>&1
    '';
  };
  scripts.possums-test-go = {
    description = "Test the attestation helper";
    exec = ''
      cd "$DEVENV_ROOT/attestation-helper"
      exec go test -mod=readonly ./...
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
    echo "  possums-tor            opt-in foreground Tor SOCKS client (see docs/tor-development.md)"
  '';

  enterTest = ''
    possums-check
    possums-test
  '';
}
