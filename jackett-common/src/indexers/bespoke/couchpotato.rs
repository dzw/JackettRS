//! Mirror of Jackett.Common/Indexers/Definitions/Abstract/CouchPotatoTracker.cs.
//!
//! CouchPotato / TorrentPotato-style JSON API:
//! `GET {SiteLink}{endpoint}?imdbid=&search=&passkey=&user=` returning
//! `{error, total_results, results[]}`. The C# base currently has **no derived
//! indexers** (legacy), so this port exposes a generic builder that can be
//! pointed at any compatible endpoint via env.

use std::sync::Arc;

use async_trait::async_trait;
use url::Url;

use crate::indexers::i_indexer::IIndexer;
use crate::models::release_info::ReleaseInfo;
use crate::models::torznab_capabilities::TorznabCapabilities;
use crate::models::torznab_query::TorznabQuery;
use crate::utils::browser_util::BrowserUtil;
use crate::utils::date_time_util::DateTimeUtil;

#[derive(Clone, Default)]
pub struct CouchPotatoConfig {
    pub username: String,
    pub passkey: String,
}

pub struct CouchPotatoSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub site_link: &'static str,
    pub language: &'static str,
    pub endpoint: &'static str,
    pub caps: fn() -> TorznabCapabilities,
}

impl CouchPotatoSpec {
    pub fn build(&self, config: CouchPotatoConfig) -> anyhow::Result<Arc<dyn IIndexer>> {
        let client = reqwest::Client::builder()
            .user_agent(BrowserUtil::default_user_agent())
            .build()?;
        Ok(Arc::new(CouchPotatoTracker {
            id: self.id.to_string(),
            name: self.name.to_string(),
            description: self.description.to_string(),
            site_link: self.site_link.trim_end_matches('/').to_string(),
            language: self.language.to_string(),
            endpoint: self.endpoint.to_string(),
            caps: (self.caps)(),
            client,
            config,
        }))
    }
}

/// Builds a CouchPotato/TorrentPotato-compatible indexer from dynamic (env)
/// values. The C# abstract base has no derived classes; this generic builder
/// makes the port usable for any compatible endpoint.
pub fn build_generic(
    id: String,
    name: String,
    site_link: String,
    endpoint: String,
    config: CouchPotatoConfig,
) -> anyhow::Result<Arc<dyn IIndexer>> {
    let client = reqwest::Client::builder()
        .user_agent(BrowserUtil::default_user_agent())
        .build()?;
    Ok(Arc::new(CouchPotatoTracker {
        description: format!("{name} (CouchPotato/TorrentPotato API)"),
        language: "en-US".to_string(),
        caps: generic_caps(),
        id,
        name,
        site_link: site_link.trim_end_matches('/').to_string(),
        endpoint,
        client,
        config,
    }))
}

fn generic_caps() -> TorznabCapabilities {
    let mut caps = TorznabCapabilities::new();
    for (key, name) in [("movie", "Movies"), ("tv", "TV"), ("music", "Audio")] {
        if let Some(cat) = crate::models::torznab_cat_type::TorznabCatType::get_cat_by_name(name) {
            caps.categories.add_category_mapping(key, cat.clone(), None);
        }
    }
    caps
}

pub struct CouchPotatoTracker {
    id: String,
    name: String,
    description: String,
    site_link: String,
    language: String,
    endpoint: String,
    caps: TorznabCapabilities,
    client: reqwest::Client,
    config: CouchPotatoConfig,
}

impl CouchPotatoTracker {
    fn api_url(&self) -> String {
        format!("{}{}", self.site_link, self.endpoint)
    }

    async fn run_query(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let mut url = Url::parse(&self.api_url())?;
        {
            let mut q = url.query_pairs_mut();
            if let Some(imdb) = &query.imdb_id {
                q.append_pair("imdbid", imdb);
            }
            q.append_pair("search", &query.get_query_string());
            q.append_pair("passkey", &self.config.passkey);
            q.append_pair("user", &self.config.username);
        }
        let resp = self.client.get(url).send().await?;
        let body = resp.text().await?;
        parse_couchpotato_response(&body, &self.caps)
    }
}

