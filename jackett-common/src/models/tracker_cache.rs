use std::collections::HashMap;

use super::tracker_cache_query::TrackerCacheQuery;

/// Mirror of Jackett.Common/Models/TrackerCache.cs
#[derive(Debug, Clone, Default)]
pub struct TrackerCache {
    pub tracker_id: Option<String>,
    pub tracker_name: Option<String>,
    pub tracker_type: Option<String>,
    pub queries: HashMap<String, TrackerCacheQuery>,
}
