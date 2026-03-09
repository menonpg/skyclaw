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
                return Ok(OutboundMessage {
                    chat_id: msg.chat_id.clone(),
                    text: "Session cleared. Fresh start — I've forgotten everything from this conversation. My long-term memory (MEMORY.md) is still intact.".to_string(),
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
