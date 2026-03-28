use super::provider::AgentProvider;
use super::types::*;
use anyhow::Result;
use photomind_storage::models::ToolDef;

const SYSTEM_PROMPT: &str = r#"You are PhotoMind, an AI-powered photo search assistant.

You can:
1. Search for photos using natural language descriptions (use the search_photos tool)
2. Show photo details and information
3. Help users organize photos using available tools (move files, create folders, etc.)

Rules:
- NEVER delete photos. If a user asks to delete, suggest moving them to a specific folder (e.g. "/photos/to-delete/") so they can manually review and delete.
- All operations that modify files require user confirmation. When you want to perform an action, use the appropriate tool - the system will ask the user to confirm before executing.
- When showing search results, describe what you found and let the user know they can see the photos in the search results.
- Be helpful, concise, and focused on photo management tasks.
- If tools are not available or not configured, let the user know what they need to set up.
"#;

pub struct AgentEngine {
    provider: AgentProvider,
}

impl AgentEngine {
    pub fn new(provider: AgentProvider) -> Self {
        Self { provider }
    }

    pub fn from_config(
        provider_name: Option<&str>,
        base_url: Option<&str>,
        api_key: Option<&str>,
        model: Option<&str>,
    ) -> Option<Self> {
        AgentProvider::from_config(provider_name, base_url, api_key, model)
            .map(|p| Self::new(p))
    }

    /// Run one turn of the agent conversation.
    /// Returns the text response and any tool calls that need user confirmation.
    pub async fn chat(
        &self,
        history: &[AgentMessage],
        user_message: &str,
        enabled_tools: &[ToolDef],
    ) -> Result<AgentResponse> {
        let mut messages = vec![AgentMessage {
            role: Role::System,
            content: SYSTEM_PROMPT.to_string(),
            tool_call_id: None,
        }];

        // Add conversation history
        messages.extend_from_slice(history);

        // Add the new user message
        messages.push(AgentMessage {
            role: Role::User,
            content: user_message.to_string(),
            tool_call_id: None,
        });

        // Build tool definitions from enabled tools
        let tool_defs = build_tool_definitions(enabled_tools);

        // Call the LLM
        let response = self.provider.chat(&messages, &tool_defs).await?;

        // Check for delete intent in tool calls
        let filtered_tool_calls = filter_delete_intent(response.tool_calls);

        Ok(AgentResponse {
            content: response.content,
            tool_calls: filtered_tool_calls,
        })
    }

    /// Continue conversation after tool execution results.
    pub async fn continue_with_tool_results(
        &self,
        history: &[AgentMessage],
        enabled_tools: &[ToolDef],
    ) -> Result<AgentResponse> {
        let mut messages = vec![AgentMessage {
            role: Role::System,
            content: SYSTEM_PROMPT.to_string(),
            tool_call_id: None,
        }];

        messages.extend_from_slice(history);

        let tool_defs = build_tool_definitions(enabled_tools);
        self.provider.chat(&messages, &tool_defs).await
    }
}

fn build_tool_definitions(tools: &[ToolDef]) -> Vec<ToolDefinition> {
    tools
        .iter()
        .map(|t| {
            // Use the tool ID as the function name (sanitized)
            let name = t.id.replace(':', "_");
            ToolDefinition {
                name,
                description: t.description.clone().unwrap_or_default(),
                parameters: t.schema.clone().unwrap_or(serde_json::json!({
                    "type": "object",
                    "properties": {}
                })),
            }
        })
        .collect()
}

/// Filter out any tool calls that look like delete operations.
/// Replace them with a message suggesting to move instead.
fn filter_delete_intent(tool_calls: Vec<AgentToolCall>) -> Vec<AgentToolCall> {
    tool_calls
        .into_iter()
        .filter(|tc| {
            let name_lower = tc.name.to_lowercase();
            // Block any tool that looks like a delete operation
            !name_lower.contains("delete") && !name_lower.contains("remove") && !name_lower.contains("trash")
        })
        .collect()
}
