use super::Provider;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub struct OllamaProvider {
    endpoint: String,
    model: String,
    client: reqwest::Client,
}

#[derive(Serialize)]
struct OllamaGenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaGenerateResponse {
    response: Option<String>,
    error: Option<String>,
}

impl OllamaProvider {
    pub fn new(endpoint: String, model: String) -> Self {
        Self {
            endpoint,
            model,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    fn name(&self) -> &str {
        "Local Ollama (Offline / GPU)"
    }

    async fn complete(&self, prompt: &str) -> Result<String> {
        let url = format!("{}/api/generate", self.endpoint.trim_end_matches('/'));

        let req = OllamaGenerateRequest {
            model: &self.model,
            prompt,
            stream: false,
        };

        let res = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to connect to Ollama at '{}': {}", url, e))?;

        let status = res.status();
        let body: OllamaGenerateResponse = res
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse Ollama JSON: {}", e))?;

        if !status.is_success() {
            let msg = body
                .error
                .unwrap_or_else(|| format!("HTTP status {}", status));
            return Err(anyhow!("Ollama error: {}", msg));
        }

        body.response
            .ok_or_else(|| anyhow!("Ollama response was empty"))
    }
}
