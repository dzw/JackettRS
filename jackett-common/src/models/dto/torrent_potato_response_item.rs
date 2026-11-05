use serde::Serialize;

/// Mirror of Jackett.Common/Models/DTO/TorrentPotatoResponseItem.cs
#[derive(Debug, Clone, Default, Serialize)]
pub struct TorrentPotatoResponseItem {
    #[serde(rename = "release_name")]
    pub release_name: Option<String>,
    #[serde(rename = "torrent_id")]
    pub torrent_id: Option<String>,
    #[serde(rename = "details_url")]
    pub details_url: Option<String>,
    #[serde(rename = "download_url")]
    pub download_url: Option<String>,
    #[serde(rename = "imdb_id")]
    pub imdb_id: Option<String>,
    #[serde(rename = "freeleech")]
    pub freeleech: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    #[serde(rename = "size")]
    pub size: i64,
    #[serde(rename = "leechers")]
    pub leechers: i64,
    #[serde(rename = "seeders")]
    pub seeders: i64,
    #[serde(rename = "publish_date")]
    pub publish_date: Option<String>,
}
