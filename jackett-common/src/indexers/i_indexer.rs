use async_trait::async_trait;

use crate::models::torznab_capabilities::TorznabCapabilities;
use crate::models::torznab_query::TorznabQuery;

/// Mirror of Jackett.Common/Indexers/IIndexer.cs
///
/// C# interface methods become an async trait. Returns are owned values since
/// Rust has no shared mutable object graph like the C# service locator.
#[async_trait]
pub trait IIndexer: Send + Sync {
    fn id(&self) -> &str;

    fn name(&self) -> &str;

    fn description(&self) -> &str;

    fn type_(&self) -> &str;

    fn site_link(&self) -> &str;

    fn alternative_site_links(&self) -> Vec<String>;

    fn language(&self) -> &str;

    fn tags(&self) -> Vec<String>;

    fn is_configured(&self) -> bool;

    fn last_error(&self) -> Option<String>;

    fn torznab_caps(&self) -> &TorznabCapabilities;

    /// Performs a Torznab query against the indexer.
    async fn query(
        &self,
        query: &TorznabQuery,
    ) -> Result<Vec<crate::models::release_info::ReleaseInfo>, anyhow::Error>;

    /// Downloads the torrent at `link` (mirrors `Download(Uri link)`).
    async fn download(&self, link: &url::Url) -> Result<Vec<u8>, anyhow::Error>;
}
