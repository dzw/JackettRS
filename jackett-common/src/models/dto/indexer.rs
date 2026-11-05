use serde::Serialize;

use crate::indexers::i_indexer::IIndexer;
use crate::models::torznab_cat_type::TorznabCatType;

#[derive(Debug, Clone, Serialize)]
pub struct Capability {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Name")]
    pub name: Option<String>,
}

/// Mirror of Jackett.Common/Models/DTO/Indexer.cs
#[derive(Debug, Clone, Serialize)]
pub struct Indexer {
    #[serde(rename = "id")]
    pub id: Option<String>,
    #[serde(rename = "name")]
    pub name: Option<String>,
    #[serde(rename = "description")]
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    #[serde(rename = "configured")]
    pub configured: bool,
    #[serde(rename = "site_link")]
    pub site_link: String,
    #[serde(rename = "alternativesitelinks")]
    pub alternative_site_links: Vec<String>,
    #[serde(rename = "language")]
    pub language: Option<String>,
    #[serde(rename = "tags")]
    pub tags: Vec<String>,
    #[serde(rename = "last_error")]
    pub last_error: String,
    #[serde(rename = "potatoenabled")]
    pub potato_enabled: bool,
    #[serde(rename = "caps")]
    pub caps: Vec<Capability>,
}

impl Indexer {
    pub fn from_indexer(indexer: &dyn IIndexer) -> Self {
        let tree = indexer.torznab_caps().categories.get_torznab_category_tree(false);
        let potato_enabled = tree.iter().any(|i| TorznabCatType::movies().contains(i));

        let caps = indexer
            .torznab_caps()
            .categories
            .get_torznab_category_list(true)
            .into_iter()
            .map(|c| Capability {
                id: c.id.to_string(),
                name: c.name,
            })
            .collect();

        Self {
            id: Some(indexer.id().to_string()),
            name: Some(indexer.name().to_string()),
            description: Some(indexer.description().to_string()),
            type_: Some(indexer.type_().to_string()),
            configured: indexer.is_configured(),
            site_link: indexer.site_link().to_string(),
            alternative_site_links: indexer.alternative_site_links(),
            language: Some(indexer.language().to_string()),
            tags: indexer.tags(),
            last_error: indexer.last_error().unwrap_or_default(),
            potato_enabled,
            caps,
        }
    }
}
