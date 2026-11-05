use chrono::Local;

use super::release_info::ReleaseInfo;

/// Mirror of Jackett.Common/Models/CachedQueryResult.cs
#[derive(Debug, Clone)]
pub struct CachedQueryResult {
    results: Vec<ReleaseInfo>,
    pub created: chrono::DateTime<Local>,
    pub query: Option<String>,
}

impl CachedQueryResult {
    pub fn new(query: &str, results: Vec<ReleaseInfo>) -> Self {
        Self {
            results,
            created: Local::now(),
            query: Some(query.to_string()),
        }
    }

    pub fn results(&self) -> &[ReleaseInfo] {
        &self.results
    }
}
