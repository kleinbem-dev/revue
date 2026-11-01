use super::Provider;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

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
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| anyhow!("GEMINI_API_KEY environment variable not set"))?;
        Ok(Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    fn name(&self) -> &str {
        "Google Gemini (AI Studio)"
    }

    async fn complete(&self, prompt: &str) -> Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let req = GeminiRequest {
            contents: vec![Content {
                parts: vec![Part { text: prompt }],
            }],
        };

        let res = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| anyhow!("Gemini API request failed: {}", e))?;

        let status = res.status();
        let body: GeminiResponse = res
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse Gemini API JSON response: {}", e))?;

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
