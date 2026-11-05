use anyhow::Result;

use crate::models::config::server_config::ServerConfig;
use crate::models::config::runtime_settings::RuntimeSettings;

/// Mirror of Jackett.Common/Services/ConfigurationService.cs
///
/// C# read/wrote `serverconfig.json` in the data folder and observed
/// ServerConfig changes; here we persist with serde_json.
pub struct ConfigurationService {
    runtime_settings: RuntimeSettings,
}

impl ConfigurationService {
    pub fn new(runtime_settings: RuntimeSettings) -> Self {
        Self { runtime_settings }
    }

    pub fn server_config_path(&self) -> std::path::PathBuf {
        self.runtime_settings.data_folder().join("serverconfig.json")
    }

    /// Mirror of `Load` — reads serverconfig.json, creating defaults when absent.
    pub fn load(&self) -> Result<ServerConfig> {
        let path = self.server_config_path();
        if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            let mut config: ServerConfig = serde_json::from_str(&text)?;
            config.on_deserialized();
            Ok(config)
        } else {
            Ok(ServerConfig::new(self.runtime_settings.clone()))
        }
    }

    /// Mirror of `Save`.
    pub fn save(&self, config: &ServerConfig) -> Result<()> {
        let path = self.server_config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(config)?)?;
        Ok(())
    }
}
