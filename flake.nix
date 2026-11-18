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
      version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;

      mkSystemOutputs = pkgs:
        let
          npmWorkspaceSrc = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              ./package.json
              ./package-lock.json
              ./preload
              ./web
              ./examples/sway-full-bar
              ./examples/react-tray
            ];
          };

          npmDeps = pkgs.importNpmLock {
            npmRoot = npmWorkspaceSrc;
          };

          preloadPkg = pkgs.buildNpmPackage {
            pname = "webbar-preload";
            inherit version npmDeps;
            src = npmWorkspaceSrc;
            npmConfigHook = pkgs.importNpmLock.npmConfigHook;
            npmWorkspace = "preload";

            preBuild = ''
              mkdir -p generated
            '';

            doCheck = true;
            checkPhase = ''
              runHook preCheck
              npm run --workspace=preload typecheck
              npm --workspace=preload test
              runHook postCheck
            '';

            installPhase = ''
              runHook preInstall
              mkdir -p $out/generated/types
              cp generated/preload.js $out/generated/
              cp -r preload/dist/* $out/generated/types/
              runHook postInstall
            '';
          };

          mkWorkspaceUi = { pname, workspace }:
            pkgs.buildNpmPackage {
              inherit pname version npmDeps;
              src = npmWorkspaceSrc;
              npmConfigHook = pkgs.importNpmLock.npmConfigHook;
              npmWorkspace = workspace;

              doCheck = true;
              checkPhase = ''
                runHook preCheck
                npm run --workspace=${workspace} typecheck
                runHook postCheck
              '';

              installPhase = ''
                runHook preInstall
                mkdir -p $out
                cp -r ${workspace}/dist/* $out/
                runHook postInstall
              '';
            };

          webUi = mkWorkspaceUi {
            pname = "webbar-ui";
            workspace = "web";
          };

          swayFullBarUi = mkWorkspaceUi {
            pname = "webbar-sway-full-bar";
            workspace = "examples/sway-full-bar";
          };

          reactTrayUi = mkWorkspaceUi {
            pname = "webbar-react-tray";
            workspace = "examples/react-tray";
          };

          craneLib = crane.mkLib pkgs;

          rustSrc = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./build.rs
              ./src
              ./generated/preload.js
            ];
          };

          commonArgs = {
            pname = "webbar";
            inherit version;
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
              mkdir -p generated
              cp ${preloadPkg}/generated/preload.js generated/preload.js
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
            sway-full-bar-ui = swayFullBarUi;
            sway-full-bar = pkgs.writeShellScriptBin "webbar-sway-full-bar" ''
              exec ${webbarPkg}/bin/webbar --dir ${swayFullBarUi} --height 86 "$@"
            '';
            sway-full-bar-dev = pkgs.writeShellApplication {
              name = "webbar-sway-full-bar-dev";
              runtimeInputs = [ pkgs.nodejs pkgs.curl ];
              text = ''
                if [ ! -f examples/sway-full-bar/package.json ]; then
                  echo "webbar-sway-full-bar-dev: run from the webbar repository root" >&2
                  exit 1
                fi

                if [ ! -d node_modules ]; then
                  npm install
                fi

                npm --workspace=examples/sway-full-bar run dev -- --port 5173 --strictPort &
                VITE_PID=$!
                trap 'kill "$VITE_PID" 2>/dev/null || true' EXIT INT TERM

                for _ in $(seq 1 100); do
                  if curl -fsS http://localhost:5173 >/dev/null 2>&1; then
                    break
                  fi
                  sleep 0.15
                done

                ${webbarPkg}/bin/webbar --url http://localhost:5173 --height 86 --replace "$@"
              '';
            };
            sway-full-bar-browser-dev = pkgs.writeShellApplication {
              name = "webbar-sway-full-bar-browser-dev";
              runtimeInputs = [ pkgs.nodejs pkgs.curl pkgs.coreutils ];
              text = ''
                if [ ! -f examples/sway-full-bar/package.json ]; then
                  echo "webbar-sway-full-bar-browser-dev: run from the webbar repository root" >&2
                  exit 1
                fi

                if [ ! -d node_modules ]; then
                  npm install
                fi

                if [ ! -f preload/dist/preload.mjs ]; then
                  npm --workspace=@webbar/types run build
                fi

                WS_PORT="''${WEBBAR_WS_PORT:-9123}"
                VITE_PORT="''${WEBBAR_VITE_PORT:-5173}"
                WS_TOKEN="''${WEBBAR_WS_TOKEN:-$(head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n')}"

                ${webbarPkg}/bin/webbar --ws-bridge "$WS_PORT" --ws-token "$WS_TOKEN" &
                WS_PID=$!
                VITE_PID=""
                cleanup() {
                  kill "$WS_PID" 2>/dev/null || true
                  if [ -n "$VITE_PID" ]; then
                    kill "$VITE_PID" 2>/dev/null || true
                  fi
                }
                trap cleanup EXIT INT TERM

                npm --workspace=examples/sway-full-bar run dev -- --port "$VITE_PORT" --strictPort &
                VITE_PID=$!

                ready=0
                for _ in $(seq 1 100); do
                  if ! kill -0 "$WS_PID" 2>/dev/null; then
                    echo "webbar-sway-full-bar-browser-dev: ws-bridge exited early" >&2
                    exit 1
                  fi
                  if ! kill -0 "$VITE_PID" 2>/dev/null; then
                    echo "webbar-sway-full-bar-browser-dev: vite exited early" >&2
                    exit 1
                  fi
                  if curl -fsS "http://localhost:$VITE_PORT" >/dev/null 2>&1; then
                    ready=1
                    break
                  fi
                  sleep 0.15
                done

                if [ "$ready" -ne 1 ]; then
                  echo "webbar-sway-full-bar-browser-dev: timed out waiting for Vite on port $VITE_PORT" >&2
                  exit 1
                fi

                echo "webbar-sway-full-bar-browser-dev: open http://localhost:$VITE_PORT?wsPort=$WS_PORT&wsToken=$WS_TOKEN"
                wait "$VITE_PID"
              '';
            };
          };

          checks = {
            rust-test = craneLib.cargoTest (commonArgs // {
              inherit cargoArtifacts;
            });
            rust-clippy = craneLib.cargoClippy (commonArgs // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });
            rust-ts-bindings = craneLib.cargoTest (commonArgs // {
              inherit cargoArtifacts;
              cargoTestExtraArgs = "export_bindings_";
              preCheck = ''
                export TS_RS_EXPORT_DIR="$PWD/target/ts-root/dummy"
                export WEBBAR_TS_DIR="$PWD/target/ts-root/preload/src/generated"
                mkdir -p "$TS_RS_EXPORT_DIR" "$WEBBAR_TS_DIR"
              '';
              postCheck = ''
                diff -ru ${./preload/src/generated} "$PWD/target/ts-root/preload/src/generated"
              '';
            });
            preload-test = preloadPkg;
            web-ui = webUi;
            sway-full-bar-ui = swayFullBarUi;
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
