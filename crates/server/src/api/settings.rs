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
    let url = format!("{}/v1/models?key={}", base_url.trim_end_matches('/'), key);
    let resp: serde_json::Value = reqwest::get(&url).await?.json().await?;
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

async fn fetch_anthropic_models(base_url: &str, key: &str) -> anyhow::Result<Vec<ModelInfo>> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .get(&url)
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
