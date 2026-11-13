{
  lib,
  rustPlatform,
  buildNpmPackage,
  pkg-config,
  sqlite,
  versionCheckHook,
  nix-update-script,
  runCommand,
  curl,
  git,
}:

let
  version = "0.1.0";

  # When copying to Nixpkgs (pkgs/by-name/re/revue/package.nix), replace localSrc
  # with fetchFromGitHub { owner = "kleinbem-dev"; repo = "revue"; tag = "v${finalAttrs.version}"; hash = "..."; }.
  localSrc = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../README.md
      ../LICENSE
      ../src
      ../tests
      ../web/package.json
      ../web/package-lock.json
      ../web/.npmrc
      ../web/tsconfig.json
      ../web/vite.config.ts
      ../web/src
      ../web/static
      ../web/tests
    ];
  };

  web = buildNpmPackage {
    pname = "revue-web";
    inherit version;
    src = "${localSrc}/web";

    npmDepsFetcherVersion = 2;
    npmDepsHash = "sha256-us6O8WZLEs6m4vI1DlN6ECDIJajsE1b+fygHe5Cu0b4=";

    strictDeps = true;
    __structuredAttrs = true;

    doCheck = true;
    checkPhase = ''
      runHook preCheck
      npm test
      npm run check
      runHook postCheck
    '';

    installPhase = ''
      runHook preInstall
      mkdir -p $out
      cp -r build/. $out/
      runHook postInstall
    '';
  };
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "revue";
  inherit version;
  src = localSrc;

  cargoLock = {
    lockFile = ../Cargo.lock;
  };

  strictDeps = true;
  __structuredAttrs = true;

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ sqlite ];
  nativeCheckInputs = [ git ];

  # Force libsqlite3-sys to link nixpkgs' patched system sqlite instead of its bundled C amalgamation.
  env = {
    LIBSQLITE3_SYS_USE_PKG_CONFIG = "1";
  };

  # Upstream CI runs cargo tests in the dev profile; avoids slow single-CGU LTO test linking.
  checkType = "debug";

  postInstall = ''
    mkdir -p $out/share/revue/web
    cp -r ${finalAttrs.passthru.web}/. $out/share/revue/web/
  '';

  nativeInstallCheckInputs = [ versionCheckHook ];
  doInstallCheck = true;

  passthru = {
    inherit web;
    updateScript = nix-update-script { };
    tests = {
      daemon-smoke =
        runCommand "revue-daemon-smoke"
          {
            nativeBuildInputs = [
              finalAttrs.finalPackage
              curl
            ];
          }
          ''
            export HOME="$TMPDIR/home"
            mkdir -p "$HOME" "$TMPDIR/repo"

            (
              cd "$TMPDIR/repo"
              revue init
              test -s .revue.yaml
            )

            revue daemon \
              --listen 127.0.0.1:18383 \
              --database-url "sqlite://$TMPDIR/revue.db?mode=rwc" &
            DAEMON_PID=$!
            trap 'kill "$DAEMON_PID" 2>/dev/null || true' EXIT

            ready=0
            for _ in $(seq 1 30); do
              if curl -fsS http://127.0.0.1:18383/health >/dev/null 2>&1; then
                ready=1
                break
              fi
              sleep 0.2
            done
            test "$ready" -eq 1

            health="$(curl -fsS http://127.0.0.1:18383/health)"
            grep -q "revue daemon is healthy" <<<"$health"

            status_json="$(curl -fsS http://127.0.0.1:18383/api/status)"
            grep -q '"status":"healthy"' <<<"$status_json"
            grep -q '"engine":"revue"' <<<"$status_json"

            reviews_json="$(curl -fsS http://127.0.0.1:18383/api/reviews)"
            grep -q '^\[' <<<"$reviews_json"

            index_html="$(curl -fsS http://127.0.0.1:18383/)"
            grep -qi '<!doctype html>' <<<"$index_html"

            kill "$DAEMON_PID"
            wait "$DAEMON_PID" || true
            touch "$out"
          '';
    };
  };

  meta = {
    description = "Self-hosted, forge-agnostic AI code review engine";
    homepage = "https://github.com/kleinbem-dev/revue";
    changelog = "https://github.com/kleinbem-dev/revue/releases/tag/v${finalAttrs.version}";
    license = lib.licenses.mit;
    maintainers = with lib.maintainers; [ kleinbem ];
    mainProgram = "revue";
    platforms = lib.platforms.unix;
  };
})
