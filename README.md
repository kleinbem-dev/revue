# Revue 🎭

> **Self-hosted, forge-agnostic AI code review engine with subscription fallback (Gemini, Claude, Ollama)**

[![CI](https://github.com/kleinbem-dev/revue/actions/workflows/ci.yml/badge.svg)](https://github.com/kleinbem-dev/revue/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**Revue** is a self-hostable, forge-agnostic AI code review daemon and CLI tool built in Rust. It was designed as an open-source, subscription-friendly replacement for SaaS code review tools like CodeRabbit, built specifically to:

1. **Re-use your active AI subscriptions:** Works directly with your Google Gemini subscription ($0, zero API keys required via headless CLI) or Claude Code subscription (`claude -p`), avoiding per-seat or per-token SaaS markups.
2. **Two-Pass Review Pipeline:**
   - **Pass 1 (Detective):** High-recall broad search for security vulnerabilities, logic flaws, architectural regression, and edge cases.
   - **Pass 2 (Judge):** Rigorous verification that checks candidate findings against surrounding file context, discarding hallucinations and enforcing confidence scoring ($\ge 8/10$).
3. **Surrounding Context Expansion:** Automatically reads $\pm 40$ lines (or whole files) from disk around modified diff hunks so the models see call-sites, type definitions, and struct fields rather than diff snippets in isolation.
4. **Forge-Agnostic:** Native support for **Forgejo** and **Gitea** alongside **GitHub**.
5. **HMAC-SHA256 Webhook Verification:** Supports `X-Hub-Signature-256` (GitHub) and `X-Gitea-Signature` / `X-Forgejo-Signature` (Forgejo/Gitea) with constant-time verification.
6. **Multi-Model Provider Cascade:** Cascades gracefully on quota limits:
   `Gemini Subscription (Headless CLI)` $\rightarrow$ `Gemini AI Studio (API)` $\rightarrow$ `Ollama (Local Offline GPU)` $\rightarrow$ `Claude CLI`.
7. **Cross-Platform & Nix-First:** Runs on any Linux distribution, macOS, Windows, or Docker/Podman container, with first-class Nix flakes and NixOS module support.

---

## Architecture & Provider Cascade

```text
                      ┌─────────────────────────┐
                      │  PR Webhook / Diff CLI  │
                      └────────────┬────────────┘
                                   │
                      ┌────────────▼────────────┐
                      │    Read Context (±40)   │
                      │    Filter .revue.yaml   │
                      └────────────┬────────────┘
                                   │
             ┌─────────────────────▼─────────────────────┐
             │ 1. Google Gemini (Subscription Headless)  │──(Success)──┐
             │    $0 API key, low latency (~15s)         │             │
             └─────────────────────┬─────────────────────┘             │
                             (Quota / Error)                           │
             ┌─────────────────────▼─────────────────────┐             │
             │ 2. Gemini AI Studio REST API              │──(Success)──┤
             │    Google AI Studio Key                   │             │
             └─────────────────────┬─────────────────────┘             │
                               (Failover)                              │
             ┌─────────────────────▼─────────────────────┐             │
             │ 3. Local Ollama (Qwen2.5-Coder / DeepSeek)│──(Success)──┤
             │    100% Offline / Local GPU               │             │
             └─────────────────────┬─────────────────────┘             │
                               (Failover)                              │
             ┌─────────────────────▼─────────────────────┐             │
             │ 4. Claude CLI (claude -p)                 │──(Success)──┤
             │    Active Claude Pro/Team subscription    │             │
             └───────────────────────────────────────────┘             │
                                                                       │
                                    ┌──────────────────────────────────┘
                                    │
                      ┌─────────────▼─────────────┐
                      │  Pass 1: Broad Detective  │
                      └─────────────┬─────────────┘
                                    │ (candidate bugs)
                      ┌─────────────▼─────────────┐
                      │  Pass 2: Strict Judge     │
                      └─────────────┬─────────────┘
                                    │ (confidence >= 8)
                      ┌─────────────▼─────────────┐
                      │ Post Inline Comments &    │
                      │ High-Level Summary to PR  │
                      └───────────────────────────┘
```

---

## Installation & Deployment

Revue is a standard, portable Rust binary and can be installed via **Cargo**, **Docker**, **Nix**, or **pre-built binary**.

### 1. Cargo (Any Linux, macOS, Windows)

```bash
cargo install --git https://github.com/kleinbem-dev/revue
```

### 2. Docker / Podman (Any System)

```bash
docker run -d \
  --name revue \
  -p 8383:8383 \
  -e REVUE_WEBHOOK_SECRET="your-webhook-secret" \
  -e FORGEJO_TOKEN="your-forgejo-token" \
  -e GITHUB_TOKEN="your-github-token" \
  ghcr.io/kleinbem-dev/revue:latest
```

### 3. Nix Flake (Any Linux / macOS)

Run without installing:
```bash
nix run github:kleinbem-dev/revue -- diff main
```

Add to your devshell:
```nix
packages = [ inputs.revue.packages.${system}.default ];
```

### 4. NixOS Module

Add to your NixOS configuration:

```nix
{
  inputs = {
    revue.url = "github:kleinbem-dev/revue";
  };

  outputs = { self, nixpkgs, revue, ... }: {
    nixosConfigurations.my-host = nixpkgs.lib.nixosSystem {
      modules = [
        revue.nixosModules.default
        {
          services.revue = {
            enable = true;
            listen = "0.0.0.0:8383";
            user = "martin"; # Allows access to user credentials (~/.gemini or ~/.config)
            environmentFile = "/run/secrets/revue.env"; # REVUE_WEBHOOK_SECRET, FORGEJO_TOKEN, etc.
          };
        }
      ];
    };
  };
}
```

---

## CLI Usage

### Local Diff Review

Review uncommitted or branch changes before pushing:

```bash
# Review current branch against main
revue diff main

# JSON output for CI pipelines
revue diff main --format json
```

### Manual PR Review

Trigger a review on any Pull Request:

```bash
# Dry run: prints comments to terminal
revue pr 42 --dry-run

# Post comments directly to Forgejo / GitHub
revue pr 42 --repo kleinbem/nix-config
```

### Webhook Daemon Mode

Start the HTTP webhook listener:

```bash
revue daemon --listen 0.0.0.0:8383 --webhook-secret "my-hmac-secret"
```

Configure your webhook in **GitHub** or **Forgejo / Gitea**:
- **Payload URL:** `https://revue.yourdomain.com/webhook`
- **Content type:** `application/json`
- **Secret:** `my-hmac-secret`
- **Events:** `Pull request` and `Issue comments`

### Web Control Dashboard (SvelteKit + Svelte 5)

Revue includes a reactive web control panel built with modern SvelteKit and Svelte 5 runes (`$state`, `$derived`, `$props`). The dashboard provides:
- **Live Pipeline Metrics:** Total reviews, Pass 1 candidates, Pass 2 filtered noise, verified findings, uptime.
- **Provider Cascade Status:** Visual health and status for Gemini Subscription ($0 flat), Gemini AI Studio API, Local Ollama, and Claude Code.
- **Review Inspector:** Filter findings by severity (Errors, Warnings, Info), inspect Judge confidence scores, and preview code diff snippets with line numbers.
- **On-Demand PR Review Trigger:** Queue reviews for any GitHub or Forgejo PR directly from the web interface.

```bash
# Build the web dashboard static bundle
cd web
npm install
npm run build

# Run the daemon (automatically serves web/build)
revue daemon --listen 0.0.0.0:8383
```
Open `http://localhost:8383` in your browser.

---

## Configuration (`.revue.yaml`)

Initialize a default configuration:

```bash
revue init
```

Example `.revue.yaml`:

```yaml
version: 1

forge:
  # Supported: auto, forgejo, gitea, github
  type: auto
  url: "https://git.kleinbem.dev"
  # webhook_secret: "override-secret-here" # Or set REVUE_WEBHOOK_SECRET

providers:
  cascade:
    - antigravity  # Google Gemini subscription ($0, headless CLI)
    - gemini-api   # Google AI Studio REST API
    - ollama       # Local offline GPU inference
    - claude       # Claude Code CLI (fallback)
  
  ollama_url: "http://127.0.0.1:11434"
  ollama_model: "qwen2.5-coder:14b"
  gemini_model: "gemini-2.0-flash"

review:
  language: en-GB
  tone: "Short and direct. Lead with the problem and the fix. No praise, no filler."
  
  # Two-pass verification: Pass 1 discovers bugs, Pass 2 judges & filters false positives
  two_pass: true

  # Minimum severity level to post inline comments: info | warning | error | critical
  min_severity: warning

  # Glob patterns to ignore
  ignore:
    - "**/flake.lock"
    - "**/Cargo.lock"
    - "**/package-lock.json"
    - "dist/**"
    - "target/**"

  # Path-specific domain rules
  rules:
    - path: "**"
      instructions: |
        Every workaround or fix for someone else's bug needs an upstream issue link.
        Flag workarounds without one.

    - path: "**/*.nix"
      instructions: |
        NixOS fleet checks:
        - Ensure sops secrets are gated by their feature enable option.
        - Host /var is tmpfs: check for missing persistence entries.
        - No plaintext secrets.

    - path: "**/*.rs"
      instructions: |
        Check for unwrap() in production paths, missing error propagation,
        and potential deadlocks.
```

---

## License

[MIT](LICENSE)
