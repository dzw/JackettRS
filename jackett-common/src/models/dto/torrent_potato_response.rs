use serde::Serialize;

use super::torrent_potato_response_item::TorrentPotatoResponseItem;

/// Mirror of Jackett.Common/Models/DTO/TorrentPotatoResponse.cs
#[derive(Debug, Clone, Default, Serialize)]
pub struct TorrentPotatoResponse {
    #[serde(rename = "results")]
    pub results: Vec<TorrentPotatoResponseItem>,
}

impl TorrentPotatoResponse {
    pub fn total_results(&self) -> usize {
        self.results.len()
    }
}
