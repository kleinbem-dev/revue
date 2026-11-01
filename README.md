# nix-revue

> **Nix-native AI PR code review engine with subscription fallback and Forgejo support.**

`nix-revue` is a self-hostable, forge-agnostic AI code review tool built in Rust. It was designed as an open-source alternative to CodeRabbit and hosted AI bots, built specifically to:

1. **Re-use your active AI subscriptions** (`claude -p` CLI, Gemini AI Studio free tier) instead of metered SaaS tokens.
2. **Support Forgejo and Gitea natively** alongside GitHub.
3. **Drop-in support for `.coderabbit.yaml`** (path filters, tone instructions, and language-specific fleet rules).
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
                    │ Filter .coderabbit.yaml │
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

## Configuration (`.coderabbit.yaml`)

`nix-revue` reads `.coderabbit.yaml` directly:

```yaml
language: en-GB
tone_instructions: "Short and direct. Lead with the problem and the fix. No praise, no filler."

reviews:
  high_level_summary: true
  path_filters:
    - "!**/flake.lock"
    - "!**/Cargo.lock"

  path_instructions:
    - path: "**/*.nix"
      instructions: "Check that /var state has impermanence persistence entries and sops secrets are properly gated."
    - path: "**/*.sh"
      instructions: "Scripts run under `set -euo pipefail`: flag unbound variables and unquoted expansions."
```

---

## NixOS Module

Add `nix-revue` to your NixOS configuration:

```nix
{
  services.nix-revue = {
    enable = true;
    listen = "127.0.0.1:8383";
    configFile = "/etc/nix-revue/.coderabbit.yaml";
    environmentFile = "/run/secrets/revue.env"; # FORGEJO_TOKEN, GEMINI_API_KEY
  };
}
```

---

## License

MIT OR Apache-2.0
