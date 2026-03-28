use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::atomic::Ordering;

use crate::AppState;
use photomind_core::embedding::{EmbeddingClient, EmbeddingPipeline, TaskProgress};
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

/// Get current scan/embed progress.
pub async fn get_scan_progress(
    State(state): State<AppState>,
) -> Json<TaskProgress> {
    let progress = state.task_progress.lock().unwrap().clone();
    Json(progress)
}

/// Pause the running scan/embed task.
pub async fn pause_scan(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    state.task_paused.store(true, Ordering::Relaxed);
    // Immediately update phase so the UI reflects the intent
    if let Ok(mut progress) = state.task_progress.lock() {
        if progress.phase == "scanning" || progress.phase == "embedding" {
            progress.phase = "pausing".to_string();
        }
    }
    Json(serde_json::json!({ "status": "pausing" }))
}

/// Resume a paused scan/embed task.
pub async fn resume_scan(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    state.task_paused.store(false, Ordering::Relaxed);
    // The pipeline loop will set phase back to "embedding" on next iteration
    Json(serde_json::json!({ "status": "resumed" }))
}

/// Stop/cancel the running scan/embed task.
pub async fn stop_scan(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    state.task_cancelled.store(true, Ordering::Relaxed);
    state.task_paused.store(false, Ordering::Relaxed);
    // Immediately update phase so the UI reflects the intent
    if let Ok(mut progress) = state.task_progress.lock() {
        if matches!(progress.phase.as_str(), "scanning" | "embedding" | "paused" | "pausing") {
            progress.phase = "stopping".to_string();
        }
    }
    Json(serde_json::json!({ "status": "stopping" }))
}

/// Trigger a scan + embedding run. Runs in background, returns immediately.
pub async fn trigger_scan(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Check if already running
    {
        let progress = state.task_progress.lock().unwrap();
        match progress.phase.as_str() {
            "scanning" | "embedding" | "paused" | "pausing" | "stopping" => {
                return Ok(Json(serde_json::json!({
                    "status": "already_running",
                    "phase": progress.phase,
                })));
            }
            _ => {}
        }
    }

    // Reset state for new run
    state.task_cancelled.store(false, Ordering::Relaxed);
    state.task_paused.store(false, Ordering::Relaxed);
    {
        let mut progress = state.task_progress.lock().unwrap();
        *progress = TaskProgress {
            phase: "scanning".to_string(),
            started_at: Some(chrono::Utc::now().timestamp_millis()),
            ..Default::default()
        };
    }

    tokio::spawn(async move {
        if let Err(e) = run_scan_and_embed(&state).await {
            tracing::error!("Scan+embed failed: {}", e);
            if let Ok(mut progress) = state.task_progress.lock() {
                progress.phase = "error".to_string();
                progress.error = Some(e.to_string());
                progress.current_file.clear();
            }
        }
    });
    Ok(Json(serde_json::json!({ "status": "started" })))
}

pub async fn run_scan_and_embed(state: &AppState) -> anyhow::Result<()> {
    // Scanning phase
    {
        let mut progress = state.task_progress.lock().unwrap();
        progress.phase = "scanning".to_string();
    }
    run_scan_only(state).await?;

    // Check if cancelled during scan
    if state.task_cancelled.load(Ordering::Relaxed) {
        let mut progress = state.task_progress.lock().unwrap();
        progress.phase = "idle".to_string();
        progress.current_file.clear();
        return Ok(());
    }

    // Run embedding pipeline
    let pool = state.db.pool();
    let url = ConfigRepo::get(pool, "embedding_url")
        .await?
        .and_then(|v| v.as_str().map(String::from));
    let key = ConfigRepo::get(pool, "embedding_key")
        .await?
        .and_then(|v| v.as_str().map(String::from));
    let model = ConfigRepo::get(pool, "embedding_model")
        .await?
        .and_then(|v| v.as_str().map(String::from));

    let dimension = ConfigRepo::get(pool, "embedding_dimension")
        .await?
        .and_then(|v| v.as_u64().map(|n| n as u32));

    if let Some(client) = EmbeddingClient::from_config(url.as_deref(), key.as_deref(), model.as_deref(), dimension) {
        // Load image-to-text config
        let vision_client = {
            let i2t_enabled = ConfigRepo::get(pool, "i2t_enabled")
                .await?
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if i2t_enabled {
                let i2t_provider = ConfigRepo::get(pool, "i2t_provider")
                    .await?
                    .and_then(|v| v.as_str().map(String::from));
                let i2t_url = ConfigRepo::get(pool, "i2t_url")
                    .await?
                    .and_then(|v| v.as_str().map(String::from));
                let i2t_key = ConfigRepo::get(pool, "i2t_key")
                    .await?
                    .and_then(|v| v.as_str().map(String::from));
                let i2t_model = ConfigRepo::get(pool, "i2t_model")
                    .await?
                    .and_then(|v| v.as_str().map(String::from));

                photomind_core::vision::VisionClient::from_config(
                    i2t_provider.as_deref(),
                    i2t_url.as_deref(),
                    i2t_key.as_deref(),
                    i2t_model.as_deref(),
                )
            } else {
                None
            }
        };

        let pipeline = EmbeddingPipeline::new(
            client,
            pool.clone(),
            vision_client,
            state.task_progress.clone(),
            state.task_paused.clone(),
            state.task_cancelled.clone(),
            {
                let c = ConfigRepo::get(pool, "embedding_concurrency")
                    .await
                    .ok()
                    .flatten()
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1) as usize;
                c.max(1).min(32)
            },
        );
        let count = pipeline.run_to_completion(20).await?;
        tracing::info!("Embedding complete: {} new embeddings", count);

        // Reload vector index
        state.vector_index.load_from_db(pool).await?;
    } else {
        tracing::info!("Embedding model not configured, skipping");
        let mut progress = state.task_progress.lock().unwrap();
        progress.phase = "done".to_string();
    }

    Ok(())
}

/// Scan only — no embedding. Used on startup.
pub async fn run_scan_only(state: &AppState) -> anyhow::Result<()> {
    let pool = state.db.pool();

    let scan_dirs = ConfigRepo::get(pool, "scan_dirs")
        .await?
        .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
        .unwrap_or_default();

    if !scan_dirs.is_empty() {
        let scanner = PhotoScanner::new(pool.clone());
        let new_photos = scanner.scan_all(&scan_dirs).await?;
        tracing::info!("Scan complete: {} new photos", new_photos);
    }

    Ok(())
}
