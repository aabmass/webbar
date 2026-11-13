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
    in
    {
      packages = forAllSystems (pkgs:
        let
          webUi = pkgs.buildNpmPackage {
            pname = "webbar-ui";
            version = "0.1.0";
            src = ./web;

            npmDepsHash = "sha256-VKOg+lKp/NDpD3B4fxS1ORxsbQpqNodb1FtBU7tjkmM=";

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
            src = ./examples/ags-bar;

            npmDepsHash = "sha256-ECu2RknM6+YVzy2geTTkWFQ2VzNIVv4ZSmkR3hLhhUg=";

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
            ];
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          webbarPkg = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;

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
          default = webbarPkg;
          ags-bar-ui = agsBarUi;
          ags-bar = pkgs.writeShellScriptBin "webbar-ags-bar" ''
            exec ${webbarPkg}/bin/webbar --dir ${agsBarUi} --height 86 "$@"
          '';
        });

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
          ];
        };
      });
    };
}
