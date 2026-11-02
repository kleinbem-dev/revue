use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tracing::{info, warn};

pub mod antigravity;
pub mod claude;
pub mod gemini;
pub mod mesh_llm;
pub mod ollama;

#[async_trait]
pub trait Provider: Send + Sync {
    /// Human-readable name of the provider
    fn name(&self) -> &str;

    /// Execute a review prompt and return the raw LLM text response
    async fn complete(&self, prompt: &str) -> Result<String>;
}

pub struct ProviderChain {
    providers: Vec<Box<dyn Provider>>,
}

impl ProviderChain {
    pub fn new(providers: Vec<Box<dyn Provider>>) -> Self {
        Self { providers }
    }

    /// Try providers in sequence. If one fails, automatically fall back to the next provider in the chain.
    pub async fn complete_with_fallback(&self, prompt: &str) -> Result<(String, String)> {
        if self.providers.is_empty() {
            return Err(anyhow!("No LLM providers configured in fallback chain"));
        }

        let mut errors = Vec::new();

        for provider in &self.providers {
            info!("Attempting review with provider: {}", provider.name());
            match provider.complete(prompt).await {
                Ok(response) => {
                    info!("Successfully generated review using {}", provider.name());
                    return Ok((response, provider.name().to_string()));
                }
                Err(err) => {
                    warn!(
                        "Provider '{}' failed or hit limits: {}. Failing over...",
                        provider.name(),
                        err
                    );
                    errors.push(format!("{}: {}", provider.name(), err));
                }
            }
        }

        Err(anyhow!(
            "All providers in the fallback chain failed:\n{}",
            errors.join("\n")
        ))
    }
}
