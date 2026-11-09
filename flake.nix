{
  description = "Self-hosted, forge-agnostic AI code review engine with multi-provider cascade (Gemini, Claude, Ollama)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      inherit (nixpkgs) lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (
        pkgs:
        let
          web = pkgs.buildNpmPackage {
            pname = "revue-web";
            version = "0.1.0";
            src = ./web;
            # Hashes come from package-lock.json, so Dependabot bumps need no hash update.
            npmDeps = pkgs.importNpmLock { npmRoot = ./web; };
            npmConfigHook = pkgs.importNpmLock.npmConfigHook;
            installPhase = ''
              mkdir -p $out
              cp -r build/* $out/
            '';
          };
        in
        {
          inherit web;
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "revue";
            version = "0.1.0";
            src = ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
            };
            nativeBuildInputs = [ pkgs.pkg-config ];
            postInstall = ''
              ln -s $out/bin/revue $out/bin/nix-revue
              mkdir -p $out/share/revue/web
              cp -r ${web}/* $out/share/revue/web/
            '';
            meta = with lib; {
              description = "Self-hosted, forge-agnostic AI code review engine";
              homepage = "https://github.com/kleinbem-dev/revue";
              license = licenses.mit;
              mainProgram = "revue";
            };
          };
        }
      );

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            clippy
            rustfmt
            rust-analyzer
            pkg-config
          ];
        };
      });

      overlays.default = final: _prev: {
        revue = self.packages.${final.stdenv.hostPlatform.system}.default;
        nix-revue = self.packages.${final.stdenv.hostPlatform.system}.default;
      };

      nixosModules = {
        default = {
          imports = [ ./nix/module.nix ];
          nixpkgs.overlays = [ self.overlays.default ];
        };
        revue = self.nixosModules.default;
        nix-revue = self.nixosModules.default;
      };
    };
}
