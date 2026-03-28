use anyhow::{anyhow, Result};
use base64::Engine;
use serde_json::json;

use crate::agent::provider::ProviderKind;

const DESCRIBE_PROMPT: &str = "Describe this photo in detail for search indexing. Include: main subjects, actions, setting/location, colors, mood, objects, text visible, time of day, weather, and any notable details. Be thorough but concise. Output only the description, no preamble.";

const BATCH_DESCRIBE_PROMPT: &str = "Describe each photo in detail for search indexing. For each photo, include: main subjects, actions, setting/location, colors, mood, objects, text visible, time of day, weather, and any notable details. Be thorough but concise.\n\nOutput exactly one description per photo, separated by the delimiter line:\n---PHOTO_SEP---\nOutput descriptions in the same order as the photos. No numbering, no preamble, no extra text.";

/// Maximum images per batch vision call. Conservative default to stay within context limits.
const MAX_VISION_BATCH: usize = 4;

/// Client for calling vision LLMs to generate text descriptions of images.
pub struct VisionClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    kind: ProviderKind,
}

impl VisionClient {
    pub fn new(kind: ProviderKind, base_url: &str, api_key: &str, model: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            kind,
        }
    }

    pub fn from_config(
        provider: Option<&str>,
        base_url: Option<&str>,
        api_key: Option<&str>,
        model: Option<&str>,
    ) -> Option<Self> {
        match (base_url, api_key, model) {
            (Some(url), Some(key), Some(m))
                if !url.is_empty() && !key.is_empty() && !m.is_empty() =>
            {
                let kind = ProviderKind::from_str(provider.unwrap_or("openai"));
                Some(Self::new(kind, url, key, m))
            }
            _ => None,
        }
    }

    /// Describe an image using a vision LLM. Returns (text description, tokens used).
    pub async fn describe_image(&self, image_data: &[u8], mime_type: &str) -> Result<(String, u64)> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(image_data);

        match self.kind {
            ProviderKind::OpenAI | ProviderKind::OpenAICompat => {
                self.describe_openai(&b64, mime_type).await
            }
            ProviderKind::Anthropic => self.describe_anthropic(&b64, mime_type).await,
            ProviderKind::Google => self.describe_google(&b64, mime_type).await,
        }
    }

    /// Maximum images per batch call.
    pub fn max_batch_size(&self) -> usize {
        MAX_VISION_BATCH
    }

    /// Describe multiple images in a single API call.
    /// Returns Vec<(description, _)> in same order as input, plus total tokens used.
    /// If batch size is 1, falls back to single-image call.
    pub async fn describe_images_batch(
        &self,
        images: &[(&[u8], &str)], // (image_data, mime_type)
    ) -> Result<(Vec<String>, u64)> {
        if images.is_empty() {
            return Ok((vec![], 0));
        }
        if images.len() == 1 {
            let (desc, tokens) = self.describe_image(images[0].0, images[0].1).await?;
            return Ok((vec![desc], tokens));
        }

        let b64_images: Vec<(String, &str)> = images
            .iter()
            .map(|(data, mime)| {
                (base64::engine::general_purpose::STANDARD.encode(data), *mime)
            })
            .collect();

        let (raw_text, tokens) = match self.kind {
            ProviderKind::OpenAI | ProviderKind::OpenAICompat => {
                self.batch_openai(&b64_images).await?
            }
            ProviderKind::Anthropic => {
                self.batch_anthropic(&b64_images).await?
            }
            ProviderKind::Google => {
                self.batch_google(&b64_images).await?
            }
        };

        // Split by delimiter
        let descriptions: Vec<String> = raw_text
            .split("---PHOTO_SEP---")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        // If parsing failed (wrong number of descriptions), fall back to single calls
        if descriptions.len() != images.len() {
            tracing::warn!(
                "Batch vision returned {} descriptions for {} images, falling back to single calls",
                descriptions.len(),
                images.len()
            );
            let mut results = Vec::new();
            let mut total_tokens = 0u64;
            for (data, mime) in images {
                let (desc, t) = self.describe_image(data, mime).await?;
                results.push(desc);
                total_tokens += t;
            }
            return Ok((results, total_tokens));
        }

        Ok((descriptions, tokens))
    }

    /// Simple text test (no image) to verify the model is reachable.
    pub async fn test(&self) -> Result<String> {
        // Use a text-only request to verify connectivity
        match self.kind {
            ProviderKind::OpenAI | ProviderKind::OpenAICompat => self.test_openai().await,
            ProviderKind::Anthropic => self.test_anthropic().await,
            ProviderKind::Google => self.test_google().await,
        }
    }

    // ── OpenAI / OpenAI-Compatible ──

    async fn describe_openai(&self, b64_image: &str, mime_type: &str) -> Result<(String, u64)> {
        let url = if self.kind == ProviderKind::OpenAICompat {
            format!("{}/v1/responses", self.base_url)
        } else {
            format!("{}/v1/chat/completions", self.base_url)
        };

        let body = json!({
            "model": &self.model,
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": DESCRIBE_PROMPT },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:{};base64,{}", mime_type, b64_image),
                        }
                    }
                ]
            }]
        });

        let resp = self.http.post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Vision OpenAI API error {}: {}", status, text));
        }

        let data: serde_json::Value = resp.json().await?;
        let tokens = data.get("usage")
            .map(|u| {
                u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                    + u.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
            })
            .unwrap_or(0);
        let text = data["choices"][0]["message"]["content"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| anyhow!("No content in OpenAI vision response"))?;
        Ok((text, tokens))
    }

    async fn test_openai(&self) -> Result<String> {
        let url = if self.kind == ProviderKind::OpenAICompat {
            format!("{}/v1/responses", self.base_url)
        } else {
            format!("{}/v1/chat/completions", self.base_url)
        };

        let body = json!({
            "model": &self.model,
            "max_tokens": 64,
            "messages": [{ "role": "user", "content": "Say hello in one sentence." }]
        });

        let resp = self.http.post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Vision test error {}: {}", status, text));
        }

        let data: serde_json::Value = resp.json().await?;
        data["choices"][0]["message"]["content"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| anyhow!("No content in response"))
    }

    // ── Anthropic ──

    async fn describe_anthropic(&self, b64_image: &str, mime_type: &str) -> Result<(String, u64)> {
        let body = json!({
            "model": &self.model,
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": mime_type,
                            "data": b64_image,
                        }
                    },
                    { "type": "text", "text": DESCRIBE_PROMPT }
                ]
            }]
        });

        let resp = self.http
            .post(format!("{}/v1/messages", self.base_url))
            .bearer_auth(&self.api_key)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Vision Anthropic API error {}: {}", status, text));
        }

        let data: serde_json::Value = resp.json().await?;
        let tokens = data.get("usage")
            .map(|u| {
                u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                    + u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
            })
            .unwrap_or(0);
        let text = data["content"]
            .as_array()
            .and_then(|blocks| {
                blocks.iter().find_map(|b| {
                    if b["type"].as_str() == Some("text") {
                        b["text"].as_str().map(String::from)
                    } else {
                        None
                    }
                })
            })
            .ok_or_else(|| anyhow!("No text in Anthropic vision response"))?;
        Ok((text, tokens))
    }

    async fn test_anthropic(&self) -> Result<String> {
        let body = json!({
            "model": &self.model,
            "max_tokens": 64,
            "messages": [{ "role": "user", "content": "Say hello in one sentence." }]
        });

        let resp = self.http
            .post(format!("{}/v1/messages", self.base_url))
            .bearer_auth(&self.api_key)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Vision test error {}: {}", status, text));
        }

        let data: serde_json::Value = resp.json().await?;
        data["content"]
            .as_array()
            .and_then(|blocks| {
                blocks.iter().find_map(|b| {
                    if b["type"].as_str() == Some("text") {
                        b["text"].as_str().map(String::from)
                    } else {
                        None
                    }
                })
            })
            .ok_or_else(|| anyhow!("No text in response"))
    }

    // ── Google ──

    async fn describe_google(&self, b64_image: &str, mime_type: &str) -> Result<(String, u64)> {
        let body = json!({
            "contents": [{
                "role": "user",
                "parts": [
                    {
                        "inlineData": {
                            "mimeType": mime_type,
                            "data": b64_image,
                        }
                    },
                    { "text": DESCRIBE_PROMPT }
                ]
            }]
        });

        let model_path = if self.model.starts_with("models/") {
            self.model.clone()
        } else {
            format!("models/{}", self.model)
        };
        let url = format!("{}/v1beta/{}:generateContent", self.base_url, model_path);

        let resp = self.http.post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Vision Google API error {}: {}", status, text));
        }

        let data: serde_json::Value = resp.json().await?;
        let tokens = data.get("usageMetadata")
            .map(|u| {
                u.get("promptTokenCount").and_then(|v| v.as_u64()).unwrap_or(0)
                    + u.get("candidatesTokenCount").and_then(|v| v.as_u64()).unwrap_or(0)
            })
            .unwrap_or(0);
        let text = data["candidates"][0]["content"]["parts"]
            .as_array()
            .and_then(|parts| {
                parts.iter().find_map(|p| p["text"].as_str().map(String::from))
            })
            .ok_or_else(|| anyhow!("No text in Google vision response"))?;
        Ok((text, tokens))
    }

    async fn test_google(&self) -> Result<String> {
        let body = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": "Say hello in one sentence." }]
            }]
        });

        let model_path = if self.model.starts_with("models/") {
            self.model.clone()
        } else {
            format!("models/{}", self.model)
        };
        let url = format!("{}/v1beta/{}:generateContent", self.base_url, model_path);

        let resp = self.http.post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Vision test error {}: {}", status, text));
        }

        let data: serde_json::Value = resp.json().await?;
        data["candidates"][0]["content"]["parts"]
            .as_array()
            .and_then(|parts| {
                parts.iter().find_map(|p| p["text"].as_str().map(String::from))
            })
            .ok_or_else(|| anyhow!("No text in response"))
    }

    // ── Batch methods ──

    async fn batch_openai(&self, images: &[(String, &str)]) -> Result<(String, u64)> {
        let url = if self.kind == ProviderKind::OpenAICompat {
            format!("{}/v1/responses", self.base_url)
        } else {
            format!("{}/v1/chat/completions", self.base_url)
        };

        let mut content_parts = vec![json!({ "type": "text", "text": BATCH_DESCRIBE_PROMPT })];
        for (b64, mime) in images {
            content_parts.push(json!({
                "type": "image_url",
                "image_url": {
                    "url": format!("data:{};base64,{}", mime, b64),
                }
            }));
        }

        let body = json!({
            "model": &self.model,
            "max_tokens": 1024 * images.len(),
            "messages": [{ "role": "user", "content": content_parts }]
        });

        let resp = self.http.post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Vision batch OpenAI error {}: {}", status, text));
        }

        let data: serde_json::Value = resp.json().await?;
        let tokens = data.get("usage")
            .map(|u| {
                u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                    + u.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
            })
            .unwrap_or(0);
        let text = data["choices"][0]["message"]["content"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| anyhow!("No content in batch OpenAI response"))?;
        Ok((text, tokens))
    }

    async fn batch_anthropic(&self, images: &[(String, &str)]) -> Result<(String, u64)> {
        let mut content_parts: Vec<serde_json::Value> = Vec::new();
        for (b64, mime) in images {
            content_parts.push(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": mime,
                    "data": b64,
                }
            }));
        }
        content_parts.push(json!({ "type": "text", "text": BATCH_DESCRIBE_PROMPT }));

        let body = json!({
            "model": &self.model,
            "max_tokens": 1024 * images.len(),
            "messages": [{ "role": "user", "content": content_parts }]
        });

        let resp = self.http
            .post(format!("{}/v1/messages", self.base_url))
            .bearer_auth(&self.api_key)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Vision batch Anthropic error {}: {}", status, text));
        }

        let data: serde_json::Value = resp.json().await?;
        let tokens = data.get("usage")
            .map(|u| {
                u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                    + u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
            })
            .unwrap_or(0);
        let text = data["content"]
            .as_array()
            .and_then(|blocks| {
                blocks.iter().find_map(|b| {
                    if b["type"].as_str() == Some("text") {
                        b["text"].as_str().map(String::from)
                    } else {
                        None
                    }
                })
            })
            .ok_or_else(|| anyhow!("No text in batch Anthropic response"))?;
        Ok((text, tokens))
    }

    async fn batch_google(&self, images: &[(String, &str)]) -> Result<(String, u64)> {
        let mut parts: Vec<serde_json::Value> = Vec::new();
        for (b64, mime) in images {
            parts.push(json!({
                "inlineData": {
                    "mimeType": mime,
                    "data": b64,
                }
            }));
        }
        parts.push(json!({ "text": BATCH_DESCRIBE_PROMPT }));

        let body = json!({
            "contents": [{ "role": "user", "parts": parts }]
        });

        let model_path = if self.model.starts_with("models/") {
            self.model.clone()
        } else {
            format!("models/{}", self.model)
        };
        let url = format!("{}/v1beta/{}:generateContent", self.base_url, model_path);

        let resp = self.http.post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Vision batch Google error {}: {}", status, text));
        }

        let data: serde_json::Value = resp.json().await?;
        let tokens = data.get("usageMetadata")
            .map(|u| {
                u.get("promptTokenCount").and_then(|v| v.as_u64()).unwrap_or(0)
                    + u.get("candidatesTokenCount").and_then(|v| v.as_u64()).unwrap_or(0)
            })
            .unwrap_or(0);
        let text = data["candidates"][0]["content"]["parts"]
            .as_array()
            .and_then(|parts| {
                parts.iter().find_map(|p| p["text"].as_str().map(String::from))
            })
            .ok_or_else(|| anyhow!("No text in batch Google response"))?;
        Ok((text, tokens))
    }
}
