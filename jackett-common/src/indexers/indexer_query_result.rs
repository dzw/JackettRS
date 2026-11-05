use crate::models::release_info::ReleaseInfo;

/// Mirror of Jackett.Common/Indexers/IndexerQueryResult.cs
#[derive(Debug, Clone, Default)]
pub struct IndexerQueryResult {
    pub releases: Vec<ReleaseInfo>,
    /// A search that produced no results but may succeed later
    /// (mirror of `HasRssNavigation`/retry semantics kept simple).
    pub query_was_cached: bool,
}
