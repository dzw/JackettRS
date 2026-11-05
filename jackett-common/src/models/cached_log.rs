/// Mirror of Jackett.Common/Models/CachedLog.cs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedLog {
    pub level: Option<String>,
    pub message: Option<String>,
    pub when: chrono::DateTime<chrono::Local>,
}
