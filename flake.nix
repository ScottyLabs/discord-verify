{
  description = "Discord Verify bot";

  nixConfig = {
    extra-substituters = [ "https://scottylabs.cachix.org" ];
    extra-trusted-public-keys = [
      "scottylabs.cachix.org-1:hajjEX5SLi/Y7yYloiXTt2IOr3towcTGRhMh1vu6Tjg="
    ];
  };

  inputs = {
    scottylabs.url = "git+https://git.cmu.dev/ScottyLabs/kennel";
    nixpkgs.follows = "scottylabs/nixpkgs";
    flake-parts.follows = "scottylabs/flake-parts";
  };

  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ inputs.scottylabs.flakeModules.default ];
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      perSystem =
        { pkgs, helpers, ... }:
        {
          scottylabs = {
            project.name = "discord-verify";
            rust.enable = true;
            valkey.enable = true;
            ricochet = {
              enable = true;
              appUrl = "http://localhost:3000";
            };
            previewDeployments = false;
          };

          services.discord-verify = {
            package = helpers.buildRustService {
              src = ./.;
              pname = "discord-verify";
              version = "0.1.0";
              nativeBuildInputs = [ pkgs.makeWrapper ];
              buildArgs.postInstall = ''
                cp ${./Cargo.toml} $out/Cargo.toml
                wrapProgram $out/bin/discord-verify --chdir "$out"
              '';
            };
            customDomain = "verify.scottylabs.org";
          };
        };
    };
}
