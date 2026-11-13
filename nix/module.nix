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

  userHome =
    if cfg.user != null && lib.hasAttr cfg.user config.users.users then
      config.users.users.${cfg.user}.home
    else
      "/home/${cfg.user}";

  userGroup =
    if cfg.group != null then
      cfg.group
    else if cfg.user != null && lib.hasAttr cfg.user config.users.users then
      config.users.users.${cfg.user}.group
    else
      "users";

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
      example = "alice";
      description = "User to run the daemon as when user-level CLI credentials are required. If null, runs with DynamicUser.";
    };

    group = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "users";
      description = "Group to run the daemon as when user is set.";
    };

    listen = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1:8383";
      example = "0.0.0.0:8383";
      description = "Address and port for the HTTP dashboard and webhook listener.";
    };

    configFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = "/etc/revue/config.yaml";
      description = "Path to the .revue.yaml configuration file. If null, searches repository root or uses built-in defaults.";
    };

    environmentFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = "/run/secrets/revue.env";
      description = "EnvironmentFile containing runtime secrets (FORGEJO_TOKEN, GITHUB_TOKEN, GEMINI_API_KEY, REVUE_WEBHOOK_SECRET, REVUE_API_KEY).";
    };

    databaseUrl = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "sqlite:///var/lib/revue/revue.db?mode=rwc";
      description = "Optional SQLite or PostgreSQL connection string passed via DATABASE_URL.";
    };

    workspaceRoot = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = "/srv/repos";
      description = "Optional root directory containing local repository checkouts for tree-context inspection (REVUE_WORKSPACE_ROOT).";
    };

    skillsDir = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = "/etc/revue/skills";
      description = "Optional directory of domain SKILL.md review guides (REVUE_SKILLS_DIR).";
    };

    persistencePath = lib.mkOption {
      type = lib.types.str;
      default = "/nix/persist";
      example = "/persist";
      description = "Root path for impermanence persistent storage when enabled on the host.";
    };
  };

  extraEnv =
    lib.optional (cfg.databaseUrl != null) "DATABASE_URL=${cfg.databaseUrl}"
    ++ lib.optional (cfg.workspaceRoot != null) "REVUE_WORKSPACE_ROOT=${toString cfg.workspaceRoot}"
    ++ lib.optional (cfg.skillsDir != null) "REVUE_SKILLS_DIR=${toString cfg.skillsDir}";
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

        systemd.tmpfiles.settings."10-revue" =
          lib.optionalAttrs (cfg.user != null) {
            "${userHome}/.local/share/revue".d = {
              inherit (cfg) user;
              group = userGroup;
              mode = "0750";
            };
          }
          // lib.optionalAttrs (cfg.user == null && options.environment ? persistence) {
            "/var/lib/private".d = {
              user = "root";
              group = "root";
              mode = "0700";
            };
            "${cfg.persistencePath}/var/lib/private".d = {
              user = "root";
              group = "root";
              mode = "0700";
            };
          };

        systemd.services.revue = {
          description = "revue AI PR code review webhook daemon";
          after = [ "network.target" ];
          wantedBy = [ "multi-user.target" ];

          path = [
            pkgs.git
            pkgs.jujutsu
            pkgs.bash
          ]
          ++ lib.optionals (cfg.user != null) [
            "/etc/profiles/per-user/${cfg.user}"
            "/run/current-system/sw"
          ];

          serviceConfig = {
            ExecStart = lib.escapeShellArgs (
              [ (lib.getExe cfg.package) ]
              ++ lib.optionals (cfg.configFile != null) [
                "--config"
                (toString cfg.configFile)
              ]
              ++ [
                "daemon"
                "--listen"
                cfg.listen
              ]
            );
            EnvironmentFile = lib.mkIf (cfg.environmentFile != null) cfg.environmentFile;
            Restart = "always";
            RestartSec = "5s";
          }
          // (
            if cfg.user != null then
              {
                User = cfg.user;
                Group = userGroup;
                DynamicUser = false;
                ProtectSystem = "strict";
                ProtectHome = "read-only";
                ReadWritePaths = [
                  "-${userHome}/.gemini"
                  "${userHome}/.local/share/revue"
                ];
                NoNewPrivileges = true;
                PrivateTmp = true;
                Environment = [
                  "HOME=${userHome}"
                  "REVUE_DATA_DIR=${userHome}/.local/share/revue"
                ]
                ++ extraEnv;
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
                ]
                ++ extraEnv;
              }
          );
        };
      }
      # Persist DynamicUser state when the host uses impermanence on /var.
      (lib.optionalAttrs (options.environment ? persistence) {
        environment.persistence."${cfg.persistencePath}".directories = lib.optionals (cfg.user == null) [
          "/var/lib/private/revue"
        ];
      })
    ]
  );
}
