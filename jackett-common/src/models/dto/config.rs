/// Mirror of Jackett.Common/Models/DTO/Config.cs (ConfigItem)
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct ConfigItem {
    pub id: Option<String>,
    pub value: Option<String>,
    /// for array data (e.g. checkboxes)
    #[serde(default)]
    pub values: Vec<String>,
    /// for cookie alternative login (captcha needed + remote host)
    pub cookie: Option<String>,
}

/// The DTO container used by the config endpoints.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct Config {
    pub config: Vec<ConfigItem>,
}
