{
  description = "ssukka - HTML obfuscation tool";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustToolchain = pkgs.rust-bin.stable."1.94.0".default.override {
          targets = [ "wasm32-unknown-unknown" ];
        };
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "ssukka";
          version = (pkgs.lib.importTOML ./Cargo.toml).workspace.package.version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          # Build just the CLI crate (skips the wasm32-only member).
          cargoBuildFlags = [ "-p" "ssukka" ];
          cargoTestFlags = [ "-p" "ssukka_core" ];
        };

        devShells.default = pkgs.mkShell {
          buildInputs = [
            rustToolchain
            pkgs.cargo-watch
            pkgs.shfmt
            pkgs.shellcheck
            pkgs.taplo
            pkgs.nodejs
          ];
        };
      }
    );
}
