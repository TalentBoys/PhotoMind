use anyhow::Result;
use chrono::NaiveDateTime;
use photomind_storage::models::NewPhoto;
use photomind_storage::repo::photos::PhotoRepo;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "heic", "heif", "tiff", "tif", "bmp", "gif", "avif",
];

pub struct PhotoScanner {
    pool: SqlitePool,
}

impl PhotoScanner {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Scan a directory recursively and insert new photos into the database.
    /// Returns the number of new photos found.
    pub async fn scan_directory(&self, dir: &Path) -> Result<u64> {
        if !dir.exists() {
            warn!("Scan directory does not exist: {}", dir.display());
            return Ok(0);
        }

        info!("Scanning directory: {}", dir.display());
        let entries = Self::collect_image_files(dir)?;
        info!("Found {} image files in {}", entries.len(), dir.display());

        let mut new_count = 0u64;
        for path in entries {
            match self.process_file(&path).await {
                Ok(true) => new_count += 1,
                Ok(false) => {} // already exists
                Err(e) => warn!("Failed to process {}: {}", path.display(), e),
            }
        }

        info!(
            "Scan complete: {} new photos from {}",
            new_count,
            dir.display()
        );
        Ok(new_count)
    }

    /// Scan all configured directories.
    pub async fn scan_all(&self, dirs: &[String]) -> Result<u64> {
        let mut total = 0;
        for dir in dirs {
            total += self.scan_directory(Path::new(dir)).await?;
        }
        Ok(total)
    }

    /// Process a single file: compute hash, check if already in DB, extract metadata, insert.
    /// Returns true if it was a new photo.
    async fn process_file(&self, path: &Path) -> Result<bool> {
        let path_str = path.to_string_lossy().to_string();

        // Skip if already in database by path
        if PhotoRepo::get_by_path(&self.pool, &path_str)
            .await?
            .is_some()
        {
            return Ok(false);
        }

        // Read file and compute hash
        let data = tokio::fs::read(path).await?;
        let hash = compute_hash(&data);

        // Extract image dimensions
        let (width, height, format) = extract_image_info(path);

        // Extract EXIF taken_at
        let taken_at = extract_exif_date(&data);

        // Extract GPS coordinates
        let (latitude, longitude) = extract_exif_gps(&data);

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let file_size = data.len() as i64;

        let new_photo = NewPhoto {
            file_path: path_str,
            file_name,
            file_size: Some(file_size),
            width,
            height,
            format,
            taken_at,
            file_hash: Some(hash),
            latitude,
            longitude,
        };

        PhotoRepo::insert(&self.pool, &new_photo).await?;
        Ok(true)
    }

    /// Recursively collect all image files under a directory.
    fn collect_image_files(dir: &Path) -> Result<Vec<PathBuf>> {
        let mut results = Vec::new();
        Self::walk_dir(dir, &mut results)?;
        Ok(results)
    }

    fn walk_dir(dir: &Path, results: &mut Vec<PathBuf>) -> Result<()> {
        let entries = std::fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                Self::walk_dir(&path, results)?;
            } else if is_image_file(&path) {
                results.push(path);
            }
        }
        Ok(())
    }
}

fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| SUPPORTED_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

fn extract_image_info(path: &Path) -> (Option<i32>, Option<i32>, Option<String>) {
    match image::image_dimensions(path) {
        Ok((w, h)) => {
            let format = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase());
            (Some(w as i32), Some(h as i32), format)
        }
        Err(_) => {
            let format = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase());
            (None, None, format)
        }
    }
}

fn extract_exif_date(data: &[u8]) -> Option<NaiveDateTime> {
    let mut cursor = std::io::Cursor::new(data);
    let reader = exif::Reader::new();
    let exif_data = reader.read_from_container(&mut cursor).ok()?;

    let field = exif_data.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)?;
    let datetime_str = field.display_value().to_string();

    // EXIF datetime format: "2024:01:15 14:30:00"
    // Replace first two colons in date part with dashes
    parse_exif_datetime(&datetime_str)
}

fn parse_exif_datetime(s: &str) -> Option<NaiveDateTime> {
    // Try standard EXIF format: "2024:01:15 14:30:00"
    NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .ok()
}

fn extract_exif_gps(data: &[u8]) -> (Option<f64>, Option<f64>) {
    let mut cursor = std::io::Cursor::new(data);
    let reader = exif::Reader::new();
    let exif_data = match reader.read_from_container(&mut cursor).ok() {
        Some(e) => e,
        None => return (None, None),
    };

    let lat = exif_data
        .get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
        .and_then(|f| gps_rational_to_decimal(&f.value));
    let lat_ref = exif_data
        .get_field(exif::Tag::GPSLatitudeRef, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string());

    let lon = exif_data
        .get_field(exif::Tag::GPSLongitude, exif::In::PRIMARY)
        .and_then(|f| gps_rational_to_decimal(&f.value));
    let lon_ref = exif_data
        .get_field(exif::Tag::GPSLongitudeRef, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string());

    match (lat, lon) {
        (Some(mut la), Some(mut lo)) => {
            if lat_ref.as_deref() == Some("S") {
                la = -la;
            }
            if lon_ref.as_deref() == Some("W") {
                lo = -lo;
            }
            (Some(la), Some(lo))
        }
        _ => (None, None),
    }
}

fn gps_rational_to_decimal(value: &exif::Value) -> Option<f64> {
    if let exif::Value::Rational(ref rats) = value {
        if rats.len() >= 3 {
            let degrees = rats[0].to_f64();
            let minutes = rats[1].to_f64();
            let seconds = rats[2].to_f64();
            return Some(degrees + minutes / 60.0 + seconds / 3600.0);
        }
    }
    None
}
