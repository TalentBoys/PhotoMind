use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::AppState;
use photomind_core::agent::engine::AgentEngine;
use photomind_core::agent::types::{AgentMessage, AgentToolCall, Role, ToolDefinition};
use photomind_storage::models::NewChatMessage;
use photomind_storage::repo::chat::ChatRepo;
use photomind_storage::repo::configs::ConfigRepo;
use photomind_storage::repo::tools::ToolRepo;

const MAX_LOOP_ITERATIONS: usize = 10;

// ── Request / Response types ──

#[derive(Deserialize)]
pub struct ChatRequest {
    pub session_id: String,
    pub message: String,
    #[serde(default)]
    pub auto_approve_tools: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct ChatResponse {
    pub content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallResponse>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub auto_results: Vec<AutoToolResult>,
    pub done: bool,
}

#[derive(Serialize, Clone)]
pub struct ToolCallResponse {
    pub execution_id: i64,
    pub tool_name: String,
    pub params: serde_json::Value,
}

#[derive(Serialize, Clone)]
pub struct AutoToolResult {
    pub tool_name: String,
    pub params: serde_json::Value,
    pub result: serde_json::Value,
}

#[derive(Deserialize)]
pub struct ContinueRequest {
    pub session_id: String,
    pub tool_results: Vec<ToolResultInput>,
    #[serde(default)]
    pub auto_approve_tools: Vec<String>,
}

#[derive(Deserialize)]
pub struct ToolResultInput {
    pub execution_id: i64,
    pub confirmed: bool,
}

// ── Helpers ──

async fn get_agent_engine(state: &AppState) -> Option<AgentEngine> {
    let pool = state.db.pool();
    let provider = ConfigRepo::get(pool, "agent_provider")
        .await.ok().flatten().and_then(|v| v.as_str().map(String::from));
    let url = ConfigRepo::get(pool, "agent_url")
        .await.ok().flatten().and_then(|v| v.as_str().map(String::from));
    let key = ConfigRepo::get(pool, "agent_key")
        .await.ok().flatten().and_then(|v| v.as_str().map(String::from));
    let model = ConfigRepo::get(pool, "agent_model")
        .await.ok().flatten().and_then(|v| v.as_str().map(String::from));

    AgentEngine::from_config(
        provider.as_deref(), url.as_deref(), key.as_deref(), model.as_deref(),
    )
}

fn tool_name_to_id(name: &str) -> String {
    if let Some(pos) = name.find('_') {
        format!("{}:{}", &name[..pos], &name[pos + 1..])
    } else {
        name.to_string()
    }
}

fn db_messages_to_agent(msgs: &[photomind_storage::models::ChatMessage]) -> Vec<AgentMessage> {
    msgs.iter().map(|m| {
        let raw_content = if m.role == "assistant" {
            m.metadata.as_ref().and_then(|meta| meta.get("raw_content").cloned())
        } else {
            None
        };
        AgentMessage {
            role: match m.role.as_str() {
                "user" => Role::User,
                "assistant" => Role::Assistant,
                "system" => Role::System,
                _ => Role::Tool,
            },
            content: m.content.clone(),
            tool_call_id: if m.role == "tool" {
                m.metadata.as_ref().and_then(|meta| meta["tool_call_id"].as_str().map(String::from))
            } else {
                None
            },
            raw_content,
        }
    }).collect()
}

async fn save_assistant_msg(
    pool: &sqlx::SqlitePool,
    session_id: &str,
    content: &str,
    raw_content: Option<&serde_json::Value>,
    tool_calls_meta: Option<serde_json::Value>,
) {
    let mut metadata = serde_json::Map::new();
    if let Some(rc) = raw_content {
        metadata.insert("raw_content".to_string(), rc.clone());
    }
    if let Some(tc) = tool_calls_meta {
        metadata.insert("tool_calls".to_string(), tc);
    }
    let meta_val = if metadata.is_empty() { None } else { Some(serde_json::Value::Object(metadata)) };

    ChatRepo::insert(pool, &NewChatMessage {
        session_id: session_id.to_string(),
        role: "assistant".to_string(),
        content: content.to_string(),
        metadata: meta_val,
    }).await.ok();
}

async fn save_tool_msg(
    pool: &sqlx::SqlitePool,
    session_id: &str,
    tool_call_id: &str,
    result_content: &str,
) {
    let metadata = serde_json::json!({ "tool_call_id": tool_call_id });
    ChatRepo::insert(pool, &NewChatMessage {
        session_id: session_id.to_string(),
        role: "tool".to_string(),
        content: result_content.to_string(),
        metadata: Some(metadata),
    }).await.ok();
}

/// Run the agent loop: call LLM, execute auto-approved tools, loop until done or needs confirmation.
async fn run_agent_loop(
    state: &AppState,
    engine: &AgentEngine,
    messages: &mut Vec<AgentMessage>,
    tool_defs: &[ToolDefinition],
    auto_approve: &[String],
    session_id: &str,
) -> Result<ChatResponse, StatusCode> {
    let pool = state.db.pool();
    let mut all_auto_results: Vec<AutoToolResult> = Vec::new();
    let mut final_content = String::new();

    for iteration in 0..MAX_LOOP_ITERATIONS {
        tracing::info!("Agent loop iteration {}", iteration);

        let response = engine.call(messages, tool_defs).await.map_err(|e| {
            tracing::error!("Agent error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        let content = response.content.clone().unwrap_or_default();
        if !content.is_empty() {
            final_content = content.clone();
        }

        // No tool calls — done
        if response.tool_calls.is_empty() {
            save_assistant_msg(pool, session_id, &content, None, None).await;
            return Ok(ChatResponse {
                content: final_content,
                tool_calls: vec![],
                auto_results: all_auto_results,
                done: true,
            });
        }

        // Has tool calls — separate into auto-approved and pending
        let mut auto_calls: Vec<&AgentToolCall> = Vec::new();
        let mut pending_calls: Vec<&AgentToolCall> = Vec::new();

        for tc in &response.tool_calls {
            if auto_approve.contains(&tc.name) {
                auto_calls.push(tc);
            } else {
                pending_calls.push(tc);
            }
        }

        // Save assistant message with raw_content (for replay) and tool_calls info
        let tc_meta = serde_json::to_value(&response.tool_calls.iter().map(|tc| {
            serde_json::json!({ "id": tc.id, "name": tc.name, "arguments": tc.arguments })
        }).collect::<Vec<_>>()).ok();
        save_assistant_msg(pool, session_id, &content, response.raw_content.as_ref(), tc_meta).await;

        // Add the assistant message to conversation
        messages.push(AgentMessage {
            role: Role::Assistant,
            content: content.clone(),
            tool_call_id: None,
            raw_content: response.raw_content.clone(),
        });

        // Execute auto-approved tools
        for tc in &auto_calls {
            let tool_id = tool_name_to_id(&tc.name);
            let result = execute_builtin_tool(state, &tool_id, &tc.arguments).await;
            let result_val = match result {
                Ok(v) => v,
                Err(e) => serde_json::json!({ "error": e.to_string() }),
            };
            let result_str = serde_json::to_string(&result_val).unwrap_or_default();

            // Save tool result to DB
            save_tool_msg(pool, session_id, &tc.id, &result_str).await;

            // Add tool result to conversation
            messages.push(AgentMessage {
                role: Role::Tool,
                content: result_str,
                tool_call_id: Some(tc.id.clone()),
                raw_content: None,
            });

            all_auto_results.push(AutoToolResult {
                tool_name: tc.name.clone(),
                params: tc.arguments.clone(),
                result: result_val,
            });
        }

        // If there are pending (needs confirmation) tool calls, pause and return
        if !pending_calls.is_empty() {
            let mut pending_responses = Vec::new();
            for tc in &pending_calls {
                let tool_id = tool_name_to_id(&tc.name);
                let exec_id = sqlx::query(
                    "INSERT INTO tool_executions (tool_id, tool_call_id, params, status) VALUES (?, ?, ?, 'pending_confirm')",
                )
                .bind(&tool_id)
                .bind(&tc.id)
                .bind(serde_json::to_string(&tc.arguments).unwrap_or_default())
                .execute(pool)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to insert tool_execution: {:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?
                .last_insert_rowid();

                pending_responses.push(ToolCallResponse {
                    execution_id: exec_id,
                    tool_name: tc.name.clone(),
                    params: tc.arguments.clone(),
                });
            }

            return Ok(ChatResponse {
                content: final_content,
                tool_calls: pending_responses,
                auto_results: all_auto_results,
                done: false,
            });
        }

        // All tools were auto-approved and executed — loop continues, LLM will see results
    }

    // Max iterations reached
    save_assistant_msg(pool, session_id, &final_content, None, None).await;
    Ok(ChatResponse {
        content: final_content,
        tool_calls: vec![],
        auto_results: all_auto_results,
        done: true,
    })
}

// ── POST /api/chat ──

pub async fn chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, StatusCode> {
    let pool = state.db.pool();

    // Load chat history BEFORE inserting the new user message
    let db_messages = ChatRepo::get_session_messages(pool, &req.session_id, 50)
        .await.map_err(|e| {
            tracing::error!("Failed to load chat history: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Save user message
    ChatRepo::insert(pool, &NewChatMessage {
        session_id: req.session_id.clone(),
        role: "user".to_string(),
        content: req.message.clone(),
        metadata: None,
    }).await.map_err(|e| {
        tracing::error!("Failed to save user message: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Try to get agent engine
    let engine = match get_agent_engine(&state).await {
        Some(e) => e,
        None => {
            let content = "Agent not configured yet. Please set up an Agent model in Settings.".to_string();
            save_assistant_msg(pool, &req.session_id, &content, None, None).await;
            return Ok(Json(ChatResponse {
                content,
                tool_calls: vec![],
                auto_results: vec![],
                done: true,
            }));
        }
    };

    let enabled_tools = ToolRepo::list_enabled(pool).await.map_err(|e| {
        tracing::error!("Failed to load enabled tools: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let tool_defs = AgentEngine::build_tool_definitions(&enabled_tools);

    // Build message list
    let mut messages = vec![AgentEngine::system_message()];
    messages.extend(db_messages_to_agent(&db_messages));
    messages.push(AgentMessage {
        role: Role::User,
        content: req.message.clone(),
        tool_call_id: None,
        raw_content: None,
    });

    let response = run_agent_loop(
        &state, &engine, &mut messages, &tool_defs, &req.auto_approve_tools, &req.session_id,
    ).await?;

    Ok(Json(response))
}

// ── POST /api/chat/continue ──

pub async fn continue_chat(
    State(state): State<AppState>,
    Json(req): Json<ContinueRequest>,
) -> Result<Json<ChatResponse>, StatusCode> {
    let pool = state.db.pool();

    // Execute or cancel each tool
    let mut tool_results: Vec<(String, String)> = Vec::new(); // (tool_call_id, result_json)
    let mut confirmed_results: Vec<AutoToolResult> = Vec::new(); // results to return to frontend

    for tr in &req.tool_results {
        let exec: Option<(String, Option<String>, String, String)> = sqlx::query_as(
            "SELECT tool_id, tool_call_id, params, status FROM tool_executions WHERE id = ?",
        )
        .bind(tr.execution_id)
        .fetch_optional(pool)
        .await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let (tool_id, tool_call_id, params_str, status) = match exec {
            Some(e) => e,
            None => continue,
        };

        let tc_id = tool_call_id.unwrap_or_else(|| format!("exec_{}", tr.execution_id));

        if status != "pending_confirm" { continue; }

        if !tr.confirmed {
            sqlx::query("UPDATE tool_executions SET status = 'cancelled', confirmed_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(tr.execution_id).execute(pool).await.ok();
            tool_results.push((tc_id, serde_json::json!({"cancelled": true}).to_string()));
            continue;
        }

        let params: serde_json::Value = serde_json::from_str(&params_str).unwrap_or(serde_json::json!({}));
        let result = execute_builtin_tool(&state, &tool_id, &params).await;

        let (status_str, result_val) = match result {
            Ok(val) => ("executed".to_string(), val),
            Err(e) => ("failed".to_string(), serde_json::json!({ "error": e.to_string() })),
        };

        sqlx::query("UPDATE tool_executions SET status = ?, result = ?, confirmed_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(&status_str)
            .bind(serde_json::to_string(&result_val).unwrap_or_default())
            .bind(tr.execution_id)
            .execute(pool).await.ok();

        // Return confirmed tool results to frontend so it can render them immediately
        confirmed_results.push(AutoToolResult {
            tool_name: tool_id.replace(':', "_"),
            params: params.clone(),
            result: result_val.clone(),
        });

        tool_results.push((tc_id, serde_json::to_string(&result_val).unwrap_or_default()));
    }

    // Save tool result messages to DB
    for (tool_call_id, result_str) in &tool_results {
        save_tool_msg(pool, &req.session_id, tool_call_id, result_str).await;
    }

    // Get engine
    let engine = get_agent_engine(&state).await.ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let enabled_tools = ToolRepo::list_enabled(pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let tool_defs = AgentEngine::build_tool_definitions(&enabled_tools);

    // Rebuild full message list from DB (now includes the tool results we just saved)
    let all_db_messages = ChatRepo::get_session_messages(pool, &req.session_id, 100)
        .await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut messages = vec![AgentEngine::system_message()];
    messages.extend(db_messages_to_agent(&all_db_messages));

    let mut response = run_agent_loop(
        &state, &engine, &mut messages, &tool_defs, &req.auto_approve_tools, &req.session_id,
    ).await?;

    // Prepend confirmed tool results so frontend can render them (e.g. photo grids)
    let mut all_auto = confirmed_results;
    all_auto.append(&mut response.auto_results);
    response.auto_results = all_auto;

    Ok(Json(response))
}

// ── Legacy confirm-tool (kept for backwards compat) ──

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

    let exec: Option<(String, String, String)> = sqlx::query_as(
        "SELECT tool_id, params, status FROM tool_executions WHERE id = ?",
    )
    .bind(req.execution_id)
    .fetch_optional(pool)
    .await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (tool_id, params_str, status) = exec.ok_or(StatusCode::NOT_FOUND)?;

    if status != "pending_confirm" {
        return Ok(Json(ConfirmToolResponse {
            status: format!("already_{}", status),
            result: None,
        }));
    }

    if !req.confirmed {
        sqlx::query("UPDATE tool_executions SET status = 'cancelled', confirmed_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(req.execution_id).execute(pool).await.ok();
        return Ok(Json(ConfirmToolResponse { status: "cancelled".to_string(), result: None }));
    }

    let params: serde_json::Value = serde_json::from_str(&params_str).unwrap_or(serde_json::json!({}));
    let result = execute_builtin_tool(&state, &tool_id, &params).await;

    let (status_str, result_val) = match result {
        Ok(val) => ("executed".to_string(), Some(val)),
        Err(e) => ("failed".to_string(), Some(serde_json::json!({ "error": e.to_string() }))),
    };

    sqlx::query("UPDATE tool_executions SET status = ?, result = ?, confirmed_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(&status_str)
        .bind(result_val.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()))
        .bind(req.execution_id)
        .execute(pool).await.ok();

    Ok(Json(ConfirmToolResponse { status: status_str, result: result_val }))
}

// ── Tool execution ──

async fn execute_builtin_tool(
    state: &AppState,
    tool_id: &str,
    params: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    match tool_id {
        "builtin:search_photos" => {
            let query = params["query"].as_str().unwrap_or("");
            let client_opt = {
                let pool = state.db.pool();
                let url = ConfigRepo::get(pool, "embedding_url").await.ok().flatten().and_then(|v| v.as_str().map(String::from));
                let key = ConfigRepo::get(pool, "embedding_key").await.ok().flatten().and_then(|v| v.as_str().map(String::from));
                let model = ConfigRepo::get(pool, "embedding_model").await.ok().flatten().and_then(|v| v.as_str().map(String::from));
                let dimension = ConfigRepo::get(pool, "embedding_dimension").await.ok().flatten().and_then(|v| v.as_u64().map(|n| n as u32));
                photomind_core::embedding::EmbeddingClient::from_config(url.as_deref(), key.as_deref(), model.as_deref(), dimension)
            };

            if let Some(client) = client_opt {
                let (vector, _) = client.embed_text(query).await?;
                let hits = state.vector_index.search(&vector, 10);
                let mut results = Vec::new();
                for hit in &hits {
                    if let Ok(photo) = photomind_storage::repo::photos::PhotoRepo::get_by_id(state.db.pool(), hit.photo_id).await {
                        results.push(serde_json::json!({
                            "id": photo.id, "file_name": photo.file_name,
                            "file_path": photo.file_path, "score": hit.score,
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
            let photo = photomind_storage::repo::photos::PhotoRepo::get_by_id(state.db.pool(), photo_id).await?;
            if let Some(parent) = std::path::Path::new(dest).parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::rename(&photo.file_path, dest).await?;
            photomind_storage::repo::photos::PhotoRepo::update_path(state.db.pool(), photo_id, dest).await?;
            Ok(serde_json::json!({ "success": true, "from": photo.file_path, "to": dest }))
        }
        "builtin:create_folder" => {
            let path = params["path"].as_str().ok_or_else(|| anyhow::anyhow!("path required"))?;
            tokio::fs::create_dir_all(path).await?;
            Ok(serde_json::json!({ "success": true, "path": path }))
        }
        "builtin:get_photo_info" => {
            let photo_id = params["photo_id"].as_i64().ok_or_else(|| anyhow::anyhow!("photo_id required"))?;
            let photo = photomind_storage::repo::photos::PhotoRepo::get_by_id(state.db.pool(), photo_id).await?;
            Ok(serde_json::to_value(&photo)?)
        }
        _ => execute_external_tool(state, tool_id, params).await,
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
                        req_builder = req_builder.header(k.as_str(), substitute_template(v_str, params));
                    }
                }
            }
            req_builder = req_builder.json(&body);
            let resp = req_builder.send().await?;
            let status = resp.status();
            let resp_body: serde_json::Value = resp.json().await.unwrap_or(serde_json::json!({}));
            Ok(serde_json::json!({ "status": status.as_u16(), "body": resp_body }))
        }
        "cli" => {
            let command_template = config["command"].as_str().unwrap_or("");
            let command = substitute_template(command_template, params);
            let output = tokio::process::Command::new("sh").arg("-c").arg(&command).output().await?;
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

fn substitute_json_template(template: &serde_json::Value, params: &serde_json::Value) -> serde_json::Value {
    match template {
        serde_json::Value::String(s) => serde_json::Value::String(substitute_template(s, params)),
        serde_json::Value::Object(obj) => {
            let mut new_obj = serde_json::Map::new();
            for (k, v) in obj { new_obj.insert(k.clone(), substitute_json_template(v, params)); }
            serde_json::Value::Object(new_obj)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(|v| substitute_json_template(v, params)).collect())
        }
        other => other.clone(),
    }
}

// ── Session management ──

#[derive(Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub title: String,
    pub last_message_at: String,
}

pub async fn list_sessions(
    State(state): State<AppState>,
) -> Result<Json<Vec<SessionInfo>>, StatusCode> {
    let pool = state.db.pool();
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT
            cm.session_id,
            COALESCE(
                (SELECT SUBSTR(content, 1, 50) FROM chat_messages
                 WHERE session_id = cm.session_id AND role = 'user'
                 ORDER BY created_at ASC LIMIT 1),
                'New Chat'
            ) as title,
            MAX(cm.created_at) as last_at
         FROM chat_messages cm
         GROUP BY cm.session_id
         ORDER BY last_at DESC",
    ).fetch_all(pool).await.map_err(|e| {
        tracing::error!("Failed to list sessions: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(rows.into_iter().map(|(session_id, title, last_message_at)| SessionInfo {
        session_id, title, last_message_at,
    }).collect()))
}

#[derive(Serialize)]
pub struct ChatMessageResponse {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: String,
}

pub async fn get_session_messages(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<Vec<ChatMessageResponse>>, StatusCode> {
    let pool = state.db.pool();
    let messages = ChatRepo::get_session_messages(pool, &session_id, 200)
        .await.map_err(|e| {
            tracing::error!("Failed to load session messages: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(messages.into_iter().map(|m| ChatMessageResponse {
        id: m.id, role: m.role, content: m.content, metadata: m.metadata,
        created_at: m.created_at.to_string(),
    }).collect()))
}

pub async fn delete_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let pool = state.db.pool();
    sqlx::query("DELETE FROM tool_executions WHERE id IN (SELECT te.id FROM tool_executions te JOIN chat_messages cm ON cm.session_id = ? WHERE cm.metadata IS NOT NULL)")
        .bind(&session_id).execute(pool).await.ok();
    sqlx::query("DELETE FROM chat_messages WHERE session_id = ?")
        .bind(&session_id).execute(pool).await.map_err(|e| {
            tracing::error!("Failed to delete session: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}
