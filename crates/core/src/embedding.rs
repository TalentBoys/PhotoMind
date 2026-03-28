use anyhow::{anyhow, Result};
use base64::Engine;
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

/// Progress state for scan & embed tasks.
#[derive(Debug, Clone, Serialize)]
pub struct TaskProgress {
    pub phase: String,
    pub total: u64,
    pub processed: u64,
    pub failed: u64,
    pub vision_calls: u64,
    pub vision_tokens: u64,
    pub embed_calls: u64,
    pub embed_tokens: u64,
    pub current_file: String,
    pub error: Option<String>,
    pub started_at: Option<i64>,
}

impl Default for TaskProgress {
    fn default() -> Self {
        Self {
            phase: "idle".to_string(),
            total: 0,
            processed: 0,
            failed: 0,
            vision_calls: 0,
            vision_tokens: 0,
            embed_calls: 0,
            embed_tokens: 0,
            current_file: String::new(),
            error: None,
            started_at: None,
        }
    }
}

/// Client for calling embedding APIs (Google Generative AI).
pub struct EmbeddingClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    dimension: u32,
}

#[derive(Serialize)]
struct EmbedRequest {
    content: EmbedContent,
    output_dimensionality: u32,
}

#[derive(Serialize)]
struct EmbedContent {
    parts: Vec<EmbedPart>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum EmbedPart {
    Text { text: String },
    InlineData { inline_data: InlineData },
}

#[derive(Serialize)]
struct InlineData {
    mime_type: String,
    data: String,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embedding: Option<EmbeddingValues>,
}

#[derive(Deserialize)]
struct EmbeddingValues {
    values: Vec<f32>,
}

pub const DEFAULT_EMBEDDING_DIMENSION: u32 = 768;

impl EmbeddingClient {
    pub fn new(base_url: &str, api_key: &str, model: &str, dimension: u32) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            dimension,
        }
    }

    /// Create from stored config values. Returns None if not configured.
    pub fn from_config(
        base_url: Option<&str>,
        api_key: Option<&str>,
        model: Option<&str>,
        dimension: Option<u32>,
    ) -> Option<Self> {
        match (base_url, api_key, model) {
            (Some(url), Some(key), Some(m)) if !url.is_empty() && !key.is_empty() && !m.is_empty() => {
                Some(Self::new(url, key, m, dimension.unwrap_or(DEFAULT_EMBEDDING_DIMENSION)))
            }
            _ => None,
        }
    }

    /// Embed a text query. Returns (embedding vector, tokens used).
    pub async fn embed_text(&self, text: &str) -> Result<(Vec<f32>, u64)> {
        let body = EmbedRequest {
            content: EmbedContent {
                parts: vec![EmbedPart::Text {
                    text: text.to_string(),
                }],
            },
            output_dimensionality: self.dimension,
        };
        self.call_embed_api(&body).await
    }

    /// Embed an image (raw bytes). Returns (embedding vector, tokens used).
    pub async fn embed_image(&self, image_data: &[u8], mime_type: &str) -> Result<(Vec<f32>, u64)> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(image_data);
        let body = EmbedRequest {
            content: EmbedContent {
                parts: vec![EmbedPart::InlineData {
                    inline_data: InlineData {
                        mime_type: mime_type.to_string(),
                        data: b64,
                    },
                }],
            },
            output_dimensionality: self.dimension,
        };
        self.call_embed_api(&body).await
    }

    async fn call_embed_api(&self, body: &EmbedRequest) -> Result<(Vec<f32>, u64)> {
        let model_path = if self.model.starts_with("models/") {
            self.model.clone()
        } else {
            format!("models/{}", self.model)
        };
        let url = format!(
            "{}/v1/{}:embedContent",
            self.base_url, model_path
        );

        let resp = self.http.post(&url)
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Embedding API error {}: {}", status, text));
        }

        let data: serde_json::Value = resp.json().await?;

        // Parse token usage if available (Google returns metadata.billableCharacterCount)
        let tokens = data.get("metadata")
            .and_then(|m| m.get("billableCharacterCount"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let result: EmbedResponse = serde_json::from_value(data)?;
        let vector = result
            .embedding
            .map(|e| e.values)
            .ok_or_else(|| anyhow!("No embedding in response"))?;
        Ok((vector, tokens))
    }
}

