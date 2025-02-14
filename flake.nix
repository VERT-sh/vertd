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
    flake-utils.lib.eachDefaultSystem (
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

            nativeBuildInputs = [ pkgs.makeWrapper ];

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
        checks = {
          # Build the crate as part of `nix flake check` for convenience
          inherit vertd;

          # Run clippy (and deny all warnings) on the crate source,
          # again, reusing the dependency artifacts from above.
          #
          # Note that this is done as a separate derivation so that
          # we can block the CI if there are issues here, but not
          # prevent downstream consumers from building our crate by itself.
          vertd-clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            }
          );

          vertd-doc = craneLib.cargoDoc (
            commonArgs
            // {
              inherit cargoArtifacts;
            }
          );

          # Check formatting
          vertd-fmt = craneLib.cargoFmt {
            inherit src;
          };

          vertd-toml-fmt = craneLib.taploFmt {
            src = pkgs.lib.sources.sourceFilesBySuffices src [ ".toml" ];
            # taplo arguments can be further customized below as needed
            # taploExtraArgs = "--config ./taplo.toml";
          };

          # Run tests with cargo-nextest
          # Consider setting `doCheck = false` on `vertd` if you do not want
          # the tests to run twice
          vertd-nextest = craneLib.cargoNextest (
            commonArgs
            // {
              inherit cargoArtifacts;
              partitions = 1;
              partitionType = "count";
              cargoNextestPartitionsExtraArgs = "--no-tests=pass";
            }
          );
        };

        packages =
          {
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
          checks = self.checks.${system};
        };

        nixosModules.default =
          { config, ... }:
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
            option.services.vertd = {
              enable = mkEnableOption "vertd video converter service";
              port = mkOption {
                types = types.port;
                description = "Port that vertd should listen to";
                example = 8080;
              };
            };

            config = mkIf cfg.enable {
              systemd.services.vertd = {
                description = "vertd video converter service";
                wantedBy = [ "multi-user.target" ];
                after = [ "network.target" ];
                script = lib.getExe vertd;
                serviceConfig = {
                  User = "vertd";
                  Group = "vertd";
                  Restart = "always";
                  RestartSec = 5;
                  ExecStartPre = "mkdir -p /var/lib/vertd; chown vertd:vertd /var/lib/vertd";
                  WorkingDirectory = "/var/lib/vertd";
                };
              };
            };
          };
      }
    );
}
