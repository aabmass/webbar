{
  description = "webbar: Wayland top bar using GTK4, gtk4-layer-shell, WebKitGTK 6, and a TypeScript/esbuild frontend";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, crane }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      mkSystemOutputs = pkgs:
        let
          preloadPkg = pkgs.buildNpmPackage {
            pname = "webbar-preload";
            version = "0.1.0";
            src = ./preload;

            npmDepsHash = "sha256-oOTyHTzkOQl8TIzdx6o1QavKqTwWc0JsEG98jsXDiDI=";

            preBuild = ''
              mkdir -p ../src ../types
            '';

            doCheck = true;
            checkPhase = ''
              runHook preCheck
              npm run typecheck
              npm test
              runHook postCheck
            '';

            installPhase = ''
              runHook preInstall
              mkdir -p $out/src $out/types
              cp ../src/preload.js $out/src/
              cp -r ../types/* $out/types/
              runHook postInstall
            '';
          };

          webUi = pkgs.buildNpmPackage {
            pname = "webbar-ui";
            version = "0.1.0";
            src = ./web;

            npmDepsHash = "sha256-gEUedwYGL5Ps6sIqa+CZpLKIr3XC7FuQ3MiycIpl6QE=";

            preBuild = ''
              mkdir -p ../types
              cp -r ${preloadPkg}/types/* ../types/
            '';

            doCheck = true;
            checkPhase = ''
              runHook preCheck
              npm run typecheck
              runHook postCheck
            '';

            installPhase = ''
              runHook preInstall
              mkdir -p $out
              cp -r dist/* $out/
              runHook postInstall
            '';
          };

          agsBarUi = pkgs.buildNpmPackage {
            pname = "webbar-ags-bar";
            version = "0.1.0";
            src = pkgs.lib.fileset.toSource {
              root = ./.;
              fileset = pkgs.lib.fileset.unions [
                ./examples/ags-bar
              ];
            };
            sourceRoot = "source/examples/ags-bar";

            npmDeps = pkgs.fetchNpmDeps {
              src = ./examples/ags-bar;
              hash = "sha256-/uT+VOYzCz6VHu5o2Dvl+48gzjrwscME8Dcq3qIjTrU=";
            };

            postUnpack = ''
              chmod u+w source
              mkdir -p source/types
              cp -r ${preloadPkg}/types/* source/types/
            '';

            doCheck = true;
            checkPhase = ''
              runHook preCheck
              npm run typecheck
              runHook postCheck
            '';

            installPhase = ''
              runHook preInstall
              mkdir -p $out
              cp -r dist/* $out/
              runHook postInstall
            '';
          };

          reactTrayUi = pkgs.buildNpmPackage {
            pname = "webbar-react-tray";
            version = "0.1.0";
            src = pkgs.lib.fileset.toSource {
              root = ./.;
              fileset = pkgs.lib.fileset.unions [
                ./examples/react-tray
              ];
            };
            sourceRoot = "source/examples/react-tray";

            npmDeps = pkgs.fetchNpmDeps {
              src = ./examples/react-tray;
              hash = "sha256-n9YrqcbW9MxJZJAGPJn1B3AT1XVGqBfLTbG1uY8FVgg=";
            };

            postUnpack = ''
              chmod u+w source
              mkdir -p source/types
              cp -r ${preloadPkg}/types/* source/types/
            '';

            doCheck = true;
            checkPhase = ''
              runHook preCheck
              npm run typecheck
              runHook postCheck
            '';

            installPhase = ''
              runHook preInstall
              mkdir -p $out
              cp -r dist/* $out/
              runHook postInstall
            '';
          };

          craneLib = crane.mkLib pkgs;

          rustSrc = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./src
            ];
          };

          commonArgs = {
            pname = "webbar";
            version = "0.1.0";
            src = rustSrc;
            strictDeps = true;

            nativeBuildInputs = with pkgs; [
              pkg-config
              wrapGAppsHook4
            ];

            buildInputs = with pkgs; [
              gtk4
              gtk4-layer-shell
              webkitgtk_6_0
              glib
              dbus
              libpulseaudio
            ];
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          webbarPkg = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;

            preBuild = ''
              cp ${preloadPkg}/src/preload.js src/preload.js
            '';

            postInstall = ''
              mkdir -p $out/share/webbar
              cp -r ${webUi}/* $out/share/webbar/
            '';

            meta = with pkgs.lib; {
              description = "Wayland bar built with GTK4, gtk4-layer-shell, and WebKitGTK 6";
              license = licenses.mit;
              mainProgram = "webbar";
              platforms = platforms.linux;
            };
          });
        in
        {
          packages = {
            default = webbarPkg;
            ags-bar-ui = agsBarUi;
            ags-bar = pkgs.writeShellScriptBin "webbar-ags-bar" ''
              exec ${webbarPkg}/bin/webbar --dir ${agsBarUi} --height 86 "$@"
            '';
          };

          checks = {
            rust-test = craneLib.cargoTest (commonArgs // {
              inherit cargoArtifacts;
            });
            rust-clippy = craneLib.cargoClippy (commonArgs // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets";
            });
            rust-ts-bindings = craneLib.cargoTest (commonArgs // {
              inherit cargoArtifacts;
              cargoTestExtraArgs = "export_bindings_";
              preCheck = ''
                export TS_RS_EXPORT_DIR="$PWD/target/ts-root/dummy"
                mkdir -p "$TS_RS_EXPORT_DIR"
              '';
              postCheck = ''
                diff -ru ${./preload/src/generated} "$PWD/target/ts-root/preload/src/generated"
              '';
            });
            preload-test = preloadPkg;
            web-ui = webUi;
            ags-bar-ui = agsBarUi;
            react-tray-ui = reactTrayUi;
          };
        };
    in
    {
      packages = forAllSystems (pkgs: (mkSystemOutputs pkgs).packages);
      checks = forAllSystems (pkgs: (mkSystemOutputs pkgs).checks);

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            rustc
            cargo
            rust-analyzer
            pkg-config
            nodejs
          ];

          buildInputs = with pkgs; [
            gtk4
            gtk4-layer-shell
            webkitgtk_6_0
            glib
            dbus
            libpulseaudio
          ];
        };
      });
    };
}
