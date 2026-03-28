use super::types::*;
use anyhow::{anyhow, Result};
use serde_json::json;

/// Supported LLM providers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProviderKind {
    Anthropic,
    Google,
    OpenAI,
    OpenAICompat,
}

impl ProviderKind {
    pub fn from_str(s: &str) -> Self {
        match s {
            "anthropic" => Self::Anthropic,
            "google" => Self::Google,
            "openai_compat" => Self::OpenAICompat,
            _ => Self::OpenAI,
        }
    }
}

pub struct AgentProvider {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    kind: ProviderKind,
}

impl AgentProvider {
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
            (Some(url), Some(key), Some(m)) if !url.is_empty() && !key.is_empty() && !m.is_empty() => {
                let kind = ProviderKind::from_str(provider.unwrap_or("openai"));
                Some(Self::new(kind, url, key, m))
            }
            _ => None,
        }
    }

    pub async fn chat(
        &self,
        messages: &[AgentMessage],
        tools: &[ToolDefinition],
    ) -> Result<AgentResponse> {
        match self.kind {
            ProviderKind::OpenAI | ProviderKind::OpenAICompat => {
                self.chat_openai(messages, tools).await
            }
            ProviderKind::Anthropic => self.chat_anthropic(messages, tools).await,
            ProviderKind::Google => self.chat_google(messages, tools).await,
        }
    }

    // ── OpenAI / OpenAI-Compatible ──

    async fn chat_openai(
        &self,
        messages: &[AgentMessage],
        tools: &[ToolDefinition],
    ) -> Result<AgentResponse> {
        let oai_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };
                if matches!(m.role, Role::Assistant) {
                    if let Some(ref raw) = m.raw_content {
                        return raw.clone();
                    }
                }
                // Multimodal user message with image
                if matches!(m.role, Role::User) {
                    if let (Some(b64), Some(mime)) = (&m.image_b64, &m.image_mime) {
                        let mut content = vec![
                            json!({"type": "image_url", "image_url": {"url": format!("data:{};base64,{}", mime, b64)}}),
                        ];
                        if !m.content.is_empty() {
                            content.push(json!({"type": "text", "text": &m.content}));
                        }
                        let mut msg = json!({"role": role, "content": content});
                        if let Some(ref id) = m.tool_call_id {
                            msg["tool_call_id"] = json!(id);
                        }
                        return msg;
                    }
                }
                let mut msg = json!({ "role": role, "content": &m.content });
                if let Some(ref id) = m.tool_call_id {
                    msg["tool_call_id"] = json!(id);
                }
                msg
            })
            .collect();

        let oai_tools: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": &t.name,
                        "description": &t.description,
                        "parameters": &t.parameters,
                    }
                })
            })
            .collect();

        let url = if self.kind == ProviderKind::OpenAICompat {
            format!("{}/v1/responses", self.base_url)
        } else {
            format!("{}/v1/chat/completions", self.base_url)
        };

        let mut body = json!({
            "model": &self.model,
            "messages": oai_messages,
        });

        if !oai_tools.is_empty() {
            body["tools"] = json!(oai_tools);
        }

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("OpenAI API error {}: {}", status, text));
        }

        let data: serde_json::Value = resp.json().await?;
        parse_openai_response(&data)
    }

    // ── Anthropic ──

    async fn chat_anthropic(
        &self,
        messages: &[AgentMessage],
        tools: &[ToolDefinition],
    ) -> Result<AgentResponse> {
        let mut system_prompt = String::new();
        let mut anth_messages: Vec<serde_json::Value> = Vec::new();

        for m in messages {
            match m.role {
                Role::System => {
                    system_prompt = m.content.clone();
                }
                Role::User => {
                    // Multimodal user message with image
                    if let (Some(b64), Some(mime)) = (&m.image_b64, &m.image_mime) {
                        let mut content = vec![
                            json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": mime,
                                    "data": b64,
                                }
                            }),
                        ];
                        if !m.content.is_empty() {
                            content.push(json!({"type": "text", "text": &m.content}));
                        }
                        anth_messages.push(json!({"role": "user", "content": content}));
                    } else {
                        anth_messages.push(json!({
                            "role": "user",
                            "content": &m.content,
                        }));
                    }
                }
                Role::Assistant => {
                    if let Some(ref raw) = m.raw_content {
                        // Replay assistant message with tool_use content blocks
                        anth_messages.push(json!({
                            "role": "assistant",
                            "content": raw,
                        }));
                    } else {
                        anth_messages.push(json!({
                            "role": "assistant",
                            "content": &m.content,
                        }));
                    }
                }
                Role::Tool => {
                    anth_messages.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": m.tool_call_id.as_deref().unwrap_or(""),
                            "content": &m.content,
                        }],
                    }));
                }
            }
        }

        let anth_tools: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": &t.name,
                    "description": &t.description,
                    "input_schema": &t.parameters,
                })
            })
            .collect();

        let mut body = json!({
            "model": &self.model,
            "max_tokens": 4096,
            "messages": anth_messages,
        });

        if !system_prompt.is_empty() {
            body["system"] = json!(system_prompt);
        }
        if !anth_tools.is_empty() {
            body["tools"] = json!(anth_tools);
        }

        let url = format!("{}/v1/messages", self.base_url);
        tracing::info!("Anthropic request URL: {}", url);

        let body_str = serde_json::to_string_pretty(&body).unwrap_or_default();
        tracing::debug!("Anthropic request body: {}", body_str);

        let resp = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            tracing::error!("Anthropic API failed: status={}, body={}", status, text);
            return Err(anyhow!("Anthropic API error {}: {}", status, text));
        }

        let resp_text = resp.text().await?;
        tracing::debug!("Anthropic response: {}", &resp_text[..resp_text.len().min(2000)]);
        let data: serde_json::Value = serde_json::from_str(&resp_text)
            .map_err(|e| anyhow!("Failed to parse Anthropic response JSON: {}. Body: {}", e, &resp_text[..resp_text.len().min(500)]))?;
        parse_anthropic_response(&data)
    }

    // ── Google ──

    async fn chat_google(
        &self,
        messages: &[AgentMessage],
        tools: &[ToolDefinition],
    ) -> Result<AgentResponse> {
        let mut system_instruction: Option<serde_json::Value> = None;
        let mut contents: Vec<serde_json::Value> = Vec::new();

        for m in messages {
            match m.role {
                Role::System => {
                    system_instruction = Some(json!({
                        "parts": [{ "text": &m.content }]
                    }));
                }
                Role::User => {
                    // Multimodal user message with image
                    if let (Some(b64), Some(mime)) = (&m.image_b64, &m.image_mime) {
                        let mut parts = vec![
                            json!({"inlineData": {"mimeType": mime, "data": b64}}),
                        ];
                        if !m.content.is_empty() {
                            parts.push(json!({"text": &m.content}));
                        }
                        contents.push(json!({"role": "user", "parts": parts}));
                    } else {
                        contents.push(json!({
                            "role": "user",
                            "parts": [{ "text": &m.content }],
                        }));
                    }
                }
                Role::Assistant => {
                    if let Some(ref raw) = m.raw_content {
                        contents.push(raw.clone());
                    } else {
                        contents.push(json!({
                            "role": "model",
                            "parts": [{ "text": &m.content }],
                        }));
                    }
                }
                Role::Tool => {
                    contents.push(json!({
                        "role": "user",
                        "parts": [{
                            "functionResponse": {
                                "name": m.tool_call_id.as_deref().unwrap_or(""),
                                "response": { "result": &m.content },
                            }
                        }],
                    }));
                }
            }
        }

        let google_tools: Vec<serde_json::Value> = if tools.is_empty() {
            vec![]
        } else {
            let declarations: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "name": &t.name,
                        "description": &t.description,
                        "parameters": &t.parameters,
                    })
                })
                .collect();
            vec![json!({ "functionDeclarations": declarations })]
        };

        let mut body = json!({
            "contents": contents,
        });

        if let Some(si) = system_instruction {
            body["systemInstruction"] = si;
        }
        if !google_tools.is_empty() {
            body["tools"] = json!(google_tools);
        }

        let model_path = if self.model.starts_with("models/") {
            self.model.clone()
        } else {
            format!("models/{}", self.model)
        };
        let url = format!(
            "{}/v1beta/{}:generateContent",
            self.base_url, model_path
        );

        let resp = self.http.post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Google API error {}: {}", status, text));
        }

        let data: serde_json::Value = resp.json().await?;
        parse_google_response(&data)
    }
}

