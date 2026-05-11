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
          persona-mind = inputs.persona-mind.packages.${system}.default;
          persona-router = inputs.persona-router.packages.${system}.default;
          signal-persona = inputs.signal-persona.packages.${system}.default;
          signal-persona-mind = inputs.signal-persona-mind.packages.${system}.default;
          signal-persona-system = inputs.signal-persona-system.packages.${system}.default;
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
          persona-wezterm = inputs.persona-wezterm.checks.${system}.default;

          # ─── Wire-test chain: signal-persona-message ───
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
