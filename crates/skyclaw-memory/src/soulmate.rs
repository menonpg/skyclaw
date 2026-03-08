//! SoulMate API memory backend.
//!
//! Uses The Menon Lab's soul.py memory infrastructure for RAG + RLM hybrid retrieval.
//! This replaces SQLite with persistent, semantic memory powered by SoulMate.

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
    route: Option<String>,
    #[serde(default)]
    memories: Vec<String>,
}

#[derive(Debug, Serialize)]
struct StoreRequest {
    customer_id: String,
    content: String,
    metadata: serde_json::Value,
}

/// SoulMate API-backed memory.
///
/// Uses RAG + RLM hybrid retrieval from soul.py infrastructure.
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
    /// `config_url` format: `soulmate://<api_key>@<customer_id>/<soul_id>`
    /// or just `soulmate://<customer_id>` for defaults.
    pub async fn new(config_url: &str) -> Result<Self, SkyclawError> {
        // Parse config URL
        let url = config_url
            .strip_prefix("soulmate://")
            .ok_or_else(|| SkyclawError::Config("SoulMate URL must start with soulmate://".into()))?;

        let (api_key, rest) = if url.contains('@') {
            let parts: Vec<&str> = url.splitn(2, '@').collect();
            (parts[0].to_string(), parts[1])
        } else {
            // Use env var for API key
            let key = std::env::var("SOULMATE_API_KEY").unwrap_or_default();
            (key, url)
        };

        let parts: Vec<&str> = rest.split('/').collect();
        let customer_id = parts.first().unwrap_or(&"ray").to_string();
        let soul_id = parts.get(1).unwrap_or(&"ray").to_string();

        let base_url = std::env::var("SOULMATE_URL").unwrap_or_else(|_| DEFAULT_SOULMATE_URL.to_string());

        info!(
            customer_id = %customer_id,
            soul_id = %soul_id,
            base_url = %base_url,
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

    /// Ask a question with memory context (RAG + RLM hybrid).
    pub async fn ask(&self, query: &str) -> Result<String, SkyclawError> {
        let req = AskRequest {
            query: query.to_string(),
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
            .map_err(|e| SkyclawError::Memory(format!("SoulMate request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SkyclawError::Memory(format!(
                "SoulMate API error {}: {}",
                status, body
            )));
        }

        let result: AskResponse = resp
            .json()
            .await
            .map_err(|e| SkyclawError::Memory(format!("SoulMate response parse error: {e}")))?;

        debug!(route = ?result.route, "SoulMate query completed");
        Ok(result.answer)
    }
}

#[async_trait]
impl Memory for SoulMateMemory {
    async fn store(&self, entry: MemoryEntry) -> Result<(), SkyclawError> {
        // Store memories via the ask endpoint with remember=true
        // The content becomes part of the conversation history
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
        // Use the ask endpoint for semantic search
        // SoulMate's RAG will return relevant memories
        let req = AskRequest {
            query: format!("Recall memories related to: {}", query),
            customer_id: self.customer_id.clone(),
            soul_id: self.soul_id.clone(),
            remember: false, // Don't store the search query itself
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
            // Return empty results on error (graceful degradation)
            return Ok(Vec::new());
        }

        let result: AskResponse = resp.json().await.unwrap_or(AskResponse {
            answer: String::new(),
            route: None,
            memories: Vec::new(),
        });

        // Convert memories to MemoryEntry format
        let entries: Vec<MemoryEntry> = result
            .memories
            .into_iter()
            .take(opts.limit.unwrap_or(10) as usize)
            .enumerate()
            .map(|(i, content)| MemoryEntry {
                id: format!("soulmate-{}", i),
                content,
                metadata: serde_json::json!({}),
                timestamp: chrono::Utc::now(),
                session_id: Some(self.customer_id.clone()),
                entry_type: MemoryEntryType::Knowledge,
            })
            .collect();

        Ok(entries)
    }

    async fn get(&self, id: &str) -> Result<Option<MemoryEntry>, SkyclawError> {
        // SoulMate doesn't support direct ID lookup; return None
        Ok(None)
    }

    async fn delete(&self, id: &str) -> Result<(), SkyclawError> {
        // Individual entry deletion not supported; would need to clear all memory
        Ok(())
    }

    async fn list(&self, opts: SearchOpts) -> Result<Vec<MemoryEntry>, SkyclawError> {
        // Use search with empty query to get recent memories
        self.search("recent activity and context", opts).await
    }
}

fn entry_type_label(t: &MemoryEntryType) -> &'static str {
    match t {
        MemoryEntryType::Conversation => "CONV",
        MemoryEntryType::Knowledge => "KNOW",
        MemoryEntryType::Skill => "SKILL",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires live SoulMate API
    async fn test_soulmate_connection() {
        let mem = SoulMateMemory::new("soulmate://test-ray/ray")
            .await
            .unwrap();
        assert_eq!(mem.customer_id, "test-ray");
        assert_eq!(mem.soul_id, "ray");
    }
}
