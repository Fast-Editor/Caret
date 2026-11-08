{
  description = "Caret: a Cursor-class AI code editor with a Rust core";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      # Pinned to one GitHub release. scripts/update-packaging.mjs rewrites the version and the hashes
      # (hex sha256 values from SHA256SUMS.txt on the release) whenever a tag is published.
      version = "0.2.0";
      hashes = {
        aarch64-darwin = "7e7d7ee45103a02fac789d49c1a6180c05488b8d7e1b4967d49c61b34c4fc960";
        x86_64-linux = "3e2056e0c66a3b8a434425a7195f5ed8d8d7d6ea48d90ea784e64578c5cbdafd";
      };
      baseUrl = "https://github.com/Fast-Editor/Caret/releases/download/v${version}";

      mkMeta = lib: {
        description = "A Cursor-class AI code editor with a Rust core";
        homepage = "https://github.com/Fast-Editor/Caret";
        license = lib.licenses.mit;
        sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
        mainProgram = "caret";
      };

      # macOS (Apple Silicon): unpack the DMG and install Caret.app into $out/Applications.
      darwin =
        let pkgs = nixpkgs.legacyPackages.aarch64-darwin;
        in pkgs.stdenvNoCC.mkDerivation {
          pname = "caret";
          inherit version;
          src = pkgs.fetchurl {
            url = "${baseUrl}/Caret_${version}_aarch64.dmg";
            sha256 = hashes.aarch64-darwin;
          };
          nativeBuildInputs = [ pkgs.undmg ];
          sourceRoot = ".";
          # The app is ad-hoc signed; leave its binaries untouched.
          dontFixup = true;
          installPhase = ''
            runHook preInstall
            mkdir -p "$out/Applications" "$out/bin"
            cp -R Caret.app "$out/Applications/"
            ln -s "$out/Applications/Caret.app/Contents/MacOS/caret" "$out/bin/caret"
            runHook postInstall
          '';
          meta = (mkMeta pkgs.lib) // { platforms = [ "aarch64-darwin" ]; };
        };

      # Linux (x86_64): wrap the AppImage so it runs on NixOS without an FHS layout.
      linux =
        let pkgs = nixpkgs.legacyPackages.x86_64-linux;
        in pkgs.appimageTools.wrapType2 {
          pname = "caret";
          inherit version;
          src = pkgs.fetchurl {
            url = "${baseUrl}/Caret_${version}_amd64.AppImage";
            sha256 = hashes.x86_64-linux;
          };
          meta = (mkMeta pkgs.lib) // { platforms = [ "x86_64-linux" ]; };
        };
    in {
      packages.aarch64-darwin.default = darwin;
      packages.x86_64-linux.default = linux;

      apps.aarch64-darwin.default = {
        type = "app";
        program = "${darwin}/bin/caret";
      };
      apps.x86_64-linux.default = {
        type = "app";
        program = "${linux}/bin/caret";
      };
    };
}
