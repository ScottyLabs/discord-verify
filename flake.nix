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
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    { nixpkgs
    , devenv
    , crane
    , ...
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          craneLib = crane.mkLib pkgs;

          commonArgs = {
            pname = "discord-verify";
            version = "0.1.0";
            src = craneLib.cleanCargoSource ./.;
            strictDeps = true;
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = [ pkgs.openssl ];
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          discord-verify = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            doCheck = false;
            nativeBuildInputs = commonArgs.nativeBuildInputs ++ [ pkgs.makeWrapper ];
            postInstall = ''
              cp ${./Cargo.toml} $out/Cargo.toml
              wrapProgram $out/bin/discord-verify --chdir "$out"
            '';
          });
        in
        {
          inherit discord-verify;
          default = discord-verify;
          devenv = devenv.packages.${system}.devenv;
        }
      );
    };
}
