//! Context builder — assembles a CompletionRequest from session history,
//! memory search results, system prompt, and tool definitions.
//!
//! Memory is intentionally NOT queried here. Callers (AgentRuntime) are
//! responsible for fetching memory ONCE per inbound message and passing
//! the cached results in. This prevents N×tool_rounds redundant API calls.

use std::sync::Arc;

use skyclaw_core::MemoryEntry;
use skyclaw_core::Tool;
use skyclaw_core::types::message::{
    ChatMessage, CompletionRequest, MessageContent, Role, ToolDefinition,
};
use skyclaw_core::types::session::SessionContext;

/// Estimate token count from a string (rough: 1 token ≈ 4 chars).
fn estimate_tokens(s: &str) -> usize {
    s.len() / 4
}

/// Estimate token count for a ChatMessage.
fn estimate_message_tokens(msg: &ChatMessage) -> usize {
    match &msg.content {
        MessageContent::Text(t) => estimate_tokens(t),
        MessageContent::Parts(parts) => {
            parts.iter().map(|p| match p {
                skyclaw_core::types::message::ContentPart::Text { text } => estimate_tokens(text),
                skyclaw_core::types::message::ContentPart::ToolUse { input, .. } => estimate_tokens(&input.to_string()),
                skyclaw_core::types::message::ContentPart::ToolResult { content, .. } => estimate_tokens(content),
            }).sum()
        }
    }
}

