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
    terminal-cell = {
      url = "github:LiGoldragon/terminal-cell";
      flake = false;
    };
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
          terminalCellSrc = craneLib.cleanCargoSource inputs.terminal-cell;
          terminalCellCommonArgs = {
            src = terminalCellSrc;
            strictDeps = true;
            nativeBuildInputs = [
              pkgs.bash
              pkgs.coreutils
            ];
            TERMINAL_CELL_TEST_SHELL = "${pkgs.bash}/bin/bash";
          };
          terminalCellCargoArtifacts = craneLib.buildDepsOnly terminalCellCommonArgs;
          terminalCellBinaries = craneLib.buildPackage (
            terminalCellCommonArgs
            // {
              cargoArtifacts = terminalCellCargoArtifacts;
              doCheck = false;
              cargoExtraArgs = "--bins";
              pname = "terminal-cell";
              meta.mainProgram = "terminal-cell-daemon";
            }
          );
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
              export PERSONA_DEV_STACK_SMOKE=${personaDevStack "smoke"}/bin/persona-dev-stack-smoke
              export PERSONA_TERMINAL_CELL_SMOKE=${
                personaEngineSandboxTerminalCellSmoke
              }/bin/persona-engine-sandbox-terminal-cell-smoke
              exec ${pkgs.bash}/bin/bash ${./scripts/persona-engine-sandbox} "$@"
            '';
          };
          personaEngineSandboxAttach = pkgs.writeShellApplication {
            name = "persona-engine-sandbox-attach";
            runtimeInputs = [
              pkgs.coreutils
              pkgs.util-linux
            ];
            text = ''
              exec ${pkgs.bash}/bin/bash ${./scripts/persona-engine-sandbox-attach} "$@"
            '';
          };
          personaEngineSandboxTerminalCellSmoke = pkgs.writeShellApplication {
            name = "persona-engine-sandbox-terminal-cell-smoke";
            runtimeInputs = [
              pkgs.coreutils
              pkgs.gnugrep
              pkgs.util-linux
            ];
            text = ''
              export PERSONA_BASH=${pkgs.bash}/bin/bash
              export TERMINAL_CELL_DAEMON=${terminalCellBinaries}/bin/terminal-cell-daemon
              export TERMINAL_CELL_SEND=${terminalCellBinaries}/bin/terminal-cell-send
              export TERMINAL_CELL_WAIT=${terminalCellBinaries}/bin/terminal-cell-wait
              export TERMINAL_CELL_CAPTURE=${terminalCellBinaries}/bin/terminal-cell-capture
              export TERMINAL_CELL_VIEW=${terminalCellBinaries}/bin/terminal-cell-view
              export PERSONA_ENGINE_SANDBOX_ATTACH=${
                personaEngineSandboxAttach
              }/bin/persona-engine-sandbox-attach
              exec ${pkgs.bash}/bin/bash ${./scripts/persona-engine-sandbox-terminal-cell-smoke} "$@"
            '';
          };
          personaEngineSandboxDevStackSmoke = pkgs.writeShellApplication {
            name = "persona-engine-sandbox-dev-stack-smoke";
            runtimeInputs = [
              pkgs.coreutils
            ];
            text = ''
              if [ "$#" -eq 0 ]; then
                sandbox_dir="$(mktemp -d -t persona-engine-sandbox-dev-stack-smoke.XXXXXX)"
                set -- --sandbox-dir "$sandbox_dir"
              fi
              exec ${personaEngineSandbox}/bin/persona-engine-sandbox --inside-unit --harness pi "$@"
            '';
          };
          personaEngineSandboxTerminalCellPiSmoke = pkgs.writeShellApplication {
            name = "persona-engine-sandbox-terminal-cell-pi-smoke";
            runtimeInputs = [
              pkgs.coreutils
            ];
            text = ''
              if [ "$#" -eq 0 ]; then
                sandbox_dir="$(mktemp -d -t persona-engine-sandbox-terminal-cell-pi.XXXXXX)"
                set -- --sandbox-dir "$sandbox_dir"
              fi
              exec ${personaEngineSandbox}/bin/persona-engine-sandbox --test terminal-cell --harness pi "$@"
            '';
          };
          personaEngineSandboxTerminalCellFixtureSmoke = pkgs.writeShellApplication {
            name = "persona-engine-sandbox-terminal-cell-fixture-smoke";
            runtimeInputs = [
              pkgs.coreutils
            ];
            text = ''
              if [ "$#" -eq 0 ]; then
                sandbox_dir="$(mktemp -d -t persona-engine-sandbox-terminal-cell-fixture.XXXXXX)"
                set -- --sandbox-dir "$sandbox_dir"
              fi
              exec ${personaEngineSandbox}/bin/persona-engine-sandbox --inside-unit --test terminal-cell --harness fixture "$@"
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
            personaEngineSandboxAttach
            personaEngineSandboxDevStackSmoke
            personaEngineSandboxTerminalCellSmoke
            personaEngineSandboxTerminalCellPiSmoke
            personaEngineSandboxTerminalCellFixtureSmoke
            terminalCellBinaries
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
          terminal-cell = context.terminalCellBinaries;
          persona-dev-stack = context.personaDevStack "run";
          persona-dev-stack-smoke = context.personaDevStack "smoke";
          persona-engine-sandbox = context.personaEngineSandbox;
          persona-engine-sandbox-attach = context.personaEngineSandboxAttach;
          persona-engine-sandbox-dev-stack-smoke = context.personaEngineSandboxDevStackSmoke;
          persona-engine-sandbox-terminal-cell-smoke = context.personaEngineSandboxTerminalCellSmoke;
          persona-engine-sandbox-terminal-cell-pi-smoke =
            context.personaEngineSandboxTerminalCellPiSmoke;
          persona-engine-sandbox-terminal-cell-fixture-smoke =
            context.personaEngineSandboxTerminalCellFixtureSmoke;
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
                  test -f "$root/artifacts/bwrap-profile.nota"
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
                grep -Fq 'DedicatedRunnerHome' "$root/artifacts/credential-policy.nota"
                grep -Fq 'live host credentials are not copied' "$root/artifacts/credential-policy.nota"
              '';
          persona-engine-sandbox-bootstrap-auth-dry-run =
            context.pkgs.runCommand "persona-engine-sandbox-bootstrap-auth-dry-run" { }
              ''
                mkdir -p "$out"
                for harness in pi claude codex codex-api; do
                  root=$out/$harness
                  mkdir -p "$root"
                  export PI_PACKAGE_DIR="$root/pi-package"
                  ${self.packages.${system}.persona-engine-sandbox}/bin/persona-engine-sandbox \
                    --dry-run \
                    --bootstrap-auth \
                    --harness "$harness" \
                    --sandbox-dir "$root/sandbox" \
                    --credential-root "$root/credentials" \
                    > "$root/bootstrap.stdout"
                  test -f "$root/sandbox/artifacts/auth-bootstrap.nota"
                  test -f "$root/sandbox/artifacts/auth-bootstrap-env.sh"
                  test ! -e "$root/credentials"
                done
                grep -Fq 'CODEX_HOME=' "$out/codex/sandbox/artifacts/auth-bootstrap-env.sh"
                grep -Fq 'codex login --device-auth' "$out/codex/sandbox/artifacts/auth-bootstrap.nota"
                grep -Fq 'CLAUDE_CONFIG_DIR=' "$out/claude/sandbox/artifacts/auth-bootstrap-env.sh"
                grep -Fq 'CLAUDE_CODE_OAUTH_TOKEN=' "$out/claude/sandbox/artifacts/auth-bootstrap-env.sh"
                grep -Fq 'claude auth login --claudeai' "$out/claude/sandbox/artifacts/auth-bootstrap.nota"
                grep -Fq 'PI_CODING_AGENT_DIR=' "$out/pi/sandbox/artifacts/auth-bootstrap-env.sh"
                grep -Fq 'PI_CODING_AGENT_SESSION_DIR=' "$out/pi/sandbox/artifacts/auth-bootstrap-env.sh"
                grep -Fq "PI_PACKAGE_DIR='$out/pi/pi-package'" "$out/pi/sandbox/artifacts/auth-bootstrap-env.sh"
                grep -Fq 'OPENAI_API_KEY=' "$out/codex-api/sandbox/artifacts/auth-bootstrap-env.sh"
                grep -Fq 'PERSONA_OPENAI_API_KEY_FILE' "$out/codex-api/sandbox/artifacts/auth-bootstrap.nota"
              '';
          persona-engine-sandbox-pi-bootstrap-creates-isolated-dirs =
            context.pkgs.runCommand "persona-engine-sandbox-pi-bootstrap-creates-isolated-dirs" { }
              ''
                mkdir -p "$out"
                PI_PACKAGE_DIR="$out/pi-package" \
                  ${self.packages.${system}.persona-engine-sandbox}/bin/persona-engine-sandbox \
                    --bootstrap-auth \
                    --harness pi \
                    --sandbox-dir "$out/sandbox" \
                    --credential-root "$out/credentials"
                test -d "$out/credentials/pi/config"
                test -d "$out/credentials/pi/session"
                grep -Fq "PI_PACKAGE_DIR='$out/pi-package'" "$out/sandbox/artifacts/auth-bootstrap-env.sh"
              '';
          persona-engine-sandbox-auth-isolation-witness =
            context.pkgs.runCommand "persona-engine-sandbox-auth-isolation-witness" { }
              ''
                mkdir -p "$out"
                PERSONA_ENGINE_SANDBOX_BIN=${
                  self.packages.${system}.persona-engine-sandbox
                }/bin/persona-engine-sandbox \
                  ${context.pkgs.bash}/bin/bash ${./scripts/persona-engine-sandbox-auth-isolation-witness} "$out"
                test -f "$out/auth-isolation-witness.nota"
                grep -Fq '(AuthIsolationWitness Passed)' "$out/auth-isolation-witness.nota"
              '';
          persona-engine-sandbox-attach-script-builds =
            context.pkgs.runCommand "persona-engine-sandbox-attach-script-builds" { }
              ''
                test -x ${self.packages.${system}.persona-engine-sandbox-attach}/bin/persona-engine-sandbox-attach
                touch $out
              '';
          persona-engine-sandbox-dev-stack-smoke-script-builds =
            context.pkgs.runCommand "persona-engine-sandbox-dev-stack-smoke-script-builds" { }
              ''
                test -x ${
                  self.packages.${system}.persona-engine-sandbox-dev-stack-smoke
                }/bin/persona-engine-sandbox-dev-stack-smoke
                touch $out
              '';
          persona-engine-sandbox-terminal-cell-script-builds =
            context.pkgs.runCommand "persona-engine-sandbox-terminal-cell-script-builds" { }
              ''
                test -x ${
                  self.packages.${system}.persona-engine-sandbox-terminal-cell-smoke
                }/bin/persona-engine-sandbox-terminal-cell-smoke
                test -x ${
                  self.packages.${system}.persona-engine-sandbox-terminal-cell-pi-smoke
                }/bin/persona-engine-sandbox-terminal-cell-pi-smoke
                test -x ${
                  self.packages.${system}.persona-engine-sandbox-terminal-cell-fixture-smoke
                }/bin/persona-engine-sandbox-terminal-cell-fixture-smoke
                test -x ${self.packages.${system}.terminal-cell}/bin/terminal-cell-daemon
                test -x ${self.packages.${system}.terminal-cell}/bin/terminal-cell-send
                test -x ${self.packages.${system}.terminal-cell}/bin/terminal-cell-wait
                test -x ${self.packages.${system}.terminal-cell}/bin/terminal-cell-capture
                test -x ${self.packages.${system}.terminal-cell}/bin/terminal-cell-view
                touch $out
              '';
          persona-engine-sandbox-attach-plans-host-ghostty =
            context.pkgs.runCommand "persona-engine-sandbox-attach-plans-host-ghostty" { }
              ''
                mkdir -p "$out/run"
                GHOSTTY_BIN="$out/fake-ghostty" \
                  ${self.packages.${system}.persona-engine-sandbox-attach}/bin/persona-engine-sandbox-attach \
                    --dry-run \
                    --sandbox-dir "$out" \
                    > "$out/attach.stdout"
                test -f "$out/artifacts/host-attach.nota"
                test -f "$out/artifacts/host-attach-command.txt"
                grep -Fq '(WaylandIntoSandbox false)' "$out/artifacts/host-attach.nota"
                grep -Fq "$out/run/cell.sock" "$out/artifacts/host-attach-command.txt"
                grep -Fq 'terminal-cell-view' "$out/artifacts/host-attach-command.txt"
                if grep -R -Fq 'WAYLAND_DISPLAY' "$out/artifacts"; then
                  exit 1
                fi
              '';
          persona-engine-sandbox-documents-bwrap-strict-profile =
            context.pkgs.runCommand "persona-engine-sandbox-documents-bwrap-strict-profile" { }
              ''
                mkdir -p "$out"
                ${self.packages.${system}.persona-engine-sandbox}/bin/persona-engine-sandbox \
                  --dry-run \
                  --harness pi \
                  --sandbox-dir "$out/sandbox" \
                  > "$out/dry-run.stdout"
                profile="$out/sandbox/artifacts/bwrap-profile.nota"
                grep -Fq '(ReadOnlyBind "/nix")' "$profile"
                grep -Fq '(ReadOnlyBind "/run/current-system")' "$profile"
                grep -Fq "(ReadWriteBind \"$out/sandbox\")" "$profile"
                grep -Fq '(WaylandSocketIntoSandbox false)' "$profile"
                grep -Fq '(Status DocumentedNotEnabled)' "$profile"
              '';
          persona-engine-sandbox-binds-dedicated-credential-root =
            context.pkgs.runCommand "persona-engine-sandbox-binds-dedicated-credential-root" { }
              ''
                mkdir -p "$out/credentials/existing"
                sandbox="$out/sandbox"
                ${self.packages.${system}.persona-engine-sandbox}/bin/persona-engine-sandbox \
                  --dry-run \
                  --harness fixture \
                  --sandbox-dir "$sandbox" \
                  --credential-root "$out/credentials" \
                  > "$out/dry-run.stdout"
                command="$sandbox/artifacts/systemd-command.txt"
                manifest="$sandbox/artifacts/sandbox-manifest.nota"
                profile="$sandbox/artifacts/bwrap-profile.nota"
                grep -Fq -- '--property=ProtectHome=tmpfs' "$command"
                grep -Fq -- "--property=ReadWritePaths=$sandbox" "$command"
                grep -Fq -- "--property=BindPaths=$out/credentials" "$command"
                if grep -Fq -- "--property=ReadWritePaths=$out/credentials" "$command"; then
                  exit 1
                fi
                grep -Fq "(CredentialRoot \"$out/credentials\")" "$manifest"
                grep -Fq "(ReadWriteBind \"$out/credentials\")" "$profile"
                touch "$out/passed"
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
          persona-component-commands-resolve-from-nix-closure = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              cargoTestExtraArgs = "--test engine constraint_component_commands_resolve_from_nix_closure -- --exact";
            }
          );
          persona-launch-config-overrides-one-component-command = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              cargoTestExtraArgs = "--test engine constraint_launch_config_overrides_one_component_command -- --exact";
            }
          );
          persona-spawn-envelope-carries-resolved-component-command = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              cargoTestExtraArgs = "--test engine constraint_spawn_envelope_carries_resolved_component_command -- --exact";
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
          persona-engine-event-log-records-typed-manager-events = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              cargoTestExtraArgs = "--test manager_store constraint_engine_event_log_records_typed_manager_events -- --exact";
            }
          );
          persona-engine-event-log-nota-projection-is-view = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              cargoTestExtraArgs = "--test manager_store constraint_engine_event_log_nota_projection_is_view -- --exact";
            }
          );
          persona-component-launcher-does-not-block-manager-mailbox = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              PERSONA_TEST_SHELL = "${context.pkgs.bash}/bin/bash";
              cargoTestExtraArgs = "--test direct_process constraint_component_launcher_does_not_block_manager_mailbox -- --exact";
            }
          );
          persona-component-launcher-reaps-process-group = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              PERSONA_TEST_SHELL = "${context.pkgs.bash}/bin/bash";
              cargoTestExtraArgs = "--test direct_process constraint_component_launcher_reaps_process_group -- --exact";
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
        persona-engine-sandbox-attach = {
          type = "app";
          program = "${
            self.packages.${system}.persona-engine-sandbox-attach
          }/bin/persona-engine-sandbox-attach";
        };
        persona-engine-sandbox-dev-stack-smoke = {
          type = "app";
          program = "${
            self.packages.${system}.persona-engine-sandbox-dev-stack-smoke
          }/bin/persona-engine-sandbox-dev-stack-smoke";
        };
        persona-engine-sandbox-terminal-cell-pi-smoke = {
          type = "app";
          program = "${
            self.packages.${system}.persona-engine-sandbox-terminal-cell-pi-smoke
          }/bin/persona-engine-sandbox-terminal-cell-pi-smoke";
        };
        persona-engine-sandbox-terminal-cell-fixture-smoke = {
          type = "app";
          program = "${
            self.packages.${system}.persona-engine-sandbox-terminal-cell-fixture-smoke
          }/bin/persona-engine-sandbox-terminal-cell-fixture-smoke";
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
