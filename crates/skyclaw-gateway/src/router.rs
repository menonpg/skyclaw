//! Message router — routes inbound messages from any channel through
//! the agent runtime and returns the outbound reply.
//!
//! Slash commands handled here (before reaching the agent):
//!   /new, /reset  — clear conversation history for this chat
//!   /help         — list available commands
//!   /status       — show session info

use skyclaw_agent::AgentRuntime;
use skyclaw_core::types::error::SkyclawError;
use skyclaw_core::types::message::{InboundMessage, OutboundMessage, ParseMode};
use skyclaw_core::types::session::SessionContext;
use tracing::info;

use crate::session::SessionManager;

/// Route an inbound message through the agent runtime.
///
/// Intercepts slash commands before passing to the agent.
/// Takes a mutable session so the agent can append to conversation history.
/// Returns the outbound message to send back to the originating channel.
pub async fn route_message(
    msg: &InboundMessage,
    agent: &AgentRuntime,
    session: &mut SessionContext,
    session_manager: &SessionManager,
) -> Result<OutboundMessage, SkyclawError> {
    info!(
        channel = %msg.channel,
        chat_id = %msg.chat_id,
        user_id = %msg.user_id,
        "Routing message to agent runtime"
    );

    // Intercept slash commands
    if let Some(text) = &msg.text {
        let cmd = text.trim().to_lowercase();
        let cmd = cmd.split_whitespace().next().unwrap_or("");

        match cmd {
            "/new" | "/reset" | "/clear" => {
                session_manager
                    .remove_session(&msg.channel, &msg.chat_id, &msg.user_id)
                    .await;
                session.history.clear();
                info!(chat_id = %msg.chat_id, "Session reset by user command");

                // Archive SESSION-STATE.md before clearing it — preserves history across /new
                let state_path = session.workspace_path.join("SESSION-STATE.md");
                if state_path.exists() {
                    // Append old state to memory/session-history.md so it's never lost
                    let history_dir = session.workspace_path.join("memory");
                    let _ = tokio::fs::create_dir_all(&history_dir).await;
                    let history_path = history_dir.join("session-history.md");
                    let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
                    if let Ok(old_state) = tokio::fs::read_to_string(&state_path).await {
                        let archive_entry = format!(
                            "\n\n---\n## Session archived at {} (via /new)\n\n{}\n",
                            ts, old_state
                        );
                        // Append to session-history.md
                        let existing = tokio::fs::read_to_string(&history_path).await.unwrap_or_default();
                        let _ = tokio::fs::write(&history_path, format!("{}{}", existing, archive_entry)).await;
                        info!("SESSION-STATE.md archived to memory/session-history.md");
                    }
                    // Now clear SESSION-STATE.md for the fresh session
                    if let Err(e) = tokio::fs::remove_file(&state_path).await {
                        tracing::warn!("Could not delete SESSION-STATE.md: {}", e);
                    } else {
                        info!("SESSION-STATE.md cleared by /new");
                    }
                }

                return Ok(OutboundMessage {
                    chat_id: msg.chat_id.clone(),
                    text: "Session cleared. Fresh start — conversation history and session state are gone. Long-term memory is still intact.".to_string(),
                    reply_to: Some(msg.id.clone()),
                    parse_mode: Some(ParseMode::Plain),
                });
            }
            "/help" => {
                return Ok(OutboundMessage {
                    chat_id: msg.chat_id.clone(),
                    text: "Commands:\n/new — start a fresh conversation (clears this session)\n/reset — same as /new\n/status — show session info\n/help — this message\n\nOr just talk to me normally.".to_string(),
                    reply_to: Some(msg.id.clone()),
                    parse_mode: Some(ParseMode::Plain),
                });
            }
            "/status" => {
                let history_len = session.history.len();
                return Ok(OutboundMessage {
                    chat_id: msg.chat_id.clone(),
                    text: format!(
                        "Session: {}\nChannel: {}\nMessages in context: {}\nType /new to clear.",
                        session.session_id, msg.channel, history_len
                    ),
                    reply_to: Some(msg.id.clone()),
                    parse_mode: Some(ParseMode::Plain),
                });
            }
            _ => {}
        }
    }

    agent.process_message(msg, session, None, None).await
}
