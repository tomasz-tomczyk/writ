{
  description = "A local-first ledger of the steering you give coding agents";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }:
    let
      version = "0.1.0";
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in rec {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          writ = pkgs.rustPlatform.buildRustPackage {
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
          pkgs = nixpkgs.legacyPackages.${system};
        in {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              rustfmt
              clippy
              git
            ];
          };
        });
    };
}
