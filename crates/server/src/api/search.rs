use axum::extract::{Multipart, Path, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use photomind_core::embedding::EmbeddingClient;
use photomind_core::vision::VisionClient;
use photomind_storage::models::Photo;
use photomind_storage::repo::configs::ConfigRepo;
use photomind_storage::repo::photos::PhotoRepo;
use serde::{Deserialize, Serialize};

use crate::AppState;

#[derive(Deserialize)]
pub struct SearchRequest {
    pub query: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    20
}

#[derive(Serialize)]
pub struct SearchResult {
    pub id: i64,
    pub file_path: String,
    pub file_name: String,
    pub score: f32,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub format: Option<String>,
    pub taken_at: Option<chrono::NaiveDateTime>,
    pub file_size: Option<i64>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
}

/// Get embedding client from stored config.
async fn get_embedding_client(state: &AppState) -> Option<EmbeddingClient> {
    let pool = state.db.pool();
    let url = ConfigRepo::get(pool, "embedding_url")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(String::from));
    let key = ConfigRepo::get(pool, "embedding_key")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(String::from));
    let model = ConfigRepo::get(pool, "embedding_model")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(String::from));

    let dimension = ConfigRepo::get(pool, "embedding_dimension")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_u64().map(|n| n as u32));

    EmbeddingClient::from_config(url.as_deref(), key.as_deref(), model.as_deref(), dimension)
}

pub async fn search_text(
    State(state): State<AppState>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, StatusCode> {
    let query = req.query.unwrap_or_default();
    if query.is_empty() {
        return Ok(Json(SearchResponse { results: vec![] }));
    }

    let client = get_embedding_client(&state)
        .await
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let (query_vector, _) = client
        .embed_text(&query)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let hits = state.vector_index.search(&query_vector, req.limit);
    let results = build_results(&state, hits).await;

    Ok(Json(SearchResponse { results }))
}

/// Get vision client from stored config, if I2T is enabled.
async fn get_vision_client(state: &AppState) -> Option<VisionClient> {
    let pool = state.db.pool();
    let enabled = ConfigRepo::get(pool, "i2t_enabled")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !enabled {
        return None;
    }

    let provider = ConfigRepo::get(pool, "i2t_provider")
        .await.ok().flatten().and_then(|v| v.as_str().map(String::from));
    let url = ConfigRepo::get(pool, "i2t_url")
        .await.ok().flatten().and_then(|v| v.as_str().map(String::from));
    let key = ConfigRepo::get(pool, "i2t_key")
        .await.ok().flatten().and_then(|v| v.as_str().map(String::from));
    let model = ConfigRepo::get(pool, "i2t_model")
        .await.ok().flatten().and_then(|v| v.as_str().map(String::from));

    VisionClient::from_config(
        provider.as_deref(),
        url.as_deref(),
        key.as_deref(),
        model.as_deref(),
    )
}

pub async fn search_image(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<SearchResponse>, StatusCode> {
    let mut image_data: Option<Vec<u8>> = None;
    let mut mime_type = "image/jpeg".to_string();
    let mut limit = 20usize;

    while let Some(field) = multipart.next_field().await.map_err(|_| StatusCode::BAD_REQUEST)? {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "image" => {
                if let Some(ct) = field.content_type() {
                    mime_type = ct.to_string();
                }
                image_data = Some(field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?.to_vec());
            }
            "limit" => {
                if let Ok(text) = field.text().await {
                    limit = text.parse().unwrap_or(20);
                }
            }
            _ => {}
        }
    }

    let data = image_data.ok_or(StatusCode::BAD_REQUEST)?;

    let client = get_embedding_client(&state)
        .await
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // If I2T (vision) is enabled, use the same path as indexing:
    // image → vision describe → embed_text
    let query_vector = if let Some(vision) = get_vision_client(&state).await {
        let (description, _) = vision
            .describe_image(&data, &mime_type)
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
        let (vec, _) = client
            .embed_text(&description)
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
        vec
    } else {
        let (vec, _) = client
            .embed_image(&data, &mime_type)
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
        vec
    };

    let hits = state.vector_index.search(&query_vector, limit);
    let results = build_results(&state, hits).await;

    Ok(Json(SearchResponse { results }))
}

async fn build_results(
    state: &AppState,
    hits: Vec<photomind_core::search::SearchHit>,
) -> Vec<SearchResult> {
    let mut results = Vec::new();
    for hit in hits {
        if let Ok(photo) = PhotoRepo::get_by_id(state.db.pool(), hit.photo_id).await {
            results.push(SearchResult {
                id: photo.id,
                file_path: photo.file_path,
                file_name: photo.file_name,
                score: hit.score,
                width: photo.width,
                height: photo.height,
                format: photo.format,
                taken_at: photo.taken_at,
                file_size: photo.file_size,
                latitude: photo.latitude,
                longitude: photo.longitude,
            });
        }
    }
    results
}

pub async fn get_thumbnail(
    State(state): State<AppState>,
    Path(photo_id): Path<i64>,
) -> Result<impl IntoResponse, StatusCode> {
    let photo = PhotoRepo::get_by_id(state.db.pool(), photo_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let bytes = state
        .thumbnails
        .get_or_generate(photo_id, &photo.file_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(([(header::CONTENT_TYPE, "image/jpeg")], bytes))
}

pub async fn get_preview(
    State(state): State<AppState>,
    Path(photo_id): Path<i64>,
) -> Result<impl IntoResponse, StatusCode> {
    let photo = PhotoRepo::get_by_id(state.db.pool(), photo_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let bytes = state
        .thumbnails
        .get_or_generate_preview(photo_id, &photo.file_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(([(header::CONTENT_TYPE, "image/jpeg")], bytes))
}

pub async fn get_photo_info(
    State(state): State<AppState>,
    Path(photo_id): Path<i64>,
) -> Result<Json<Photo>, StatusCode> {
    let photo = PhotoRepo::get_by_id(state.db.pool(), photo_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(photo))
}

pub async fn get_photo_file(
    State(state): State<AppState>,
    Path(photo_id): Path<i64>,
) -> Result<impl IntoResponse, StatusCode> {
    let photo = PhotoRepo::get_by_id(state.db.pool(), photo_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let path = std::path::Path::new(&photo.file_path);
    if !path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let content_type = match photo.format.as_deref() {
        Some("jpeg") | Some("jpg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("tiff") | Some("tif") => "image/tiff",
        Some("heic") | Some("heif") => "image/heic",
        _ => {
            // Guess from extension
            match path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
                Some("jpg") | Some("jpeg") => "image/jpeg",
                Some("png") => "image/png",
                Some("gif") => "image/gif",
                Some("webp") => "image/webp",
                Some("bmp") => "image/bmp",
                Some("tiff") | Some("tif") => "image/tiff",
                Some("heic") | Some("heif") => "image/heic",
                _ => "application/octet-stream",
            }
        }
    };

    Ok((
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", photo.file_name),
            ),
        ],
        bytes,
    ))
}
