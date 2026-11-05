use serde::Serialize;

use crate::models::config::server_config::ServerConfig as CoreServerConfig;
use crate::models::config::proxy_type::ProxyType;

/// Mirror of Jackett.Common/Models/DTO/ServerConfig.cs
#[derive(Debug, Clone, Serialize)]
pub struct ServerConfig {
    #[serde(rename = "notices")]
    pub notices: Vec<String>,
    #[serde(rename = "port")]
    pub port: i32,
    #[serde(rename = "external")]
    pub external: bool,
    #[serde(rename = "local_bind_address")]
    pub local_bind_address: String,
    #[serde(rename = "cors")]
    pub cors: bool,
    #[serde(rename = "api_key")]
    pub api_key: Option<String>,
    #[serde(rename = "blackholedir")]
    pub blackhole_dir: Option<String>,
    #[serde(rename = "updatedisabled")]
    pub update_disabled: bool,
    #[serde(rename = "prerelease")]
    pub prerelease: bool,
    #[serde(rename = "password")]
    pub password: String,
    #[serde(rename = "logging")]
    pub logging: bool,
    #[serde(rename = "basepathoverride")]
    pub base_path_override: Option<String>,
    #[serde(rename = "baseurloverride")]
    pub base_url_override: Option<String>,
    #[serde(rename = "cache_enabled")]
    pub cache_enabled: bool,
    #[serde(rename = "cache_ttl")]
    pub cache_ttl: i64,
    #[serde(rename = "cache_max_results_per_indexer")]
    pub cache_max_results_per_indexer: i64,
    #[serde(rename = "flaresolverrurl")]
    pub flare_solverr_url: Option<String>,
    #[serde(rename = "flaresolverr_maxtimeout")]
    pub flare_solverr_max_timeout: i32,
    #[serde(rename = "omdbkey")]
    pub omdb_key: Option<String>,
    #[serde(rename = "omdburl")]
    pub omdb_url: Option<String>,
    #[serde(rename = "app_version")]
    pub app_version: String,
    #[serde(rename = "can_run_netcore")]
    pub can_run_net_core: bool,

    #[serde(rename = "proxy_type")]
    pub proxy_type: i32,
    #[serde(rename = "proxy_url")]
    pub proxy_url: Option<String>,
    #[serde(rename = "proxy_port")]
    pub proxy_port: Option<i32>,
    #[serde(rename = "proxy_username")]
    pub proxy_username: Option<String>,
    #[serde(rename = "proxy_password")]
    pub proxy_password: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            notices: Vec::new(),
            port: 0,
            external: false,
            local_bind_address: String::new(),
            cors: false,
            api_key: None,
            blackhole_dir: None,
            update_disabled: false,
            prerelease: false,
            password: String::new(),
            logging: false,
            base_path_override: None,
            base_url_override: None,
            cache_enabled: false,
            cache_ttl: 0,
            cache_max_results_per_indexer: 0,
            flare_solverr_url: None,
            flare_solverr_max_timeout: 0,
            omdb_key: None,
            omdb_url: None,
            app_version: String::new(),
            can_run_net_core: false,
            proxy_type: ProxyType::Disabled as i32,
            proxy_url: None,
            proxy_port: None,
            proxy_username: None,
            proxy_password: None,
        }
    }
}

impl ServerConfig {
    pub fn new(
        notices: Vec<String>,
        config: &CoreServerConfig,
        version: &str,
        can_run_net_core: bool,
    ) -> Self {
        Self {
            notices,
            port: config.port,
            external: config.allow_external,
            local_bind_address: config.local_bind_address.clone(),
            cors: config.allow_cors,
            api_key: config.api_key.clone(),
            blackhole_dir: config.blackhole_dir.clone(),
            update_disabled: config.update_disabled,
            prerelease: config.update_prerelease,
            password: config
                .admin_password
                .as_ref()
                .filter(|p| !p.is_empty())
                .map(|p| p.chars().take(10).collect())
                .unwrap_or_default(),
            logging: config.runtime_settings.tracing_enabled,
            base_path_override: config.base_path_override.clone(),
            base_url_override: config.base_url_override.clone(),
            cache_enabled: config.cache_enabled,
            cache_ttl: config.cache_ttl,
            cache_max_results_per_indexer: config.cache_max_results_per_indexer,
            flare_solverr_url: config.flare_solverr_url.clone(),
            flare_solverr_max_timeout: config.flare_solverr_max_timeout,
            omdb_key: config.omdb_api_key.clone(),
            omdb_url: config.omdb_api_url.clone(),
            app_version: version.to_string(),
            can_run_net_core,
            proxy_type: config.proxy_type,
            proxy_url: config.proxy_url.clone(),
            proxy_port: config.proxy_port,
            proxy_username: config.proxy_username.clone(),
            proxy_password: config.proxy_password.clone(),
        }
    }
}
