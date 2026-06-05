{ pkgs, inputs, ... }:
{
  imports = [
    inputs.scottylabs.devenvModules.default
  ];

  scottylabs = {
    enable = true;
    project.name = "discord-verify";

    rust.enable = true;
    valkey.enable = true;
    secrets.enable = true;

    kennel.services.discord-verify = {
      customDomain = "verify.scottylabs.org";
      oidc.redirectPaths = [
        "/auth/callback"
        "/link-callback"
      ];
    };
  };

  processes.discord-verify = {
    exec = "secretspec run -- cargo run";
    ready.http.get = {
      port = 3000;
      path = "/health";
    };
  };

  env.REDIS_URL = "redis://127.0.0.1:6379";
}
