use url::Url;

use crate::indexers::i_indexer::IIndexer;
use crate::models::torznab_query::TorznabQuery;

/// Mirror of Jackett.Common/Indexers/IndexerBase.cs (shared behaviour stub:
/// request throttling, error wrapping, capabilities caching live in C# base class).
pub struct IndexerBase {
    pub id: String,
    pub name: String,
    pub description: String,
    pub type_: String,
    pub site_link: String,
    pub language: String,
    pub tags: Vec<String>,
    pub torznab_caps: crate::models::torznab_capabilities::TorznabCapabilities,
    pub is_configured: bool,
    pub last_error: Option<String>,
}

impl IndexerBase {
    /// Mirrors `PerformQuery` error wrapping the concrete indexers implement.
    pub async fn perform_query(
        &self,
        indexer: &dyn IIndexer,
        query: &TorznabQuery,
    ) -> Result<Vec<crate::models::release_info::ReleaseInfo>, anyhow::Error> {
        match indexer.query(query).await {
            Ok(releases) => Ok(releases),
            Err(e) => {
                Err(anyhow::anyhow!("[{}] {}", self.id, e))
            }
        }
    }

    /// Mirror of `ResolveSiteLink` — picks first reachable site link.
    pub fn resolve_site_link(&self, alternative_links: &[String]) -> Url {
        let mut candidates = std::iter::once(self.site_link.clone())
            .chain(alternative_links.iter().cloned());
        candidates
            .find_map(|l| Url::parse(&l).ok())
            .unwrap_or_else(|| Url::parse("http://localhost/").unwrap())
    }
}
