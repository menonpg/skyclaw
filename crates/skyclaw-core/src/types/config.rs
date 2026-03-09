use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level SkyClaw configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkyclawConfig {
    #[serde(default)]
    pub skyclaw: SkyclawSection,
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub provider: ProviderConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub vault: VaultConfig,
    #[serde(default)]
    pub filestore: FileStoreConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub heartbeat: HeartbeatConfig,
    #[serde(default)]
    pub cron: CronConfig,
    #[serde(default)]
    pub channel: HashMap<String, ChannelConfig>,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub tunnel: Option<TunnelConfig>,
    #[serde(default)]
    pub observability: ObservabilityConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkyclawSection {
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub tenant_isolation: bool,
}

impl Default for SkyclawSection {
    fn default() -> Self {
        Self {
            mode: "auto".to_string(),
            tenant_isolation: false,
        }
    }
}

fn default_mode() -> String { "auto".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub tls: bool,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            tls: false,
            tls_cert: None,
            tls_key: None,
        }
    }
}

fn default_host() -> String { "127.0.0.1".to_string() }
fn default_port() -> u16 { 8080 }

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
}

impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("name", &self.name)
            .field("api_key", &self.api_key.as_ref().map(|k| {
                if k.len() > 8 {
                    format!("{}...{}", &k[..4], &k[k.len()-4..])
                } else {
                    "***".to_string()
                }
            }))
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    #[serde(default = "default_memory_backend")]
    pub backend: String,
    pub path: Option<String>,
    pub connection_string: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub search: SearchConfig,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            backend: "sqlite".to_string(),
            path: None,
            connection_string: None,
            api_key: None,
            search: SearchConfig::default(),
        }
    }
}

fn default_memory_backend() -> String { "sqlite".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    #[serde(default = "default_vector_weight")]
    pub vector_weight: f32,
    #[serde(default = "default_keyword_weight")]
    pub keyword_weight: f32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            vector_weight: 0.7,
            keyword_weight: 0.3,
        }
    }
}

fn default_vector_weight() -> f32 { 0.7 }
fn default_keyword_weight() -> f32 { 0.3 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultConfig {
    #[serde(default = "default_vault_backend")]
    pub backend: String,
    pub key_file: Option<String>,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            backend: "local-chacha20".to_string(),
            key_file: None,
        }
    }
}

fn default_vault_backend() -> String { "local-chacha20".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStoreConfig {
    #[serde(default = "default_filestore_backend")]
    pub backend: String,
    pub bucket: Option<String>,
    pub region: Option<String>,
    pub path: Option<String>,
}

impl Default for FileStoreConfig {
    fn default() -> Self {
        Self {
            backend: "local".to_string(),
            bucket: None,
            region: None,
            path: None,
        }
    }
}

fn default_filestore_backend() -> String { "local".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    #[serde(default = "default_sandbox")]
    pub sandbox: String,
    #[serde(default = "default_true")]
    pub file_scanning: bool,
    #[serde(default = "default_skill_signing")]
    pub skill_signing: String,
    #[serde(default = "default_true")]
    pub audit_log: bool,
    #[serde(default)]
    pub rate_limit: Option<RateLimitConfig>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            sandbox: "mandatory".to_string(),
            file_scanning: true,
            skill_signing: "required".to_string(),
            audit_log: true,
            rate_limit: None,
        }
    }
}

fn default_sandbox() -> String { "mandatory".to_string() }
fn default_skill_signing() -> String { "required".to_string() }
fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_heartbeat_interval")]
    pub interval: String,
    #[serde(default = "default_heartbeat_checklist")]
    pub checklist: String,
    /// Chat ID to send heartbeat reports to (e.g. Telegram chat).
    /// If unset, heartbeat responses are only logged.
    #[serde(default)]
    pub report_to: Option<String>,
    /// Active hours window (24h format). Heartbeats only fire within
    /// this range. Example: "08:00-22:00". Unset = always active.
    #[serde(default)]
    pub active_hours: Option<String>,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: "30m".to_string(),
            checklist: "HEARTBEAT.md".to_string(),
            report_to: None,
            active_hours: None,
        }
    }
}

