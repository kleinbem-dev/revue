use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub tone_instructions: Option<String>,
    #[serde(default)]
    pub reviews: ReviewsConfig,
    #[serde(default)]
    pub forgejo: ForgejoConfig,
    #[serde(default)]
    pub providers: ProvidersConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReviewsConfig {
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default = "default_true")]
    pub high_level_summary: bool,
    #[serde(default)]
    pub path_filters: Vec<String>,
    #[serde(default)]
    pub path_instructions: Vec<PathInstruction>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathInstruction {
    pub path: String,
    pub instructions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgejoConfig {
    #[serde(default = "default_forgejo_url")]
    pub url: String,
    #[serde(default)]
    pub token: Option<String>,
}

fn default_forgejo_url() -> String {
    std::env::var("FORGEJO_URL").unwrap_or_else(|_| "https://git.kleinbem.dev".to_string())
}

impl Default for ForgejoConfig {
    fn default() -> Self {
        Self {
            url: default_forgejo_url(),
            token: std::env::var("FORGEJO_TOKEN").ok(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersConfig {
    #[serde(default = "default_fallback_order")]
    pub order: Vec<String>,
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
    #[serde(default = "default_ollama_model")]
    pub ollama_model: String,
    #[serde(default = "default_gemini_model")]
    pub gemini_model: String,
}

fn default_fallback_order() -> Vec<String> {
    vec![
        "claude-cli".to_string(),
        "gemini".to_string(),
        "ollama".to_string(),
    ]
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
            order: default_fallback_order(),
            ollama_url: default_ollama_url(),
            ollama_model: default_ollama_model(),
            gemini_model: default_gemini_model(),
        }
    }
}

impl AppConfig {
    /// Load config from `.coderabbit.yaml` or `.revue.yaml`, falling back to sensible defaults.
    pub fn load_or_default<P: AsRef<Path>>(path: P) -> Result<Self> {
        let p = path.as_ref();
        if p.exists() {
            let content = std::fs::read_to_string(p)
                .with_context(|| format!("Failed to read config from {:?}", p))?;
            let mut cfg: AppConfig = serde_yaml::from_str(&content)
                .with_context(|| format!("Failed to parse YAML from {:?}", p))?;
            
            // Enrich with env vars if not set in YAML
            if cfg.forgejo.token.is_none() {
                cfg.forgejo.token = std::env::var("FORGEJO_TOKEN").ok();
            }
            return Ok(cfg);
        }

        Ok(Self::default())
    }
}
