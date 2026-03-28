use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc;
use tracing::{info, warn};

use super::scanner::PhotoScanner;

pub struct FileWatcher {
    _watcher: RecommendedWatcher,
}

impl FileWatcher {
    /// Start watching the given directories. New image files trigger a scan.
    pub fn start(
        dirs: Vec<String>,
        pool: sqlx::SqlitePool,
    ) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

        let mut watcher = notify::recommended_watcher(tx)?;

        for dir in &dirs {
            let path = Path::new(dir);
            if path.exists() {
                watcher.watch(path, RecursiveMode::Recursive)?;
                info!("Watching directory: {}", dir);
            } else {
                warn!("Watch directory does not exist: {}", dir);
            }
        }

        // Spawn a thread to process file events
        let pool_clone = pool.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime for file watcher");

            let scanner = PhotoScanner::new(pool_clone);

            for result in rx {
                match result {
                    Ok(event) => {
                        if should_process_event(&event) {
                            for path in &event.paths {
                                if is_image_file(path) {
                                    let path = path.clone();
                                    let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
                                    rt.block_on(async {
                                        if let Err(e) = scanner.scan_directory(&dir).await {
                                            warn!("Watch scan error: {}", e);
                                        }
                                    });
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("File watch error: {}", e);
                    }
                }
            }
        });

        Ok(Self { _watcher: watcher })
    }
}

fn should_process_event(event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_)
    )
}

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "heic", "heif", "tiff", "tif", "bmp", "gif", "avif",
];

fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}
