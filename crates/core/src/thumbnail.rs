use anyhow::Result;
use image::imageops::FilterType;
use std::path::{Path, PathBuf};
use tracing::warn;

const THUMBNAIL_SIZE: u32 = 400;
const PREVIEW_SIZE: u32 = 1600;

pub struct ThumbnailGenerator {
    cache_dir: PathBuf,
}

impl ThumbnailGenerator {
    pub fn new(data_dir: &Path) -> Result<Self> {
        let cache_dir = data_dir.join("thumbnails");
        std::fs::create_dir_all(&cache_dir)?;
        Ok(Self { cache_dir })
    }

    /// Get or generate a thumbnail (400px). Returns the JPEG bytes.
    pub async fn get_or_generate(&self, photo_id: i64, source_path: &str) -> Result<Vec<u8>> {
        let cache_path = self.cache_path(photo_id, "thumb");

        if cache_path.exists() {
            return Ok(tokio::fs::read(&cache_path).await?);
        }

        let source = PathBuf::from(source_path);
        let cache = cache_path.clone();
        let bytes = tokio::task::spawn_blocking(move || resize_and_cache(&source, &cache, THUMBNAIL_SIZE))
            .await??;

        Ok(bytes)
    }

    /// Get or generate a preview (1600px). Returns the JPEG bytes.
    pub async fn get_or_generate_preview(&self, photo_id: i64, source_path: &str) -> Result<Vec<u8>> {
        let cache_path = self.cache_path(photo_id, "preview");

        if cache_path.exists() {
            return Ok(tokio::fs::read(&cache_path).await?);
        }

        let source = PathBuf::from(source_path);
        let cache = cache_path.clone();
        let bytes = tokio::task::spawn_blocking(move || resize_and_cache(&source, &cache, PREVIEW_SIZE))
            .await??;

        Ok(bytes)
    }

    fn cache_path(&self, photo_id: i64, kind: &str) -> PathBuf {
        let subdir = format!("{:02}", photo_id % 100);
        let dir = self.cache_dir.join(&subdir);
        std::fs::create_dir_all(&dir).ok();
        dir.join(format!("{}_{}.jpg", photo_id, kind))
    }
}

fn resize_and_cache(source: &Path, cache: &Path, max_size: u32) -> Result<Vec<u8>> {
    let img = image::open(source)?;
    let resized = img.resize(max_size, max_size, FilterType::Lanczos3);

    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    resized.write_to(&mut cursor, image::ImageFormat::Jpeg)?;

    if let Err(e) = std::fs::write(cache, &buf) {
        warn!("Failed to cache image: {}", e);
    }

    Ok(buf)
}
