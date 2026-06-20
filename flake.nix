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
    scottylabs = {
      url = "git+https://codeberg.org/ScottyLabs/devenv";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs
    , devenv
    , scottylabs
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

          discord-verify = (scottylabs.mkLib pkgs).buildRustService {
            src = ./.;
            pname = "discord-verify";
            version = "0.1.0";
            nativeBuildInputs = [ pkgs.pkg-config pkgs.makeWrapper ];
            buildInputs = [ pkgs.openssl ];
            buildArgs.postInstall = ''
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
