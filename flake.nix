{
  description = "Persona multi-harness coordination framework.";

  inputs = {
    nixpkgs.url = "github:LiGoldragon/nixpkgs?ref=main";

    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";

    crane.url = "github:ipetkov/crane";

    persona-harness.url = "github:LiGoldragon/persona-harness";
    persona-harness.inputs.nixpkgs.follows = "nixpkgs";
    persona-harness.inputs.fenix.follows = "fenix";
    persona-harness.inputs.crane.follows = "crane";
    persona-introspect.url = "github:LiGoldragon/persona-introspect";
    persona-introspect.inputs.nixpkgs.follows = "nixpkgs";
    persona-introspect.inputs.fenix.follows = "fenix";
    persona-introspect.inputs.crane.follows = "crane";
    persona-message.url = "github:LiGoldragon/persona-message";
    persona-mind.url = "github:LiGoldragon/persona-mind";
    persona-router.url = "github:LiGoldragon/persona-router";
    signal-persona.url = "github:LiGoldragon/signal-persona";
    signal-persona-mind.url = "github:LiGoldragon/signal-persona-mind";
    signal-persona-system.url = "github:LiGoldragon/signal-persona-system";
    signal-persona-system.inputs.nixpkgs.follows = "nixpkgs";
    signal-persona-system.inputs.fenix.follows = "fenix";
    signal-persona-system.inputs.crane.follows = "crane";
    persona-system.url = "github:LiGoldragon/persona-system";
    persona-system.inputs.nixpkgs.follows = "nixpkgs";
    persona-system.inputs.fenix.follows = "fenix";
    persona-system.inputs.crane.follows = "crane";
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
          toolchain = fenix.packages.${system}.stable.withComponents [
            "cargo"
            "rustc"
            "rustfmt"
            "clippy"
            "rust-analyzer"
            "rust-src"
          ];
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
          mkPrototypeLauncher =
            {
              name,
              actual,
              command,
              runtimeInputs ? [ ],
            }:
            pkgs.writeShellApplication {
              inherit name;
              runtimeInputs = [
                pkgs.bash
                pkgs.coreutils
              ]
              ++ runtimeInputs;
              text = ''
                state_dir="$(dirname "''${PERSONA_STATE_PATH:?}")"
                mkdir -p "$state_dir"
                {
                  printf 'engine=%s\n' "''${PERSONA_ENGINE_ID:?}"
                  printf 'component=%s\n' "''${PERSONA_COMPONENT:?}"
                  printf 'process=%s\n' "$$"
                  printf 'domain_socket=%s\n' "''${PERSONA_DOMAIN_SOCKET_PATH:?}"
                  printf 'supervision_socket=%s\n' "''${PERSONA_SUPERVISION_SOCKET_PATH:?}"
                  printf 'spawn_envelope=%s\n' "''${PERSONA_SPAWN_ENVELOPE:?}"
                  printf 'manager_socket=%s\n' "''${PERSONA_MANAGER_SOCKET:?}"
                  printf 'domain_mode=%s\n' "''${PERSONA_DOMAIN_SOCKET_MODE:?}"
                  printf 'supervision_mode=%s\n' "''${PERSONA_SUPERVISION_SOCKET_MODE:?}"
                  printf 'peer_count=%s\n' "''${PERSONA_PEER_SOCKET_COUNT:?}"
                  printf 'actual=%s\n' '${actual}'
                } > "$state_dir/$PERSONA_COMPONENT.env"
                ${command}
              '';
            };
          prototypeMindLauncher = mkPrototypeLauncher {
            name = "persona-mind-prototype-launcher";
            actual = "${inputs.persona-mind.packages.${system}.default}/bin/mind";
            command = ''
              exec ${inputs.persona-mind.packages.${system}.default}/bin/mind daemon \
                --socket "$PERSONA_DOMAIN_SOCKET_PATH" \
                --store "$PERSONA_STATE_PATH"
            '';
          };
          prototypeRouterLauncher = mkPrototypeLauncher {
            name = "persona-router-prototype-launcher";
            actual = "${inputs.persona-router.packages.${system}.default}/bin/persona-router-daemon";
            command = ''
              exec ${inputs.persona-router.packages.${system}.default}/bin/persona-router-daemon daemon \
                --socket "$PERSONA_DOMAIN_SOCKET_PATH" \
                --store "$PERSONA_STATE_PATH"
            '';
          };
          prototypeSystemLauncher = mkPrototypeLauncher {
            name = "persona-system-prototype-launcher";
            actual = "${inputs.persona-system.packages.${system}.default}/bin/persona-system-daemon";
            command = ''
              exec ${inputs.persona-system.packages.${system}.default}/bin/persona-system-daemon \
                "$PERSONA_DOMAIN_SOCKET_PATH"
            '';
          };
          prototypeHarnessLauncher = mkPrototypeLauncher {
            name = "persona-harness-prototype-launcher";
            actual = "${inputs.persona-harness.packages.${system}.default}/bin/persona-harness-daemon";
            command = ''
              exec ${inputs.persona-harness.packages.${system}.default}/bin/persona-harness-daemon \
                "$PERSONA_DOMAIN_SOCKET_PATH" \
                "$PERSONA_COMPONENT"
            '';
          };
          prototypeTerminalLauncher = mkPrototypeLauncher {
            name = "persona-terminal-prototype-launcher";
            actual = "${inputs.persona-terminal.packages.${system}.default}/bin/persona-terminal-supervisor";
            command = ''
              exec ${inputs.persona-terminal.packages.${system}.default}/bin/persona-terminal-supervisor \
                --socket "$PERSONA_DOMAIN_SOCKET_PATH" \
                --store "$PERSONA_STATE_PATH"
            '';
          };
          prototypeMessageLauncher = mkPrototypeLauncher {
            name = "persona-message-prototype-launcher";
            actual = "${inputs.persona-message.packages.${system}.default}/bin/persona-message-daemon";
            command = ''
              exec ${inputs.persona-message.packages.${system}.default}/bin/persona-message-daemon
            '';
          };
          prototypeIntrospectLauncher = mkPrototypeLauncher {
            name = "persona-introspect-prototype-launcher";
            actual = "${inputs.persona-introspect.packages.${system}.default}/bin/persona-introspect-daemon";
            command = ''
              exec ${inputs.persona-introspect.packages.${system}.default}/bin/persona-introspect-daemon
            '';
          };
          prototypeComponentLaunchers = pkgs.symlinkJoin {
            name = "persona-prototype-component-launchers";
            paths = [
              prototypeMindLauncher
              prototypeRouterLauncher
              prototypeSystemLauncher
              prototypeHarnessLauncher
              prototypeTerminalLauncher
              prototypeMessageLauncher
              prototypeIntrospectLauncher
            ];
          };
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
              export PERSONA_TERMINAL_CELL_SMOKE=${personaEngineSandboxTerminalCellSmoke}/bin/persona-engine-sandbox-terminal-cell-smoke
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
              export PERSONA_ENGINE_SANDBOX_ATTACH=${personaEngineSandboxAttach}/bin/persona-engine-sandbox-attach
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
            prototypeComponentLaunchers
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
          persona-introspect = inputs.persona-introspect.packages.${system}.default;
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
          persona-engine-sandbox-terminal-cell-pi-smoke = context.personaEngineSandboxTerminalCellPiSmoke;
          persona-engine-sandbox-terminal-cell-fixture-smoke =
            context.personaEngineSandboxTerminalCellFixtureSmoke;
          persona-prototype-component-launchers = context.prototypeComponentLaunchers;
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
          #
          # Each derivation captures exactly one wire-layer boundary so
          # a failure pinpoints which bytes-on-the-line shape regressed.
          # The chain has four tiers:
          #   T1 per-record round-trips (no daemon, pure)
          #   T2 origin-shape coverage (no daemon, pure)
          #   T3 negative tests / signals caught (no daemon, pure)
          #   T4 chained midway witnesses (4 intermediate artifacts, pure)
          #
          # T1 — per-record round-trips
          wire-message-channel-round-trip = context.pkgs.runCommand "wire-message-channel-round-trip" { } ''
            ${personaShims}/bin/wire-emit-message \
              --recipient designer \
              --body 'message-only round-trip' \
              | ${personaShims}/bin/wire-decode-message \
              --expect-recipient designer \
              --expect-body 'message-only round-trip'
            touch $out
          '';
          wire-stamped-submission-round-trip = context.pkgs.runCommand "wire-stamped-submission-round-trip" { } ''
            ${personaShims}/bin/wire-emit-message \
              --variant stamped \
              --recipient designer \
              --body 'stamped round-trip' \
              --origin 'internal:message' \
              --stamped-at 1234 \
              | ${personaShims}/bin/wire-decode-message \
              --expect-recipient designer \
              --expect-body 'stamped round-trip' \
              --expect-variant stamped \
              --expect-origin 'internal:message'
            touch $out
          '';
          wire-inbox-query-round-trip = context.pkgs.runCommand "wire-inbox-query-round-trip" { } ''
            ${personaShims}/bin/wire-emit-message \
              --variant inbox-query \
              --recipient designer \
              --body 'ignored for inbox-query' \
              | ${personaShims}/bin/wire-decode-message \
              --expect-recipient designer \
              --expect-body 'ignored for inbox-query' \
              --expect-variant inbox-query
            touch $out
          '';
          wire-submission-accepted-reply-round-trip = context.pkgs.runCommand "wire-submission-accepted-reply-round-trip" { } ''
            ${personaShims}/bin/wire-emit-message-reply \
              --variant submission-accepted \
              --slot 42 \
              | ${personaShims}/bin/wire-decode-message-reply \
              --expect submission-accepted \
              --expect-slot 42
            touch $out
          '';
          wire-inbox-listing-reply-round-trip = context.pkgs.runCommand "wire-inbox-listing-reply-round-trip" { } ''
            ${personaShims}/bin/wire-emit-message-reply \
              --variant inbox-listing \
              --entry 'slot=1,sender=owner,body=first message' \
              --entry 'slot=2,sender=owner,body=second message' \
              --entry 'slot=3,sender=mind,body=third from mind' \
              | ${personaShims}/bin/wire-decode-message-reply \
              --expect inbox-listing \
              --expect-entry-count 3 \
              --expect-entry-body 'second message' \
              --expect-entry-sender mind
            touch $out
          '';
          wire-message-unimplemented-reply-round-trip = context.pkgs.runCommand "wire-message-unimplemented-reply-round-trip" { } ''
            ${personaShims}/bin/wire-emit-message-reply \
              --variant unimplemented \
              --operation submission \
              --reason not-in-prototype-scope \
              | ${personaShims}/bin/wire-decode-message-reply \
              --expect unimplemented \
              --expect-operation submission
            touch $out
          '';

          # T2 — origin-shape coverage. Each MessageOrigin variant
          # must encode and decode byte-perfect through the channel
          # frame. The security boundary (SO_PEERCRED stamping)
          # depends on these shapes being stable.
          wire-stamped-origin-internal-mind-round-trip = context.pkgs.runCommand "wire-stamped-origin-internal-mind-round-trip" { } ''
            ${personaShims}/bin/wire-emit-message \
              --variant stamped --recipient designer --body witness \
              --origin 'internal:mind' --stamped-at 1 \
              | ${personaShims}/bin/wire-decode-message \
              --expect-recipient designer --expect-body witness \
              --expect-origin 'internal:mind'
            touch $out
          '';
          wire-stamped-origin-internal-router-round-trip = context.pkgs.runCommand "wire-stamped-origin-internal-router-round-trip" { } ''
            ${personaShims}/bin/wire-emit-message \
              --variant stamped --recipient designer --body witness \
              --origin 'internal:router' --stamped-at 1 \
              | ${personaShims}/bin/wire-decode-message \
              --expect-recipient designer --expect-body witness \
              --expect-origin 'internal:router'
            touch $out
          '';
          wire-stamped-origin-external-owner-round-trip = context.pkgs.runCommand "wire-stamped-origin-external-owner-round-trip" { } ''
            ${personaShims}/bin/wire-emit-message \
              --variant stamped --recipient designer --body witness \
              --origin 'external:owner' --stamped-at 1 \
              | ${personaShims}/bin/wire-decode-message \
              --expect-recipient designer --expect-body witness \
              --expect-origin 'external:owner'
            touch $out
          '';
          wire-stamped-origin-external-non-owner-uid-round-trip = context.pkgs.runCommand "wire-stamped-origin-external-non-owner-uid-round-trip" { } ''
            ${personaShims}/bin/wire-emit-message \
              --variant stamped --recipient designer --body witness \
              --origin 'external:non-owner-user:1000' --stamped-at 1 \
              | ${personaShims}/bin/wire-decode-message \
              --expect-recipient designer --expect-body witness \
              --expect-origin 'external:non-owner-user:1000'
            touch $out
          '';
          wire-stamped-origin-external-network-peer-round-trip = context.pkgs.runCommand "wire-stamped-origin-external-network-peer-round-trip" { } ''
            ${personaShims}/bin/wire-emit-message \
              --variant stamped --recipient designer --body witness \
              --origin 'external:network:10.0.0.1' --stamped-at 1 \
              | ${personaShims}/bin/wire-decode-message \
              --expect-recipient designer --expect-body witness \
              --expect-origin 'external:network:10.0.0.1'
            touch $out
          '';

          # T3 — negative tests / signals caught. The decoder must
          # reject malformed bytes, truncated frames, and wrong
          # frame kinds with a typed error rather than silently
          # passing or panicking with no diagnostic.
          wire-malformed-bytes-decode-rejects = context.pkgs.runCommand "wire-malformed-bytes-decode-rejects" { } ''
            set +e
            printf '\x42\x42\x42\x42garbage-not-a-frame' | \
              ${personaShims}/bin/wire-decode-message \
              --expect-recipient designer --expect-body anything 2> stderr.txt
            decode_exit=$?
            set -e
            if [ "$decode_exit" -eq 0 ]; then
              printf 'wire-decode-message did not reject malformed bytes\n' >&2
              exit 1
            fi
            test -s stderr.txt
            mkdir -p $out
            cp stderr.txt $out/stderr.txt
            printf 'rejected as expected with exit %s\n' "$decode_exit" > $out/witness.txt
          '';
          wire-truncated-frame-decode-rejects = context.pkgs.runCommand "wire-truncated-frame-decode-rejects" { } ''
            set +e
            ${personaShims}/bin/wire-emit-message \
              --recipient designer --body 'will be truncated' \
              | head -c 8 \
              | ${personaShims}/bin/wire-decode-message \
              --expect-recipient designer --expect-body 'will be truncated' 2> stderr.txt
            decode_exit=$?
            set -e
            if [ "$decode_exit" -eq 0 ]; then
              printf 'wire-decode-message did not reject truncated frame\n' >&2
              exit 1
            fi
            test -s stderr.txt
            mkdir -p $out
            cp stderr.txt $out/stderr.txt
            printf 'rejected as expected with exit %s\n' "$decode_exit" > $out/witness.txt
          '';
          wire-wrong-frame-kind-decode-rejects = context.pkgs.runCommand "wire-wrong-frame-kind-decode-rejects" { } ''
            set +e
            ${personaShims}/bin/wire-emit-message-reply \
              --variant submission-accepted --slot 1 \
              | ${personaShims}/bin/wire-decode-message \
              --expect-recipient designer --expect-body anything 2> stderr.txt
            decode_exit=$?
            set -e
            if [ "$decode_exit" -eq 0 ]; then
              printf 'wire-decode-message accepted a reply frame on the request decoder\n' >&2
              exit 1
            fi
            test -s stderr.txt
            mkdir -p $out
            cp stderr.txt $out/stderr.txt
            printf 'rejected as expected with exit %s\n' "$decode_exit" > $out/witness.txt
          '';

          # T4 — chained midway witnesses. Each step is its own
          # derivation; intermediate artifacts (frame bytes, NOTA
          # records) land in /nix/store/ and the final summary
          # derivation ties them together. If any link breaks, the
          # specific intermediate that diverged is the one that fails.
          wire-chain-request-bytes = context.pkgs.runCommand "wire-chain-request-bytes" { } ''
            ${personaShims}/bin/wire-emit-message \
              --variant stamped \
              --recipient designer \
              --body 'chained-midway-witness' \
              --origin 'external:owner' \
              --stamped-at 99 \
              > $out
          '';
          wire-chain-request-nota = context.pkgs.runCommand "wire-chain-request-nota" { } ''
            ${personaShims}/bin/wire-decode-message \
              --expect-recipient designer \
              --expect-body 'chained-midway-witness' \
              --expect-origin 'external:owner' \
              --capture-nota $out \
              < ${self.checks.${system}.wire-chain-request-bytes}
          '';
          wire-chain-reply-bytes = context.pkgs.runCommand "wire-chain-reply-bytes" { } ''
            ${personaShims}/bin/wire-emit-message-reply \
              --variant inbox-listing \
              --entry 'slot=1,sender=owner,body=chained-midway-witness' \
              > $out
          '';
          wire-chain-reply-nota = context.pkgs.runCommand "wire-chain-reply-nota" { } ''
            ${personaShims}/bin/wire-decode-message-reply \
              --expect inbox-listing \
              --expect-entry-count 1 \
              --expect-entry-body 'chained-midway-witness' \
              --capture-nota $out \
              < ${self.checks.${system}.wire-chain-reply-bytes}
          '';
          # T4-bonus: real-daemon midway witness. Spawn the actual
          # persona-message-daemon and a one-shot wire-tap-router as
          # its forwarding target. The tap captures the bytes the
          # daemon actually sends toward "the router," writes them to
          # a Nix-store artifact, and we decode + assert the origin
          # the daemon's SO_PEERCRED stamping produced. This is the
          # midway witness: the daemon's wire output is no longer a
          # black-box claim, it's an inspectable byte sequence.
          persona-message-daemon-stamps-origin-via-tap = context.pkgs.runCommand "persona-message-daemon-stamps-origin-via-tap" {
            nativeBuildInputs = [ context.pkgs.coreutils ];
          } ''
            set -euo pipefail
            workdir="$(mktemp -d)"
            tap_socket="$workdir/tap.sock"
            message_socket="$workdir/message.sock"
            spawn_envelope="$workdir/message.envelope"
            captured_bytes="$workdir/captured.bytes"
            tap_ready="$workdir/tap.ready"
            daemon_stderr="$workdir/daemon.stderr"
            cli_out="$workdir/cli.out"
            cli_err="$workdir/cli.err"

            # 1. Start the tap-router. It binds tap.sock, captures
            #    the first frame it receives, replies with a canned
            #    SubmissionAccepted slot=999, and exits.
            ${personaShims}/bin/wire-tap-router \
              --socket "$tap_socket" \
              --capture "$captured_bytes" \
              --reply 'submission-accepted-slot=999' \
              --ready-file "$tap_ready" &
            tap_pid=$!

            # Wait for the tap to be bound (ready file written).
            for _ in $(seq 1 100); do
              [ -f "$tap_ready" ] && break
              sleep 0.05
            done
            test -f "$tap_ready"

            # 2. Write the manager-style typed spawn envelope and start
            #    persona-message-daemon. It reads owner identity from the
            #    envelope, binds message.sock, and forwards to tap.sock as
            #    if it were the router.
            builder_uid="$(id -u)"
            printf '(SpawnEnvelope default Message Message (UnixUser %s) "%s" "%s" 432 "%s" 384 [(PeerSocket Router "%s")] "%s" 1)\n' \
              "$builder_uid" "$workdir" "$message_socket" "$workdir/message.supervision.sock" "$tap_socket" "$workdir/persona.sock" \
              > "$spawn_envelope"
            PERSONA_SPAWN_ENVELOPE="$spawn_envelope" \
              ${inputs.persona-message.packages.${system}.default}/bin/persona-message-daemon \
              "$message_socket" "$tap_socket" 2> "$daemon_stderr" &
            daemon_pid=$!

            # Wait for the message socket to appear.
            for _ in $(seq 1 100); do
              [ -S "$message_socket" ] && break
              sleep 0.05
            done
            test -S "$message_socket"

            # 3. Send a real message through the CLI. The daemon
            #    accepts, reads SO_PEERCRED, compares it to the envelope
            #    owner identity, wraps into StampedMessageSubmission,
            #    forwards to tap_socket. The tap captures and replies.
            set +e
            PERSONA_MESSAGE_SOCKET="$message_socket" \
              ${inputs.persona-message.packages.${system}.default}/bin/message \
              '(Send tap-recipient "tap-captured-body")' > "$cli_out" 2> "$cli_err"
            cli_exit=$?
            set -e

            # 4. Wait for the tap to finish writing the capture and exit.
            wait "$tap_pid" || true
            # Shut the daemon down — its job is done.
            kill "$daemon_pid" 2>/dev/null || true
            wait "$daemon_pid" 2>/dev/null || true

            # 5. Assert the capture exists.
            test -s "$captured_bytes"

            # 6. Decode the captured bytes through our shim and
            #    assert the daemon stamped the origin correctly.
            ${personaShims}/bin/wire-decode-message \
              --expect-recipient tap-recipient \
              --expect-body 'tap-captured-body' \
              --expect-variant stamped \
              --expect-origin 'external:owner' \
              --capture-nota "$workdir/stamped.nota" \
              < "$captured_bytes"

            # 7. Land artifacts in /nix/store/ for forensic inspection.
            mkdir -p $out
            cp "$captured_bytes" $out/captured.bytes
            cp "$workdir/stamped.nota" $out/stamped.nota
            cp "$cli_out" $out/cli.out
            cp "$cli_err" $out/cli.err
            cp "$daemon_stderr" $out/daemon.stderr
            printf 'midway witness: persona-message-daemon stamped origin in flight\n' > $out/witness.txt
            printf '  cli exit:        %s\n' "$cli_exit" >> $out/witness.txt
            printf '  captured bytes:  %s\n' "$(wc -c < $out/captured.bytes)" >> $out/witness.txt
            printf '  decoded nota:    %s\n' "$(cat $out/stamped.nota)" >> $out/witness.txt
            printf '  expected origin: External(Owner) (Nix builder uid)\n' >> $out/witness.txt
          '';

          wire-chain-summary = context.pkgs.runCommand "wire-chain-summary" { } ''
            mkdir -p $out
            cp ${self.checks.${system}.wire-chain-request-bytes} $out/request.bytes
            cp ${self.checks.${system}.wire-chain-request-nota} $out/request.nota
            cp ${self.checks.${system}.wire-chain-reply-bytes} $out/reply.bytes
            cp ${self.checks.${system}.wire-chain-reply-nota} $out/reply.nota
            # Consistency: the body string travels byte-stable across
            # all 4 intermediate artifacts. If any link mutates the
            # body, this final assertion catches it.
            grep -Fq 'chained-midway-witness' $out/request.nota || {
              printf 'request.nota missing expected body string\n' >&2
              exit 1
            }
            grep -Fq 'chained-midway-witness' $out/reply.nota || {
              printf 'reply.nota missing expected body string\n' >&2
              exit 1
            }
            request_size=$(wc -c < $out/request.bytes)
            reply_size=$(wc -c < $out/reply.bytes)
            printf 'chained witness: 4 intermediate artifacts consistent\n' > $out/chain-summary.txt
            printf '  request.bytes:  %s bytes\n' "$request_size" >> $out/chain-summary.txt
            printf '  request.nota:   %s\n' "$(cat $out/request.nota)" >> $out/chain-summary.txt
            printf '  reply.bytes:    %s bytes\n' "$reply_size" >> $out/chain-summary.txt
            printf '  reply.nota:     %s\n' "$(cat $out/reply.nota)" >> $out/chain-summary.txt
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
          persona-engine-layout-can-select-message-router-topology = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              cargoTestExtraArgs = "--test engine constraint_engine_layout_can_select_message_router_topology -- --exact";
            }
          );
          persona-spawn-envelope-carries-component-paths-and-peer-sockets = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              cargoTestExtraArgs = "--test engine constraint_spawn_envelope_carries_component_paths_and_peer_sockets -- --exact";
            }
          );
          persona-message-router-topology-spawn-envelope-has-one-peer-socket =
            context.craneLib.cargoTest
              (
                context.commonArgs
                // {
                  inherit (context) cargoArtifacts;
                  cargoTestExtraArgs = "--test engine constraint_message_router_topology_spawn_envelope_has_one_peer_socket -- --exact";
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
          persona-component-ready-requires-socket-metadata-from-spawn-envelope =
            context.craneLib.cargoTest
              (
                context.commonArgs
                // {
                  inherit (context) cargoArtifacts;
                  cargoTestExtraArgs = "--test readiness constraint_component_ready_requires_socket_metadata_from_spawn_envelope -- --exact";
                }
              );
          persona-component-ready-rejects-wrong-socket-mode =
            context.craneLib.cargoTest
              (
                context.commonArgs
                // {
                  inherit (context) cargoArtifacts;
                  cargoTestExtraArgs = "--test readiness constraint_component_ready_rejects_wrong_socket_mode -- --exact";
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
          persona-component-launcher-passes-spawn-envelope-environment = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              PERSONA_TEST_SHELL = "${context.pkgs.bash}/bin/bash";
              cargoTestExtraArgs = "--test direct_process constraint_component_launcher_passes_spawn_envelope_to_child_environment -- --exact";
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
          persona-engine-supervisor-launches-prototype-supervised-components-through-process-launcher =
            context.craneLib.cargoTest
              (
                context.commonArgs
                // {
                  inherit (context) cargoArtifacts;
                  PERSONA_TEST_SHELL = "${context.pkgs.bash}/bin/bash";
                  cargoTestExtraArgs = "--test supervisor constraint_engine_supervisor_launches_prototype_supervised_components_through_process_launcher -- --exact";
                }
              );
          persona-engine-supervisor-launches-message-router-topology-without-full-stack =
            context.craneLib.cargoTest
              (
                context.commonArgs
                // {
                  inherit (context) cargoArtifacts;
                  PERSONA_TEST_SHELL = "${context.pkgs.bash}/bin/bash";
                  cargoTestExtraArgs = "--test supervisor constraint_engine_supervisor_launches_message_router_topology_without_full_stack -- --exact";
                }
              );
          persona-daemon-persists-cli-mutation-to-manager-store = context.craneLib.cargoTest (
            context.commonArgs
            // {
              inherit (context) cargoArtifacts;
              cargoTestExtraArgs = "--test daemon constraint_persona_daemon_persists_cli_mutation_to_manager_store -- --exact";
            }
          );
          persona-daemon-launches-prototype-supervised-components-through-engine-supervisor =
            context.craneLib.cargoTest
              (
                context.commonArgs
                // {
                  inherit (context) cargoArtifacts;
                  PERSONA_TEST_SHELL = "${context.pkgs.bash}/bin/bash";
                  cargoTestExtraArgs = "--test daemon constraint_persona_daemon_launches_prototype_supervised_components_through_engine_supervisor -- --exact";
                }
              );
          persona-daemon-launches-message-router-topology-through-engine-supervisor =
            context.craneLib.cargoTest
              (
                context.commonArgs
                // {
                  inherit (context) cargoArtifacts;
                  PERSONA_TEST_SHELL = "${context.pkgs.bash}/bin/bash";
                  cargoTestExtraArgs = "--test daemon constraint_persona_daemon_launches_message_router_topology_through_engine_supervisor -- --exact";
                }
              );
          persona-daemon-launches-nix-built-prototype-topology =
            context.pkgs.runCommand "persona-daemon-launches-nix-built-prototype-topology"
              {
                nativeBuildInputs = [
                  context.pkgs.bash
                  context.pkgs.coreutils
                  context.pkgs.gnugrep
                ];
              }
              ''
                set -eu
                work="$TMPDIR/persona-daemon-nix-built-topology"
                mkdir -p "$work/state" "$work/run" "$work/artifacts"
                manager_socket="$work/persona.sock"

                export PERSONA_MANAGER_STORE="$work/manager.redb"
                export PERSONA_STATE_ROOT="$work/state"
                export PERSONA_RUN_ROOT="$work/run"
                export PERSONA_MIND_EXECUTABLE=${context.prototypeComponentLaunchers}/bin/persona-mind-prototype-launcher
                export PERSONA_ROUTER_EXECUTABLE=${context.prototypeComponentLaunchers}/bin/persona-router-prototype-launcher
                export PERSONA_SYSTEM_EXECUTABLE=${context.prototypeComponentLaunchers}/bin/persona-system-prototype-launcher
                export PERSONA_HARNESS_EXECUTABLE=${context.prototypeComponentLaunchers}/bin/persona-harness-prototype-launcher
                export PERSONA_TERMINAL_EXECUTABLE=${context.prototypeComponentLaunchers}/bin/persona-terminal-prototype-launcher
                export PERSONA_MESSAGE_DAEMON_EXECUTABLE=${context.prototypeComponentLaunchers}/bin/persona-message-prototype-launcher
                export PERSONA_INTROSPECT_DAEMON_EXECUTABLE=${context.prototypeComponentLaunchers}/bin/persona-introspect-prototype-launcher

                ${self.packages.${system}.default}/bin/persona-daemon "$manager_socket" \
                  > "$work/persona-daemon.stdout" \
                  2> "$work/persona-daemon.stderr" &
                daemon="$!"

                cleanup() {
                  kill "$daemon" 2>/dev/null || true
                  if [ -d "$work/state/default" ]; then
                    for capture in "$work/state/default"/*.env; do
                      [ -e "$capture" ] || continue
                      process="$(sed -n 's/^process=//p' "$capture" | head -n 1)"
                      if [ -n "$process" ]; then
                        kill -- "-$process" 2>/dev/null || true
                      fi
                    done
                  fi
                  wait "$daemon" 2>/dev/null || true
                }
                trap cleanup EXIT

                for attempt in $(seq 1 100); do
                  if grep -Fq "persona-daemon socket=$manager_socket" "$work/persona-daemon.stdout"; then
                    break
                  fi
                  if ! kill -0 "$daemon" 2>/dev/null; then
                    cat "$work/persona-daemon.stdout"
                    cat "$work/persona-daemon.stderr" >&2
                    exit 1
                  fi
                  sleep 0.1
                done
                grep -Fq "persona-daemon socket=$manager_socket" "$work/persona-daemon.stdout"

                for component in mind router system harness terminal message introspect; do
                  capture="$work/state/default/$component.env"
                  for attempt in $(seq 1 100); do
                    if [ -f "$capture" ]; then
                      break
                    fi
                    sleep 0.1
                  done
                  test -f "$capture"
                  grep -Fx "engine=default" "$capture"
                  grep -Fx "component=$component" "$capture"
                  grep -Fx "peer_count=6" "$capture"
                  grep -Fx "spawn_envelope=$work/run/default/$component.envelope" "$capture"
                  grep -Fx "manager_socket=$work/persona.sock" "$capture"
                  if [ "$component" = "message" ]; then
                    grep -Fx "domain_mode=660" "$capture"
                  else
                    grep -Fx "domain_mode=600" "$capture"
                  fi
                  grep -Fx "supervision_mode=600" "$capture"
                  test -f "$work/run/default/$component.envelope"
                  grep -Fq "(SpawnEnvelope default" "$work/run/default/$component.envelope"
                  grep -Fq "$component.sock" "$work/run/default/$component.envelope"
                  grep -Fq "$component.supervision.sock" "$work/run/default/$component.envelope"
                  grep -Fq "\"$work/persona.sock\"" "$work/run/default/$component.envelope"
                done

                grep -Fx "actual=${
                  inputs.persona-mind.packages.${system}.default
                }/bin/mind" "$work/state/default/mind.env"
                grep -Fx "actual=${
                  inputs.persona-router.packages.${system}.default
                }/bin/persona-router-daemon" "$work/state/default/router.env"
                grep -Fx "actual=${
                  inputs.persona-system.packages.${system}.default
                }/bin/persona-system-daemon" "$work/state/default/system.env"
                grep -Fx "actual=${
                  inputs.persona-harness.packages.${system}.default
                }/bin/persona-harness-daemon" "$work/state/default/harness.env"
                grep -Fx "actual=${
                  inputs.persona-terminal.packages.${system}.default
                }/bin/persona-terminal-supervisor" "$work/state/default/terminal.env"
                grep -Fx "actual=${
                  inputs.persona-message.packages.${system}.default
                }/bin/persona-message-daemon" "$work/state/default/message.env"
                grep -Fx "actual=${
                  inputs.persona-introspect.packages.${system}.default
                }/bin/persona-introspect-daemon" "$work/state/default/introspect.env"

                for socket in mind router system harness terminal message introspect; do
                  path="$work/run/default/$socket.sock"
                  for attempt in $(seq 1 100); do
                    if [ -S "$path" ]; then
                      break
                    fi
                    sleep 0.1
                  done
                  if [ ! -S "$path" ]; then
                    echo "missing component socket: $path" >&2
                    cat "$work/persona-daemon.stdout" >&2
                    cat "$work/persona-daemon.stderr" >&2
                    exit 1
                  fi
                  actual_mode="$(stat -c '%a' "$path")"
                  if [ "$socket" = "message" ]; then
                    test "$actual_mode" = "660"
                  else
                    test "$actual_mode" = "600"
                  fi
                done

                mkdir -p "$out"
                cp "$work/persona-daemon.stdout" "$out/persona-daemon.stdout"
                cp "$work/persona-daemon.stderr" "$out/persona-daemon.stderr"
                cp "$work/state/default"/*.env "$out/"
                touch "$out/passed"
              '';
          persona-daemon-launches-nix-built-message-router-topology =
            context.pkgs.runCommand "persona-daemon-launches-nix-built-message-router-topology"
              {
                nativeBuildInputs = [
                  context.pkgs.bash
                  context.pkgs.coreutils
                  context.pkgs.gnugrep
                ];
              }
              ''
                set -eu
                work="$TMPDIR/persona-daemon-nix-built-message-router"
                mkdir -p "$work/state" "$work/run" "$work/artifacts"
                manager_socket="$work/persona.sock"

                export PERSONA_MANAGER_STORE="$work/manager.redb"
                export PERSONA_STATE_ROOT="$work/state"
                export PERSONA_RUN_ROOT="$work/run"
                export PERSONA_ENGINE_TOPOLOGY=message-router
                export PERSONA_ROUTER_EXECUTABLE=${context.prototypeComponentLaunchers}/bin/persona-router-prototype-launcher
                export PERSONA_MESSAGE_DAEMON_EXECUTABLE=${context.prototypeComponentLaunchers}/bin/persona-message-prototype-launcher

                ${self.packages.${system}.default}/bin/persona-daemon "$manager_socket" \
                  > "$work/persona-daemon.stdout" \
                  2> "$work/persona-daemon.stderr" &
                daemon="$!"

                cleanup() {
                  kill "$daemon" 2>/dev/null || true
                  if [ -d "$work/state/default" ]; then
                    for capture in "$work/state/default"/*.env; do
                      [ -e "$capture" ] || continue
                      process="$(sed -n 's/^process=//p' "$capture" | head -n 1)"
                      if [ -n "$process" ]; then
                        kill -- "-$process" 2>/dev/null || true
                      fi
                    done
                  fi
                  wait "$daemon" 2>/dev/null || true
                }
                trap cleanup EXIT

                for attempt in $(seq 1 100); do
                  if grep -Fq "persona-daemon socket=$manager_socket" "$work/persona-daemon.stdout"; then
                    break
                  fi
                  if ! kill -0 "$daemon" 2>/dev/null; then
                    cat "$work/persona-daemon.stdout"
                    cat "$work/persona-daemon.stderr" >&2
                    exit 1
                  fi
                  sleep 0.1
                done
                grep -Fq "persona-daemon socket=$manager_socket" "$work/persona-daemon.stdout"

                for component in message router; do
                  capture="$work/state/default/$component.env"
                  for attempt in $(seq 1 100); do
                    if [ -f "$capture" ]; then
                      break
                    fi
                    sleep 0.1
                  done
                  test -f "$capture"
                  grep -Fx "engine=default" "$capture"
                  grep -Fx "component=$component" "$capture"
                  grep -Fx "peer_count=1" "$capture"
                  grep -Fx "spawn_envelope=$work/run/default/$component.envelope" "$capture"
                  grep -Fx "manager_socket=$work/persona.sock" "$capture"
                  if [ "$component" = "message" ]; then
                    grep -Fx "domain_mode=660" "$capture"
                  else
                    grep -Fx "domain_mode=600" "$capture"
                  fi
                  grep -Fx "supervision_mode=600" "$capture"
                  test -f "$work/run/default/$component.envelope"
                  grep -Fq "(SpawnEnvelope default" "$work/run/default/$component.envelope"
                  grep -Fq "$component.sock" "$work/run/default/$component.envelope"
                  grep -Fq "$component.supervision.sock" "$work/run/default/$component.envelope"
                done

                for absent in mind system harness terminal introspect; do
                  test ! -e "$work/state/default/$absent.env"
                  test ! -e "$work/run/default/$absent.envelope"
                  test ! -S "$work/run/default/$absent.sock"
                done

                for socket in message router; do
                  path="$work/run/default/$socket.sock"
                  for attempt in $(seq 1 100); do
                    if [ -S "$path" ]; then
                      break
                    fi
                    sleep 0.1
                  done
                  if [ ! -S "$path" ]; then
                    echo "missing component socket: $path" >&2
                    cat "$work/persona-daemon.stdout" >&2
                    cat "$work/persona-daemon.stderr" >&2
                    exit 1
                  fi
                  actual_mode="$(stat -c '%a' "$path")"
                  if [ "$socket" = "message" ]; then
                    test "$actual_mode" = "660"
                  else
                    test "$actual_mode" = "600"
                  fi
                done

                send_output="$work/message-send.nota"
                send_error="$work/message-send.stderr"
                inbox_output="$work/message-inbox.nota"
                inbox_error="$work/message-inbox.stderr"

                PERSONA_MESSAGE_SOCKET="$work/run/default/message.sock" \
                  ${inputs.persona-message.packages.${system}.default}/bin/message \
                    '(Send responder "supervised message-router smoke")' \
                    > "$send_output" \
                    2> "$send_error"
                grep -Fq "(SubmissionAccepted " "$send_output"

                PERSONA_MESSAGE_SOCKET="$work/run/default/message.sock" \
                  ${inputs.persona-message.packages.${system}.default}/bin/message \
                    '(Inbox responder)' \
                    > "$inbox_output" \
                    2> "$inbox_error"
                grep -Fq "supervised message-router smoke" "$inbox_output"

                mkdir -p "$out"
                cp "$work/persona-daemon.stdout" "$out/persona-daemon.stdout"
                cp "$work/persona-daemon.stderr" "$out/persona-daemon.stderr"
                cp "$send_output" "$out/message-send.nota"
                cp "$send_error" "$out/message-send.stderr"
                cp "$inbox_output" "$out/message-inbox.nota"
                cp "$inbox_error" "$out/message-inbox.stderr"
                cp "$work/state/default"/*.env "$out/"
                touch "$out/passed"
              '';
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
