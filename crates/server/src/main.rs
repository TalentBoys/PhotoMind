mod api;

use anyhow::Result;
use axum::routing::{delete, get, patch, post, put};
use axum::Router;
use photomind_core::embedding::TaskProgress;
use photomind_core::search::VectorIndex;
use photomind_core::thumbnail::ThumbnailGenerator;
use photomind_storage::Database;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub data_dir: PathBuf,
    pub thumbnails: Arc<ThumbnailGenerator>,
    pub vector_index: Arc<VectorIndex>,
    pub task_progress: Arc<Mutex<TaskProgress>>,
    pub task_paused: Arc<AtomicBool>,
    pub task_cancelled: Arc<AtomicBool>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let data_dir = std::env::var("PHOTOMIND_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data"));

    tracing::info!("Data directory: {}", data_dir.display());

    let db = Database::new(&data_dir).await?;

    // Register built-in tools on startup
    register_builtin_tools(&db).await?;

    let thumbnails = Arc::new(ThumbnailGenerator::new(&data_dir)?);
    let vector_index = Arc::new(VectorIndex::new());

    // Load existing embeddings into memory
    vector_index.load_from_db(db.pool()).await?;

    let state = AppState {
        db: db.clone(),
        data_dir: data_dir.clone(),
        thumbnails,
        vector_index,
        task_progress: Arc::new(Mutex::new(TaskProgress::default())),
        task_paused: Arc::new(AtomicBool::new(false)),
        task_cancelled: Arc::new(AtomicBool::new(false)),
    };

    // Spawn background scan task (scan only, no embedding on startup)
    let scan_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = api::status::run_scan_only(&scan_state).await {
            tracing::error!("Initial scan failed: {}", e);
        }
    });

    // Start file watcher for configured directories
    let _watcher = {
        let scan_dirs = photomind_storage::repo::configs::ConfigRepo::get(db.pool(), "scan_dirs")
            .await
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
            .unwrap_or_default();
        if !scan_dirs.is_empty() {
            match photomind_core::watcher::FileWatcher::start(scan_dirs, db.pool().clone()) {
                Ok(w) => Some(w),
                Err(e) => {
                    tracing::warn!("Failed to start file watcher: {}", e);
                    None
                }
            }
        } else {
            None
        }
    };

    let api_routes = Router::new()
        // Settings
        .route("/settings", get(api::settings::get_settings))
        .route("/settings", put(api::settings::update_settings))
        .route(
            "/settings/embedding-models",
            post(api::settings::fetch_embedding_models),
        )
        .route(
            "/settings/agent-models",
            post(api::settings::fetch_agent_models),
        )
        .route("/browse-dirs", post(api::settings::browse_dirs))
        .route(
            "/settings/test-embedding",
            post(api::settings::test_embedding),
        )
        .route(
            "/settings/test-agent",
            post(api::settings::test_agent),
        )
        .route(
            "/settings/test-i2t",
            post(api::settings::test_i2t),
        )
        // Tools
        .route("/tools", get(api::tools::list_tools))
        .route("/tools", post(api::tools::create_tool))
        .route("/tools/{tool_id}", patch(api::tools::toggle_tool))
        .route("/tools/{tool_id}", delete(api::tools::delete_tool))
        // Status
        .route("/status", get(api::status::get_status))
        .route("/scan", post(api::status::trigger_scan))
        .route("/scan/progress", get(api::status::get_scan_progress))
        .route("/scan/pause", post(api::status::pause_scan))
        .route("/scan/resume", post(api::status::resume_scan))
        .route("/scan/stop", post(api::status::stop_scan))
        // Search
        .route("/search", post(api::search::search_text))
        .route("/search/image", post(api::search::search_image))
        .route("/photos/{photo_id}/thumbnail", get(api::search::get_thumbnail))
        .route("/photos/{photo_id}/preview", get(api::search::get_preview))
        .route("/photos/{photo_id}/file", get(api::search::get_photo_file))
        .route("/photos/{photo_id}", get(api::search::get_photo_info))
        // Chat
        .route("/chat", post(api::chat::chat))
        .route("/chat/continue", post(api::chat::continue_chat))
        .route("/chat/confirm-tool", post(api::chat::confirm_tool))
        .route("/chat/sessions", get(api::chat::list_sessions))
        .route("/chat/sessions/{session_id}", delete(api::chat::delete_session))
        .route("/chat/sessions/{session_id}/messages", get(api::chat::get_session_messages));

    // Serve React frontend from dist/ or web/dist/
    let static_dir = if PathBuf::from("web/dist").exists() {
        "web/dist"
    } else if PathBuf::from("dist").exists() {
        "dist"
    } else {
        "web/dist"
    };

    let app = Router::new()
        .nest("/api", api_routes)
        .fallback_service(ServeDir::new(static_dir).fallback(
            tower_http::services::ServeFile::new(format!("{}/index.html", static_dir)),
        ))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = std::env::var("PHOTOMIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("PhotoMind listening on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}

async fn register_builtin_tools(db: &Database) -> Result<()> {
    use photomind_storage::models::NewToolDef;
    use photomind_storage::repo::tools::ToolRepo;

    let builtin_tools = vec![
        NewToolDef {
            id: "builtin:search_photos".to_string(),
            name: "Search Photos".to_string(),
            description: Some("Search for photos using natural language description".to_string()),
            category: "builtin".to_string(),
            config: None,
            schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Natural language description of the photo" }
                },
                "required": ["query"]
            })),
        },
        NewToolDef {
            id: "builtin:move_file".to_string(),
            name: "Move File".to_string(),
            description: Some("Move a photo file to a different location".to_string()),
            category: "builtin".to_string(),
            config: None,
            schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "photo_id": { "type": "integer", "description": "ID of the photo to move" },
                    "destination": { "type": "string", "description": "Destination file path" }
                },
                "required": ["photo_id", "destination"]
            })),
        },
        NewToolDef {
            id: "builtin:create_folder".to_string(),
            name: "Create Folder".to_string(),
            description: Some("Create a new folder".to_string()),
            category: "builtin".to_string(),
            config: None,
            schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path of the folder to create" }
                },
                "required": ["path"]
            })),
        },
        NewToolDef {
            id: "builtin:get_photo_info".to_string(),
            name: "Get Photo Info".to_string(),
            description: Some("Get detailed information about a photo".to_string()),
            category: "builtin".to_string(),
            config: None,
            schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "photo_id": { "type": "integer", "description": "ID of the photo" }
                },
                "required": ["photo_id"]
            })),
        },
    ];

    for tool in &builtin_tools {
        ToolRepo::upsert(db.pool(), tool).await?;
    }

    tracing::info!("Registered {} built-in tools", builtin_tools.len());
    Ok(())
}