fn default_heartbeat_interval() -> String { "30m".to_string() }
fn default_heartbeat_checklist() -> String { "HEARTBEAT.md".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronConfig {
    #[serde(default = "default_cron_storage")]
    pub storage: String,
}

impl Default for CronConfig {
    fn default() -> Self {
        Self { storage: "sqlite".to_string() }
    }
}

fn default_cron_storage() -> String { "sqlite".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    #[serde(default)]
    pub enabled: bool,
    pub token: Option<String>,
    #[serde(default)]
    pub allowlist: Vec<String>,
    #[serde(default = "default_true")]
    pub file_transfer: bool,
    pub max_file_size: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Maximum number of recent message pairs (user+assistant) to keep in context.
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,
    /// Maximum estimated token count for the entire context window.
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: usize,
    /// Maximum number of tool-use rounds per message before forcing a text reply.
    #[serde(default = "default_max_tool_rounds")]
    pub max_tool_rounds: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_turns: 6,
            max_context_tokens: 30_000,
            max_tool_rounds: 25,
        }
    }
}

fn default_max_turns() -> usize { 6 }
fn default_max_context_tokens() -> usize { 30_000 }
fn default_max_tool_rounds() -> usize { 25 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default = "default_true")]
    pub shell: bool,
    #[serde(default = "default_true")]
    pub browser: bool,
    #[serde(default = "default_true")]
    pub file: bool,
    #[serde(default = "default_true")]
    pub git: bool,
    #[serde(default = "default_true")]
    pub cron: bool,
    #[serde(default = "default_true")]
    pub http: bool,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            shell: true,
            browser: true,
            file: true,
            git: true,
            cron: true,
            http: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
    pub provider: String,
    pub token: Option<String>,
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default)]
    pub otel_enabled: bool,
    pub otel_endpoint: Option<String>,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            otel_enabled: false,
            otel_endpoint: None,
        }
    }
}

fn default_log_level() -> String { "info".to_string() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_serde_roundtrip() {
        let config = SkyclawConfig {
            skyclaw: SkyclawSection { mode: "cloud".to_string(), tenant_isolation: true },
            gateway: GatewayConfig { host: "0.0.0.0".to_string(), port: 443, tls: true, tls_cert: Some("cert.pem".to_string()), tls_key: Some("key.pem".to_string()) },
            provider: ProviderConfig { name: Some("anthropic".to_string()), api_key: Some("sk-test".to_string()), model: Some("claude-sonnet-4-6".to_string()), base_url: None },
            memory: MemoryConfig::default(),
            vault: VaultConfig::default(),
            filestore: FileStoreConfig::default(),
            security: SecurityConfig::default(),
            heartbeat: HeartbeatConfig { enabled: true, ..Default::default() },
            cron: CronConfig::default(),
            channel: HashMap::new(),
            agent: AgentConfig::default(),
            tools: ToolsConfig::default(),
            tunnel: None,
            observability: ObservabilityConfig::default(),
        };

        let toml_str = toml::to_string(&config).unwrap();
        let restored: SkyclawConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(restored.skyclaw.mode, "cloud");
        assert!(restored.skyclaw.tenant_isolation);
        assert_eq!(restored.gateway.port, 443);
        assert!(restored.gateway.tls);
        assert_eq!(restored.provider.name.as_deref(), Some("anthropic"));
    }

    #[test]
    fn defaults_are_sensible() {
        let gw = GatewayConfig::default();
        assert_eq!(gw.host, "127.0.0.1");
        assert_eq!(gw.port, 8080);
        assert!(!gw.tls);

        let mem = MemoryConfig::default();
        assert_eq!(mem.backend, "sqlite");

        let sec = SecurityConfig::default();
        assert_eq!(sec.sandbox, "mandatory");
        assert!(sec.file_scanning);
        assert!(sec.audit_log);

        let tools = ToolsConfig::default();
        assert!(tools.shell);
        assert!(tools.browser);
        assert!(tools.file);
    }
}
