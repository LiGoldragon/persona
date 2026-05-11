{
  description = "Persona multi-harness coordination framework.";

  inputs = {
    nixpkgs.url = "github:LiGoldragon/nixpkgs?ref=main";

    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";

    crane.url = "github:ipetkov/crane";

    persona-harness.url = "github:LiGoldragon/persona-harness";
    persona-message.url = "github:LiGoldragon/persona-message";
    persona-mind.url = "github:LiGoldragon/persona-mind";
    persona-router.url = "github:LiGoldragon/persona-router";
    signal-persona.url = "github:LiGoldragon/signal-persona";
    signal-persona-mind.url = "github:LiGoldragon/signal-persona-mind";
    signal-persona-system.url = "github:LiGoldragon/signal-persona-system";
    persona-system.url = "github:LiGoldragon/persona-system";
    persona-terminal.url = "github:LiGoldragon/persona-terminal";
  };

  outputs =
    inputs@{
      self,
      nixpkgs,
      fenix,
      crane,
      ...
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forSystems = function: nixpkgs.lib.genAttrs systems (system: function system);

      mkContext =
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          toolchain = fenix.packages.${system}.fromToolchainFile {
            file = ./rust-toolchain.toml;
            sha256 = "sha256-gh/xTkxKHL4eiRXzWv8KP7vfjSk61Iq48x47BEDFgfk=";
          };
          craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
          src = craneLib.cleanCargoSource ./.;
          cargoVendorDir = craneLib.vendorCargoDeps { inherit src; };
          commonArgs = {
            inherit src cargoVendorDir;
            strictDeps = true;
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          personaDevStack =
            mode:
            pkgs.writeShellApplication {
              name = if mode == "smoke" then "persona-dev-stack-smoke" else "persona-dev-stack";
              runtimeInputs = [
                pkgs.coreutils
                pkgs.gnugrep
              ];
              text = ''
                export PERSONA_MESSAGE_PACKAGE=${inputs.persona-message.packages.${system}.default}
                export PERSONA_ROUTER_PACKAGE=${inputs.persona-router.packages.${system}.default}
                export PERSONA_TERMINAL_PACKAGE=${inputs.persona-terminal.packages.${system}.default}
                export PERSONA_BASH=${pkgs.bash}/bin/bash
                exec ${pkgs.bash}/bin/bash ${./scripts/persona-dev-stack} ${mode} "$@"
              '';
            };
          personaEngineSandbox = pkgs.writeShellApplication {
            name = "persona-engine-sandbox";
            runtimeInputs = [
              pkgs.coreutils
              pkgs.systemd
            ];
            text = ''
              export PERSONA_BASH=${pkgs.bash}/bin/bash
              exec ${pkgs.bash}/bin/bash ${./scripts/persona-engine-sandbox} "$@"
            '';
          };
        in
        {
          inherit
            pkgs
            toolchain
            craneLib
            commonArgs
            cargoArtifacts
            personaDevStack
            personaEngineSandbox
            ;
        };
    in
    {
      packages = forSystems (
        system:
        let
          context = mkContext system;
        in
        {
          default = context.craneLib.buildPackage (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              pname = "persona";
              meta.mainProgram = "persona";
            }
          );
          persona-harness = inputs.persona-harness.packages.${system}.default;
          persona-message = inputs.persona-message.packages.${system}.default;
          persona-mind = inputs.persona-mind.packages.${system}.default;
          persona-router = inputs.persona-router.packages.${system}.default;
          signal-persona = inputs.signal-persona.packages.${system}.default;
          signal-persona-mind = inputs.signal-persona-mind.packages.${system}.default;
          signal-persona-system = inputs.signal-persona-system.packages.${system}.default;
          persona-system = inputs.persona-system.packages.${system}.default;
          persona-terminal = inputs.persona-terminal.packages.${system}.default;
          persona-dev-stack = context.personaDevStack "run";
          persona-dev-stack-smoke = context.personaDevStack "smoke";
          persona-engine-sandbox = context.personaEngineSandbox;
        }
      );

      checks = forSystems (
        system:
        let
          context = mkContext system;
          personaShims = self.packages.${system}.default;
        in
        {
          default = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
            }
          );
          persona-harness = inputs.persona-harness.checks.${system}.default;
          persona-message = inputs.persona-message.checks.${system}.default;
          persona-mind = inputs.persona-mind.checks.${system}.default;
          persona-router = inputs.persona-router.checks.${system}.default;
          signal-persona-build = inputs.signal-persona.checks.${system}.build;
          signal-persona-test = inputs.signal-persona.checks.${system}.test;
          signal-persona-test-engine-manager = inputs.signal-persona.checks.${system}.test-engine-manager;
          signal-persona-test-version = inputs.signal-persona.checks.${system}.test-version;
          signal-persona-doc = inputs.signal-persona.checks.${system}.doc;
          signal-persona-fmt = inputs.signal-persona.checks.${system}.fmt;
          signal-persona-clippy = inputs.signal-persona.checks.${system}.clippy;
          signal-persona-mind = inputs.signal-persona-mind.checks.${system}.test;
          signal-persona-system-build = inputs.signal-persona-system.checks.${system}.build;
          signal-persona-system-test = inputs.signal-persona-system.checks.${system}.test;
          signal-persona-system-round-trip = inputs.signal-persona-system.checks.${system}.test-round-trip;
          signal-persona-system-test-doc = inputs.signal-persona-system.checks.${system}.test-doc;
          signal-persona-system-doc = inputs.signal-persona-system.checks.${system}.doc;
          signal-persona-system-fmt = inputs.signal-persona-system.checks.${system}.fmt;
          signal-persona-system-clippy = inputs.signal-persona-system.checks.${system}.clippy;
          persona-system = inputs.persona-system.checks.${system}.default;
          persona-terminal = inputs.persona-terminal.checks.${system}.default;

          # ─── Wire-test chain: signal-persona-message ───
          wire-message-channel-round-trip = context.pkgs.runCommand "wire-message-channel-round-trip" { } ''
            ${personaShims}/bin/wire-emit-message \
              --recipient designer \
              --body 'message-only round-trip' \
              | ${personaShims}/bin/wire-decode-message \
              --expect-recipient designer \
              --expect-body 'message-only round-trip'
            touch $out
          '';
          persona-dev-stack-script-builds = context.pkgs.runCommand "persona-dev-stack-script-builds" { } ''
            test -x ${self.packages.${system}.persona-dev-stack}/bin/persona-dev-stack
            test -x ${self.packages.${system}.persona-dev-stack-smoke}/bin/persona-dev-stack-smoke
            touch $out
          '';
          persona-engine-sandbox-script-builds =
            context.pkgs.runCommand "persona-engine-sandbox-script-builds" { }
              ''
                test -x ${self.packages.${system}.persona-engine-sandbox}/bin/persona-engine-sandbox
                touch $out
              '';
          persona-engine-sandbox-supports-all-harnesses =
            context.pkgs.runCommand "persona-engine-sandbox-supports-all-harnesses" { }
              ''
                mkdir -p "$out"
                for harness in pi claude codex codex-api; do
                  root=$out/$harness
                  mkdir -p "$root"
                  ${self.packages.${system}.persona-engine-sandbox}/bin/persona-engine-sandbox \
                    --dry-run \
                    --harness "$harness" \
                    --sandbox-dir "$root" \
                    > "$root/dry-run.stdout"
                  test -d "$root/state"
                  test -d "$root/run"
                  test -d "$root/home"
                  test -d "$root/work"
                  test -d "$root/artifacts"
                  test -f "$root/artifacts/sandbox-manifest.nota"
                  test -f "$root/artifacts/credential-policy.nota"
                  test -f "$root/artifacts/systemd-command.txt"
                done
                grep -Fq '(Harness Pi)' "$out/pi/artifacts/sandbox-manifest.nota"
                grep -Fq '(Harness Claude)' "$out/claude/artifacts/sandbox-manifest.nota"
                grep -Fq '(Harness Codex)' "$out/codex/artifacts/sandbox-manifest.nota"
                grep -Fq '(Harness CodexApi)' "$out/codex-api/artifacts/sandbox-manifest.nota"
              '';
          persona-engine-sandbox-documents-dedicated-auth =
            context.pkgs.runCommand "persona-engine-sandbox-documents-dedicated-auth" { }
              ''
                mkdir -p "$out"
                root=$out/codex
                mkdir -p "$root"
                ${self.packages.${system}.persona-engine-sandbox}/bin/persona-engine-sandbox \
                  --dry-run \
                  --harness codex \
                  --sandbox-dir "$root"
                grep -Fq 'DedicatedRunnerHome' "$root/artifacts/credential-policy.nota"
                grep -Fq 'live host auth.json is not copied' "$root/artifacts/credential-policy.nota"

                root=$out/claude
                mkdir -p "$root"
                ${self.packages.${system}.persona-engine-sandbox}/bin/persona-engine-sandbox \
                  --dry-run \
                  --harness claude \
                  --sandbox-dir "$root"
                grep -Fq 'MissingDedicatedCredential' "$root/artifacts/credential-policy.nota"
                grep -Fq 'live host credentials are not copied' "$root/artifacts/credential-policy.nota"
              '';
          persona-engine-layout-uses-engine-id-scoped-paths = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              cargoTestExtraArgs = "--test engine constraint_engine_layout_uses_engine_id_scoped_paths -- --exact";
            }
          );
          persona-engine-layout-assigns-socket-modes-by-component-boundary = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              cargoTestExtraArgs = "--test engine constraint_engine_layout_assigns_socket_modes_by_component_boundary -- --exact";
            }
          );
          persona-spawn-envelope-carries-component-paths-and-peer-sockets = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              cargoTestExtraArgs = "--test engine constraint_spawn_envelope_carries_component_paths_and_peer_sockets -- --exact";
            }
          );
          persona-engine-layout-prepares-only-engine-scoped-directories = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              cargoTestExtraArgs = "--test engine constraint_engine_layout_prepares_only_engine_scoped_directories -- --exact";
            }
          );
          persona-manager-store-writes-engine-status-through-writer-actor = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              cargoTestExtraArgs = "--test manager_store constraint_manager_store_writes_engine_status_through_writer_actor -- --exact";
            }
          );
          persona-engine-manager-persists-component-mutation-through-manager-store =
            context.craneLib.cargoTest
              (
                context.commonArgs
                // {
                  inherit (context) cargoArtifacts;
                  cargoTestExtraArgs = "--test manager_store constraint_engine_manager_persists_component_mutation_through_manager_store -- --exact";
                }
              );
          persona-daemon-persists-cli-mutation-to-manager-store = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              cargoTestExtraArgs = "--test daemon constraint_persona_daemon_persists_cli_mutation_to_manager_store -- --exact";
            }
          );
        }
      );

      apps = forSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/persona";
        };
        dev-stack = {
          type = "app";
          program = "${self.packages.${system}.persona-dev-stack}/bin/persona-dev-stack";
        };
        dev-stack-smoke = {
          type = "app";
          program = "${self.packages.${system}.persona-dev-stack-smoke}/bin/persona-dev-stack-smoke";
        };
        persona-daemon = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/persona-daemon";
        };
        persona-engine-sandbox = {
          type = "app";
          program = "${self.packages.${system}.persona-engine-sandbox}/bin/persona-engine-sandbox";
        };
      });

      devShells = forSystems (
        system:
        let
          context = mkContext system;
        in
        {
          default = context.pkgs.mkShell {
            packages = [
              context.toolchain
              context.pkgs.jujutsu
              context.pkgs.nix
              context.pkgs.nixos-rebuild
            ];
          };
        }
      );

      formatter = forSystems (
        system:
        let
          context = mkContext system;
        in
        context.pkgs.nixfmt
      );
    };
}
