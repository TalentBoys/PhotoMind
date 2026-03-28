use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

// ── Photo ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Photo {
    pub id: i64,
    pub file_path: String,
    pub file_name: String,
    pub file_size: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub format: Option<String>,
    pub taken_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub file_hash: Option<String>,
    pub embedded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewPhoto {
    pub file_path: String,
    pub file_name: String,
    pub file_size: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub format: Option<String>,
    pub taken_at: Option<NaiveDateTime>,
    pub file_hash: Option<String>,
}

// ── Embedding ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Embedding {
    pub id: i64,
    pub photo_id: i64,
    pub vector: Vec<f32>,
    pub model_name: String,
    pub created_at: NaiveDateTime,
}

// ── Config ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub key: String,
    pub value: serde_json::Value,
    pub updated_at: NaiveDateTime,
}

// ── Tool Definition ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub category: String, // "builtin" or "external"
    pub enabled: bool,
    pub config: Option<serde_json::Value>,
    pub schema: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewToolDef {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub category: String,
    pub config: Option<serde_json::Value>,
    pub schema: Option<serde_json::Value>,
}

// ── Tool Execution ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecution {
    pub id: i64,
    pub tool_id: String,
    pub params: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub status: String, // pending_confirm / confirmed / executed / failed
    pub created_at: NaiveDateTime,
    pub confirmed_at: Option<NaiveDateTime>,
}

// ── Chat Message ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: i64,
    pub session_id: String,
    pub role: String, // user / assistant / system / tool
    pub content: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewChatMessage {
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub metadata: Option<serde_json::Value>,
}
