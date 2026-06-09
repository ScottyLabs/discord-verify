{
  description = "Discord Verify bot";

  nixConfig = {
    extra-substituters = [ "https://scottylabs.cachix.org" ];
    extra-trusted-public-keys = [
      "scottylabs.cachix.org-1:hajjEX5SLi/Y7yYloiXTt2IOr3towcTGRhMh1vu6Tjg="
    ];
  };

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    devenv.url = "github:cachix/devenv";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, devenv, fenix, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          toolchain = fenix.packages.${system}.latest.toolchain;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };
          discord-verify = rustPlatform.buildRustPackage {
            pname = "discord-verify";
            version = "0.1.0";
            src = ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "serenity-0.12.4" = "sha256-+/7gCmHF97/7HqJ7pIADCzwRPJ/+LVq9q5reFuz3pTk=";
              };
            };
            nativeBuildInputs = [
              pkgs.pkg-config
              pkgs.llvmPackages.bintools
              pkgs.makeWrapper
            ];
            buildInputs = [ pkgs.openssl ];
            RUSTFLAGS = "-Clink-self-contained=-linker";

            postInstall = ''
              cp ${./Cargo.toml} $out/Cargo.toml
              wrapProgram $out/bin/discord-verify --chdir "$out"
            '';
          };
        in
        {
          inherit discord-verify;
          default = discord-verify;
          devenv = devenv.packages.${system}.devenv;
        }
      );
    };
}
