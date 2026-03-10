//! SoulMate API memory backend.
//!
//! Uses The Menon Lab's soul.py memory infrastructure for RAG + RLM hybrid retrieval.
//!
//! # Fix (2026-03-08)
//! - `store()`: was silently failing due to missing SOULMATE_API_KEY env var defaulting
//!   to empty string → 401 on every call. Now warns loudly if key is missing.
//! - `store()`: now uses POST /v1/ask with remember=true + BYOK llm_key.
//!   Prefixed with "Remember this:" so the LLM stores it as a fact, not a query.
//! - `search()`: uses GET /v1/memory/{customer_id} — fast flat retrieval, no LLM call.
//!   Raw lines injected directly into Ray's context; his own reasoning does the synthesis.
//!
//! # Fix (2026-03-10)
//! - DEFAULT_SOULMATE_URL updated to v2 (Qdrant + Azure embeddings).
//! - search() intentionally stays as GET /v1/memory (fast, no LLM call in hot path).
//!   Semantic Qdrant retrieval will be added via a dedicated /v1/retrieve endpoint
//!   once that's added to the v2 API.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use skyclaw_core::error::SkyclawError;
use skyclaw_core::{Memory, MemoryEntry, MemoryEntryType, SearchOpts};
use tracing::{debug, info, warn};

const DEFAULT_SOULMATE_URL: &str = "https://soulmate-api-v2.themenonlab.com";

#[derive(Debug, Serialize)]
struct AskRequest {
    query: String,
    customer_id: String,
    soul_id: String,
    remember: bool,
    llm_provider: String,
    llm_key: String,
}

#[derive(Debug, Deserialize)]
struct MemoryResponse {
    #[allow(dead_code)]
    customer_id: String,
    #[allow(dead_code)]
    entries: u64,
    content: String,
}

/// SoulMate API-backed memory.
pub struct SoulMateMemory {
    client: Client,
    base_url: String,
    api_key: String,
    customer_id: String,
    soul_id: String,
    llm_key: String,
}

impl SoulMateMemory {
    /// Create a new SoulMateMemory backend.
    /// `config_url` format: `soulmate://customer_id/soul_id`
    pub async fn new(config_url: &str) -> Result<Self, SkyclawError> {
        let url = config_url
            .strip_prefix("soulmate://")
            .ok_or_else(|| SkyclawError::Config("SoulMate URL must start with soulmate://".into()))?;

        let parts: Vec<&str> = url.split('/').collect();
        let customer_id = parts.first().unwrap_or(&"ray").to_string();
        let soul_id     = parts.get(1).unwrap_or(&"ray").to_string();

        let api_key = std::env::var("SOULMATE_API_KEY").unwrap_or_default();
        if api_key.is_empty() {
            warn!("SOULMATE_API_KEY is not set — all memory calls will fail with 401.");
        }

        let base_url = std::env::var("SOULMATE_URL")
            .unwrap_or_else(|_| DEFAULT_SOULMATE_URL.to_string());

        let llm_key = std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .unwrap_or_default();
        if llm_key.is_empty() {
            warn!("No LLM key found — SoulMate store() will fail.");
        }

        info!(
            customer_id = %customer_id,
            soul_id     = %soul_id,
            base_url    = %base_url,
            "SoulMate memory backend initialized (v2)"
        );

        Ok(Self { client: Client::new(), base_url, api_key, customer_id, soul_id, llm_key })
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.api_key)
    }
}

#[async_trait]
impl Memory for SoulMateMemory {
    /// Store a memory entry via POST /v1/ask with remember=true (BYOK).
    async fn store(&self, entry: MemoryEntry) -> Result<(), SkyclawError> {
        let content = format!(
            "Remember this: [{}] {}: {}",
            entry.timestamp.format("%Y-%m-%d %H:%M UTC"),
            entry_type_label(&entry.entry_type),
            entry.content
        );

        let req = AskRequest {
            query:        content,
            customer_id:  self.customer_id.clone(),
            soul_id:      self.soul_id.clone(),
            remember:     true,
            llm_provider: "anthropic".to_string(),
            llm_key:      self.llm_key.clone(),
        };

        let resp = self
            .client
            .post(format!("{}/v1/ask", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&req)
            .send()
            .await
            .map_err(|e| SkyclawError::Memory(format!("SoulMate store request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body   = resp.text().await.unwrap_or_default();
            return Err(SkyclawError::Memory(format!(
                "SoulMate store error {status}: {body}"
            )));
        }

        debug!(id = %entry.id, "Stored memory entry via SoulMate");
        Ok(())
    }

    /// Retrieve memories via GET /v1/memory/{customer_id}.
    ///
    /// Fast flat retrieval — no LLM call in the hot path. Raw memory lines are
    /// injected directly into Ray's context; his own reasoning handles synthesis.
    ///
    /// Semantic Qdrant retrieval will be added via /v1/retrieve once that endpoint
    /// exists in the v2 API.
    async fn search(
        &self,
        _query: &str,
        opts: SearchOpts,
    ) -> Result<Vec<MemoryEntry>, SkyclawError> {
        let resp = self
            .client
            .get(format!("{}/v1/memory/{}", self.base_url, self.customer_id))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| SkyclawError::Memory(format!("SoulMate memory fetch failed: {e}")))?;

        if !resp.status().is_success() {
            warn!(status = %resp.status(), "SoulMate memory fetch returned non-200 — no memories injected");
            return Ok(Vec::new());
        }

        let result: MemoryResponse = resp.json().await.map_err(|e| {
            SkyclawError::Memory(format!("SoulMate memory parse failed: {e}"))
        })?;

        let entries: Vec<MemoryEntry> = result
            .content
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .take(opts.limit)
            .enumerate()
            .map(|(i, line)| MemoryEntry {
                id:         format!("soulmate-{i}"),
                content:    line.to_string(),
                metadata:   serde_json::json!({"source": "soulmate-v2-flat"}),
                timestamp:  chrono::Utc::now(),
                session_id: Some(self.customer_id.clone()),
                entry_type: MemoryEntryType::LongTerm,
            })
            .collect();

        debug!(count = entries.len(), "SoulMate flat memory loaded");
        Ok(entries)
    }

    async fn get(&self, _id: &str) -> Result<Option<MemoryEntry>, SkyclawError> {
        Ok(None)
    }

    async fn delete(&self, _id: &str) -> Result<(), SkyclawError> {
        Ok(())
    }

    async fn list_sessions(&self) -> Result<Vec<String>, SkyclawError> {
        Ok(vec![self.customer_id.clone()])
    }

    async fn get_session_history(
        &self,
        _session_id: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>, SkyclawError> {
        self.search("", SearchOpts { limit, ..Default::default() }).await
    }

    fn backend_name(&self) -> &str {
        "soulmate-v2"
    }
}

fn entry_type_label(t: &MemoryEntryType) -> &'static str {
    match t {
        MemoryEntryType::Conversation => "CONV",
        MemoryEntryType::LongTerm     => "LONG",
        MemoryEntryType::DailyLog     => "LOG",
        MemoryEntryType::Skill        => "SKILL",
    }
}
