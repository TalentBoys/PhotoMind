use anyhow::{anyhow, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Client for calling embedding APIs (Google Generative AI).
pub struct EmbeddingClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

#[derive(Serialize)]
struct EmbedRequest {
    content: EmbedContent,
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

impl EmbeddingClient {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }

    /// Create from stored config values. Returns None if not configured.
    pub fn from_config(
        base_url: Option<&str>,
        api_key: Option<&str>,
        model: Option<&str>,
    ) -> Option<Self> {
        match (base_url, api_key, model) {
            (Some(url), Some(key), Some(m)) if !url.is_empty() && !key.is_empty() && !m.is_empty() => {
                Some(Self::new(url, key, m))
            }
            _ => None,
        }
    }

    /// Embed a text query. Returns the embedding vector.
    pub async fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        let body = EmbedRequest {
            content: EmbedContent {
                parts: vec![EmbedPart::Text {
                    text: text.to_string(),
                }],
            },
        };
        self.call_embed_api(&body).await
    }

    /// Embed an image (raw bytes). Returns the embedding vector.
    pub async fn embed_image(&self, image_data: &[u8], mime_type: &str) -> Result<Vec<f32>> {
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
        };
        self.call_embed_api(&body).await
    }

    async fn call_embed_api(&self, body: &EmbedRequest) -> Result<Vec<f32>> {
        let url = format!(
            "{}/v1/models/{}:embedContent?key={}",
            self.base_url, self.model, self.api_key
        );

        let resp = self.http.post(&url).json(body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Embedding API error {}: {}", status, text));
        }

        let result: EmbedResponse = resp.json().await?;
        result
            .embedding
            .map(|e| e.values)
            .ok_or_else(|| anyhow!("No embedding in response"))
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

/// Embedding pipeline: processes unembedded photos in batches.
pub struct EmbeddingPipeline {
    client: EmbeddingClient,
    pool: sqlx::SqlitePool,
}

impl EmbeddingPipeline {
    pub fn new(client: EmbeddingClient, pool: sqlx::SqlitePool) -> Self {
        Self { client, pool }
    }

    /// Process a batch of unembedded photos. Returns number processed.
    pub async fn process_batch(&self, batch_size: i64) -> Result<u64> {
        use photomind_storage::repo::embeddings::EmbeddingRepo;
        use photomind_storage::repo::photos::PhotoRepo;

        let photos = PhotoRepo::list_unembedded(&self.pool, batch_size).await?;
        if photos.is_empty() {
            return Ok(0);
        }

        info!("Processing {} photos for embedding", photos.len());
        let mut count = 0u64;

        for photo in &photos {
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
                    continue;
                }
            };

            // Call embedding API
            match self.client.embed_image(&data, mime).await {
                Ok(vector) => {
                    // Store embedding
                    EmbeddingRepo::insert(&self.pool, photo.id, &vector, &self.client.model)
                        .await?;
                    PhotoRepo::mark_embedded(&self.pool, photo.id).await?;
                    count += 1;
                }
                Err(e) => {
                    warn!("Failed to embed photo {}: {}", photo.id, e);
                    // Rate limit: small delay on error
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }

            // Small delay between requests to respect rate limits
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        info!("Embedded {} photos in this batch", count);
        Ok(count)
    }

    /// Run the embedding pipeline continuously until all photos are embedded.
    pub async fn run_to_completion(&self, batch_size: i64) -> Result<u64> {
        let mut total = 0;
        loop {
            let count = self.process_batch(batch_size).await?;
            if count == 0 {
                break;
            }
            total += count;
        }
        info!("Embedding pipeline complete: {} total", total);
        Ok(total)
    }
}
