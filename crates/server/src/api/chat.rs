use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::AppState;
use photomind_core::agent::engine::AgentEngine;
use photomind_core::agent::types::{AgentMessage, Role};
use photomind_storage::models::NewChatMessage;
use photomind_storage::repo::chat::ChatRepo;
use photomind_storage::repo::configs::ConfigRepo;
use photomind_storage::repo::tools::ToolRepo;

#[derive(Deserialize)]
pub struct ChatRequest {
    pub session_id: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct ChatResponse {
    pub content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallResponse>,
}

#[derive(Serialize)]
pub struct ToolCallResponse {
    pub execution_id: i64,
    pub tool_name: String,
    pub params: serde_json::Value,
}

async fn get_agent_engine(state: &AppState) -> Option<AgentEngine> {
    let pool = state.db.pool();
    let provider = ConfigRepo::get(pool, "agent_provider")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(String::from));
    let url = ConfigRepo::get(pool, "agent_url")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(String::from));
    let key = ConfigRepo::get(pool, "agent_key")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(String::from));
    let model = ConfigRepo::get(pool, "agent_model")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(String::from));

    AgentEngine::from_config(
        provider.as_deref(),
        url.as_deref(),
        key.as_deref(),
        model.as_deref(),
    )
}

pub async fn chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, StatusCode> {
    let pool = state.db.pool();

    // Save user message
    ChatRepo::insert(
        pool,
        &NewChatMessage {
            session_id: req.session_id.clone(),
            role: "user".to_string(),
            content: req.message.clone(),
            metadata: None,
        },
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Try to get agent engine
    let engine = match get_agent_engine(&state).await {
        Some(e) => e,
        None => {
            let content =
                "Agent not configured yet. Please set up an Agent model in Settings.".to_string();
            ChatRepo::insert(
                pool,
                &NewChatMessage {
                    session_id: req.session_id,
                    role: "assistant".to_string(),
                    content: content.clone(),
                    metadata: None,
                },
            )
            .await
            .ok();
            return Ok(Json(ChatResponse {
                content,
                tool_calls: vec![],
            }));
        }
    };

    // Load chat history
    let db_messages = ChatRepo::get_session_messages(pool, &req.session_id, 50)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Convert to agent messages (exclude the just-inserted user message, it's passed separately)
    let history: Vec<AgentMessage> = db_messages
        .iter()
        .filter(|m| m.id != db_messages.last().map(|l| l.id).unwrap_or(0)) // skip last (current user msg)
        .map(|m| AgentMessage {
            role: match m.role.as_str() {
                "user" => Role::User,
                "assistant" => Role::Assistant,
                "system" => Role::System,
                _ => Role::Tool,
            },
            content: m.content.clone(),
            tool_call_id: None,
        })
        .collect();

    // Get enabled tools
    let enabled_tools = ToolRepo::list_enabled(pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Call agent
    let response = engine
        .chat(&history, &req.message, &enabled_tools)
        .await
        .map_err(|e| {
            tracing::error!("Agent error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let content = response.content.unwrap_or_default();

    // Process tool calls — create pending executions
    let mut tool_call_responses = Vec::new();
    for tc in &response.tool_calls {
        // Map the tool function name back to tool ID
        let tool_id = tc.name.replace('_', ":");

        let exec_id = sqlx::query(
            "INSERT INTO tool_executions (tool_id, params, status) VALUES (?, ?, 'pending_confirm')",
        )
        .bind(&tool_id)
        .bind(serde_json::to_string(&tc.arguments).unwrap_or_default())
        .execute(pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .last_insert_rowid();

        tool_call_responses.push(ToolCallResponse {
            execution_id: exec_id,
            tool_name: tc.name.clone(),
            params: tc.arguments.clone(),
        });
    }

    // Save assistant message
    let metadata = if !tool_call_responses.is_empty() {
        Some(serde_json::to_value(&tool_call_responses).unwrap_or_default())
    } else {
        None
    };

    ChatRepo::insert(
        pool,
        &NewChatMessage {
            session_id: req.session_id,
            role: "assistant".to_string(),
            content: content.clone(),
            metadata,
        },
    )
    .await
    .ok();

    Ok(Json(ChatResponse {
        content,
        tool_calls: tool_call_responses,
    }))
}

#[derive(Deserialize)]
pub struct ConfirmToolRequest {
    pub execution_id: i64,
    pub confirmed: bool,
}

#[derive(Serialize)]
pub struct ConfirmToolResponse {
    pub status: String,
    pub result: Option<serde_json::Value>,
}

pub async fn confirm_tool(
    State(state): State<AppState>,
    Json(req): Json<ConfirmToolRequest>,
) -> Result<Json<ConfirmToolResponse>, StatusCode> {
    let pool = state.db.pool();

    // Get the execution
    let exec: Option<(String, String, String)> = sqlx::query_as(
        "SELECT tool_id, params, status FROM tool_executions WHERE id = ?",
    )
    .bind(req.execution_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (tool_id, params_str, status) = exec.ok_or(StatusCode::NOT_FOUND)?;

    if status != "pending_confirm" {
        return Ok(Json(ConfirmToolResponse {
            status: format!("already_{}", status),
            result: None,
        }));
    }

    if !req.confirmed {
        sqlx::query(
            "UPDATE tool_executions SET status = 'cancelled', confirmed_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(req.execution_id)
        .execute(pool)
        .await
        .ok();

        return Ok(Json(ConfirmToolResponse {
            status: "cancelled".to_string(),
            result: None,
        }));
    }

    // Execute the tool
    let params: serde_json::Value =
        serde_json::from_str(&params_str).unwrap_or(serde_json::json!({}));
    let result = execute_builtin_tool(&state, &tool_id, &params).await;

    let (status_str, result_val) = match result {
        Ok(val) => ("executed".to_string(), Some(val)),
        Err(e) => ("failed".to_string(), Some(serde_json::json!({ "error": e.to_string() }))),
    };

    sqlx::query(
        "UPDATE tool_executions SET status = ?, result = ?, confirmed_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(&status_str)
    .bind(result_val.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()))
    .bind(req.execution_id)
    .execute(pool)
    .await
    .ok();

    Ok(Json(ConfirmToolResponse {
        status: status_str,
        result: result_val,
    }))
}

async fn execute_builtin_tool(
    state: &AppState,
    tool_id: &str,
    params: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    match tool_id {
        "builtin:search_photos" => {
            let query = params["query"].as_str().unwrap_or("");
            // Use embedding client to search
            let client_opt = {
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
                photomind_core::embedding::EmbeddingClient::from_config(
                    url.as_deref(),
                    key.as_deref(),
                    model.as_deref(),
                )
            };

            if let Some(client) = client_opt {
                let vector = client.embed_text(query).await?;
                let hits = state.vector_index.search(&vector, 10);
                let mut results = Vec::new();
                for hit in &hits {
                    if let Ok(photo) =
                        photomind_storage::repo::photos::PhotoRepo::get_by_id(state.db.pool(), hit.photo_id).await
                    {
                        results.push(serde_json::json!({
                            "id": photo.id,
                            "file_name": photo.file_name,
                            "file_path": photo.file_path,
                            "score": hit.score,
                        }));
                    }
                }
                Ok(serde_json::json!({ "results": results, "count": results.len() }))
            } else {
                Ok(serde_json::json!({ "error": "Embedding model not configured" }))
            }
        }
        "builtin:move_file" => {
            let photo_id = params["photo_id"].as_i64().ok_or_else(|| anyhow::anyhow!("photo_id required"))?;
            let dest = params["destination"].as_str().ok_or_else(|| anyhow::anyhow!("destination required"))?;

            let photo =
                photomind_storage::repo::photos::PhotoRepo::get_by_id(state.db.pool(), photo_id).await?;

            // Ensure destination directory exists
            if let Some(parent) = std::path::Path::new(dest).parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            // Move the file
            tokio::fs::rename(&photo.file_path, dest).await?;

            // Update database
            photomind_storage::repo::photos::PhotoRepo::update_path(state.db.pool(), photo_id, dest)
                .await?;

            Ok(serde_json::json!({
                "success": true,
                "from": photo.file_path,
                "to": dest,
            }))
        }
        "builtin:create_folder" => {
            let path = params["path"].as_str().ok_or_else(|| anyhow::anyhow!("path required"))?;
            tokio::fs::create_dir_all(path).await?;
            Ok(serde_json::json!({ "success": true, "path": path }))
        }
        "builtin:get_photo_info" => {
            let photo_id = params["photo_id"].as_i64().ok_or_else(|| anyhow::anyhow!("photo_id required"))?;
            let photo =
                photomind_storage::repo::photos::PhotoRepo::get_by_id(state.db.pool(), photo_id).await?;
            Ok(serde_json::to_value(&photo)?)
        }
        _ => {
            // External tool execution
            execute_external_tool(state, tool_id, params).await
        }
    }
}

async fn execute_external_tool(
    state: &AppState,
    tool_id: &str,
    params: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let tool = photomind_storage::repo::tools::ToolRepo::get(state.db.pool(), tool_id).await?;
    let config = tool.config.ok_or_else(|| anyhow::anyhow!("Tool has no config"))?;

    let tool_type = config["type"].as_str().unwrap_or("http");

    match tool_type {
        "http" => {
            let method = config["method"].as_str().unwrap_or("POST");
            let url_template = config["url"].as_str().unwrap_or("");
            let headers = config["headers"].as_object();
            let body_template = &config["body"];

            // Simple template substitution: replace {param_name} with param values
            let url = substitute_template(url_template, params);

            let body = substitute_json_template(body_template, params);

            let client = reqwest::Client::new();
            let mut req_builder = match method.to_uppercase().as_str() {
                "GET" => client.get(&url),
                "PUT" => client.put(&url),
                "DELETE" => client.delete(&url),
                "PATCH" => client.patch(&url),
                _ => client.post(&url),
            };

            if let Some(hdrs) = headers {
                for (k, v) in hdrs {
                    if let Some(v_str) = v.as_str() {
                        let v_subst = substitute_template(v_str, params);
                        req_builder = req_builder.header(k.as_str(), v_subst);
                    }
                }
            }

            req_builder = req_builder.json(&body);
            let resp = req_builder.send().await?;
            let status = resp.status();
            let resp_body: serde_json::Value = resp.json().await.unwrap_or(serde_json::json!({}));

            Ok(serde_json::json!({
                "status": status.as_u16(),
                "body": resp_body,
            }))
        }
        "cli" => {
            let command_template = config["command"].as_str().unwrap_or("");
            let command = substitute_template(command_template, params);

            let output = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .output()
                .await?;

            Ok(serde_json::json!({
                "exit_code": output.status.code(),
                "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
                "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            }))
        }
        _ => Err(anyhow::anyhow!("Unknown tool type: {}", tool_type)),
    }
}

fn substitute_template(template: &str, params: &serde_json::Value) -> String {
    let mut result = template.to_string();
    if let Some(obj) = params.as_object() {
        for (k, v) in obj {
            let placeholder = format!("{{{}}}", k);
            let value_str = match v {
                serde_json::Value::String(s) => s.clone(),
                _ => v.to_string(),
            };
            result = result.replace(&placeholder, &value_str);
        }
    }
    result
}

fn substitute_json_template(
    template: &serde_json::Value,
    params: &serde_json::Value,
) -> serde_json::Value {
    match template {
        serde_json::Value::String(s) => {
            serde_json::Value::String(substitute_template(s, params))
        }
        serde_json::Value::Object(obj) => {
            let mut new_obj = serde_json::Map::new();
            for (k, v) in obj {
                new_obj.insert(k.clone(), substitute_json_template(v, params));
            }
            serde_json::Value::Object(new_obj)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(|v| substitute_json_template(v, params)).collect())
        }
        other => other.clone(),
    }
}
