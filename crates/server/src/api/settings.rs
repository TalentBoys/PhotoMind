use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use crate::AppState;
use photomind_storage::repo::configs::ConfigRepo;

#[derive(Serialize)]
pub struct SettingsResponse {
    #[serde(flatten)]
    pub settings: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    #[serde(flatten)]
    pub settings: serde_json::Map<String, serde_json::Value>,
}

pub async fn get_settings(
    State(state): State<AppState>,
) -> Result<Json<SettingsResponse>, StatusCode> {
    let settings = ConfigRepo::get_all(state.db.pool())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(SettingsResponse { settings }))
}

pub async fn update_settings(
    State(state): State<AppState>,
    Json(req): Json<UpdateSettingsRequest>,
) -> Result<StatusCode, StatusCode> {
    for (key, value) in req.settings {
        ConfigRepo::set(state.db.pool(), &key, &value)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    Ok(StatusCode::OK)
}

#[derive(Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize)]
pub struct FetchModelsRequest {
    pub url: String,
    pub key: String,
    #[serde(default)]
    pub provider: Option<String>,
}

pub async fn fetch_embedding_models(
    Json(req): Json<FetchModelsRequest>,
) -> Result<Json<Vec<ModelInfo>>, StatusCode> {
    let models = fetch_google_models(&req.url, &req.key)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(models))
}

pub async fn fetch_agent_models(
    Json(req): Json<FetchModelsRequest>,
) -> Result<Json<Vec<ModelInfo>>, StatusCode> {
    let provider = req.provider.as_deref().unwrap_or("openai");
    let models = match provider {
        "google" => fetch_google_models(&req.url, &req.key).await,
        "anthropic" => fetch_anthropic_models(&req.url, &req.key).await,
        _ => fetch_openai_models(&req.url, &req.key).await,
    }
    .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(models))
}

async fn fetch_google_models(base_url: &str, key: &str) -> anyhow::Result<Vec<ModelInfo>> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .get(&url)
        .bearer_auth(key)
        .send()
        .await?
        .json()
        .await?;
    let models = resp["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let name = m["name"].as_str()?.to_string();
                    let display = m["displayName"].as_str().unwrap_or(&name).to_string();
                    Some(ModelInfo { id: name, name: display })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(models)
}

async fn fetch_openai_models(base_url: &str, key: &str) -> anyhow::Result<Vec<ModelInfo>> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .get(&url)
        .bearer_auth(key)
        .send()
        .await?
        .json()
        .await?;
    let models = resp["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    Some(ModelInfo { id: id.clone(), name: id })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(models)
}

#[derive(Deserialize)]
pub struct BrowseDirsRequest {
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
}

#[derive(Serialize)]
pub struct BrowseDirsResponse {
    pub current: String,
    pub parent: Option<String>,
    pub dirs: Vec<DirEntry>,
}

pub async fn browse_dirs(
    Json(req): Json<BrowseDirsRequest>,
) -> Result<Json<BrowseDirsResponse>, StatusCode> {
    let raw_path = req.path.unwrap_or_else(|| "/".to_string());
    let raw_path = if raw_path.is_empty() { "/".to_string() } else { raw_path };

    let canonical = std::fs::canonicalize(&raw_path)
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    if !canonical.is_dir() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let parent = canonical.parent().map(|p| p.to_string_lossy().to_string());

    let mut dirs = Vec::new();
    let read_dir = std::fs::read_dir(&canonical)
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    for entry in read_dir.flatten() {
        if let Ok(ft) = entry.file_type() {
            if ft.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                // Skip hidden directories
                if name.starts_with('.') {
                    continue;
                }
                let path = entry.path().to_string_lossy().to_string();
                dirs.push(DirEntry { name, path });
            }
        }
    }

    dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Ok(Json(BrowseDirsResponse {
        current: canonical.to_string_lossy().to_string(),
        parent,
        dirs,
    }))
}

#[derive(Deserialize)]
pub struct TestEmbeddingRequest {
    pub url: String,
    pub key: String,
    pub model: String,
    #[serde(default)]
    pub dimension: Option<u32>,
}

#[derive(Deserialize)]
pub struct TestAgentRequest {
    pub provider: String,
    pub url: String,
    pub key: String,
    pub model: String,
}

#[derive(Serialize)]
pub struct TestResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub async fn test_embedding(
    Json(req): Json<TestEmbeddingRequest>,
) -> Json<TestResult> {
    let dim = req.dimension.unwrap_or(photomind_core::embedding::DEFAULT_EMBEDDING_DIMENSION);
    let client = photomind_core::embedding::EmbeddingClient::new(&req.url, &req.key, &req.model, dim);
    match client.embed_text("test").await {
        Ok((vec, _)) => Json(TestResult {
            ok: true,
            message: Some(format!("Success! Vector dimension: {}", vec.len())),
            error: None,
        }),
        Err(e) => Json(TestResult {
            ok: false,
            message: None,
            error: Some(e.to_string()),
        }),
    }
}

pub async fn test_agent(
    Json(req): Json<TestAgentRequest>,
) -> Json<TestResult> {
    use photomind_core::agent::provider::{AgentProvider, ProviderKind};
    use photomind_core::agent::types::{AgentMessage, Role};

    tracing::info!(
        "test_agent: provider={}, url={}, model={}, key_len={}",
        req.provider, req.url, req.model, req.key.len()
    );

    let kind = ProviderKind::from_str(&req.provider);
    let provider = AgentProvider::new(kind, &req.url, &req.key, &req.model);
    let messages = vec![AgentMessage {
        role: Role::User,
        content: "Say hello in one sentence.".to_string(),
        tool_call_id: None,
        raw_content: None,
        image_b64: None,
        image_mime: None,
    }];
    match provider.chat(&messages, &[]).await {
        Ok(resp) => {
            let reply = resp.content.unwrap_or_default();
            let short = if reply.len() > 200 {
                format!("{}...", &reply[..200])
            } else {
                reply
            };
            Json(TestResult {
                ok: true,
                message: Some(short),
                error: None,
            })
        }
        Err(e) => Json(TestResult {
            ok: false,
            message: None,
            error: Some(e.to_string()),
        }),
    }
}

pub async fn test_i2t(
    Json(req): Json<TestAgentRequest>,
) -> Json<TestResult> {
    use photomind_core::agent::provider::ProviderKind;

    let kind = ProviderKind::from_str(&req.provider);
    let client = photomind_core::vision::VisionClient::new(kind, &req.url, &req.key, &req.model);
    match client.test().await {
        Ok(reply) => {
            let short = if reply.len() > 200 {
                format!("{}...", &reply[..200])
            } else {
                reply
            };
            Json(TestResult {
                ok: true,
                message: Some(short),
                error: None,
            })
        }
        Err(e) => Json(TestResult {
            ok: false,
            message: None,
            error: Some(e.to_string()),
        }),
    }
}

async fn fetch_anthropic_models(base_url: &str, key: &str) -> anyhow::Result<Vec<ModelInfo>> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .get(&url)
        .bearer_auth(key)
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await?
        .json()
        .await?;
    let models = resp["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    let display = m["display_name"].as_str().unwrap_or(&id).to_string();
                    Some(ModelInfo { id, name: display })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(models)
}