// ── Response parsers ──

fn parse_openai_response(data: &serde_json::Value) -> Result<AgentResponse> {
    let choice = data["choices"]
        .as_array()
        .and_then(|c| c.first())
        .ok_or_else(|| anyhow!("No choices in OpenAI response"))?;

    let message = &choice["message"];
    let content = message["content"].as_str().map(String::from);

    let tool_calls: Vec<AgentToolCall> = message["tool_calls"]
        .as_array()
        .map(|calls| {
            calls
                .iter()
                .filter_map(|tc| {
                    let id = tc["id"].as_str()?.to_string();
                    let name = tc["function"]["name"].as_str()?.to_string();
                    let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                    let arguments = serde_json::from_str(args_str).unwrap_or(json!({}));
                    Some(AgentToolCall {
                        id,
                        name,
                        arguments,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let tool_calls_present = !tool_calls.is_empty();

    Ok(AgentResponse {
        content,
        tool_calls,
        raw_content: if tool_calls_present { Some(message.clone()) } else { None },
    })
}

fn parse_anthropic_response(data: &serde_json::Value) -> Result<AgentResponse> {
    let content_blocks = data["content"]
        .as_array()
        .ok_or_else(|| anyhow!("No content in Anthropic response"))?;

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in content_blocks {
        match block["type"].as_str() {
            Some("text") => {
                if let Some(t) = block["text"].as_str() {
                    text_parts.push(t.to_string());
                }
            }
            Some("tool_use") => {
                if let (Some(id), Some(name)) = (block["id"].as_str(), block["name"].as_str()) {
                    tool_calls.push(AgentToolCall {
                        id: id.to_string(),
                        name: name.to_string(),
                        arguments: block["input"].clone(),
                    });
                }
            }
            _ => {}
        }
    }

    let content = if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join("\n"))
    };

    Ok(AgentResponse {
        content,
        raw_content: if tool_calls.is_empty() { None } else { Some(json!(content_blocks)) },
        tool_calls,
    })
}

fn parse_google_response(data: &serde_json::Value) -> Result<AgentResponse> {
    let candidate = data["candidates"]
        .as_array()
        .and_then(|c| c.first())
        .ok_or_else(|| anyhow!("No candidates in Google response"))?;

    let parts = candidate["content"]["parts"]
        .as_array()
        .ok_or_else(|| anyhow!("No parts in Google response"))?;

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for part in parts {
        if let Some(text) = part["text"].as_str() {
            text_parts.push(text.to_string());
        }
        if let Some(fc) = part.get("functionCall") {
            if let Some(name) = fc["name"].as_str() {
                tool_calls.push(AgentToolCall {
                    id: name.to_string(), // Google doesn't have separate IDs
                    name: name.to_string(),
                    arguments: fc["args"].clone(),
                });
            }
        }
    }

    let content = if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join("\n"))
    };

    Ok(AgentResponse {
        content,
        raw_content: if tool_calls.is_empty() { None } else { Some(candidate["content"].clone()) },
        tool_calls,
    })
}
