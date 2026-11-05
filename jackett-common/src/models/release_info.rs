use serde::{Deserialize, Serialize};
use url::Url;

/// Mirror of Jackett.Common/Models/ReleaseInfo.cs
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub title: Option<String>,
    pub guid: Option<Url>,
    pub link: Option<Url>,
    pub details: Option<Url>,
    pub publish_date: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub category: Vec<i32>,
    pub size: Option<i64>,
    pub files: Option<i64>,
    pub grabs: Option<i64>,
    pub description: Option<String>,
    pub rage_id: Option<i64>,
    #[serde(rename = "tvdbId")]
    pub tvdb_id: Option<i64>,
    pub imdb: Option<i64>,
    #[serde(rename = "tmDb")]
    pub tmdb: Option<i64>,
    #[serde(rename = "tvMazeId")]
    pub tvmaze_id: Option<i64>,
    pub trakt_id: Option<i64>,
    pub douban_id: Option<i64>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub subs: Vec<String>,
    pub year: Option<i64>,
    pub author: Option<String>,
    pub book_title: Option<String>,
    pub publisher: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub label: Option<String>,
    pub track: Option<String>,
    pub seeders: Option<i64>,
    pub peers: Option<i64>,
    pub poster: Option<Url>,
    pub info_hash: Option<String>,
    pub magnet_uri: Option<Url>,
    pub minimum_ratio: Option<f64>,
    pub minimum_seed_time: Option<i64>,
    pub download_volume_factor: Option<f64>,
    pub upload_volume_factor: Option<f64>,
    /// Not serialized to the manual search API (mirrors C# `[JsonIgnore] Origin`).
    /// In Rust we hold the indexer's id string instead of an `IIndexer` reference.
    #[serde(skip)]
    pub origin: Option<String>,
}

impl ReleaseInfo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn gigabytes_from_bytes(size: Option<f64>) -> Option<f64> {
        size.map(|s| s / 1024.0 / 1024.0 / 1024.0)
    }

    pub fn gain(&self) -> Option<f64> {
        let seeders = self.seeders? as f64;
        Some(seeders * Self::gigabytes_from_bytes(self.size.map(|s| s as f64))?)
    }
}

impl std::fmt::Display for ReleaseInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[ReleaseInfo: Title={:?}, Guid={}, Link={}, Details={}, PublishDate={}, Category={:?}, Size={}, Seeders={}, Peers={}, Gain={}]",
            self.title,
            fmt_opt_url(&self.guid),
            fmt_opt_url(&self.link),
            fmt_opt_url(&self.details),
            fmt_opt(self.publish_date.map(|d| d.to_rfc3339())),
            self.category,
            fmt_opt(self.size),
            fmt_opt(self.seeders),
            fmt_opt(self.peers),
            fmt_opt(self.gain()),
        )
    }
}

fn fmt_opt<T: std::fmt::Display>(v: Option<T>) -> String {
    v.map(|v| v.to_string()).unwrap_or_default()
}

fn fmt_opt_url(v: &Option<Url>) -> String {
    v.as_ref().map(|u| u.to_string()).unwrap_or_default()
}
