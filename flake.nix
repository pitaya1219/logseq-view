{
  description = "TUI viewer for Logseq markdown files";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        packages = {
          logseq-view = pkgs.rustPlatform.buildRustPackage {
            pname = "logseq-view";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            meta = with pkgs.lib; {
              description = "TUI viewer for Logseq markdown files";
              homepage = "https://github.com/pitaya1219/logseq-view";
              license = licenses.mit;
              mainProgram = "lqview";
            };
          };
          default = self.packages.${system}.logseq-view;
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustc
            rust-analyzer
            clippy
            rustfmt
          ];
        };
      }
    );
}
