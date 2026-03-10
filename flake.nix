{
  description = "QBZ — Native hi-fi Qobuz desktop player for Linux";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage rec {
          pname = "qbz";
          version = "1.1.19";

          src = ./.;

          cargoRoot = "src-tauri";
          buildAndTestSubdir = cargoRoot;

          cargoLock = {
            lockFile = ./src-tauri/Cargo.lock;
          };

          npmDeps = pkgs.fetchNpmDeps {
            name = "${pname}-${version}-npm-deps";
            inherit src;
            hash = ""; # Run `nix build` once — Nix will report the correct hash
          };

          env.LIBCLANG_PATH = "${pkgs.lib.getLib pkgs.llvmPackages.libclang}/lib";

          nativeBuildInputs = with pkgs; [
            clang
            cargo-tauri.hook
            nodejs
            npmHooks.npmConfigHook
            pkg-config
            makeWrapper
          ];

          buildInputs = with pkgs; [
            alsa-lib
            openssl
            webkitgtk_4_1
            libappindicator-gtk3
            libayatana-appindicator
          ];

          checkFlags = [
            # These require a writable HOME and D-Bus keyring service
            "--skip=credentials::tests::test_credentials_roundtrip"
            "--skip=credentials::tests::test_encryption_roundtrip"
          ];

          postInstall = ''
            wrapProgram $out/bin/qbz \
              --prefix LD_LIBRARY_PATH : ${
                pkgs.lib.makeLibraryPath [
                  pkgs.libappindicator
                  pkgs.libappindicator-gtk3
                  pkgs.libayatana-appindicator
                ]
              }
          '';

          meta = with pkgs.lib; {
            description = "Native, full-featured hi-fi Qobuz desktop player for Linux";
            homepage = "https://qbz.lol";
            license = licenses.mit;
            mainProgram = "qbz";
            platforms = platforms.linux;
          };
        };

        # Dev shell with all build dependencies
        devShells.default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.default ];
          packages = with pkgs; [
            rust-analyzer
            rustfmt
            clippy
          ];
        };
      });
}
