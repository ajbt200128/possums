{
  description = "Possums Phase 0 gateway";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    nixpkgs-unstable.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane/v0.21.0";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, nixpkgs-unstable, flake-utils, crane, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        unstablePkgs = import nixpkgs-unstable { inherit system; };
        # The pinned attestation SDK requires 1.27.2; retain the locked package sets.
        attestationGo = unstablePkgs.go_1_27.overrideAttrs (finalAttrs: _: {
          version = "1.27.2";
          src = unstablePkgs.fetchurl {
            url = "https://go.dev/dl/go${finalAttrs.version}.src.tar.gz";
            hash = "sha256-A0ldorpkiU1A9cSZLklFT6eLUGkGBP+Stq//UIG3bmI=";
          };
        });
        buildGoModule = unstablePkgs.buildGoModule.override { go = attestationGo; };
        craneLib = (crane.mkLib pkgs).overrideToolchain
          (p: p.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml);
        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            let
              base = baseNameOf path;
              sdkAssets = map toString [
                ./vendor/tinfoil/assets
                ./vendor/tinfoil/assets/genoa_cert_chain.pem
                ./vendor/tinfoil/assets/trusted_root.json
                ./vendor/tinfoil/assets/rekor_test_bundle.json
              ];
            in
            (craneLib.filterCargoSources path type || builtins.elem (toString path) sdkAssets) &&
            base != ".pi" && base != ".env";
        };
        common = {
          inherit src;
          strictDeps = true;
          SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
        };
        cargoArtifacts = craneLib.buildDepsOnly common;
        # Tests have their own check; image builds should not compile/run them again.
        gateway = craneLib.buildPackage (common // {
          inherit cargoArtifacts;
          doCheck = false;
        });
        attestationHelper = buildGoModule {
          pname = "possums-attestation";
          version = "0.1.0";
          src = ./attestation-helper;
          vendorHash = "sha256-n2X/SxW2bmHG27bSiRVC2S55Pct3fQWwneZGIAozk9g=";
          postInstall = ''
            mv $out/bin/attestation-helper $out/bin/possums-attestation
          '';
        };
        apiSmokeFixture = craneLib.buildPackage (common // {
          inherit cargoArtifacts;
          cargoExtraArgs = "--example api_smoke_fixture";
          doCheck = false;
          installPhaseCommand = ''
            mkdir -p $out/bin
            cp target/release/examples/api_smoke_fixture $out/bin/
          '';
        });
        gatewayEntrypoint = pkgs.writeShellScriptBin "possums-entrypoint" ''
          ulimit -c 0
          exec ${gateway}/bin/possums
        '';
        gatewayImage = pkgs.dockerTools.buildLayeredImage {
          name = "possums-gateway";
          tag = "phase0";
          contents = [ gateway gatewayEntrypoint attestationHelper pkgs.cacert ];
          config = {
            Entrypoint = [ "${gatewayEntrypoint}/bin/possums-entrypoint" ];
            Env = [ "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" ];
            User = "65532:65532";
            WorkingDir = "/tmp";
          };
        };
        # Cache external inputs, never the application-specific image assembly.
        imageStages = image: [
          image
          image.stream
          image.stream.conf
          image.stream.conf.layersJsonFile
        ] ++ image.stream.conf.layersJsonFile.exclude_paths;
        dependencyImage = gatewayImage.override {
          name = "possums-dependency-probe";
          contents = [ pkgs.cacert ];
          config = { };
        };
        releaseBuildDeps = (pkgs.linkFarm "release-build-deps"
          (pkgs.lib.imap0 (n: drv: {
            name = toString n;
            # Nix 2.34 rejects 25.05's null structured checks on input-only wrappers.
            # Remove those checks only here; real image output constraints stay intact.
            path = builtins.derivation (builtins.removeAttrs drv.inputDerivation.drvAttrs [ "outputChecks" ]);
          }) ([ gateway attestationHelper ] ++ imageStages dependencyImage))).overrideAttrs (_: {
            passthru.rebuildDerivations = map (drv: drv.drvPath)
              ([ gateway attestationHelper gatewayEntrypoint ] ++ imageStages gatewayImage);
          });
        smokeImage = pkgs.dockerTools.buildLayeredImage {
          name = "possums-gateway-smoke";
          tag = "phase0";
          contents = [ apiSmokeFixture pkgs.cacert ];
          config = {
            Entrypoint = [ "${apiSmokeFixture}/bin/api_smoke_fixture" ];
            Env = [ "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" ];
            User = "65532:65532";
            WorkingDir = "/tmp";
          };
        };
      in {
        packages = {
          default = gateway;
          attestation-helper = attestationHelper;
          api-smoke-fixture = apiSmokeFixture;
        } // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          gateway-image = gatewayImage;
          gateway-smoke-image = smokeImage;
          release-build-deps = releaseBuildDeps;
        };
        checks = {
          inherit gateway;
          fmt = craneLib.cargoFmt { inherit src; };
          clippy = craneLib.cargoClippy (common // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets --all-features -- -D warnings";
          });
          tests = craneLib.cargoTest (common // { inherit cargoArtifacts; });
          attestation-helper = attestationHelper;
        };
        devShells.default = craneLib.devShell { checks = self.checks.${system}; };
      });
}
