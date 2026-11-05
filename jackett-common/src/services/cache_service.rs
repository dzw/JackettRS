use std::collections::HashMap;
use std::sync::Mutex;

use chrono::Local;

use crate::models::cached_query_result::CachedQueryResult;
use crate::models::release_info::ReleaseInfo;
use crate::models::tracker_cache_result::TrackerCacheResult;

/// Mirror of Jackett.Common/Services/CacheService.cs
pub struct CacheService {
    cache: Mutex<HashMap<String, CachedQueryResult>>,
}

impl CacheService {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Cache key: indexer id + query fingerprint (mirror of C# key construction).
    fn cache_key(indexer_id: &str, query: &crate::models::torznab_query::TorznabQuery) -> String {
        format!(
            "{}|{}|{:?}|{:?}|{:?}",
            indexer_id,
            query.get_query_string(),
            query.categories,
            query.season,
            query.episode
        )
    }

    /// Mirror of `CacheResult`.
    pub fn cache_result(
        &self,
        indexer_id: &str,
        query: &crate::models::torznab_query::TorznabQuery,
        releases: &[ReleaseInfo],
    ) {
        let mut cache = self.cache.lock().unwrap();
        let entry = CachedQueryResult::new(&query.get_query_string(), releases.to_vec());
        cache.insert(Self::cache_key(indexer_id, query), entry);
    }

    /// Mirror of `Search`: returns cached releases if fresh enough.
    pub fn search(
        &self,
        indexer_id: &str,
        query: &crate::models::torznab_query::TorznabQuery,
        cache_ttl_seconds: i64,
        cache_max_results_per_indexer: i64,
    ) -> Option<Vec<ReleaseInfo>> {
        let cache = self.cache.lock().unwrap();
        let entry = cache.get(&Self::cache_key(indexer_id, query))?;
        let created: chrono::DateTime<Local> = entry.created;
        let elapsed = (Local::now() - created).num_seconds();
        if elapsed > cache_ttl_seconds {
            return None;
        }
        let max = cache_max_results_per_indexer.max(0) as usize;
        Some(entry.results().iter().take(max).cloned().collect())
    }

    /// Mirror of `CacheService.Results` (read side): returns every cached
    /// release across all indexers, each tagged with its source indexer id.
    pub fn get_all(&self) -> Vec<TrackerCacheResult> {
        let cache = self.cache.lock().unwrap();
        let mut out = Vec::new();
        for (key, entry) in cache.iter() {
            let indexer_id = key.split('|').next().unwrap_or("").to_string();
            for r in entry.results() {
                let mut tcr = TrackerCacheResult::new(r.clone());
                tcr.tracker_id = Some(indexer_id.clone());
                tcr.tracker = Some(indexer_id.clone());
                out.push(tcr);
            }
        }
        out
    }
}

impl Default for CacheService {
    fn default() -> Self {
        Self::new()
    }
}
