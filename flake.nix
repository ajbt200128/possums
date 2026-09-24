{
  description = "Possums Phase 0 gateway";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, crane, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        craneLib = (crane.mkLib pkgs).overrideToolchain
          (p: p.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml);
        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            let base = baseNameOf path; in
            craneLib.filterCargoSources path type &&
            base != ".pi" && base != ".env";
        };
        common = { inherit src; strictDeps = true; };
        cargoArtifacts = craneLib.buildDepsOnly common;
        gateway = craneLib.buildPackage (common // { inherit cargoArtifacts; });
        gatewayImage = pkgs.dockerTools.buildLayeredImage {
          name = "possums-gateway";
          tag = "phase0";
          contents = [ gateway pkgs.cacert ];
          config = {
            Entrypoint = [ "${gateway}/bin/possums" ];
            Env = [ "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" ];
            User = "65532:65532";
            WorkingDir = "/tmp";
          };
        };
      in {
        packages = { default = gateway; gateway-image = gatewayImage; };
        checks = {
          inherit gateway;
          fmt = craneLib.cargoFmt { inherit src; };
          clippy = craneLib.cargoClippy (common // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets --all-features -- -D warnings";
          });
          tests = craneLib.cargoTest (common // { inherit cargoArtifacts; });
        };
        devShells.default = craneLib.devShell { checks = self.checks.${system}; };
      });
}
