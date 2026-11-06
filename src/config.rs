use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevueConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub forge: ForgeConfig,
    #[serde(default)]
    pub providers: ProvidersConfig,
    #[serde(default)]
    pub review: ReviewSection,
    #[serde(default)]
    pub server: ServerConfig,
}

fn default_version() -> u32 {
    1
}

/// Server listening and authentication settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Default bind address (defaults to 127.0.0.1:8383)
    #[serde(default = "default_bind_addr")]
    pub bind: String,
    /// Optional API key for authenticating /api routes (or set REVUE_API_KEY)
    #[serde(default)]
    pub api_key: Option<String>,
    /// Allow unauthenticated webhooks (insecure testing mode only)
    #[serde(default)]
    pub insecure_webhooks: bool,
    /// Allowed CORS origins
    #[serde(default)]
    pub cors_origins: Vec<String>,
}

fn default_bind_addr() -> String {
    "127.0.0.1:8383".to_string()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind_addr(),
            api_key: std::env::var("REVUE_API_KEY").ok(),
            insecure_webhooks: false,
            cors_origins: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeConfig {
    /// Forge type: "forgejo", "gitea", "github", or "auto"
    #[serde(default = "default_forge_type")]
    pub r#type: String,
    /// Base URL for the forge (e.g. https://git.kleinbem.dev)
    #[serde(default = "default_forge_url")]
    pub url: String,
    /// Access token (overridden by FORGEJO_TOKEN / GITHUB_TOKEN if empty)
    #[serde(default)]
    pub token: Option<String>,
    /// Webhook HMAC secret for signature verification (overridden by REVUE_WEBHOOK_SECRET)
    #[serde(default)]
    pub webhook_secret: Option<String>,
}

fn default_forge_type() -> String {
    "auto".to_string()
}

fn default_forge_url() -> String {
    std::env::var("FORGEJO_URL").unwrap_or_else(|_| "https://git.kleinbem.dev".to_string())
}

impl Default for ForgeConfig {
    fn default() -> Self {
        Self {
            r#type: default_forge_type(),
            url: default_forge_url(),
            token: std::env::var("FORGEJO_TOKEN")
                .or_else(|_| std::env::var("GITHUB_TOKEN"))
                .ok(),
            webhook_secret: std::env::var("REVUE_WEBHOOK_SECRET").ok(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersConfig {
    /// Cascade failover order (e.g. ["antigravity", "mesh-llm", "gemini-api", "ollama", "claude"])
    #[serde(default = "default_cascade")]
    pub cascade: Vec<String>,
    #[serde(default = "default_mesh_llm_url")]
    pub mesh_llm_url: String,
    #[serde(default = "default_mesh_llm_model")]
    pub mesh_llm_model: String,
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
    #[serde(default = "default_ollama_model")]
    pub ollama_model: String,
    #[serde(default = "default_gemini_model")]
    pub gemini_model: String,
}

fn default_cascade() -> Vec<String> {
    vec![
        "antigravity".to_string(),
        "mesh-llm".to_string(),
        "gemini-api".to_string(),
        "ollama".to_string(),
        "claude".to_string(),
    ]
}

fn default_mesh_llm_url() -> String {
    std::env::var("MESH_LLM_URL").unwrap_or_else(|_| "http://127.0.0.1:9337/v1".to_string())
}

fn default_mesh_llm_model() -> String {
    std::env::var("MESH_LLM_MODEL").unwrap_or_else(|_| "Qwen3-4B-Q4_K_M".to_string())
}

fn default_ollama_url() -> String {
    std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".to_string())
}

fn default_ollama_model() -> String {
    std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "qwen2.5-coder:14b".to_string())
}

fn default_gemini_model() -> String {
    std::env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-2.0-flash".to_string())
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        Self {
            cascade: default_cascade(),
            mesh_llm_url: default_mesh_llm_url(),
            mesh_llm_model: default_mesh_llm_model(),
            ollama_url: default_ollama_url(),
            ollama_model: default_ollama_model(),
            gemini_model: default_gemini_model(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSection {
    #[serde(default = "default_tone")]
    pub tone: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_min_severity")]
    pub min_severity: String,
    #[serde(default = "default_true")]
    pub summary: bool,
    #[serde(default = "default_true")]
    pub two_pass: bool,
    #[serde(default = "default_true")]
    pub semgrep: bool,
    #[serde(default)]
    pub semgrep_config: Option<String>,
    #[serde(default)]
    pub ignore: Vec<String>,
    #[serde(default)]
    pub skills_dir: Option<String>,
    #[serde(default)]
    pub rules: Vec<PathRule>,
}

fn default_tone() -> String {
    "Short and direct. Lead with the problem and the fix. No praise, no filler.".to_string()
}

fn default_language() -> String {
    "en-GB".to_string()
}

fn default_min_severity() -> String {
    "warning".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathRule {
    pub path: String,
    pub instructions: String,
}

impl Default for ReviewSection {
    fn default() -> Self {
        Self {
            tone: default_tone(),
            language: default_language(),
            min_severity: default_min_severity(),
            summary: true,
            two_pass: true,
            semgrep: true,
            semgrep_config: None,
            ignore: vec![
                "**/flake.lock".to_string(),
                "**/Cargo.lock".to_string(),
                "**/package-lock.json".to_string(),
            ],
            rules: Vec::new(),
            skills_dir: None,
        }
    }
}

impl Default for RevueConfig {
    fn default() -> Self {
        Self {
            version: 1,
            forge: ForgeConfig::default(),
            providers: ProvidersConfig::default(),
            review: ReviewSection::default(),
            server: ServerConfig::default(),
        }
    }
}

impl RevueConfig {
    /// Detect and load configuration from .revue.yaml, revue.yaml, or legacy .coderabbit.yaml
    pub fn find_and_load(explicit_path: Option<&str>) -> Result<Self> {
        if let Some(path) = explicit_path {
            return Self::load_from_path(Path::new(path));
        }

        let candidates = [
            ".revue.yaml",
            "revue.yaml",
            ".revue.yml",
            ".nix-revue.yaml",
            ".coderabbit.yaml", // Legacy compatibility
        ];

        for c in &candidates {
            let p = Path::new(c);
            if p.exists() {
                if *c == ".coderabbit.yaml" {
                    info!("Detected legacy .coderabbit.yaml. Parsing with backward compatibility.");
                    return Self::load_legacy_coderabbit(p);
                }
                info!("Loading config from {}", c);
                return Self::load_from_path(p);
            }
        }

        info!("No configuration file found. Using default fleet settings.");
        Ok(Self::default())
    }

    pub fn load_from_path(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config from {:?}", path))?;

        let mut cfg: RevueConfig = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse YAML from {:?}", path))?;

        if cfg.forge.token.is_none() {
            cfg.forge.token = std::env::var("FORGEJO_TOKEN")
                .or_else(|_| std::env::var("GITHUB_TOKEN"))
                .ok();
        }

        if cfg.forge.webhook_secret.is_none() {
            cfg.forge.webhook_secret = std::env::var("REVUE_WEBHOOK_SECRET").ok();
        }

        if cfg.server.api_key.is_none() {
            cfg.server.api_key = std::env::var("REVUE_API_KEY").ok();
        }

        Ok(cfg)
    }

    /// Convert legacy .coderabbit.yaml into native RevueConfig
    fn load_legacy_coderabbit(path: &Path) -> Result<Self> {
        #[derive(Deserialize)]
        struct LegacyConfig {
            language: Option<String>,
            tone_instructions: Option<String>,
            reviews: Option<LegacyReviews>,
        }

        #[derive(Deserialize)]
        struct LegacyReviews {
            high_level_summary: Option<bool>,
            path_filters: Option<Vec<String>>,
            path_instructions: Option<Vec<LegacyPathInstruction>>,
        }

        #[derive(Deserialize)]
        struct LegacyPathInstruction {
            path: String,
            instructions: String,
        }

        let content = std::fs::read_to_string(path)?;
        let legacy: LegacyConfig = serde_yaml::from_str(&content)?;

        let mut config = Self::default();

        if let Some(lang) = legacy.language {
            config.review.language = lang;
        }
        if let Some(tone) = legacy.tone_instructions {
            config.review.tone = tone;
        }
        if let Some(reviews) = legacy.reviews {
            if let Some(summary) = reviews.high_level_summary {
                config.review.summary = summary;
            }
            if let Some(filters) = reviews.path_filters {
                config.review.ignore = filters
                    .into_iter()
                    .map(|f| f.strip_prefix('!').unwrap_or(&f).to_string())
                    .collect();
            }
            if let Some(instructions) = reviews.path_instructions {
                config.review.rules = instructions
                    .into_iter()
                    .map(|i| PathRule {
                        path: i.path,
                        instructions: i.instructions,
                    })
                    .collect();
            }
        }

        Ok(config)
    }

    /// Generate sample .revue.yaml content
    pub fn default_yaml_template() -> &'static str {
        r#"# nix-revue configuration
# Schema version
version: 1

# Forge configuration
forge:
  # "forgejo", "gitea", "github", or "auto" (detected from git remote)
  type: auto
  # Base URL for Forgejo (used when forge is self-hosted)
  url: "https://git.kleinbem.dev"
  # Webhook HMAC secret for signature verification (or set REVUE_WEBHOOK_SECRET)
  # webhook_secret: "your-shared-secret"

# Multi-model provider cascade (primary -> fallback 1 -> fallback 2)
providers:
  cascade:
    - antigravity  # Google Gemini subscription via Antigravity CLI ($0 key, zero setup)
    - mesh-llm     # Fleet P2P GPU mesh (via local router or orin-nano)
    - gemini-api   # Google AI Studio REST API (fallback if key is set)
    - ollama       # Local offline GPU inference (fallback)
    - claude       # Claude Code CLI (last resort)

  mesh_llm_url: "http://127.0.0.1:9337/v1"
  mesh_llm_model: "Qwen3-4B-Q4_K_M"
  ollama_url: "http://127.0.0.1:11434"
  ollama_model: "qwen2.5-coder:14b"
  gemini_model: "gemini-2.0-flash"

# Review settings and inspection rules
review:
  language: en-GB
  tone: "Short and direct. Lead with the problem and the fix. No praise, no filler."
  
  # Two-pass verification: Pass 1 discovers candidate bugs, Pass 2 judges & filters false positives
  two_pass: true

  # Pass 0: Deterministic static AST analysis via Semgrep before LLM cascade
  semgrep: true
  # Optional explicit config path (defaults to .semgrep/ or .semgrep.yml in repo root)
  # semgrep_config: ".semgrep"

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
        NixOS checks:
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
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_native_revue_yaml() {
        let yaml = RevueConfig::default_yaml_template();
        let config: RevueConfig =
            serde_yaml::from_str(yaml).expect("Template should be valid YAML");
        assert_eq!(config.version, 1);
        assert_eq!(config.forge.url, "https://git.kleinbem.dev");
        assert_eq!(config.providers.cascade.len(), 5);
        assert_eq!(config.providers.cascade[0], "antigravity");
        assert_eq!(config.providers.cascade[1], "mesh-llm");
        assert!(config.review.semgrep);
        assert!(config.review.ignore.contains(&"**/flake.lock".to_string()));
    }

    #[test]
    fn test_legacy_coderabbit_conversion() {
        let legacy_yaml = r#"
language: en-GB
tone_instructions: "Direct and crisp"
reviews:
  high_level_summary: true
  path_filters:
    - "!**/flake.lock"
    - "!**/Cargo.lock"
  path_instructions:
    - path: "**/*.nix"
      instructions: "No plaintext secrets"
"#;
        let tmp_path = std::env::temp_dir().join("test_coderabbit.yaml");
        std::fs::write(&tmp_path, legacy_yaml).unwrap();

        let config = RevueConfig::load_legacy_coderabbit(&tmp_path)
            .expect("Legacy conversion should succeed");
        let _ = std::fs::remove_file(tmp_path);
        assert_eq!(config.review.language, "en-GB");
        assert_eq!(config.review.tone, "Direct and crisp");
        assert_eq!(config.review.ignore, vec!["**/flake.lock", "**/Cargo.lock"]);
        assert_eq!(config.review.rules.len(), 1);
        assert_eq!(config.review.rules[0].instructions, "No plaintext secrets");
    }
}
