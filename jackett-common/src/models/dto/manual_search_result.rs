use serde::Serialize;

use crate::models::tracker_cache_result::TrackerCacheResult;

/// Mirror of Jackett.Common/Models/DTO/ManualSearchResult.cs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ManualSearchResultIndexerStatus {
    Unknown = 0,
    Error = 1,
    OK = 2,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManualSearchResultIndexer {
    #[serde(rename = "ID")]
    pub id: Option<String>,
    #[serde(rename = "Name")]
    pub name: Option<String>,
    #[serde(rename = "Status")]
    pub status: ManualSearchResultIndexerStatus,
    #[serde(rename = "Results")]
    pub results: i32,
    #[serde(rename = "Error")]
    pub error: Option<String>,
    #[serde(rename = "ElapsedTime")]
    pub elapsed_time: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManualSearchResult {
    #[serde(rename = "Results")]
    pub results: Vec<TrackerCacheResult>,
    #[serde(rename = "Indexers")]
    pub indexers: Vec<ManualSearchResultIndexer>,
}
