use anyhow::{Result, anyhow};
use async_trait::async_trait;
use tracing::{info, warn};

pub mod antigravity;
pub mod claude;
pub mod cli;
pub mod gemini;
pub mod openai_compat;

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
        self.complete_and_parse_with_fallback(prompt, |resp| Ok(resp.to_string()))
            .await
    }

    /// Try providers in sequence, parsing each response and failing over when completion or parsing fails.
    pub async fn complete_and_parse_with_fallback<T, F>(
        &self,
        prompt: &str,
        mut parse: F,
    ) -> Result<(T, String)>
    where
        F: FnMut(&str) -> Result<T>,
    {
        if self.providers.is_empty() {
            return Err(anyhow!("No LLM providers configured in fallback chain"));
        }

        let mut errors = Vec::new();

        for provider in &self.providers {
            info!("Attempting review with provider: {}", provider.name());
            match provider.complete(prompt).await {
                Ok(response) => match parse(&response) {
                    Ok(parsed) => {
                        info!("Successfully generated review using {}", provider.name());
                        return Ok((parsed, provider.name().to_string()));
                    }
                    Err(err) => {
                        warn!(
                            "Provider '{}' returned unparseable output: {}. Failing over...",
                            provider.name(),
                            err
                        );
                        errors.push(format!("{}: {}", provider.name(), err));
                    }
                },
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

#[cfg(test)]
mod tests {
    use super::*;

    struct StubProvider {
        name: &'static str,
        output: &'static str,
    }

    #[async_trait]
    impl Provider for StubProvider {
        fn name(&self) -> &str {
            self.name
        }

        async fn complete(&self, _prompt: &str) -> Result<String> {
            Ok(self.output.to_string())
        }
    }

    #[tokio::test]
    async fn cascades_when_first_provider_returns_unparseable_output() {
        let chain = ProviderChain::new(vec![
            Box::new(StubProvider {
                name: "broken",
                output: "```json\n{\"summary\":\"truncated",
            }),
            Box::new(StubProvider {
                name: "healthy",
                output: "{\"ok\":true}",
            }),
        ]);
        let (val, used) = chain
            .complete_and_parse_with_fallback("prompt", |s| {
                serde_json::from_str::<serde_json::Value>(s).map_err(Into::into)
            })
            .await
            .expect("second provider should succeed");
        assert_eq!(used, "healthy");
        assert_eq!(val["ok"], true);
    }
}
