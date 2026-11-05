use chrono::Local;

use super::release_info::ReleaseInfo;

/// Mirror of Jackett.Common/Models/TrackerCacheQuery.cs
#[derive(Debug, Clone, Default)]
pub struct TrackerCacheQuery {
    pub created: Option<chrono::DateTime<Local>>,
    pub results: Vec<ReleaseInfo>,
}

impl TrackerCacheQuery {
    pub fn new() -> Self {
        Self {
            created: Some(Local::now()),
            results: Vec::new(),
        }
    }
}