/// Get the MIME type for an image file extension.
pub fn mime_for_extension(ext: &str) -> &str {
    match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tiff" | "tif" => "image/tiff",
        "heic" | "heif" => "image/heic",
        "avif" => "image/avif",
        _ => "image/jpeg",
    }
}

use crate::vision::VisionClient;

/// Embedding pipeline: processes unembedded photos with configurable concurrency.
pub struct EmbeddingPipeline {
    client: Arc<EmbeddingClient>,
    pool: sqlx::SqlitePool,
    vision: Option<Arc<VisionClient>>,
    progress: Arc<Mutex<TaskProgress>>,
    paused: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    concurrency: usize,
}

impl EmbeddingPipeline {
    pub fn new(
        client: EmbeddingClient,
        pool: sqlx::SqlitePool,
        vision: Option<VisionClient>,
        progress: Arc<Mutex<TaskProgress>>,
        paused: Arc<AtomicBool>,
        cancelled: Arc<AtomicBool>,
        concurrency: usize,
    ) -> Self {
        Self {
            client: Arc::new(client),
            pool,
            vision: vision.map(Arc::new),
            progress,
            paused,
            cancelled,
            concurrency: concurrency.max(1),
        }
    }

    fn update_progress<F: FnOnce(&mut TaskProgress)>(&self, f: F) {
        if let Ok(mut p) = self.progress.lock() {
            f(&mut p);
        }
    }

    /// Check if we should stop. Returns true if cancelled.
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Wait while paused. Returns true if cancelled during wait.
    async fn wait_if_paused(&self) -> bool {
        while self.paused.load(Ordering::Relaxed) {
            self.update_progress(|p| p.phase = "paused".to_string());
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            if self.is_cancelled() {
                return true;
            }
        }
        self.update_progress(|p| {
            if p.phase == "paused" {
                p.phase = "embedding".to_string();
            }
        });
        false
    }

