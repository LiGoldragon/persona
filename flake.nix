{
  description = "Persona multi-harness coordination framework.";

  inputs = {
    nixpkgs.url = "github:LiGoldragon/nixpkgs?ref=main";

    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";

    crane.url = "github:ipetkov/crane";

    persona-harness.url = "github:LiGoldragon/persona-harness";
    persona-message.url = "github:LiGoldragon/persona-message";
    persona-orchestrate.url = "github:LiGoldragon/persona-orchestrate";
    persona-router.url = "github:LiGoldragon/persona-router";
    signal-persona.url = "github:LiGoldragon/signal-persona";
    persona-sema.url = "github:LiGoldragon/persona-sema";
    persona-system.url = "github:LiGoldragon/persona-system";
    persona-wezterm.url = "github:LiGoldragon/persona-wezterm";
  };

  outputs =
    inputs@{ self, nixpkgs, fenix, crane, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
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
        in
        {
          inherit pkgs toolchain craneLib commonArgs cargoArtifacts;
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
          persona-orchestrate = inputs.persona-orchestrate.packages.${system}.default;
          persona-router = inputs.persona-router.packages.${system}.default;
          signal-persona = inputs.signal-persona.packages.${system}.default;
          persona-sema = inputs.persona-sema.packages.${system}.default;
          persona-system = inputs.persona-system.packages.${system}.default;
          persona-wezterm = inputs.persona-wezterm.packages.${system}.default;
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
          persona-orchestrate = inputs.persona-orchestrate.checks.${system}.default;
          persona-router = inputs.persona-router.checks.${system}.default;
          signal-persona = inputs.signal-persona.checks.${system}.default;
          persona-sema = inputs.persona-sema.checks.${system}.default;
          persona-system = inputs.persona-system.checks.${system}.default;
          persona-wezterm = inputs.persona-wezterm.checks.${system}.default;

          # ─── Wire-test chain: message → relay → store ───
          #
          # Architectural-truth witness for the message +
          # store channel pair. Each derivation is a separate
          # Nix build, so the wire bytes ARE the only thing
          # that travels between them. In-process memory
          # fakery cannot succeed — if the writer doesn't
          # actually emit bytes, the reader has nothing to
          # read. See `~/primary/skills/architectural-truth-tests.md`
          # §"Nix-chained tests — the strongest witness".
          wire-step-1-emit-message = context.pkgs.runCommand "wire-step-1-emit-message" {} ''
            ${personaShims}/bin/wire-emit-message \
              --recipient designer \
              --body 'wire test 2026-05-09' \
              > $out
            test -s $out
          '';

          wire-step-2-relay-message-to-store = context.pkgs.runCommand "wire-step-2-relay-message-to-store" {} ''
            ${personaShims}/bin/wire-relay-message-to-store \
              --sender operator \
              < ${self.checks.${system}.wire-step-1-emit-message} \
              > $out
            test -s $out
          '';

          wire-step-3-decode-store = context.pkgs.runCommand "wire-step-3-decode-store" {} ''
            ${personaShims}/bin/wire-decode-store \
              --expect-recipient designer \
              --expect-sender operator \
              --expect-body 'wire test 2026-05-09' \
              < ${self.checks.${system}.wire-step-2-relay-message-to-store}
            touch $out
          '';

          wire-message-channel-round-trip = context.pkgs.runCommand "wire-message-channel-round-trip" {} ''
            ${personaShims}/bin/wire-emit-message \
              --recipient designer \
              --body 'message-only round-trip' \
              | ${personaShims}/bin/wire-decode-message \
              --expect-recipient designer \
              --expect-body 'message-only round-trip'
            touch $out
          '';
        }
      );

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
