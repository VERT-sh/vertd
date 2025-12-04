{
  description = "Build a cargo project";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane.url = "github:ipetkov/crane";

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-analyzer-src.follows = "";
    };

    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      fenix,
      flake-utils,
      ...
    }:
    (flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        inherit (pkgs) lib;

        craneLib = crane.mkLib pkgs;
        src = craneLib.cleanCargoSource ./.;

        commonArgs = {
          inherit src;
          strictDeps = true;
        };

        craneLibLLvmTools = craneLib.overrideToolchain (
          fenix.packages.${system}.complete.withComponents [
            "cargo"
            "llvm-tools"
            "rustc"
            "rustfmt"
          ]
        );

        # Build *just* the cargo dependencies, so we can reuse
        # all of that work (e.g. via cachix) when running in CI
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        # Build the actual crate itself, reusing the dependency
        # artifacts from above.
        vertd = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;

            nativeBuildInputs = [
              pkgs.makeWrapper
              pkgs.pkg-config
              pkgs.openssl
            ];

            postFixup = ''
              wrapProgram $out/bin/vertd --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath [ pkgs.libGL ]}"
            '';

            meta = {
              description = "VERT's solution to crappy video conversion services.";
              homepage = "https://github.com/vert-sh/vertd";
              license = lib.licenses.gpl3;
              platforms = lib.platforms.linux;
              maintainers = with lib.maintainers; [ justdeeevin ];
              mainProgram = "vertd";
            };
          }
        );
      in
      {
        packages = {
          default = vertd;
        }
        // lib.optionalAttrs (!pkgs.stdenv.isDarwin) {
          vertd-llvm-coverage = craneLibLLvmTools.cargoLlvmCov (
            commonArgs
            // {
              inherit cargoArtifacts;
            }
          );
        };

        apps.default = flake-utils.lib.mkApp {
          drv = vertd;
        };

        devShells.default = craneLib.devShell {
          packages = [
            pkgs.pkg-config
            pkgs.openssl
          ];
        };
      }
    ))
    // {
      nixosModules.default =
        {
          config,
          pkgs,
          lib,
          ...
        }:
        let
          inherit (lib)
            mkEnableOption
            mkIf
            mkOption
            types
            ;

          cfg = config.services.vertd;
        in
        {
          options.services.vertd = {
            enable = mkEnableOption "vertd video converter service";
            port = mkOption {
              type = types.port;
              description = "Port that vertd should listen to";
              example = 8080;
              default = 24153;
            };
          };

          config = mkIf cfg.enable {
            users.users.vertd = {
              group = "vertd";
              isSystemUser = true;
            };
            users.groups.vertd = { };

            environment.sessionVariables.PORT = cfg.port;

            systemd.services.vertd = {
              description = "vertd video converter service";
              wantedBy = [ "multi-user.target" ];
              after = [ "network.target" ];
              path = [ pkgs.ffmpeg ];
              serviceConfig = {
                User = "vertd";
                Group = "vertd";
                Restart = "always";
                RestartSec = 5;
                ExecStart = lib.getExe self.packages.${pkgs.system}.default;
                StateDirectory = "vertd";
                WorkingDirectory = "/var/lib/vertd";
                Environment = "PORT=${builtins.toString cfg.port}";
              };
            };
          };
        };
    };
}
