{
  description = "A local-first ledger of the steering you give coding agents";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    # Pin the Rust toolchain so the Nix build tracks Cargo.toml's rust-version
    # independently of whatever rustc nixpkgs-unstable currently ships.
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      version = "0.1.0";
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in rec {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          # Keep this >= workspace.package.rust-version in Cargo.toml.
          rustToolchain = pkgs.rust-bin.stable."1.98.0".default;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };
          writ = rustPlatform.buildRustPackage {
            pname = "writ";
            inherit version;
            src = self;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "-p" "writ-cli" ];
            cargoTestFlags = [ "-p" "writ-cli" ];
            # Tests run in dedicated CI jobs via mise; keep the Nix build fast.
            doCheck = false;
            meta = with nixpkgs.lib; {
              description = "A local-first ledger of the steering you give coding agents";
              homepage = "https://github.com/tomasz-tomczyk/writ";
              license = licenses.mit;
              mainProgram = "writ";
            };
          };
        in {
          inherit writ;
          default = writ;
        });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${packages.${system}.default}/bin/writ";
        };
      });

      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          rustToolchain = pkgs.rust-bin.stable."1.98.0".default;
        in {
          default = pkgs.mkShell {
            packages = [
              rustToolchain
              rustToolchain.rustfmt
              rustToolchain.clippy
              pkgs.git
            ];
          };
        });
    };
}
