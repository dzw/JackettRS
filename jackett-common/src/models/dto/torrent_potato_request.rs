use crate::models::torznab_cat_type::TorznabCatType;
use crate::models::torznab_query::TorznabQuery;

/// Mirror of Jackett.Common/Models/DTO/TorrentPotatoRequest.cs
#[derive(Debug, Clone, Default)]
pub struct TorrentPotatoRequest {
    pub username: Option<String>,
    pub imdb_id: Option<String>,
    pub search: Option<String>,
}

impl TorrentPotatoRequest {
    pub fn to_torznab_query(request: &TorrentPotatoRequest) -> TorznabQuery {
        TorznabQuery {
            categories: vec![TorznabCatType::movies().id],
            search_term: request.search.clone(),
            imdb_id: request.imdb_id.clone(),
            query_type: Some("TorrentPotato".to_string()),
            ..Default::default()
        }
    }
}
