//! SkyClaw Memory crate
//!
//! Provides persistent memory backends for conversations, long-term knowledge,
//! and skills. Three backends ship out of the box:
//!
//! - [`SqliteMemory`] — SQLite-backed (via sqlx), suitable for production.
//! - [`MarkdownMemory`] — Flat Markdown files, compatible with OpenClaw.
//! - [`SoulMateMemory`] — RAG + RLM hybrid via The Menon Lab's soul.py infrastructure.

pub mod markdown;
pub mod search;
pub mod soulmate;
pub mod sqlite;

pub use markdown::MarkdownMemory;
pub use soulmate::SoulMateMemory;
pub use sqlite::SqliteMemory;

use skyclaw_core::Memory;
use skyclaw_core::error::SkyclawError;

/// Factory function: create a memory backend by name.
///
/// Supported backends:
/// - `"sqlite"` — requires `url` (e.g. `"sqlite:memory.db"` or `"sqlite::memory:"`).
/// - `"markdown"` — requires `url` to be a directory path.
/// - `"soulmate"` — RAG + RLM via SoulMate API (url: `soulmate://customer_id/soul_id`).
pub async fn create_memory_backend(
    backend: &str,
    url: &str,
) -> Result<Box<dyn Memory>, SkyclawError> {
    match backend {
        "sqlite" => {
            let mem = SqliteMemory::new(url).await?;
            Ok(Box::new(mem))
        }
        "markdown" => {
            let mem = MarkdownMemory::new(url).await?;
            Ok(Box::new(mem))
        }
        "soulmate" => {
            let mem = SoulMateMemory::new(url).await?;
            Ok(Box::new(mem))
        }
        other => Err(SkyclawError::Config(format!(
            "Unknown memory backend: {other}"
        ))),
    }
}
