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

        # ──────────────────────────────────────────────
        # VERSION BUMP: update version, rev, and hashes
        # when tagging a new release.
        # ──────────────────────────────────────────────
        qbzVersion = "1.2.5";
        qbzRev     = "v${qbzVersion}";
        srcHash    = ""; # nix build will report the correct hash on first run
        npmHash    = ""; # nix build will report the correct hash on first run
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage rec {
          pname = "qbz";
          version = qbzVersion;

          src = pkgs.fetchFromGitHub {
            owner = "vicrodh";
            repo  = "qbz";
            rev   = qbzRev;
            hash  = srcHash;
          };

          cargoRoot = "src-tauri";
          buildAndTestSubdir = cargoRoot;

          cargoLock = {
            lockFile = "${src}/src-tauri/Cargo.lock";
          };

          npmDeps = pkgs.fetchNpmDeps {
            name = "${pname}-${version}-npm-deps";
            inherit src;
            hash = npmHash;
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

          # `inputsFrom` pulls in buildInputs/nativeBuildInputs from the
          # package but does NOT propagate `env.*` attributes, so we must
          # re-export LIBCLANG_PATH here — otherwise `mupdf-sys`'s bindgen
          # fails with "Unable to find libclang" when running
          # `npm run tauri dev` inside `nix develop` (issue #312).
          LIBCLANG_PATH = "${pkgs.lib.getLib pkgs.llvmPackages.libclang}/lib";

          packages = with pkgs; [
            rust-analyzer
            rustfmt
            clippy
          ];

          # The package's `postInstall` wraps the installed binary with
          # LD_LIBRARY_PATH so libappindicator is dlopen-able at runtime.
          # Inside `nix develop` we run `target/debug/qbz` directly, with
          # no wrapper, so we replicate that here — otherwise the tray
          # init panics with "Failed to load ayatana-appindicator3 or
          # appindicator3 dynamic library" (issue #312).
          shellHook = ''
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath [
              pkgs.libappindicator
              pkgs.libappindicator-gtk3
              pkgs.libayatana-appindicator
            ]}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
          '';
        };
      });
}
