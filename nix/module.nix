{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.nix-revue;
in
{
  options.services.nix-revue = {
    enable = lib.mkEnableOption "nix-revue AI PR code review daemon";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The nix-revue package to run.";
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
      description = "Path to an EnvironmentFile containing secrets (FORGEJO_TOKEN, GEMINI_API_KEY, GITHUB_TOKEN).";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.nix-revue = {
      description = "nix-revue AI PR code review webhook daemon";
      after = [ "network.target" ];
      wantedBy = [ "multi-user.target" ];

      serviceConfig = {
        ExecStart =
          let
            configArg = lib.optionalString (cfg.configFile != null) "--config ${cfg.configFile}";
          in
          "${lib.getExe cfg.package} ${configArg} daemon --listen ${cfg.listen}";
        EnvironmentFile = lib.mkIf (cfg.environmentFile != null) cfg.environmentFile;
        Restart = "always";
        RestartSec = "5s";
        DynamicUser = true;

        # Hardening
        ProtectSystem = "strict";
        ProtectHome = true;
        NoNewPrivileges = true;
        PrivateTmp = true;
      };
    };
  };
}
