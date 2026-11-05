use super::Provider;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Generic client for any OpenAI-compatible `/v1/chat/completions` endpoint.
/// Works with Mesh-LLM, Ollama (/v1), vLLM, LiteLLM, LocalAI, LM Studio, etc.
pub struct OpenAiCompatProvider {
    display_name: String,
    endpoint: String,
    model: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct OpenAiChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    temperature: f32,
}

#[derive(Deserialize)]
struct ChatChoiceMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: Option<ChatChoiceMessage>,
}

#[derive(Deserialize)]
struct OpenAiChatResponse {
    choices: Option<Vec<ChatChoice>>,
    error: Option<OpenAiError>,
}

#[derive(Deserialize)]
struct OpenAiError {
    message: Option<String>,
}

impl OpenAiCompatProvider {
    pub fn new(
        display_name: String,
        endpoint: String,
        model: String,
        api_key: Option<String>,
    ) -> Self {
        Self {
            display_name,
            endpoint,
            model,
            api_key,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn mesh_llm(endpoint: String, model: String) -> Self {
        Self::new("Mesh-LLM (Fleet P2P)".to_string(), endpoint, model, None)
    }

    pub fn ollama(endpoint: String, model: String) -> Self {
        // Ollama exposes OpenAI compatibility under /v1
        let ep = if endpoint.ends_with("/v1") {
            endpoint
        } else {
            format!("{}/v1", endpoint.trim_end_matches('/'))
        };
        Self::new("Local Ollama (Offline / GPU)".to_string(), ep, model, None)
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

#[async_trait]
impl Provider for OpenAiCompatProvider {
    fn name(&self) -> &str {
        &self.display_name
    }

    async fn complete(&self, prompt: &str) -> Result<String> {
        let base = self.endpoint.trim_end_matches('/');
        let url = if base.ends_with("/chat/completions") {
            base.to_string()
        } else if base.ends_with("/v1") {
            format!("{}/chat/completions", base)
        } else {
            format!("{}/v1/chat/completions", base)
        };

        let req_body = OpenAiChatRequest {
            model: &self.model,
            messages: vec![ChatMessage {
                role: "user",
                content: prompt,
            }],
            temperature: 0.1,
        };

        let mut req = self.client.post(&url).json(&req_body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let res = req
            .send()
            .await
            .map_err(|e| anyhow!("Failed to connect to provider at '{}': {}", url, e))?;

        let status = res.status();
        let body: OpenAiChatResponse = res
            .json()
            .await
            .with_context(|| format!("Failed to parse JSON response from '{}'", url))?;

        if !status.is_success() {
            let msg = body
                .error
                .and_then(|e| e.message)
                .unwrap_or_else(|| format!("HTTP status {}", status));
            return Err(anyhow!("Provider error at '{}': {}", url, msg));
        }

        let content = body
            .choices
            .and_then(|mut c| c.pop())
            .and_then(|c| c.message)
            .and_then(|m| m.content)
            .ok_or_else(|| anyhow!("Provider at '{}' returned empty choices", url))?;

        Ok(content)
    }
}
