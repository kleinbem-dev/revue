use super::Provider;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{info, warn};

/// Generic client for any OpenAI-compatible `/v1/chat/completions` endpoint.
/// Works with Mesh-LLM, Ollama (/v1), vLLM, LiteLLM, LocalAI, LM Studio, etc.
pub struct OpenAiCompatProvider {
    display_name: String,
    endpoint: String,
    model: String,
    api_key: Option<String>,
    unload_url: Option<String>,
    invoked: AtomicBool,
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
            unload_url: None,
            invoked: AtomicBool::new(false),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Configures an optional post-review VRAM handoff endpoint (e.g. `POST /v1/unload`
    /// on Mesh-LLM / LayerStream) invoked after a review using this provider finishes.
    pub fn with_unload_url(mut self, unload_url: Option<String>) -> Self {
        self.unload_url = unload_url.filter(|u| !u.trim().is_empty());
        self
    }

    /// Derives the default `/v1/unload` URL from an OpenAI-compatible base endpoint.
    pub fn derive_unload_url(endpoint: &str) -> String {
        let base = endpoint.trim_end_matches('/');
        if let Some(prefix) = base.strip_suffix("/chat/completions") {
            format!("{prefix}/unload")
        } else if base.ends_with("/v1") {
            format!("{base}/unload")
        } else {
            format!("{base}/v1/unload")
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
        self.invoked.store(true, Ordering::Relaxed);
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

    async fn release_vram(&self) {
        let Some(url) = &self.unload_url else {
            return;
        };
        if !self.invoked.swap(false, Ordering::Relaxed) {
            return;
        }
        let mut req = self
            .client
            .post(url)
            .timeout(std::time::Duration::from_secs(5));
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                info!(
                    provider = %self.display_name,
                    unload_url = %url,
                    status = %resp.status(),
                    "VRAM handoff unload succeeded"
                );
            }
            Ok(resp) => {
                warn!(
                    provider = %self.display_name,
                    unload_url = %url,
                    status = %resp.status(),
                    "VRAM handoff unload returned non-success status"
                );
            }
            Err(err) => {
                warn!(
                    provider = %self.display_name,
                    unload_url = %url,
                    error = %err,
                    "VRAM handoff unload request failed"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn derive_unload_url_handles_common_endpoint_forms() {
        assert_eq!(
            OpenAiCompatProvider::derive_unload_url("http://127.0.0.1:9337/v1"),
            "http://127.0.0.1:9337/v1/unload"
        );
        assert_eq!(
            OpenAiCompatProvider::derive_unload_url("http://127.0.0.1:9337/v1/chat/completions"),
            "http://127.0.0.1:9337/v1/unload"
        );
        assert_eq!(
            OpenAiCompatProvider::derive_unload_url("http://127.0.0.1:9337/"),
            "http://127.0.0.1:9337/v1/unload"
        );
    }

    #[tokio::test]
    async fn release_vram_posts_unload_only_after_complete_was_invoked() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let unload_hits = Arc::new(AtomicUsize::new(0));
        let hits_bg = unload_hits.clone();

        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let first = req.lines().next().unwrap_or("");
                let (status, body) = if first.starts_with("POST /v1/chat/completions") {
                    (
                        "200 OK",
                        r#"{"choices":[{"message":{"content":"{\"summary\":\"ok\",\"issues\":[]}"}}]}"#,
                    )
                } else if first.starts_with("POST /v1/unload") {
                    hits_bg.fetch_add(1, Ordering::SeqCst);
                    ("200 OK", r#"{"status":"unloaded"}"#)
                } else {
                    ("404 Not Found", "{}")
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let endpoint = format!("http://{addr}/v1");
        let unload_url = OpenAiCompatProvider::derive_unload_url(&endpoint);
        let provider = OpenAiCompatProvider::mesh_llm(endpoint, "test-model".to_string())
            .with_unload_url(Some(unload_url));

        // Before `complete` is called, `release_vram` must be a no-op.
        provider.release_vram().await;
        assert_eq!(unload_hits.load(Ordering::SeqCst), 0);

        let reply = provider.complete("review this").await.unwrap();
        assert!(reply.contains("ok"));

        // First release_vram fires POST /v1/unload; second call is idempotent no-op.
        provider.release_vram().await;
        provider.release_vram().await;
        assert_eq!(unload_hits.load(Ordering::SeqCst), 1);

        server.await.unwrap();
    }
}
