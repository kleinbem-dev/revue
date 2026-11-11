use super::Provider;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub struct GeminiProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

#[derive(Serialize)]
struct GeminiRequest<'a> {
    contents: Vec<Content<'a>>,
}

#[derive(Serialize)]
struct Content<'a> {
    parts: Vec<Part<'a>>,
}

#[derive(Serialize)]
struct Part<'a> {
    text: &'a str,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<Candidate>>,
    error: Option<GeminiError>,
}

#[derive(Deserialize)]
struct Candidate {
    content: Option<CandidateContent>,
}

#[derive(Deserialize)]
struct CandidateContent {
    parts: Option<Vec<CandidatePart>>,
}

#[derive(Deserialize)]
struct CandidatePart {
    text: Option<String>,
}

#[derive(Deserialize)]
struct GeminiError {
    message: String,
}

impl GeminiProvider {
    pub fn new(model: String) -> Result<Self> {
        let api_key = resolve_gemini_key()
            .ok_or_else(|| anyhow!("Neither GEMINI_API_KEY nor GOOGLE_API_KEY found in environment or ~/.config/gemini/api_key"))?;

        Ok(Self {
            api_key,
            model,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        })
    }
}

impl GeminiProvider {
    /// The key goes in a header: a `?key=` URL ends up in error messages, logs and the database.
    fn request(&self, prompt: &str) -> reqwest::RequestBuilder {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.model
        );
        let req = GeminiRequest {
            contents: vec![Content {
                parts: vec![Part { text: prompt }],
            }],
        };
        let mut builder = self.client.post(url).json(&req);
        if let Ok(mut key) = reqwest::header::HeaderValue::from_str(&self.api_key) {
            key.set_sensitive(true);
            builder = builder.header("x-goog-api-key", key);
        }
        builder
    }
}

fn resolve_gemini_key() -> Option<String> {
    // 1. Env vars
    if let Ok(key) = std::env::var("GEMINI_API_KEY")
        && !key.trim().is_empty()
    {
        return Some(key.trim().to_string());
    }
    if let Ok(key) = std::env::var("GOOGLE_API_KEY")
        && !key.trim().is_empty()
    {
        return Some(key.trim().to_string());
    }

    // 2. File fallback: ~/.config/gemini/api_key or ~/.config/google/api_key
    if let Ok(home) = std::env::var("HOME") {
        let paths = [
            PathBuf::from(&home).join(".config/gemini/api_key"),
            PathBuf::from(&home).join(".config/google/api_key"),
            PathBuf::from(&home).join(".gemini/api_key"),
        ];
        for path in &paths {
            if let Ok(content) = std::fs::read_to_string(path) {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    None
}

#[async_trait]
impl Provider for GeminiProvider {
    fn name(&self) -> &str {
        "Google Gemini (API Key)"
    }

    async fn complete(&self, prompt: &str) -> Result<String> {
        let res = self
            .request(prompt)
            .send()
            .await
            // reqwest errors print the URL; keep them free of it in case a key lands there again.
            .map_err(|e| anyhow!("Gemini API request failed: {}", e.without_url()))?;

        let status = res.status();
        let body: GeminiResponse = res.json().await.map_err(|e| {
            anyhow!(
                "Failed to parse Gemini API JSON response: {}",
                e.without_url()
            )
        })?;

        if !status.is_success() {
            let msg = body
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| format!("HTTP status {}", status));
            return Err(anyhow!("Gemini API error: {}", msg));
        }

        let text = body
            .candidates
            .and_then(|c| c.into_iter().next())
            .and_then(|c| c.content)
            .and_then(|c| c.parts)
            .and_then(|mut p| p.pop())
            .and_then(|p| p.text)
            .ok_or_else(|| anyhow!("Gemini returned empty candidate output"))?;

        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_goes_in_a_header_not_the_url() {
        let provider = GeminiProvider {
            api_key: "test-secret-key".to_string(),
            model: "gemini-2.0-flash".to_string(),
            client: reqwest::Client::new(),
        };
        let req = provider.request("hi").build().unwrap();
        assert!(!req.url().as_str().contains("test-secret-key"));
        assert!(req.url().query().is_none());
        let key = &req.headers()["x-goog-api-key"];
        assert_eq!(key, "test-secret-key");
        assert!(key.is_sensitive());
        assert!(!format!("{:?}", req).contains("test-secret-key"));
    }
}
