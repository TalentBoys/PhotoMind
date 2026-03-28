use anyhow::Result;
use image::imageops::FilterType;
use std::path::{Path, PathBuf};
use tracing::warn;

const THUMBNAIL_SIZE: u32 = 400;

pub struct ThumbnailGenerator {
    cache_dir: PathBuf,
}

impl ThumbnailGenerator {
    pub fn new(data_dir: &Path) -> Result<Self> {
        let cache_dir = data_dir.join("thumbnails");
        std::fs::create_dir_all(&cache_dir)?;
        Ok(Self { cache_dir })
    }

    /// Get or generate a thumbnail. Returns the JPEG bytes.
    pub async fn get_or_generate(&self, photo_id: i64, source_path: &str) -> Result<Vec<u8>> {
        let cache_path = self.cache_path(photo_id);

        // Check cache first
        if cache_path.exists() {
            return Ok(tokio::fs::read(&cache_path).await?);
        }

        // Generate thumbnail
        let source = PathBuf::from(source_path);
        let cache = cache_path.clone();
        let bytes = tokio::task::spawn_blocking(move || generate_thumbnail(&source, &cache))
            .await??;

        Ok(bytes)
    }

    fn cache_path(&self, photo_id: i64) -> PathBuf {
        // Use subdirectories to avoid too many files in one dir
        let subdir = format!("{:02}", photo_id % 100);
        let dir = self.cache_dir.join(&subdir);
        std::fs::create_dir_all(&dir).ok();
        dir.join(format!("{}.jpg", photo_id))
    }
}

fn generate_thumbnail(source: &Path, cache: &Path) -> Result<Vec<u8>> {
    let img = image::open(source)?;
    let thumb = img.resize(THUMBNAIL_SIZE, THUMBNAIL_SIZE, FilterType::Lanczos3);

    // Encode as JPEG
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    thumb.write_to(&mut cursor, image::ImageFormat::Jpeg)?;

    // Save to cache
    if let Err(e) = std::fs::write(cache, &buf) {
        warn!("Failed to cache thumbnail: {}", e);
    }

    Ok(buf)
}
