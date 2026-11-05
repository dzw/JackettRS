/// Mirror of Jackett.Common/Models/GitHub/Asset.cs
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Asset {
    pub name: Option<String>,
    #[serde(rename = "Browser_download_url")]
    pub browser_download_url: Option<String>,
}
