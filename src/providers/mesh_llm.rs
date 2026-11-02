use super::Provider;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub struct MeshLlmProvider {
    endpoint: String,
    model: String,
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

impl MeshLlmProvider {
    pub fn new(endpoint: String, model: String) -> Self {
        Self {
            endpoint,
            model,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

#[async_trait]
impl Provider for MeshLlmProvider {
    fn name(&self) -> &str {
        "Mesh-LLM (Fleet P2P GPU)"
    }

    async fn complete(&self, prompt: &str) -> Result<String> {
        let base = self.endpoint.trim_end_matches('/');
        let url = if base.ends_with("/v1") {
            format!("{}/chat/completions", base)
        } else {
            format!("{}/v1/chat/completions", base)
        };

        let req = OpenAiChatRequest {
            model: &self.model,
            messages: vec![ChatMessage {
                role: "user",
                content: prompt,
            }],
            temperature: 0.2,
        };

        let res = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to connect to Mesh-LLM at '{}': {}", url, e))?;

        let status = res.status();
        let body: OpenAiChatResponse = res
            .json()
            .await
            .with_context(|| format!("Failed to parse JSON response from Mesh-LLM ({})", status))?;

        if let Some(err) = body.error {
            return Err(anyhow!(
                "Mesh-LLM error: {}",
                err.message.unwrap_or_else(|| "Unknown error".to_string())
            ));
        }

        if let Some(choices) = body.choices {
            if let Some(first) = choices.into_iter().next() {
                if let Some(msg) = first.message {
                    if let Some(content) = msg.content {
                        return Ok(content);
                    }
                }
            }
        }

        Err(anyhow!("Empty completion choices received from Mesh-LLM"))
    }
}
