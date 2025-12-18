{
  description = "Discord Verify bot";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, fenix }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          rustToolchain = fenix.packages.${system}.latest.toolchain;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain.cargo;
            rustc = rustToolchain.rustc;
          };
        in
        {
          default = rustPlatform.buildRustPackage {
            pname = "discord-verify";
            version = "0.1.0";
            src = ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "serenity-0.12.4" = "sha256-+/7gCmHF97/7HqJ7pIADCzwRPJ/+LVq9q5reFuz3pTk=";
              };
            };
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = [ pkgs.openssl ];
          };
        });

      nixosModules.default = import ./nix/module.nix { inherit self; };
    };
}
