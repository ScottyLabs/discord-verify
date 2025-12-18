{ self }:

{ config, lib, pkgs, ... }:

let
  cfg = config.services.discord-verify;
in
{
  options.services.discord-verify = {
    enable = lib.mkEnableOption "Discord Verify bot";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      description = "The discord-verify package to use";
    };

    environmentFile = lib.mkOption {
      type = lib.types.path;
      description = "Path to environment file containing all configuration";
    };
  };

  config = lib.mkIf cfg.enable {
    users.users.discord-verify = {
      isSystemUser = true;
      group = "discord-verify";
    };
    users.groups.discord-verify = {};

    systemd.services.discord-verify = {
      description = "Discord Verify Bot";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];

      serviceConfig = {
        Type = "simple";
        User = "discord-verify";
        Group = "discord-verify";
        WorkingDirectory = "${cfg.package}";
        EnvironmentFile = cfg.environmentFile;
        ExecStart = "${cfg.package}/bin/discord-verify";
        Restart = "on-failure";
        RestartSec = "10s";

        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
      };
    };
  };
}
