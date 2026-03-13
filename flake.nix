{
  description = "ABA: Agent Builds Agent";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, crane, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        craneLib = crane.mkLib pkgs;

        commonArgs = {
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;
          buildInputs = [ pkgs.openssl ]
            ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              pkgs.libiconv
              pkgs.darwin.apple_sdk.frameworks.Security
              pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
            ];
          nativeBuildInputs = [ pkgs.pkg-config ];
        };

        # Build deps separately for caching
        cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
          pname = "aba-deps";
        });

        # The core ABA agent binary
        aba = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = "aba";
        });

      in
      {
        packages = {
          default = aba;
          aba = aba;

          # --- Future packages: declared now, built when code exists ---
          # The agent can see these targets and implement toward them.
          # `nix build .#aba-server` will fail with a clear message until implemented.

          aba-server = builtins.throw ''
            aba-server is not yet implemented.
            This package will provide the HTTP API for remote agent orchestration.
            See specs/agent-core.md for the planned design.
          '';

          aba-dashboard = builtins.throw ''
            aba-dashboard is not yet implemented.
            This package will provide a web UI for monitoring Ralph loops.
            See specs/self-bootstrapping.md Tier 5: Observability.
          '';

          aba-worker = builtins.throw ''
            aba-worker is not yet implemented.
            This package will provide the background weaver process for remote execution.
          '';
        };

        # Container image for Coolify deployment
        packages.aba-image = pkgs.dockerTools.buildImage {
          name = "aba";
          tag = "latest";
          copyToRoot = pkgs.buildEnv {
            name = "aba-env";
            paths = [
              aba
              pkgs.bashInteractive
              pkgs.coreutils
              pkgs.git
              pkgs.cacert
            ];
          };
          config = {
            Cmd = [ "${aba}/bin/aba" ];
            Env = [
              "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
              "GIT_SSL_CAINFO=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
            ];
          };
        };

        devShells.default = craneLib.devShell {
          packages = with pkgs; [
            rust-analyzer
            cargo-watch
            sops
            age
          ];
        };

        checks = {
          inherit aba;
          aba-clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          });
          aba-fmt = craneLib.cargoFmt {
            src = craneLib.cleanCargoSource ./.;
          };
        };
      }
    );
}
