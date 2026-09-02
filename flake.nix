# Dev shell for the fleetwatch backend (Rust). Enter with: nix develop
# Pure-Rust TLS (rustls) so there's no openssl/pkg-config native dep.
{
  description = "fleetwatch — fleet monitoring platform backend";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "aarch64-darwin" "x86_64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (s: f nixpkgs.legacyPackages.${s});
    in {
      packages = forAll (pkgs: {
        # The desk-side reader (#1312) and ONLY it — the server ships as the
        # docker image, never through the store. On PATH via home-manager
        # (mac-config), the same route memview's `sessions` took: a hand-copied
        # binary goes stale silently on every rebuild (memview#1298).
        board = pkgs.rustPlatform.buildRustPackage {
          pname = "board";
          version = "0.1.0";
          # Named rather than `./.`: the repository root also holds the Angular
          # frontend's node_modules and the Android tree, and a filter that
          # relies on gitignore would drop an unstaged Rust file silently.
          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./src
              ./migrations # sqlx::migrate!() reads it at COMPILE time (db.rs)
            ];
          };
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "--bin" "board" ];
          # The gate runs the tests, against the ephemeral MariaDB this sandbox
          # has none of. A second run here would be a slower way to learn less.
          doCheck = false;
          meta.mainProgram = "board";
        };
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.board;
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rust-analyzer
            pkgs.rustfmt
            pkgs.clippy
            pkgs.sqlx-cli
            pkgs.mariadb # ephemeral test DB (scripts/with-test-db.sh) + dev-db
            pkgs.nodejs_24 # Angular 22 frontend (frontend/)
            pkgs.pnpm # the frontend's installer; node ships npm too, ignore it
          ];
        };
      });
    };
}
