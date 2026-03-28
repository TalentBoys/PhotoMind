use axum::extract::{Multipart, Path, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use photomind_core::embedding::EmbeddingClient;
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

    EmbeddingClient::from_config(url.as_deref(), key.as_deref(), model.as_deref())
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

    let query_vector = client
        .embed_text(&query)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let hits = state.vector_index.search(&query_vector, req.limit);
    let results = build_results(&state, hits).await;

    Ok(Json(SearchResponse { results }))
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

    let query_vector = client
        .embed_image(&data, &mime_type)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

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

pub async fn get_photo_info(
    State(state): State<AppState>,
    Path(photo_id): Path<i64>,
) -> Result<Json<Photo>, StatusCode> {
    let photo = PhotoRepo::get_by_id(state.db.pool(), photo_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(photo))
}
