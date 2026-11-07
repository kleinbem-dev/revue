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
            client: reqwest::Client::new(),
        })
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