#[async_trait]
impl IIndexer for CouchPotatoTracker {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn type_(&self) -> &str {
        "private"
    }
    fn site_link(&self) -> &str {
        &self.site_link
    }
    fn alternative_site_links(&self) -> Vec<String> {
        Vec::new()
    }
    fn language(&self) -> &str {
        &self.language
    }
    fn tags(&self) -> Vec<String> {
        Vec::new()
    }
    fn is_configured(&self) -> bool {
        !self.config.username.is_empty() && !self.config.passkey.is_empty()
    }
    fn last_error(&self) -> Option<String> {
        None
    }
    fn torznab_caps(&self) -> &TorznabCapabilities {
        &self.caps
    }
    async fn query(
        &self,
        query: &TorznabQuery,
    ) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        self.run_query(query).await
    }
    async fn download(&self, link: &Url) -> Result<Vec<u8>, anyhow::Error> {
        let resp = self.client.get(link.clone()).send().await?;
        Ok(resp.bytes().await?.to_vec())
    }
}

/// Parses a CouchPotato/TorrentPotato JSON response (offline-testable).
pub fn parse_couchpotato_response(
    body: &str,
    caps: &TorznabCapabilities,
) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
    let json: serde_json::Value = serde_json::from_str(body)?;
    if let Some(err) = json.get("error").and_then(|v| v.as_str()) {
        anyhow::bail!("{err}");
    }
    if json
        .get("total_results")
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
        == 0
    {
        return Ok(Vec::new());
    }
    let mut releases = Vec::new();
    let empty = Vec::new();
    let rows = json
        .get("results")
        .and_then(|v| v.as_array())
        .unwrap_or(&empty);

    for r in rows {
        let mut rel = ReleaseInfo::new();
        rel.title = r
            .get("release_name")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if let Some(u) = r.get("details_url").and_then(|v| v.as_str()) {
            rel.details = Url::parse(u).ok();
            rel.guid = rel.details.clone();
        }
        if let Some(u) = r.get("download_url").and_then(|v| v.as_str()) {
            rel.link = Url::parse(u).ok();
        }
        if let Some(imdb) = r.get("imdb_id").and_then(|v| v.as_str()) {
            let digits: String = imdb.chars().filter(|c| c.is_ascii_digit()).collect();
            rel.imdb = digits.parse().ok();
        }
        let freeleech = r
            .get("freeleech")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        rel.download_volume_factor = Some(if freeleech { 0.0 } else { 1.0 });
        rel.upload_volume_factor = Some(1.0);
        if let Some(t) = r.get("type").and_then(|v| v.as_str()) {
            rel.category = caps.categories.map_tracker_cat_to_newznab(t);
        }
        // `size` is expressed in megabytes.
        rel.size = r
            .get("size")
            .and_then(|v| v.as_i64())
            .map(|mb| mb * 1024 * 1024);
        let seeders = r.get("seeders").and_then(|v| v.as_i64());
        let leechers = r.get("leechers").and_then(|v| v.as_i64());
        rel.seeders = seeders;
        rel.peers = leechers.map(|l| l + seeders.unwrap_or(0));
        rel.publish_date = r
            .get("publish_date")
            .and_then(|v| v.as_str())
            .and_then(DateTimeUtil::from_unknown);
        releases.push(rel);
    }
    Ok(releases)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::torznab_cat_type::TorznabCatType;

    fn caps() -> TorznabCapabilities {
        let mut c = TorznabCapabilities::new();
        if let Some(cat) = TorznabCatType::get_cat_by_name("Movies") {
            c.categories.add_category_mapping("movie", cat.clone(), None);
        }
        c
    }

    #[test]
    fn parses_couchpotato_response_offline() {
        let body = r#"{
          "error": null,
          "total_results": 1,
          "results": [{
            "release_name": "Big Buck Bunny 1080p",
            "details_url": "https://example.com/details/1",
            "download_url": "https://example.com/dl/1",
            "imdb_id": "tt1254207",
            "freeleech": true,
            "type": "movie",
            "size": 1536,
            "seeders": 10,
            "leechers": 2,
            "publish_date": "2024-01-02 11:22:33"
          }]
        }"#;
        let rels = parse_couchpotato_response(body, &caps()).unwrap();
        assert_eq!(rels.len(), 1);
        let r = &rels[0];
        assert_eq!(r.title.as_deref(), Some("Big Buck Bunny 1080p"));
        assert_eq!(r.imdb, Some(1254207));
        assert!(r.category.contains(&2000), "cat: {:?}", r.category);
        assert_eq!(r.size, Some(1536 * 1024 * 1024));
        assert_eq!(r.peers, Some(12));
        assert_eq!(r.download_volume_factor, Some(0.0));
        assert!(r.publish_date.is_some());
    }

    #[test]
    fn no_results_returns_empty() {
        let rels = parse_couchpotato_response(r#"{"total_results":0,"results":[]}"#, &caps())
            .unwrap();
        assert!(rels.is_empty());
    }
}
