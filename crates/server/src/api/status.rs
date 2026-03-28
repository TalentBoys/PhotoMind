use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;

use crate::AppState;
use photomind_core::embedding::{EmbeddingClient, EmbeddingPipeline};
use photomind_core::scanner::PhotoScanner;
use photomind_storage::repo::configs::ConfigRepo;
use photomind_storage::repo::photos::PhotoRepo;

#[derive(Serialize)]
pub struct StatusResponse {
    pub total_photos: i64,
    pub embedded_photos: i64,
    pub index_size: usize,
    pub scan_dirs: Vec<String>,
}

pub async fn get_status(
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, StatusCode> {
    let total = PhotoRepo::count(state.db.pool())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let embedded = PhotoRepo::count_embedded(state.db.pool())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let scan_dirs = ConfigRepo::get(state.db.pool(), "scan_dirs")
        .await
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
        .unwrap_or_default();

    Ok(Json(StatusResponse {
        total_photos: total,
        embedded_photos: embedded,
        index_size: state.vector_index.len(),
        scan_dirs,
    }))
}

/// Trigger a scan + embedding run. Runs in background, returns immediately.
pub async fn trigger_scan(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    tokio::spawn(async move {
        if let Err(e) = run_scan_and_embed(&state).await {
            tracing::error!("Scan+embed failed: {}", e);
        }
    });
    Ok(Json(serde_json::json!({ "status": "started" })))
}

pub async fn run_scan_and_embed(state: &AppState) -> anyhow::Result<()> {
    let pool = state.db.pool();

    // 1. Scan directories
    let scan_dirs = ConfigRepo::get(pool, "scan_dirs")
        .await?
        .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
        .unwrap_or_default();

    if !scan_dirs.is_empty() {
        let scanner = PhotoScanner::new(pool.clone());
        let new_photos = scanner.scan_all(&scan_dirs).await?;
        tracing::info!("Scan complete: {} new photos", new_photos);
    }

    // 2. Run embedding pipeline
    let url = ConfigRepo::get(pool, "embedding_url")
        .await?
        .and_then(|v| v.as_str().map(String::from));
    let key = ConfigRepo::get(pool, "embedding_key")
        .await?
        .and_then(|v| v.as_str().map(String::from));
    let model = ConfigRepo::get(pool, "embedding_model")
        .await?
        .and_then(|v| v.as_str().map(String::from));

    if let Some(client) = EmbeddingClient::from_config(url.as_deref(), key.as_deref(), model.as_deref()) {
        let pipeline = EmbeddingPipeline::new(client, pool.clone());
        let count = pipeline.run_to_completion(20).await?;
        tracing::info!("Embedding complete: {} new embeddings", count);

        // Reload vector index
        state.vector_index.load_from_db(pool).await?;
    } else {
        tracing::info!("Embedding model not configured, skipping");
    }

    Ok(())
}
