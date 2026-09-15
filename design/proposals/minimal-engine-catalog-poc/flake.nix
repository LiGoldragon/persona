{
  description = "Draft-only Nix check for the Persona minimal engine catalog proof of concept";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d";

  outputs = { nixpkgs, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
    in
    {
      checks = nixpkgs.lib.genAttrs systems (system:
        let pkgs = import nixpkgs { inherit system; };
        in {
          catalog = pkgs.runCommand "persona-minimal-engine-catalog-poc-check" {
            nativeBuildInputs = [ pkgs.cargo pkgs.rustc ];
          } ''
            export CARGO_HOME="$TMPDIR/cargo-home"
            cargo test --manifest-path ${./Cargo.toml} --target-dir "$TMPDIR/target"
            touch "$out"
          '';
        });
    };
}
