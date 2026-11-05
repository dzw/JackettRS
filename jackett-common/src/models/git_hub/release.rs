use chrono::{DateTime, Utc};

use super::asset::Asset;

/// Mirror of Jackett.Common/Models/GitHub/Release.cs
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Release {
    pub name: Option<String>,
    #[serde(rename = "Created_at")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}