/// Build a CompletionRequest from all available context.
///
/// `cached_memories` — fetched ONCE per inbound message by the caller,
/// reused across all tool rounds without additional API calls.
pub async fn build_context(
    session: &SessionContext,
    cached_memories: &[MemoryEntry],
    tools: &[Arc<dyn Tool>],
    model: &str,
    system_prompt: Option<&str>,
    max_turns: usize,
    max_context_tokens: usize,
) -> CompletionRequest {
    let mut messages: Vec<ChatMessage> = Vec::new();
    // Note: cached_memories are injected into the system prompt below, not as messages
    // (Anthropic API does not allow role:system in the messages array)

    // 2. Trim session history to max_turns pairs, keeping the most recent.
    // Use max_turns * 3 slots (not *2) because tool exchanges are 3 messages:
    // User → Assistant(tool_use) → Tool(tool_results) → Assistant(reply)
    let history = &session.history;
    let window = (max_turns * 3).max(6);
    let trimmed: Vec<ChatMessage> = if max_turns > 0 && history.len() > window {
        history[history.len() - window..].to_vec()
    } else {
        history.clone()
    };

    // 3. Apply token budget — drop oldest messages until under limit
    let system_tokens = messages.iter().map(|m| estimate_message_tokens(m)).sum::<usize>();
    let tool_def_tokens: usize = tools.iter().map(|t| {
        estimate_tokens(t.name()) + estimate_tokens(t.description()) + estimate_tokens(&t.parameters_schema().to_string())
    }).sum();
    let base_tokens = system_tokens + tool_def_tokens + 500;

    let mut kept: Vec<ChatMessage> = Vec::new();
    let mut total_tokens = base_tokens;
    for msg in trimmed.iter().rev() {
        let msg_tokens = estimate_message_tokens(msg);
        if total_tokens + msg_tokens > max_context_tokens {
            break;
        }
        total_tokens += msg_tokens;
        kept.push(msg.clone());
    }
    kept.reverse();

    // Strip orphaned leading messages so the sequence is always valid for Anthropic:
    // - Must start with Role::User (never Tool or Assistant)
    // - A Role::Tool message must always be preceded by Role::Assistant with tool_use parts
    // Stripping from the front is safe — we just lose some older context.
    while !kept.is_empty() {
        match kept[0].role {
            Role::User => break, // valid start
            _ => { kept.remove(0); } // drop orphaned Tool/Assistant at front
        }
    }

    // Also strip any trailing Role::Tool message with no following Assistant.
    // This prevents ending on dangling tool_results when context is tight.
    while kept.last().map(|m| matches!(m.role, Role::Tool)).unwrap_or(false) {
        kept.pop();
    }

    messages.extend(kept);

    // 4. Build tool definitions
    let tool_defs: Vec<ToolDefinition> = tools
        .iter()
        .map(|t| ToolDefinition {
            name: t.name().to_string(),
            description: t.description().to_string(),
            parameters: t.parameters_schema(),
        })
        .collect();

    // 5. Assemble the system prompt — prepend relevant memories if any
    let memory_prefix = if !cached_memories.is_empty() {
        let lines: String = cached_memories
            .iter()
            .map(|e| format!("- [{}] {}", e.timestamp.format("%Y-%m-%d"), e.content))
            .collect::<Vec<_>>()
            .join("\n");
        format!("Relevant context from memory:\n{}\n\n---\n\n", lines)
    } else {
        String::new()
    };

    let system = system_prompt.map(|s| format!("{}{}", memory_prefix, s)).or_else(|| {
        let tool_names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        Some(format!(
            "You are SkyClaw, a cloud-native AI agent runtime. You control a computer through messaging apps.\n\
             \n\
             You have access to these tools: {}\n\
             \n\
             Workspace: All file operations use the workspace directory at {}.\n\
             Files sent by the user are automatically saved here.\n\
             \n\
             File protocol:\n\
             - Received files are saved to the workspace automatically — use file_read to read them\n\
             - To send a file to the user, use send_file with just the path (chat_id is automatic)\n\
             - Use file_write to create files in the workspace, then send_file to deliver them\n\
             - Paths are relative to the workspace directory\n\
             \n\
             Guidelines:\n\
             - Use the shell tool to run commands, install packages, manage services, check system status\n\
             - Use file tools to read, write, and list files in the workspace\n\
             - Use web_fetch to look up documentation, check APIs, or research information\n\
             - Be concise in responses — the user is on a messaging app\n\
             - When a task requires multiple steps, execute them sequentially using tools\n\
             - If a command fails, read the error and try to fix it\n\
             - Never expose secrets, API keys, or sensitive data in responses",
            tool_names.join(", "),
            session.workspace_path.display()
        ))
    });

    CompletionRequest {
        model: model.to_string(),
        messages,
        tools: tool_defs,
        max_tokens: Some(4096),
        temperature: Some(0.7),
        system,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skyclaw_core::MemoryEntryType;
    use skyclaw_test_utils::{MockTool, make_session};

    fn make_memory_entry(content: &str) -> MemoryEntry {
        MemoryEntry {
            id: "test".to_string(),
            content: content.to_string(),
            metadata: serde_json::json!({}),
            timestamp: chrono::Utc::now(),
            session_id: None,
            entry_type: MemoryEntryType::LongTerm,
        }
    }

    #[tokio::test]
    async fn context_includes_system_prompt() {
        let tools: Vec<Arc<dyn Tool>> = vec![];
        let session = make_session();
        let req = build_context(&session, &[], &tools, "test-model", Some("Custom prompt"), 6, 30_000).await;
        assert_eq!(req.system.as_deref(), Some("Custom prompt"));
        assert_eq!(req.model, "test-model");
    }

    #[tokio::test]
    async fn context_injects_cached_memories() {
        let tools: Vec<Arc<dyn Tool>> = vec![];
        let session = make_session();
        let memories = vec![make_memory_entry("User is Prahlad, founder of The Menon Lab")];
        let req = build_context(&session, &memories, &tools, "model", None, 6, 30_000).await;
        let has_memory = req.messages.iter().any(|m| match &m.content {
            skyclaw_core::types::message::MessageContent::Text(t) => t.contains("Prahlad"),
            _ => false,
        });
        assert!(has_memory);
    }

    #[tokio::test]
    async fn context_no_memory_no_system_message() {
        let tools: Vec<Arc<dyn Tool>> = vec![];
        let session = make_session();
        let req = build_context(&session, &[], &tools, "model", Some("prompt"), 6, 30_000).await;
        // With empty memories, no memory system message injected
        assert!(!req.messages.iter().any(|m| match &m.content {
            skyclaw_core::types::message::MessageContent::Text(t) => t.starts_with("Relevant context from memory"),
            _ => false,
        }));
    }

    #[tokio::test]
    async fn context_includes_tool_definitions() {
        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(MockTool::new("shell")),
            Arc::new(MockTool::new("browser")),
        ];
        let session = make_session();
        let req = build_context(&session, &[], &tools, "model", None, 6, 30_000).await;
        assert_eq!(req.tools.len(), 2);
    }
}
