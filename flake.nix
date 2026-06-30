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
        version = (pkgs.lib.importTOML ./Cargo.toml).workspace.package.version;
        # Build one workspace binary crate (doCheck off; run tests via `cargo test`).
        mkBin = crate: pkgs.rustPlatform.buildRustPackage {
          pname = crate;
          inherit version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" crate ];
          doCheck = false;
        };
      in
      {
        packages.default = mkBin "ssukka";
        packages.ssukka = mkBin "ssukka";
        packages.ssukka-proxy = mkBin "ssukka-proxy";

        devShells.default = pkgs.mkShell {
          buildInputs = [
            rustToolchain
            pkgs.cargo-watch
            pkgs.shfmt
            pkgs.shellcheck
            pkgs.taplo
            pkgs.nodejs
            # wasm packaging (`wasm-pack build wasm`)
            pkgs.wasm-pack
            pkgs.wasm-bindgen-cli
            pkgs.binaryen
          ];
        };
      }
    );
}
