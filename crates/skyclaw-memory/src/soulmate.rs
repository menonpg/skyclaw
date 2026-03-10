//! SoulMate API memory backend.
//!
//! Uses The Menon Lab's soul.py memory infrastructure for RAG + RLM hybrid retrieval.
//!
//! # Fix (2026-03-08)
//! - `store()`: was silently failing due to missing SOULMATE_API_KEY env var defaulting
//!   to empty string → 401 on every call. Now warns loudly if key is missing.
//! - `search()` / `get_session_history()`: were calling POST /v1/ask (full LLM pipeline)
//!   just to retrieve memories. Replaced with GET /v1/memory/{customer_id} — faster,
//!   cheaper, no LLM call needed for retrieval.
//! - `store()`: kept as POST /v1/ask with remember=true (correct per API spec), but now
//!   prefixed with "Remember this:" so the LLM stores it as a fact, not a query.
//!
//! # Fix (2026-03-10)
//! - `DEFAULT_SOULMATE_URL` updated to v2 (Qdrant + Azure embeddings).
//! - `search()` now uses POST /v1/ask with remember=false — triggers semantic Qdrant
//!   retrieval on v2 instead of returning the full flat markdown file. This means
//!   Ray gets the 8 most semantically relevant memories injected into context, not
//!   a raw dump of every line.

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
    // SoulMate is BYOK — the caller must supply the LLM key for server-side LLM calls
    llm_provider: String,
    llm_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    llm_model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AskResponse {
    #[allow(dead_code)]
    answer: String,
    #[serde(default)]
    rag_hits: u32,
    #[serde(default)]
    retrieval: String,
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
    /// The agent's own LLM key — passed to SoulMate for BYOK LLM calls
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
            warn!("SOULMATE_API_KEY is not set — all memory calls will fail with 401. \
                   Set this env var in Railway.");
        }

        let base_url = std::env::var("SOULMATE_URL")
            .unwrap_or_else(|_| DEFAULT_SOULMATE_URL.to_string());

        info!(
            customer_id = %customer_id,
            soul_id     = %soul_id,
            base_url    = %base_url,
            "SoulMate memory backend initialized"
        );

        // SoulMate is BYOK — pass ANTHROPIC_API_KEY (or fallback to OPENAI_API_KEY)
        let llm_key = std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .unwrap_or_default();
        if llm_key.is_empty() {
            warn!("No LLM key (ANTHROPIC_API_KEY/OPENAI_API_KEY) found — SoulMate store() will fail with 500.");
        }

        Ok(Self { client: Client::new(), base_url, api_key, customer_id, soul_id, llm_key })
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.api_key)
    }
}

#[async_trait]
impl Memory for SoulMateMemory {
    /// Store a memory entry via POST /v1/ask with remember=true.
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
            llm_model:    None, // uses SoulMate default (claude-haiku-4-5)
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

    /// Retrieve semantically relevant memories via POST /v1/ask with remember=false.
    ///
    /// On v2 this triggers Qdrant vector search + Azure embeddings — returns the
    /// top-8 most semantically similar memories for the query. No new memory is stored.
    ///
    /// Falls back to GET /v1/memory (flat markdown) if llm_key is empty.
    async fn search(
        &self,
        query: &str,
        opts: SearchOpts,
    ) -> Result<Vec<MemoryEntry>, SkyclawError> {
        // If no LLM key, fall back to flat GET (v1-style).
        // v2 semantic search requires an LLM call for context synthesis.
        if self.llm_key.is_empty() || query.is_empty() {
            return self.search_flat(opts.limit).await;
        }

        let req = AskRequest {
            query:        query.to_string(),
            customer_id:  self.customer_id.clone(),
            soul_id:      self.soul_id.clone(),
            remember:     false, // retrieve only — do NOT store this search query
            llm_provider: "anthropic".to_string(),
            llm_key:      self.llm_key.clone(),
            llm_model:    Some("claude-haiku-4-5".to_string()), // fast + cheap for retrieval
        };

        let resp = self
            .client
            .post(format!("{}/v1/ask", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&req)
            .send()
            .await
            .map_err(|e| SkyclawError::Memory(format!("SoulMate semantic search failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            warn!(status = %status, "SoulMate semantic search returned non-200 — falling back to flat");
            return self.search_flat(opts.limit).await;
        }

        let result: AskResponse = resp.json().await.map_err(|e| {
            SkyclawError::Memory(format!("SoulMate search parse failed: {e}"))
        })?;

        debug!(
            rag_hits = result.rag_hits,
            retrieval = %result.retrieval,
            "SoulMate semantic search complete"
        );

        // The answer IS the synthesized memory context — wrap it as a single entry
        // so AgentRuntime's context builder injects it into the system prompt.
        if result.answer.trim().is_empty()
            || result.answer.contains("No relevant memories")
            || result.answer.contains("No memories yet")
        {
            return Ok(Vec::new());
        }

        Ok(vec![MemoryEntry {
            id:         format!("soulmate-semantic-{}", chrono::Utc::now().timestamp()),
            content:    result.answer,
            metadata:   serde_json::json!({
                "rag_hits": result.rag_hits,
                "retrieval": result.retrieval,
                "source": "soulmate-v2"
            }),
            timestamp:  chrono::Utc::now(),
            session_id: Some(self.customer_id.clone()),
            entry_type: MemoryEntryType::LongTerm,
        }])
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
        self.search_flat(limit).await
    }

    fn backend_name(&self) -> &str {
        "soulmate-v2"
    }
}

impl SoulMateMemory {
    /// Flat memory retrieval via GET /v1/memory/{customer_id}.
    /// Used as a fallback when semantic search isn't available.
    async fn search_flat(&self, limit: usize) -> Result<Vec<MemoryEntry>, SkyclawError> {
        let resp = self
            .client
            .get(format!("{}/v1/memory/{}", self.base_url, self.customer_id))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| SkyclawError::Memory(format!("SoulMate memory fetch failed: {e}")))?;

        if !resp.status().is_success() {
            warn!(status = %resp.status(), "SoulMate flat memory fetch returned non-200 — returning empty");
            return Ok(Vec::new());
        }

        let result: MemoryResponse = resp.json().await.map_err(|e| {
            SkyclawError::Memory(format!("SoulMate memory parse failed: {e}"))
        })?;

        let entries: Vec<MemoryEntry> = result
            .content
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .take(limit)
            .enumerate()
            .map(|(i, line)| MemoryEntry {
                id:         format!("soulmate-flat-{i}"),
                content:    line.to_string(),
                metadata:   serde_json::json!({"source": "soulmate-flat"}),
                timestamp:  chrono::Utc::now(),
                session_id: Some(self.customer_id.clone()),
                entry_type: MemoryEntryType::LongTerm,
            })
            .collect();

        Ok(entries)
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
