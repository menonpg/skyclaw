//! SoulMate API memory backend.
//!
//! Uses The Menon Lab's soul.py memory infrastructure for RAG + RLM hybrid retrieval.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use skyclaw_core::error::SkyclawError;
use skyclaw_core::{Memory, MemoryEntry, MemoryEntryType, SearchOpts};
use tracing::{debug, info};

const DEFAULT_SOULMATE_URL: &str = "https://soulmate-api-production.up.railway.app";

#[derive(Debug, Serialize)]
struct AskRequest {
    query: String,
    customer_id: String,
    soul_id: String,
    remember: bool,
}

#[derive(Debug, Deserialize)]
struct AskResponse {
    answer: String,
    #[serde(default)]
    route: Option<String>,
    #[serde(default)]
    memories: Vec<String>,
}

/// SoulMate API-backed memory.
pub struct SoulMateMemory {
    client: Client,
    base_url: String,
    api_key: String,
    customer_id: String,
    soul_id: String,
}

impl SoulMateMemory {
    /// Create a new SoulMateMemory backend.
    ///
    /// `config_url` format: `soulmate://customer_id/soul_id`
    pub async fn new(config_url: &str) -> Result<Self, SkyclawError> {
        let url = config_url
            .strip_prefix("soulmate://")
            .ok_or_else(|| SkyclawError::Config("SoulMate URL must start with soulmate://".into()))?;

        let parts: Vec<&str> = url.split('/').collect();
        let customer_id = parts.first().unwrap_or(&"ray").to_string();
        let soul_id = parts.get(1).unwrap_or(&"ray").to_string();

        let api_key = std::env::var("SOULMATE_API_KEY").unwrap_or_default();
        let base_url = std::env::var("SOULMATE_URL")
            .unwrap_or_else(|_| DEFAULT_SOULMATE_URL.to_string());

        info!(
            customer_id = %customer_id,
            soul_id = %soul_id,
            "SoulMate memory backend initialized"
        );

        Ok(Self {
            client: Client::new(),
            base_url,
            api_key,
            customer_id,
            soul_id,
        })
    }
}

#[async_trait]
impl Memory for SoulMateMemory {
    async fn store(&self, entry: MemoryEntry) -> Result<(), SkyclawError> {
        let content = format!(
            "[{}] {}: {}",
            entry.timestamp.format("%Y-%m-%d %H:%M"),
            entry_type_label(&entry.entry_type),
            entry.content
        );

        let req = AskRequest {
            query: content,
            customer_id: self.customer_id.clone(),
            soul_id: self.soul_id.clone(),
            remember: true,
        };

        let resp = self
            .client
            .post(format!("{}/v1/ask", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&req)
            .send()
            .await
            .map_err(|e| SkyclawError::Memory(format!("SoulMate store failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SkyclawError::Memory(format!(
                "SoulMate store error {}: {}",
                status, body
            )));
        }

        debug!(id = %entry.id, "Stored memory entry via SoulMate");
        Ok(())
    }

    async fn search(
        &self,
        query: &str,
        opts: SearchOpts,
    ) -> Result<Vec<MemoryEntry>, SkyclawError> {
        let req = AskRequest {
            query: format!("Recall memories related to: {}", query),
            customer_id: self.customer_id.clone(),
            soul_id: self.soul_id.clone(),
            remember: false,
        };

        let resp = self
            .client
            .post(format!("{}/v1/ask", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&req)
            .send()
            .await
            .map_err(|e| SkyclawError::Memory(format!("SoulMate search failed: {e}")))?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let result: AskResponse = resp.json().await.unwrap_or(AskResponse {
            answer: String::new(),
            route: None,
            memories: Vec::new(),
        });

        let entries: Vec<MemoryEntry> = result
            .memories
            .into_iter()
            .take(opts.limit)
            .enumerate()
            .map(|(i, content)| MemoryEntry {
                id: format!("soulmate-{}", i),
                content,
                metadata: serde_json::json!({}),
                timestamp: chrono::Utc::now(),
                session_id: Some(self.customer_id.clone()),
                entry_type: MemoryEntryType::LongTerm,
            })
            .collect();

        Ok(entries)
    }

    async fn get(&self, _id: &str) -> Result<Option<MemoryEntry>, SkyclawError> {
        // SoulMate doesn't support direct ID lookup
        Ok(None)
    }

    async fn delete(&self, _id: &str) -> Result<(), SkyclawError> {
        // Individual entry deletion not supported
        Ok(())
    }

    async fn list_sessions(&self) -> Result<Vec<String>, SkyclawError> {
        // Return the current customer as the only session
        Ok(vec![self.customer_id.clone()])
    }

    async fn get_session_history(
        &self,
        _session_id: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>, SkyclawError> {
        // Use search to get recent history
        self.search("recent conversation history", SearchOpts {
            limit,
            ..Default::default()
        }).await
    }

    fn backend_name(&self) -> &str {
        "soulmate"
    }
}

fn entry_type_label(t: &MemoryEntryType) -> &'static str {
    match t {
        MemoryEntryType::Conversation => "CONV",
        MemoryEntryType::LongTerm => "LONG",
        MemoryEntryType::DailyLog => "LOG",
        MemoryEntryType::Skill => "SKILL",
    }
}
