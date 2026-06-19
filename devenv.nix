{ inputs, ... }:

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
    ricochet.enable = true;

    kennel.services.discord-verify = {
      customDomain = "verify.scottylabs.org";
    };
  };
}
