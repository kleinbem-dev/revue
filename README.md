# nix-revue

> **Nix-native AI PR code review engine with subscription fallback and Forgejo support.**

`nix-revue` is a self-hostable, forge-agnostic AI code review tool built in Rust. It was designed as an open-source, subscription-friendly code reviewer built specifically to:

1. **Re-use your active AI subscriptions** (`claude -p` CLI, Gemini AI Studio free tier) instead of paying per-token SaaS markups.
2. **Support Forgejo and Gitea natively** alongside GitHub.
3. **Native `.revue.yaml` Configuration**: Intuitive syntax with rule targeting, path ignore patterns, and severity gating (with backward-compatible support for legacy `.coderabbit.yaml`).
4. **Automatic Fallback Chain**: If your Claude CLI session hits rate limits, it automatically cascades to Gemini Flash, and then to local Ollama (`qwen2.5-coder` / `deepseek-r1`).
5. **Nix-Native**: First-class Nix flake and NixOS module (`services.nix-revue`).

---

## Architecture & Provider Cascade

```
                    ┌─────────────────────────┐
                    │      PR Diff Event      │
                    └────────────┬────────────┘
                                 │
                    ┌────────────▼────────────┐
                    │  Filter .revue.yaml     │
                    └────────────┬────────────┘
                                 │
           ┌─────────────────────▼─────────────────────┐
           │ 1. Claude CLI (claude -p)                 │──(Success)──┐
           │    Active Pro/Team subscription ($0/tok)  │             │
           └─────────────────────┬─────────────────────┘             │
                           (Rate Limit)                              │
           ┌─────────────────────▼─────────────────────┐             │
           │ 2. Google Gemini 2.0 Flash                │──(Success)──┤
           │    Google AI Studio Free Tier             │             │
           └─────────────────────┬─────────────────────┘             │
                            (Failover)                               │
           ┌─────────────────────▼─────────────────────┐             │
           │ 3. Local Ollama (Qwen2.5-Coder / DeepSeek)│──(Success)──┤
           │    100% Offline / Local GPU               │             │
           └───────────────────────────────────────────┘             │
                                                                     │
                                  ┌──────────────────────────────────▼──┐
                                  │ Post Inline Comments to Forgejo / GH│
                                  └─────────────────────────────────────┘
```

---

## Quickstart

### Run locally via Nix
```bash
# Initialize a new .revue.yaml config
nix run github:kleinbem-dev/nix-revue -- init

# Review uncommitted changes against main
nix run github:kleinbem-dev/nix-revue -- diff main

# Review a pull request on Forgejo (dry run)
nix run github:kleinbem-dev/nix-revue -- pr 42 --repo kleinbem/nix-config --dry-run
```

### Dev Shell
```bash
nix develop
cargo build
```

---

## Native Configuration (`.revue.yaml`)

Run `nix-revue init` to generate a `.revue.yaml`:

```yaml
version: 1

# Forge configuration
forge:
  # "forgejo", "gitea", "github", or "auto" (detected from git remote)
  type: auto
  url: "https://git.kleinbem.dev"

# Multi-model provider cascade
providers:
  cascade:
    - antigravity  # Google Gemini subscription via Antigravity CLI ($0 key, zero setup)
    - gemini-api   # Google AI Studio free tier API (fallback)
    - ollama       # Local offline GPU inference (fallback)
    - claude       # Claude Code CLI (last resort)

  ollama_url: "http://127.0.0.1:11434"
  ollama_model: "qwen2.5-coder:14b"
  gemini_model: "gemini-2.0-flash"

# Review settings and inspection rules
review:
  language: en-GB
  tone: "Short and direct. Lead with the problem and the fix. No praise, no filler."
  
  # Minimum severity level to post inline comments: info | warning | error | critical
  min_severity: warning

  # Glob patterns to ignore
  ignore:
    - "**/flake.lock"
    - "**/Cargo.lock"
    - "**/package-lock.json"
    - "dist/**"
    - "target/**"

  # Path-specific rules and domain instructions
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

    - path: "**/*.sh"
      instructions: |
        Scripts run under `set -euo pipefail`: flag unbound variables and unquoted expansions.
```

---

## NixOS Module

Add `nix-revue` to your NixOS configuration:

```nix
{
  services.nix-revue = {
    enable = true;
    listen = "127.0.0.1:8383";
    environmentFile = "/run/secrets/revue.env"; # FORGEJO_TOKEN, GEMINI_API_KEY
  };
}
```

---

## License

MIT OR Apache-2.0
