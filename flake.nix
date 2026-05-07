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
    persona-signal.url = "github:LiGoldragon/persona-signal";
    persona-store.url = "github:LiGoldragon/persona-store";
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
          persona-signal = inputs.persona-signal.packages.${system}.default;
          persona-store = inputs.persona-store.packages.${system}.default;
          persona-system = inputs.persona-system.packages.${system}.default;
          persona-wezterm = inputs.persona-wezterm.packages.${system}.default;
        }
      );

      checks = forSystems (
        system:
        let
          context = mkContext system;
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
          persona-signal = inputs.persona-signal.checks.${system}.default;
          persona-store = inputs.persona-store.checks.${system}.default;
          persona-system = inputs.persona-system.checks.${system}.default;
          persona-wezterm = inputs.persona-wezterm.checks.${system}.default;
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
