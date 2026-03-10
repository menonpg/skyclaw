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
    ChatMessage, CompletionRequest, ContentPart, MessageContent, Role, ToolDefinition,
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
                ContentPart::Text { text } => estimate_tokens(text),
                ContentPart::ToolUse { input, .. } => estimate_tokens(&input.to_string()),
                ContentPart::ToolResult { content, .. } => estimate_tokens(content),
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

    // Trim session history to max_turns pairs, keeping the most recent.
    // Use max_turns * 3 slots because tool exchanges are 3 messages:
    // User → Assistant(tool_use) → Tool(tool_results) → Assistant(reply)
    let history = &session.history;
    let window = (max_turns * 3).max(6);
    let trimmed: Vec<ChatMessage> = if max_turns > 0 && history.len() > window {
        history[history.len() - window..].to_vec()
    } else {
        history.clone()
    };

    // Apply token budget — drop oldest messages until under limit
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

    // Strip orphaned messages from the FRONT.
    // The sequence must start with Role::User.
    while !kept.is_empty() {
        match kept[0].role {
            Role::User => break,
            _ => { kept.remove(0); }
        }
    }

    // Strip orphaned tool_use from the END.
    // If the token budget cut through a tool_use/tool_result pair, the last
    // message may be assistant+tool_use with no following tool_result — that
    // causes Anthropic 400. Remove dangling tool_use (and any trailing
    // orphaned tool_result that lost its partner) until the tail is clean.
    loop {
        match kept.last() {
            Some(last) if matches!(last.role, Role::Assistant) => {
                let has_dangling_tool_use = match &last.content {
                    MessageContent::Parts(parts) => parts.iter().any(|p| matches!(p, ContentPart::ToolUse { .. })),
                    _ => false,
                };
                if has_dangling_tool_use {
                    tracing::warn!("context: stripping dangling assistant+tool_use from end of context window");
                    kept.pop();
                    // Also remove preceding tool_result if it lost its pair
                    if matches!(kept.last().map(|m| &m.role), Some(Role::Tool)) {
                        kept.pop();
                    }
                } else {
                    break;
                }
            }
            Some(last) if matches!(last.role, Role::Tool) => {
                // Orphaned tool_result with no preceding assistant+tool_use
                tracing::warn!("context: stripping orphaned tool_result from end of context window");
                kept.pop();
            }
            _ => break,
        }
    }

    // Safety: Anthropic requires at least one message.
    // If aggressive stripping left us with nothing, use the last user message from original history.
    if kept.is_empty() {
        tracing::warn!("context: all messages stripped — falling back to last user message");
        if let Some(last_user) = history.iter().rev().find(|m| matches!(m.role, Role::User)) {
            kept.push(last_user.clone());
        }
    }

    messages.extend(kept);

    // Build tool definitions
    let tool_defs: Vec<ToolDefinition> = tools
        .iter()
        .map(|t| ToolDefinition {
            name: t.name().to_string(),
            description: t.description().to_string(),
            parameters: t.parameters_schema(),
        })
        .collect();

    // Assemble the system prompt — prepend relevant memories if any
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
            "You are SkyClaw, a cloud-native AI agent runtime.\n\
             You have access to these tools: {}\n\
             Workspace: {}\n\
             Be concise. Use tools to get things done.",
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
        let sys = req.system.unwrap_or_default();
        assert!(sys.contains("Prahlad"));
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
