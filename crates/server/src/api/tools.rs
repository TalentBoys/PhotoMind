use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use crate::AppState;
use photomind_storage::repo::tools::ToolRepo;
use photomind_storage::models::{NewToolDef, ToolDef};

pub async fn list_tools(
    State(state): State<AppState>,
) -> Result<Json<Vec<ToolDef>>, StatusCode> {
    let tools = ToolRepo::list(state.db.pool())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(tools))
}

#[derive(Deserialize)]
pub struct ToggleToolRequest {
    pub enabled: bool,
}

pub async fn toggle_tool(
    State(state): State<AppState>,
    Path(tool_id): Path<String>,
    Json(req): Json<ToggleToolRequest>,
) -> Result<StatusCode, StatusCode> {
    ToolRepo::set_enabled(state.db.pool(), &tool_id, req.enabled)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

pub async fn create_tool(
    State(state): State<AppState>,
    Json(tool): Json<NewToolDef>,
) -> Result<StatusCode, StatusCode> {
    ToolRepo::upsert(state.db.pool(), &tool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::CREATED)
}

pub async fn delete_tool(
    State(state): State<AppState>,
    Path(tool_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    ToolRepo::delete(state.db.pool(), &tool_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}