    /// Process a single photo. Returns Ok(true) if embedded, Ok(false) if failed/skipped.
    async fn process_one(
        client: &EmbeddingClient,
        vision: Option<&VisionClient>,
        pool: &sqlx::SqlitePool,
        photo: &photomind_storage::models::Photo,
        progress: &Arc<Mutex<TaskProgress>>,
    ) -> Result<bool> {
        use photomind_storage::repo::embeddings::EmbeddingRepo;
        use photomind_storage::repo::photos::PhotoRepo;

        let file_name = std::path::Path::new(&photo.file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        if let Ok(mut p) = progress.lock() {
            p.current_file = file_name;
        }

        let ext = std::path::Path::new(&photo.file_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg");
        let mime = mime_for_extension(ext);

        // Read image file
        let data = match tokio::fs::read(&photo.file_path).await {
            Ok(d) => d,
            Err(e) => {
                warn!("Failed to read {}: {}", photo.file_path, e);
                if let Ok(mut p) = progress.lock() {
                    p.failed += 1;
                }
                return Ok(false);
            }
        };

        // Call embedding API — use vision (image→text→embed) or direct image embed
        let embed_result = if let Some(vision) = vision {
            match vision.describe_image(&data, mime).await {
                Ok((description, vision_tokens)) => {
                    info!("Described photo {}: {}...", photo.id, &description[..description.len().min(80)]);
                    if let Ok(mut p) = progress.lock() {
                        p.vision_calls += 1;
                        p.vision_tokens += vision_tokens;
                    }
                    client.embed_text(&description).await
                }
                Err(e) => Err(e),
            }
        } else {
            client.embed_image(&data, mime).await
        };

        match embed_result {
            Ok((vector, embed_tokens)) => {
                EmbeddingRepo::insert(pool, photo.id, &vector, &client.model).await?;
                PhotoRepo::mark_embedded(pool, photo.id).await?;
                if let Ok(mut p) = progress.lock() {
                    p.processed += 1;
                    p.embed_calls += 1;
                    p.embed_tokens += embed_tokens;
                }
                Ok(true)
            }
            Err(e) => {
                warn!("Failed to embed photo {}: {}", photo.id, e);
                if let Ok(mut p) = progress.lock() {
                    p.failed += 1;
                }
                Ok(false)
            }
        }
    }

    /// Embed a photo given a pre-computed description. Returns Ok(true) if embedded.
    async fn embed_with_description(
        client: &EmbeddingClient,
        pool: &sqlx::SqlitePool,
        photo_id: i64,
        description: &str,
        progress: &Arc<Mutex<TaskProgress>>,
    ) -> Result<bool> {
        use photomind_storage::repo::embeddings::EmbeddingRepo;
        use photomind_storage::repo::photos::PhotoRepo;

        match client.embed_text(description).await {
            Ok((vector, embed_tokens)) => {
                EmbeddingRepo::insert(pool, photo_id, &vector, &client.model).await?;
                PhotoRepo::mark_embedded(pool, photo_id).await?;
                if let Ok(mut p) = progress.lock() {
                    p.processed += 1;
                    p.embed_calls += 1;
                    p.embed_tokens += embed_tokens;
                }
                Ok(true)
            }
            Err(e) => {
                warn!("Failed to embed photo {}: {}", photo_id, e);
                if let Ok(mut p) = progress.lock() {
                    p.failed += 1;
                }
                Ok(false)
            }
        }
    }

    /// Process a batch of unembedded photos. Returns number processed.
    pub async fn process_batch(&self, batch_size: i64) -> Result<u64> {
        use photomind_storage::repo::photos::PhotoRepo;

        let photos = PhotoRepo::list_unembedded(&self.pool, batch_size).await?;
        if photos.is_empty() {
            return Ok(0);
        }

        info!("Processing {} photos for embedding (concurrency={})", photos.len(), self.concurrency);

        // When vision is enabled, use batch describe then concurrent embed
        if let Some(ref vision) = self.vision {
            return self.process_batch_with_vision(vision, &photos).await;
        }

        // No vision: concurrent per-photo embed_image
        let client = self.client.clone();
        let pool = self.pool.clone();
        let progress = self.progress.clone();
        let paused = self.paused.clone();
        let cancelled = self.cancelled.clone();

        let results: Vec<Result<bool>> = stream::iter(photos)
            .map(|photo| {
                let client = client.clone();
                let pool = pool.clone();
                let progress = progress.clone();
                let paused = paused.clone();
                let cancelled = cancelled.clone();
                async move {
                    if cancelled.load(Ordering::Relaxed) {
                        return Ok(false);
                    }
                    while paused.load(Ordering::Relaxed) {
                        if let Ok(mut p) = progress.lock() {
                            p.phase = "paused".to_string();
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                        if cancelled.load(Ordering::Relaxed) {
                            return Ok(false);
                        }
                    }
                    if let Ok(mut p) = progress.lock() {
                        if p.phase == "paused" {
                            p.phase = "embedding".to_string();
                        }
                    }

                    let result = Self::process_one(
                        &client, None, &pool, &photo, &progress,
                    ).await;

                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    result
                }
            })
            .buffer_unordered(self.concurrency)
            .collect()
            .await;

        let count = results.iter().filter(|r| matches!(r, Ok(true))).count() as u64;
        info!("Embedded {} photos in this batch", count);
        Ok(count)
    }

    /// Process photos with batch vision: group → batch describe → concurrent embed.
    async fn process_batch_with_vision(
        &self,
        vision: &VisionClient,
        photos: &[photomind_storage::models::Photo],
    ) -> Result<u64> {
        let vision_batch_size = vision.max_batch_size();
        let mut total_count = 0u64;

        // Process in vision sub-batches
        for chunk in photos.chunks(vision_batch_size) {
            // Check cancelled / paused
            if self.is_cancelled() {
                break;
            }
            if self.wait_if_paused().await {
                break;
            }

            // Read all images in this sub-batch
            let mut image_data: Vec<(i64, Vec<u8>, String)> = Vec::new(); // (photo_id, data, mime)
            for photo in chunk {
                let ext = std::path::Path::new(&photo.file_path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("jpg");
                let mime = mime_for_extension(ext).to_string();

                match tokio::fs::read(&photo.file_path).await {
                    Ok(data) => {
                        image_data.push((photo.id, data, mime));
                    }
                    Err(e) => {
                        warn!("Failed to read {}: {}", photo.file_path, e);
                        self.update_progress(|p| p.failed += 1);
                    }
                }
            }

            if image_data.is_empty() {
                continue;
            }

            // Update current file display
            let names: Vec<String> = chunk.iter()
                .map(|p| std::path::Path::new(&p.file_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?")
                    .to_string())
                .collect();
            self.update_progress(|p| {
                p.current_file = if names.len() == 1 {
                    names[0].clone()
                } else {
                    format!("{} (+{} more)", names[0], names.len() - 1)
                };
            });

            // Batch vision call
            let images_ref: Vec<(&[u8], &str)> = image_data.iter()
                .map(|(_, data, mime)| (data.as_slice(), mime.as_str()))
                .collect();

            let (descriptions, vision_tokens) = match vision.describe_images_batch(&images_ref).await {
                Ok(r) => r,
                Err(e) => {
                    warn!("Batch vision failed: {}", e);
                    self.update_progress(|p| p.failed += image_data.len() as u64);
                    continue;
                }
            };

            self.update_progress(|p| {
                p.vision_calls += 1;
                p.vision_tokens += vision_tokens;
            });

            // Now embed each description concurrently
            let pairs: Vec<(i64, String)> = image_data.iter()
                .map(|(id, _, _)| *id)
                .zip(descriptions.into_iter())
                .collect();

            let client = self.client.clone();
            let pool = self.pool.clone();
            let progress = self.progress.clone();

            let results: Vec<Result<bool>> = stream::iter(pairs)
                .map(|(photo_id, desc)| {
                    let client = client.clone();
                    let pool = pool.clone();
                    let progress = progress.clone();
                    async move {
                        info!("Embedding photo {} from batch description", photo_id);
                        let result = Self::embed_with_description(
                            &client, &pool, photo_id, &desc, &progress,
                        ).await;
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        result
                    }
                })
                .buffer_unordered(self.concurrency)
                .collect()
                .await;

            let batch_count = results.iter().filter(|r| matches!(r, Ok(true))).count() as u64;
            total_count += batch_count;
        }

        info!("Embedded {} photos in this batch (vision)", total_count);
        Ok(total_count)
    }

    /// Run the embedding pipeline continuously until all photos are embedded.
    pub async fn run_to_completion(&self, batch_size: i64) -> Result<u64> {
        // Count total unembedded to set progress total
        use photomind_storage::repo::photos::PhotoRepo;
        let total_unembedded = PhotoRepo::count_unembedded(&self.pool).await.unwrap_or(0) as u64;
        self.update_progress(|p| {
            p.phase = "embedding".to_string();
            p.total = p.processed + total_unembedded;
        });

        let mut total = 0;
        loop {
            if self.is_cancelled() {
                break;
            }
            if self.wait_if_paused().await {
                break;
            }
            let count = self.process_batch(batch_size).await?;
            if count == 0 {
                break;
            }
            total += count;
        }

        let final_phase = if self.is_cancelled() {
            "idle".to_string()
        } else {
            "done".to_string()
        };
        self.update_progress(|p| {
            p.phase = final_phase;
            p.current_file.clear();
        });

        info!("Embedding pipeline complete: {} total", total);
        Ok(total)
    }
}
