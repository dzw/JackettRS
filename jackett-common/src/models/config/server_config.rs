use serde::{Deserialize, Serialize};

use super::proxy_type::ProxyType;
use super::runtime_settings::RuntimeSettings;

/// Mirror of Jackett.Common/Models/Config/ServerConfig.cs
///
/// C# implemented IObservable<ServerConfig>; in Rust we keep a simple
/// observer list of boxed callbacks (config change notification).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub port: i32,
    pub local_bind_address: String,
    pub allow_external: bool,
    pub allow_cors: bool,
    pub api_key: Option<String>,
    pub admin_password: Option<String>,
    pub instance_id: Option<String>,
    pub blackhole_dir: Option<String>,
    pub update_disabled: bool,
    pub update_prerelease: bool,
    pub base_path_override: Option<String>,
    pub base_url_override: Option<String>,
    pub cache_enabled: bool,
    pub cache_ttl: i64,
    pub cache_max_results_per_indexer: i64,
    pub flare_solverr_url: Option<String>,
    pub flare_solverr_max_timeout: i32,
    pub omdb_api_key: Option<String>,
    pub omdb_api_url: Option<String>,
    #[serde(rename = "ProxyType")]
    pub proxy_type: i32,
    pub proxy_url: Option<String>,
    pub proxy_port: Option<i32>,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,

    /// Ignore as we don't really want to be saving settings specified in the
    /// command line (C# `[JsonIgnore] RuntimeSettings`).
    #[serde(skip)]
    pub runtime_settings: RuntimeSettings,
}

impl Default for ServerConfig {
    fn default() -> Self {
        // Default values
        Self {
            port: 9117,
            local_bind_address: "127.0.0.1".to_string(),
            allow_external: cfg!(unix),
            allow_cors: false,
            api_key: None,
            admin_password: None,
            instance_id: None,
            blackhole_dir: None,
            update_disabled: false,
            update_prerelease: false,
            base_path_override: None,
            base_url_override: None,
            // Sonarr 15min, Radarr 60min, LazyLibrarian 20min, Readarr 15min, Lidarr = 15min
            cache_enabled: true,
            // 35 minutes is a reasonable value for all of them and to avoid race conditions
            cache_ttl: 2100,
            cache_max_results_per_indexer: 1000,
            flare_solverr_url: None,
            flare_solverr_max_timeout: 55000,
            omdb_api_key: None,
            omdb_api_url: None,
            proxy_type: ProxyType::Disabled as i32,
            proxy_url: None,
            proxy_port: None,
            proxy_username: None,
            proxy_password: None,
            runtime_settings: RuntimeSettings::default(),
        }
    }
}

impl ServerConfig {
    pub fn new(runtime_settings: RuntimeSettings) -> Self {
        Self {
            runtime_settings,
            ..Default::default()
        }
    }

    /// Mirrors the C# `[OnDeserialized]` fixup: blank ProxyUrl implies disabled proxy.
    pub fn on_deserialized(&mut self) {
        if self.proxy_url.as_ref().map(|u| u.trim().is_empty()).unwrap_or(true) {
            self.proxy_type = ProxyType::Disabled as i32;
        }
    }

    pub fn proxy_is_anonymous(&self) -> bool {
        self.proxy_username
            .as_ref()
            .map(|u| u.trim().is_empty())
            .unwrap_or(true)
            || self
                .proxy_password
                .as_ref()
                .map(|p| p.trim().is_empty())
                .unwrap_or(true)
    }

    pub fn get_proxy_auth_string(&self) -> Option<String> {
        if !self.proxy_is_anonymous() {
            Some(format!(
                "{}:{}",
                self.proxy_username.as_deref().unwrap_or(""),
                self.proxy_password.as_deref().unwrap_or("")
            ))
        } else {
            None
        }
    }

    pub fn get_proxy_url(&self, with_creds: bool) -> Option<String> {
        let mut url = self.proxy_url.clone()?;

        // if disabled
        if self.proxy_type == ProxyType::Disabled as i32 || url.trim().is_empty() {
            return None;
        }

        // remove protocol from url
        if let Some(index) = url.find("://") {
            url = url[index + 3..].to_string();
        }

        // add port
        if let Some(port) = self.proxy_port {
            url = format!("{}:{}", url, port);
        }

        // add credentials
        let auth_string = self.get_proxy_auth_string();
        if with_creds {
            if let Some(auth) = auth_string {
                url = format!("{}@{}", auth, url);
            }
        }

        // add protocol
        let protocol = match self.proxy_type {
            t if t == ProxyType::Socks4 as i32 => "socks4",
            t if t == ProxyType::Socks5 as i32 => "socks5",
            t if t == ProxyType::Http as i32 => "http",
            _ => return Some(url),
        };
        Some(format!("{}://{}", protocol, url))
    }

    pub fn get_listen_addresses(&self) -> Vec<String> {
        self.get_listen_addresses_with(None)
    }

    pub fn get_listen_addresses_with(&self, external: Option<bool>) -> Vec<String> {
        let external = external.unwrap_or(self.allow_external);
        if external {
            vec![format!("http://*:{}/", self.port)]
        } else {
            vec![format!("http://{}:{}/", self.local_bind_address, self.port)]
        }
    }
}
