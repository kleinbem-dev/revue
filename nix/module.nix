{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = if config.services.revue.enable then config.services.revue else config.services.nix-revue;
  enabled = config.services.revue.enable || config.services.nix-revue.enable;

  revueModuleOptions = {
    enable = lib.mkEnableOption "revue AI PR code review daemon";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.revue;
      defaultText = lib.literalExpression "pkgs.revue";
      description = "The revue package to run.";
    };

    user = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "User to run the daemon as (e.g. 'martin') to access user-level AI subscriptions and tool credentials. If null, runs with DynamicUser.";
    };

    group = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Group to run the daemon as.";
    };

    listen = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1:8383";
      description = "Address and port for the webhook listener.";
    };

    configFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Path to the .revue.yaml configuration file. If null, searches repository root or uses defaults.";
    };

    environmentFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Path to an EnvironmentFile containing secrets (FORGEJO_TOKEN, GEMINI_API_KEY, GITHUB_TOKEN, REVUE_WEBHOOK_SECRET).";
    };
  };
in
{
  options.services.revue = revueModuleOptions;
  options.services.nix-revue = revueModuleOptions;

  config = lib.mkIf enabled {
    systemd.services.revue = {
      description = "revue AI PR code review webhook daemon";
      after = [ "network.target" ];
      wantedBy = [ "multi-user.target" ];

      path = [
        pkgs.git
        pkgs.jujutsu
        pkgs.bash
      ];

      serviceConfig =
        {
          ExecStart =
            let
              configArg = lib.optionalString (cfg.configFile != null) "--config ${cfg.configFile}";
            in
            "${lib.getExe cfg.package} ${configArg} daemon --listen ${cfg.listen}";
          EnvironmentFile = lib.mkIf (cfg.environmentFile != null) cfg.environmentFile;
          Restart = "always";
          RestartSec = "5s";
        }
        // (
          if cfg.user != null then
            {
              User = cfg.user;
              Group = if cfg.group != null then cfg.group else "users";
              DynamicUser = false;
              ProtectSystem = "strict";
              ProtectHome = "read-only";
              NoNewPrivileges = true;
              PrivateTmp = true;
              Environment = [
                "HOME=/home/${cfg.user}"
                "PATH=/etc/profiles/per-user/${cfg.user}/bin:/run/current-system/sw/bin"
              ];
            }
          else
            {
              DynamicUser = true;
              ProtectSystem = "strict";
              ProtectHome = true;
              NoNewPrivileges = true;
              PrivateTmp = true;
            }
        );
    };
  };
}
