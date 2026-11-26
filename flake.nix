{
  description = "webbar: Wayland top bar using GTK4, gtk4-layer-shell, WebKitGTK 6, and a TypeScript/esbuild frontend";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, crane }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;

      mkSystemOutputs = pkgs:
        let
          mkNpmSrc = dirs:
            pkgs.lib.fileset.toSource {
              root = ./.;
              fileset = pkgs.lib.fileset.unions ([
                ./package.json
                ./package-lock.json
              ] ++ dirs);
            };

          npmWorkspaceSrc = mkNpmSrc [
            ./preload/package.json
            ./examples/minimal/package.json
            ./examples/sway-full-bar-shared/package.json
            ./examples/sway-full-bar/package.json
            ./examples/sway-full-bar-svelte/package.json
            ./examples/react-tray/package.json
          ];

          npmDeps = pkgs.importNpmLock {
            npmRoot = npmWorkspaceSrc;
          };

          preloadPkg = pkgs.buildNpmPackage {
            pname = "webbar-preload";
            inherit version npmDeps;
            src = mkNpmSrc [ ./preload ];
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

          mkWorkspaceUi = { pname, workspace, extraDirs ? [] }:
            pkgs.buildNpmPackage {
              inherit pname version npmDeps;
              src = mkNpmSrc ([ ./preload (./. + "/${workspace}") ] ++ extraDirs);
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

          minimalUi = mkWorkspaceUi {
            pname = "webbar-minimal";
            workspace = "examples/minimal";
          };

          swayFullBarUi = mkWorkspaceUi {
            pname = "webbar-sway-full-bar";
            workspace = "examples/sway-full-bar";
            extraDirs = [ ./examples/sway-full-bar-shared ];
          };

          swayFullBarSvelteUi = mkWorkspaceUi {
            pname = "webbar-sway-full-bar-svelte";
            workspace = "examples/sway-full-bar-svelte";
            extraDirs = [ ./examples/sway-full-bar-shared ];
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
              libadwaita
              webkitgtk_6_0
              glib
              dbus
              libpulseaudio
            ];
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          webbarPkg = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            doCheck = false;

            preBuild = ''
              mkdir -p generated
              cp ${preloadPkg}/generated/preload.js generated/preload.js
            '';

            meta = with pkgs.lib; {
              description = "Wayland bar built with GTK4, gtk4-layer-shell, and WebKitGTK 6";
              license = licenses.mit;
              mainProgram = "webbar";
              platforms = platforms.linux;
            };
          });

          # Dev-only hermetic screenshot builder exposed strictly under
          # `apps.update-screenshots` so headless Sway / terminal / image
          # tooling is never pulled in by `packages.*` or `devShells.*`.
          screenshotsPkg = pkgs.runCommand "webbar-screenshots" {
            nativeBuildInputs = [ webbarPkg ] ++ (with pkgs; [
              sway
              swaybg
              foot
              grim
              dbus
              glib.bin
              bat
              python3
              librsvg
              oxipng
              jq
              curl
              coreutils
              fontconfig
              bash
            ]);
          } ''
            export WEBBAR_UI_DIR="${swayFullBarUi}"
            export WEBBAR_MULTI_SURFACE_DIR="${./examples/multi-surface}"
            export FONT_DIRS="${pkgs.inter}/share/fonts:${pkgs.jetbrains-mono}/share/fonts:${pkgs.dejavu_fonts}/share/fonts"
            export OUT_DIR="$out"
            # nix-daemon is already the networkless build sandbox; WebKit's
            # inner bwrap sandbox cannot nest inside it.
            export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1

            cd "${pkgs.lib.fileset.toSource {
              root = ./.;
              fileset = pkgs.lib.fileset.unions [
                ./scripts/screenshots
                ./scripts/harness
                ./examples/sway-full-bar/package.json
                ./examples/sway-full-bar/src/App.tsx
              ];
            }}"
            bash ./scripts/screenshots/update-screenshots.sh
          '';

          updateScreenshotsPkg = pkgs.writeShellApplication {
            name = "webbar-update-screenshots";
            runtimeInputs = [ pkgs.coreutils ];
            text = ''
              if [ ! -f flake.nix ] || [ ! -d docs/screenshots ]; then
                echo "update-screenshots: run from the webbar repository root" >&2
                exit 1
              fi
              mkdir -p docs/screenshots
              cp -f --no-preserve=mode ${screenshotsPkg}/*.png docs/screenshots/
              echo "update-screenshots: copied sandboxed screenshots into docs/screenshots/"
              ls -lh docs/screenshots
            '';
          };
          mkBrowserDev = { name, workspace, defaultPort }:
            pkgs.writeShellApplication {
              inherit name;
              runtimeInputs = [ pkgs.nodejs pkgs.curl pkgs.coreutils ];
              text = ''
                if [ ! -f ${workspace}/package.json ]; then
                  echo "${name}: run from the webbar repository root" >&2
                  exit 1
                fi

                if [ ! -d node_modules ]; then
                  npm install
                fi

                if [ ! -f preload/dist/preload.mjs ]; then
                  npm --workspace=@webbar/types run build
                fi

                WS_PORT="''${WEBBAR_WS_PORT:-9123}"
                VITE_PORT="''${WEBBAR_VITE_PORT:-${toString defaultPort}}"
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

                npm --workspace=${workspace} run dev -- --port "$VITE_PORT" --strictPort &
                VITE_PID=$!

                ready=0
                for _ in $(seq 1 100); do
                  if ! kill -0 "$WS_PID" 2>/dev/null; then
                    echo "${name}: ws-bridge exited early" >&2
                    exit 1
                  fi
                  if ! kill -0 "$VITE_PID" 2>/dev/null; then
                    echo "${name}: vite exited early" >&2
                    exit 1
                  fi
                  if curl -fsS "http://localhost:$VITE_PORT" >/dev/null 2>&1; then
                    ready=1
                    break
                  fi
                  sleep 0.15
                done

                if [ "$ready" -ne 1 ]; then
                  echo "${name}: timed out waiting for Vite on port $VITE_PORT" >&2
                  exit 1
                fi

                echo "${name}: open http://localhost:$VITE_PORT?wsPort=$WS_PORT&wsToken=$WS_TOKEN"
                wait "$VITE_PID"
              '';
            };

          integrationParitySrc = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              ./scripts/integration
              ./scripts/harness
            ];
          };

          mkIntegrationParityCheck = { target, uiDir }:
            pkgs.runCommand "webbar-integration-parity-${target}" {
              nativeBuildInputs = [
                webbarPkg
                pkgs.sway
                pkgs.dbus
                pkgs.stasis
                pkgs.pulseaudio
                pkgs.glib.bin
                pkgs.playwright-test
                pkgs.curl
                (pkgs.python3.withPackages (ps: [
                  ps.dbus-python
                  ps.pygobject3
                ]))
              ];
            } ''
              export WEBBAR_PRELOAD_JS="${preloadPkg}/generated/preload.js"
              export WEBBAR_TARGET="${target}"
              export WEBBAR_UI_DIR="${uiDir}"
              export PLAYWRIGHT_BROWSERS_PATH="${pkgs.playwright-driver.browsers}"
              export PLAYWRIGHT_NODE_MODULES="${pkgs.playwright-test}/lib/node_modules"
              export XDG_DATA_DIRS="${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}"
              cd "${integrationParitySrc}"
              bash ./scripts/integration/run-parity-test.sh
              touch "$out"
            '';

          mkBarDev = { name, workspace, defaultPort }:
            pkgs.writeShellApplication {
              inherit name;
              runtimeInputs = [ pkgs.nodejs pkgs.curl ];
              text = ''
                if [ ! -f ${workspace}/package.json ]; then
                  echo "${name}: run from the webbar repository root" >&2
                  exit 1
                fi

                if [ ! -d node_modules ]; then
                  npm install
                fi

                VITE_PORT="''${WEBBAR_VITE_PORT:-${toString defaultPort}}"

                npm --workspace=${workspace} run dev -- --port "$VITE_PORT" --strictPort &
                VITE_PID=$!
                trap 'kill "$VITE_PID" 2>/dev/null || true' EXIT INT TERM

                for _ in $(seq 1 100); do
                  if curl -fsS "http://localhost:$VITE_PORT" >/dev/null 2>&1; then
                    break
                  fi
                  sleep 0.15
                done

                ${webbarPkg}/bin/webbar --url "http://localhost:$VITE_PORT" --replace "$@"
              '';
            };
        in
        {
          apps = {
            update-screenshots = {
              type = "app";
              program = "${updateScreenshotsPkg}/bin/webbar-update-screenshots";
              meta.description = "Regenerate README screenshots of sway-full-bar in an isolated headless Sway session";
            };
          };

          packages = {
            default = webbarPkg;
            sway-full-bar-ui = swayFullBarUi;
            sway-full-bar = pkgs.writeShellScriptBin "webbar-sway-full-bar" ''
              exec ${webbarPkg}/bin/webbar --dir ${swayFullBarUi} "$@"
            '';
            sway-full-bar-svelte-ui = swayFullBarSvelteUi;
            sway-full-bar-svelte = pkgs.writeShellScriptBin "webbar-sway-full-bar-svelte" ''
              exec ${webbarPkg}/bin/webbar --dir ${swayFullBarSvelteUi} "$@"
            '';
            sway-full-bar-dev = mkBarDev {
              name = "webbar-sway-full-bar-dev";
              workspace = "examples/sway-full-bar";
              defaultPort = 5173;
            };
            sway-full-bar-svelte-dev = mkBarDev {
              name = "webbar-sway-full-bar-svelte-dev";
              workspace = "examples/sway-full-bar-svelte";
              defaultPort = 5174;
            };
            sway-full-bar-browser-dev = mkBrowserDev {
              name = "webbar-sway-full-bar-browser-dev";
              workspace = "examples/sway-full-bar";
              defaultPort = 5173;
            };
            sway-full-bar-svelte-browser-dev = mkBrowserDev {
              name = "webbar-sway-full-bar-svelte-browser-dev";
              workspace = "examples/sway-full-bar-svelte";
              defaultPort = 5174;
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
                export WEBBAR_SCHEMA_PATH="$PWD/target/ts-root/webbar.schema.json"
                mkdir -p "$TS_RS_EXPORT_DIR" "$WEBBAR_TS_DIR"
              '';
              postCheck = ''
                diff -ru ${./preload/src/generated} "$PWD/target/ts-root/preload/src/generated"
                diff -u ${./webbar.schema.json} "$PWD/target/ts-root/webbar.schema.json"
              '';
            });
            preload-test = preloadPkg;
            minimal-ui = minimalUi;
            sway-full-bar-ui = swayFullBarUi;
            sway-full-bar-svelte-ui = swayFullBarSvelteUi;
            react-tray-ui = reactTrayUi;
            css-vars-check = pkgs.runCommand "webbar-css-vars-check" {
              nativeBuildInputs = [ pkgs.nodejs ];
            } ''
              cd "${pkgs.lib.fileset.toSource {
                root = ./.;
                fileset = pkgs.lib.fileset.unions [
                  ./scripts/check-css-vars.mjs
                  ./examples/sway-full-bar-shared/styles
                  ./examples/sway-full-bar-svelte/src
                ];
              }}"
              node ./scripts/check-css-vars.mjs
              touch "$out"
            '';
            integration-parity-react = mkIntegrationParityCheck {
              target = "react";
              uiDir = swayFullBarUi;
            };
            integration-parity-svelte = mkIntegrationParityCheck {
              target = "svelte";
              uiDir = swayFullBarSvelteUi;
            };
            integration-multi-surface = pkgs.runCommand "webbar-integration-multi-surface" {
              nativeBuildInputs = [
                webbarPkg
                pkgs.sway
                pkgs.foot
                pkgs.grim
                pkgs.dbus
                pkgs.glib.bin
                pkgs.jq
                pkgs.nodejs
                pkgs.python3
              ];
            } ''
              export WEBBAR_MULTI_SURFACE_DIR="${./examples/multi-surface}"
              export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
              cd "${integrationParitySrc}"
              bash ./scripts/integration/run-multi-surface-test.sh
              touch "$out"
            '';
          };
        };
    in
    {
      apps = forAllSystems (pkgs: (mkSystemOutputs pkgs).apps);
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
            libadwaita
            webkitgtk_6_0
            glib
            dbus
            libpulseaudio
          ];
        };
      });
    };
}
