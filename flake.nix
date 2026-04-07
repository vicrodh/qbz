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
        qbzVersion = "1.2.4";
        qbzRev     = "v${qbzVersion}";
        srcHash    = "sha256-3MPWLovWRmSrSfaR5ciZR2+4S7QzPYYVdVKP+mczhis=";
        npmHash    = "sha256-JN3lQyEX1n5G1OcWuRNZl/KSfL7JEfsc4opeh4F/iAY=";
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
            "--skip=qconnect_service::tests::refreshes_local_renderer_id_from_unique_fingerprint_when_uuid_missing"
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
