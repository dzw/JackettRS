use chrono::Local;
use url::Url;

use super::release_info::ReleaseInfo;
use super::torznab_cat_type::TorznabCatType;

/// Mirror of Jackett.Common/Models/TrackerCacheResult.cs
/// (C# inherits ReleaseInfo; in Rust we compose and expose the release fields.)
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct TrackerCacheResult {
    pub release: ReleaseInfo,
    pub first_seen: Option<chrono::DateTime<Local>>,
    pub tracker: Option<String>,
    pub tracker_id: Option<String>,
    pub tracker_type: Option<String>,
    pub category_desc: Option<String>,
    pub blackhole_link: Option<Url>,
}

impl TrackerCacheResult {
    pub fn new(release_info: ReleaseInfo) -> Self {
        let mut r = release_info.clone();
        let category_desc = if r.category.is_empty() {
            String::new()
        } else {
            r.category
                .iter()
                .map(|c| TorznabCatType::get_cat_desc(*c))
                .filter(|x| !x.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        };

        // Use peers as leechers
        r.peers = r
            .peers
            .zip(r.seeders)
            .map(|(p, s)| p - s)
            .or(r.peers);

        Self {
            release: r,
            first_seen: Some(Local::now()),
            tracker: None,
            tracker_id: None,
            tracker_type: None,
            category_desc: Some(category_desc),
            blackhole_link: None,
        }
    }

    // Pass-through accessors for common release fields
    pub fn title(&self) -> Option<&String> {
        self.release.title.as_ref()
    }
    pub fn seeders(&self) -> Option<i64> {
        self.release.seeders
    }
    pub fn peers(&self) -> Option<i64> {
        self.release.peers
    }
}
