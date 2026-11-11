{
  config,
  lib,
  pkgs,
  options,
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

    persistencePath = lib.mkOption {
      type = lib.types.str;
      default = "/nix/persist";
      description = "Root path for impermanence persistent storage.";
    };
  };
in
{
  options.services.revue = revueModuleOptions;
  options.services.nix-revue = revueModuleOptions;

  config = lib.mkIf enabled (
    lib.mkMerge [
      {
        assertions = [
          {
            assertion = !(config.services.revue.enable && config.services.nix-revue.enable);
            message = "Cannot enable both services.revue and services.nix-revue.";
          }
        ];

        systemd.tmpfiles.rules =
          lib.optionals (cfg.user != null) [
            "d /home/${cfg.user}/.local/share/revue 0750 ${cfg.user} ${if cfg.group != null then cfg.group else "users"} -"
          ]
          ++ lib.optionals (cfg.user == null && options.environment ? persistence) [
            "d /var/lib/private 0700 root root -"
            "d ${cfg.persistencePath}/var/lib/private 0700 root root -"
          ];

        systemd.services.revue = {
          description = "revue AI PR code review webhook daemon";
          after = [ "network.target" ];
          wantedBy = [ "multi-user.target" ];

          # Jujutsu, Git, and Bash are required by revue for repo operations and ephemeral workspaces.
          path = [
            pkgs.git
            pkgs.jujutsu
            pkgs.bash
          ]
          ++ lib.optionals (cfg.user != null) [
            "/etc/profiles/per-user/${cfg.user}"
            "/run/current-system/sw"
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
                  ReadWritePaths = [
                    "/home/${cfg.user}/.gemini"
                    "/home/${cfg.user}/.local/share/revue"
                  ];
                  NoNewPrivileges = true;
                  PrivateTmp = true;
                  Environment = [
                    "HOME=/home/${cfg.user}"
                    "REVUE_DATA_DIR=/home/${cfg.user}/.local/share/revue"
                  ];
                }
              else
                {
                  DynamicUser = true;
                  StateDirectory = "revue";
                  ProtectSystem = "strict";
                  ProtectHome = true;
                  NoNewPrivileges = true;
                  PrivateTmp = true;
                  Environment = [
                    "REVUE_DATA_DIR=/var/lib/revue"
                  ];
                }
            );
        };
      }
      # /var is tmpfs on the fleet; persist the state directory across reboots.
      # A user's state is in their home, which the host persists (or mounts
      # from disk, where impermanence rejects it). optionalAttrs: hosts
      # without the impermanence module.
      (lib.optionalAttrs (options.environment ? persistence) {
        environment.persistence."${cfg.persistencePath}".directories = lib.optionals (cfg.user == null) [
          "/var/lib/private/revue"
        ];
      })
    ]
  );
}
