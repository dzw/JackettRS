//! Data-driven framework for the simpler independent bespoke indexers.
//!
//! Mirrors the "single JSON/RSS request, no browser login" subset of
//! `Jackett.Common/Indexers/Definitions/*.cs`. Each site is a [`SimpleSpec`]
//! (metadata + caps + a [`SimpleKind`]); `SimpleIndexer::query` dispatches to a
//! small per-kind request/parse function. Parsing functions are free functions
//! so they can be unit-tested offline.
//!
//! Scope: the remaining `IndexerBase`-derived C# definitions are deliberately
//! *not* in this table. Sites such as `AudioBookBay`, `EpubLibre`, `DivxTotal`,
//! `MejorTorrent`, `DonTorrent`, `TorrentNetwork` and the Cloudflare/PoW
//! trackers are 500-1000 line HTML scrapers (AngleSharp `:contains()`,
//! sibling-node walks, multi-step login/captcha flows). They need dedicated
//! ports rather than a single-request JSON/RSS framework; Cardigann YAML
//! already covers the ones with a declarative definition.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use quick_xml::events::Event;
use quick_xml::Reader;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde_json::Value;
use url::Url;

use crate::indexers::i_indexer::IIndexer;
use crate::models::release_info::ReleaseInfo;
use crate::models::torznab_capabilities::{
    BookSearchParam, MovieSearchParam, MusicSearchParam, TorznabCapabilities, TvSearchParam,
};
use crate::models::torznab_cat_type::TorznabCatType;
use crate::models::torznab_query::TorznabQuery;
use crate::utils::browser_util::BrowserUtil;
use crate::utils::date_time_util::DateTimeUtil;

/// Credentials gathered from the environment for simple indexers.
#[derive(Clone, Default)]
pub struct SimpleConfig {
    pub username: String,
    pub password: String,
    pub api_key: String,
    pub api_user: String,
    pub rss_key: String,
    pub passkey: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuthKind {
    None,
    /// HTTP Basic (`Magnetico`).
    Basic,
    /// A single API key header, e.g. `x-api-key`.
    ApiKeyHeader(&'static str),
    /// PTP's `ApiUser` + `ApiKey` headers.
    ApiUserKey,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SimpleKind {
    SubsPlease,
    TorrentsCsv,
    Knaben,
    Magnetico,
    EraiRaws,
    Anilibria,
    PassThePopcorn,
    BeyondHd,
    Hdbits,
    MteamTp,
    Nebulance,
}

/// `(tracker category id, Torznab category name, tracker description)`.
pub type CategoryDef = (&'static str, &'static str, &'static str);

pub struct SimpleSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub site_link: &'static str,
    pub language: &'static str,
    pub indexer_type: &'static str,
    pub kind: SimpleKind,
    pub auth: AuthKind,
    /// Config fields that must be non-empty before the indexer is enabled.
    pub required: &'static [&'static str],
    pub tv_params: &'static [TvSearchParam],
    pub movie_params: &'static [MovieSearchParam],
    pub music_params: &'static [MusicSearchParam],
    pub book_params: &'static [BookSearchParam],
    pub categories: &'static [CategoryDef],
    pub limits_default: Option<i32>,
    pub limits_max: Option<i32>,
    pub supports_raw_search: bool,
}

impl SimpleSpec {
    pub fn caps(&self) -> TorznabCapabilities {
        let mut caps = TorznabCapabilities::new();
        caps.tv_search_params = self.tv_params.to_vec();
        caps.movie_search_params = self.movie_params.to_vec();
        caps.music_search_params = self.music_params.to_vec();
        caps.book_search_params = self.book_params.to_vec();
        if self.limits_default.is_some() {
            caps.limits_default = self.limits_default;
        }
        if self.limits_max.is_some() {
            caps.limits_max = self.limits_max;
        }
        caps.supports_raw_search = self.supports_raw_search;
        for (id, cat_name, desc) in self.categories {
            if let Some(cat) = TorznabCatType::get_cat_by_name(cat_name) {
                caps.categories.add_category_mapping(
                    id,
                    cat.clone(),
                    if desc.is_empty() { None } else { Some(desc) },
                );
            }
        }
        caps
    }

    pub fn build(&self, config: SimpleConfig) -> anyhow::Result<Arc<dyn IIndexer>> {
        let client = reqwest::Client::builder()
            .user_agent(BrowserUtil::default_user_agent())
            .build()?;
        Ok(Arc::new(SimpleIndexer {
            id: self.id.to_string(),
            name: self.name.to_string(),
            description: self.description.to_string(),
            site_link: self.site_link.trim_end_matches('/').to_string(),
            language: self.language.to_string(),
            indexer_type: self.indexer_type.to_string(),
            kind: self.kind,
            auth: self.auth,
            required: self.required,
            caps: self.caps(),
            client,
            config,
        }))
    }

    /// Whether credentials are required for this indexer.
    pub fn requires_credentials(&self) -> bool {
        !self.required.is_empty()
    }
}

impl SimpleConfig {
    pub fn field(&self, name: &str) -> &str {
        match name {
            "username" => &self.username,
            "password" => &self.password,
            "api_key" => &self.api_key,
            "api_user" => &self.api_user,
            "rss_key" => &self.rss_key,
            "passkey" => &self.passkey,
            _ => "",
        }
    }

    pub fn is_complete(&self, required: &[&str]) -> bool {
        required.iter().all(|f| !self.field(f).is_empty())
    }
}

pub struct SimpleIndexer {
    id: String,
    name: String,
    description: String,
    site_link: String,
    language: String,
    indexer_type: String,
    kind: SimpleKind,
    auth: AuthKind,
    required: &'static [&'static str],
    caps: TorznabCapabilities,
    client: reqwest::Client,
    config: SimpleConfig,
}

impl SimpleIndexer {
    fn headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        match self.auth {
            AuthKind::None => {}
            AuthKind::Basic => {
                let token = base64_encode(&format!("{}:{}", self.config.username, self.config.password));
                if let Ok(v) = HeaderValue::from_str(&format!("Basic {token}")) {
                    h.insert(AUTHORIZATION, v);
                }
            }
            AuthKind::ApiKeyHeader(name) => {
                if let Ok(v) = HeaderValue::from_str(&self.config.api_key) {
                    if let Ok(n) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) {
                        h.insert(n, v);
                    }
                }
            }
            AuthKind::ApiUserKey => {
                if let (Ok(u), Ok(k)) = (
                    HeaderValue::from_str(&self.config.api_user),
                    HeaderValue::from_str(&self.config.api_key),
                ) {
                    h.insert("ApiUser", u);
                    h.insert("ApiKey", k);
                }
            }
        }
        h
    }

    async fn run_query(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        match self.kind {
            SimpleKind::SubsPlease => self.query_subsplease(query).await,
            SimpleKind::TorrentsCsv => self.query_torrentscsv(query).await,
            SimpleKind::Knaben => self.query_knaben(query).await,
            SimpleKind::Magnetico => self.query_magnetico(query).await,
            SimpleKind::EraiRaws => self.query_erairaws(query).await,
            SimpleKind::Anilibria => self.query_anilibria(query).await,
            SimpleKind::PassThePopcorn => self.query_ptp(query).await,
            SimpleKind::BeyondHd => self.query_beyondhd(query).await,
            SimpleKind::Hdbits => self.query_hdbits(query).await,
            SimpleKind::MteamTp => self.query_mteamtp(query).await,
            SimpleKind::Nebulance => self.query_nebulance(query).await,
        }
    }

    async fn get(&self, url: &str) -> Result<String, anyhow::Error> {
        let resp = self.client.get(url).headers(self.headers()).send().await?;
        if resp.status().as_u16() >= 400 {
            anyhow::bail!("Invalid status code {} received from indexer", resp.status());
        }
        Ok(resp.text().await?)
    }

    async fn post_json(&self, url: &str, body: &Value) -> Result<String, anyhow::Error> {
        let mut headers = self.headers();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let resp = self
            .client
            .post(url)
            .headers(headers)
            .json(body)
            .send()
            .await?;
        if resp.status().as_u16() >= 400 {
            anyhow::bail!("Invalid status code {} received from indexer", resp.status());
        }
        Ok(resp.text().await?)
    }

    // -- SubsPlease --------------------------------------------------------
    async fn query_subsplease(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let mut term = query.search_term.clone().unwrap_or_default();
        term = term.replace("[SubsPlease]", "");
        static RES: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\d{3,4}p").unwrap());
        term = RES.replace_all(&term, "").trim().to_string();
        if let Some(season) = query.season {
            if season > 1 {
                term.push_str(&format!(" S{season:02}"));
            }
        }
        let mut url = Url::parse(&format!("{}/api/", self.site_link))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("tz", "UTC");
            if term.trim().is_empty() {
                q.append_pair("f", "latest");
            } else {
                q.append_pair("f", "search");
                q.append_pair("s", term.trim());
            }
        }
        let body = self.get(url.as_str()).await?;
        parse_subsplease_response(&body, &self.caps)
    }

    // -- Torrents.csv ------------------------------------------------------
    async fn query_torrentscsv(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let term = query.get_query_string();
        if term.len() < 3 {
            return Ok(Vec::new());
        }
        let mut url = Url::parse(&format!("{}/service/search", self.site_link))?;
        url.query_pairs_mut()
            .append_pair("size", "100")
            .append_pair("q", &term);
        let body = self.get(url.as_str()).await?;
        parse_torrentscsv_response(&body, &self.caps)
    }

    // -- Knaben ------------------------------------------------------------
    async fn query_knaben(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let mut body = serde_json::json!({
            "order_by": "date",
            "order_direction": "desc",
            "from": 0,
            "size": 100,
            "hide_unsafe": true,
        });
        let term = query.get_query_string();
        if !term.trim().is_empty() {
            body["search_type"] = Value::from("100%");
            body["search_field"] = Value::from("title");
            body["query"] = Value::from(term);
        }
        let tracker_cats = self.caps.categories.map_torznab_caps_to_trackers(query, true);
        if !tracker_cats.is_empty() {
            let ints: Vec<i64> = tracker_cats
                .iter()
                .filter_map(|c| c.parse().ok())
                .collect();
            if !ints.is_empty() {
                body["categories"] = serde_json::json!(ints);
            }
        }
        let text = self.post_json("https://api.knaben.org/v1", &body).await?;
        parse_knaben_response(&text, &self.caps)
    }

    // -- Magnetico ---------------------------------------------------------
    async fn query_magnetico(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let mut url = Url::parse(&format!("{}/api/v0.1/torrents", self.site_link))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("query", &query.get_query_string());
            q.append_pair("orderBy", "RELEVANCE");
            q.append_pair("ascending", "false");
            q.append_pair("limit", "100");
        }
        let body = self.get(url.as_str()).await?;
        parse_magnetico_response(&body, &self.caps)
    }

    // -- Erai-Raws ---------------------------------------------------------
    async fn query_erairaws(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let url = format!(
            "{}/feed/?type=torrent&token={}",
            self.site_link, self.config.rss_key
        );
        let body = self.get(&url).await?;
        parse_erairaws_response(&body, &self.caps, query)
    }

    // -- Anilibria ---------------------------------------------------------
    async fn query_anilibria(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let term = query.get_query_string();
        if term.trim().is_empty() {
            // RSS fallback: ids -> details (best-effort subset).
            let rss = self
                .get(&format!("{}/api/v1/anime/torrents/rss?limit=15", self.site_link))
                .await?;
            let ids = parse_anilibria_rss_ids(&rss);
            let mut out = Vec::new();
            for id in ids.into_iter().take(15) {
                if let Ok(detail) = self
                    .get(&format!("{}/api/v1/anime/torrents/{}", self.site_link, id))
                    .await
                {
                    out.extend(parse_anilibria_detail(&detail, &self.caps));
                }
            }
            return Ok(out);
        }
        let mut url = Url::parse(&format!("{}/api/v1/app/search/releases", self.site_link))?;
        url.query_pairs_mut().append_pair("query", &term);
        let search = self.get(url.as_str()).await?;
        let found: Vec<Value> = serde_json::from_str(&search).unwrap_or_default();
        let mut out = Vec::new();
        for item in found.into_iter().take(15) {
            let Some(id) = value_path(&item, "id").and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_i64().map(|n| n.to_string()))) else {
                continue;
            };
            if let Ok(detail) = self
                .get(&format!("{}/api/v1/anime/torrents/release/{id}", self.site_link))
                .await
            {
                out.extend(parse_anilibria_detail(&detail, &self.caps));
            }
        }
        Ok(out)
    }

    // -- PassThePopcorn ----------------------------------------------------
    async fn query_ptp(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let mut url = Url::parse(&format!("{}/torrents.php", self.site_link))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("action", "advanced");
            q.append_pair("json", "noredirect");
            q.append_pair("grouping", "0");
            q.append_pair("order_by", "time");
            q.append_pair("order_way", "desc");
            if let Some(imdb) = &query.imdb_id {
                q.append_pair("searchstr", imdb);
            } else if let Some(s) = &query.search_term {
                q.append_pair("searchstr", s);
            }
            if query.limit > 0 && query.offset > 0 {
                q.append_pair("page", &(query.offset / query.limit + 1).to_string());
            }
            for cat in self.caps.categories.map_torznab_caps_to_trackers(query, true) {
                q.append_pair(&format!("filter_cat[{cat}]"), "1");
            }
        }
        let body = self.get(url.as_str()).await?;
        parse_ptp_response(&body, &self.site_link, &self.caps)
    }

    // -- BeyondHD ----------------------------------------------------------
    async fn query_beyondhd(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let mut body = serde_json::json!({
            "action": "search",
            "rsskey": self.config.rss_key,
        });
        let term = query.get_query_string();
        if !term.trim().is_empty() {
            body["search"] = Value::from(term);
        }
        if let Some(imdb) = &query.imdb_id {
            body["imdb_id"] = Value::from(imdb.clone());
        }
        if let Some(tmdb) = query.tmdb_id {
            body["tmdb_id"] = Value::from(tmdb.to_string());
        }
        let url = format!("{}/api/torrents/{}", self.site_link, self.config.api_key);
        let text = self.post_json(&url, &body).await?;
        parse_beyondhd_response(&text, &self.caps)
    }

    // -- HDBits ------------------------------------------------------------
    async fn query_hdbits(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let mut body = serde_json::json!({
            "username": self.config.username,
            "passkey": self.config.passkey,
            "limit": 100,
        });
        if let Some(imdb) = &query.imdb_id {
            body["imdb"] = serde_json::json!({ "id": imdb });
        } else {
            let term = query.get_query_string();
            if !term.trim().is_empty() {
                body["search"] = Value::from(term);
            }
        }
        let url = format!("{}/api/torrents", self.site_link);
        let text = self.post_json(&url, &body).await?;
        parse_hdbits_response(&text, &self.site_link, &self.config.passkey, &self.caps)
    }

    // -- M-Team TP ---------------------------------------------------------
    async fn query_mteamtp(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let mut body = serde_json::json!({
            "mode": "Normal",
            "categories": [],
            "pageNumber": 1,
            "pageSize": 100,
        });
        let term = query.get_query_string();
        if !term.trim().is_empty() {
            body["keyword"] = Value::from(term);
        }
        let cats: Vec<String> = self
            .caps
            .categories
            .map_torznab_caps_to_trackers(query, true);
        if !cats.is_empty() {
            body["categories"] = serde_json::json!(cats);
        }
        let url = format!("{}/api/torrent/search", self.site_link);
        let text = self.post_json(&url, &body).await?;
        parse_mteamtp_response(&text, &self.caps)
    }

    // -- Nebulance ---------------------------------------------------------
    async fn query_nebulance(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let mut url = Url::parse(&format!("{}/api.php", self.site_link))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("action", "search");
            q.append_pair("api_key", &self.config.api_key);
            q.append_pair("age", ">0");
            q.append_pair("per_page", "100");
            if let Some(s) = &query.search_term {
                q.append_pair("name", s);
            }
            if let Some(tvmaze) = query.tvmaze_id {
                q.append_pair("tvmaze", &tvmaze.to_string());
            }
            if let Some(season) = query.season {
                q.append_pair("season", &season.to_string());
            }
            if let Some(ep) = &query.episode {
                q.append_pair("episode", ep);
            }
        }
        let body = self.get(url.as_str()).await?;
        parse_nebulance_response(&body, &self.caps)
    }
}

#[async_trait]
impl IIndexer for SimpleIndexer {
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
        &self.indexer_type
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
        self.config.is_complete(self.required)
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
        let resp = self
            .client
            .get(link.clone())
            .headers(self.headers())
            .send()
            .await?;
        Ok(resp.bytes().await?.to_vec())
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn base64_encode(s: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
}

/// Dot-path lookup into a `serde_json::Value`.
pub fn value_path<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = v;
    for part in path.split('.') {
        cur = cur.get(part)?;
    }
    Some(cur)
}

fn v_str(v: &Value, path: &str) -> Option<String> {
    value_path(v, path).and_then(|x| match x {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

fn v_i64(v: &Value, path: &str) -> Option<i64> {
    value_path(v, path).and_then(|x| match x {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    })
}

fn unix_time(secs: i64) -> Option<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp(secs, 0)
}

fn map_cat(caps: &TorznabCapabilities, key: &str) -> Vec<i32> {
    caps.categories.map_tracker_cat_to_newznab(key)
}

fn magnet_uri(s: &str) -> Option<Url> {
    Url::parse(s).ok()
}

// ---------------------------------------------------------------------------
// Per-site parsers (offline-testable)
// ---------------------------------------------------------------------------

pub fn parse_subsplease_response(
    body: &str,
    caps: &TorznabCapabilities,
) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
    let json: Value = serde_json::from_str(body)?;
    let mut releases = Vec::new();
    let entries: Vec<(&String, &Value)> = match &json {
        Value::Object(m) => m.iter().collect(),
        _ => Vec::new(),
    };
    for (_, r) in entries {
        let show = v_str(r, "show").unwrap_or_default();
        let episode = v_str(r, "episode").unwrap_or_default();
        let downloads = r.get("downloads").and_then(|v| v.as_array());
        let Some(downloads) = downloads else { continue };
        for d in downloads {
            let res = v_str(d, "res").unwrap_or_default();
            let magnet = v_str(d, "magnet").unwrap_or_default();
            let Ok(uri) = Url::parse(&magnet) else { continue };
            let mut rel = ReleaseInfo::new();
            rel.title = Some(format!("[SubsPlease] {show} - {episode} ({res})"));
            rel.magnet_uri = Some(uri.clone());
            rel.guid = Some(uri);
            rel.link = None;
            rel.category = map_cat(caps, "1");
            rel.publish_date = v_str(r, "release_date")
                .and_then(|s| DateTimeUtil::from_unknown(&s))
                .or_else(|| v_str(r, "time").and_then(|s| DateTimeUtil::from_unknown(&s)));
            rel.size = magnet_size(&magnet);
            rel.download_volume_factor = Some(0.0);
            rel.upload_volume_factor = Some(1.0);
            rel.seeders = Some(1);
            releases.push(rel);
        }
    }
    Ok(releases)
}

fn magnet_size(magnet: &str) -> Option<i64> {
    static XL: Lazy<Regex> = Lazy::new(|| Regex::new(r"[?&]xl=(\d+)").unwrap());
    XL.captures(magnet)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

pub fn parse_torrentscsv_response(
    body: &str,
    caps: &TorznabCapabilities,
) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
    let json: Value = serde_json::from_str(body)?;
    let rows = json.get("torrents").and_then(|v| v.as_array());
    let mut releases = Vec::new();
    let empty = Vec::new();
    for r in rows.unwrap_or(&empty) {
        let mut rel = ReleaseInfo::new();
        rel.title = v_str(r, "name");
        let hash = v_str(r, "infohash").unwrap_or_default();
        if !hash.is_empty() {
            let uri = Url::parse(&format!("magnet:?xt=urn:btih:{hash}")).ok();
            rel.magnet_uri = uri.clone();
            rel.guid = uri;
        }
        rel.size = v_str(r, "size_bytes")
            .and_then(|s| s.parse().ok())
            .or_else(|| v_i64(r, "size_bytes"));
        rel.publish_date = v_i64(r, "created_unix").and_then(unix_time);
        rel.seeders = v_i64(r, "seeders");
        rel.peers = v_i64(r, "leechers").map(|l| l + rel.seeders.unwrap_or(0));
        rel.grabs = v_i64(r, "completed");
        rel.category = map_cat(caps, "1");
        rel.download_volume_factor = Some(0.0);
        rel.upload_volume_factor = Some(1.0);
        releases.push(rel);
    }
    Ok(releases)
}

pub fn parse_knaben_response(
    body: &str,
    caps: &TorznabCapabilities,
) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
    let json: Value = serde_json::from_str(body)?;
    let rows = json.get("hits").and_then(|v| v.as_array());
    let mut releases = Vec::new();
    let empty = Vec::new();
    for r in rows.unwrap_or(&empty) {
        if v_i64(r, "seeders").unwrap_or(0) <= 0 {
            continue;
        }
        let mut rel = ReleaseInfo::new();
        rel.title = v_str(r, "title");
        rel.details = v_str(r, "details").and_then(|s| Url::parse(&s).ok());
        rel.guid = rel.details.clone();
        rel.link = v_str(r, "link").and_then(|s| Url::parse(&s).ok());
        rel.magnet_uri = v_str(r, "magnetUrl").and_then(|s| magnet_uri(&s));
        if rel.guid.is_none() {
            rel.guid = rel.magnet_uri.clone();
        }
        rel.size = v_i64(r, "bytes").or_else(|| v_str(r, "bytes").and_then(|s| s.parse().ok()));
        rel.seeders = v_i64(r, "seeders");
        rel.peers = v_i64(r, "peers");
        rel.info_hash = v_str(r, "hash");
        if let Some(cats) = r.get("categoryId").and_then(|v| v.as_array()) {
            let mut mapped = Vec::new();
            for c in cats {
                if let Some(id) = c.as_i64().map(|n| n.to_string()).or_else(|| c.as_str().map(str::to_string)) {
                    mapped.extend(map_cat(caps, &id));
                }
            }
            rel.category = mapped;
        }
        rel.publish_date = v_str(r, "date").and_then(|s| DateTimeUtil::from_unknown(&s));
        rel.download_volume_factor = Some(0.0);
        rel.upload_volume_factor = Some(1.0);
        releases.push(rel);
    }
    Ok(releases)
}

pub fn parse_magnetico_response(
    body: &str,
    caps: &TorznabCapabilities,
) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
    let rows: Vec<Value> = serde_json::from_str(body)?;
    let mut releases = Vec::new();
    for r in &rows {
        let mut rel = ReleaseInfo::new();
        rel.title = v_str(r, "name");
        let hash = v_str(r, "infoHash").unwrap_or_default();
        if !hash.is_empty() {
            let uri = Url::parse(&format!("magnet:?xt=urn:btih:{hash}")).ok();
            rel.magnet_uri = uri.clone();
            rel.guid = uri;
        }
        rel.size = v_i64(r, "size");
        rel.files = v_i64(r, "nFiles");
        rel.publish_date = v_i64(r, "discoveredOn").and_then(unix_time);
        rel.category = map_cat(caps, "1");
        rel.download_volume_factor = Some(0.0);
        rel.upload_volume_factor = Some(1.0);
        releases.push(rel);
    }
    Ok(releases)
}

pub fn parse_erairaws_response(
    body: &str,
    caps: &TorznabCapabilities,
    query: &TorznabQuery,
) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
    let mut reader = Reader::from_str(body);
    reader.trim_text(true);
    let mut releases = Vec::new();
    let mut in_item = false;
    let mut current_tag = String::new();
    let mut item = ReleaseInfo::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.local_name().as_ref() == b"item" => {
                in_item = true;
                item = ReleaseInfo::new();
            }
            Ok(Event::Start(e)) => {
                current_tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
            }
            Ok(Event::Text(e)) if in_item => {
                let text = e.unescape().unwrap_or_default().to_string();
                match current_tag.as_str() {
                    "title" => item.title = Some(text),
                    "link" => item.link = Url::parse(&text).ok(),
                    "pubDate" => item.publish_date = DateTimeUtil::from_unknown(&text),
                    "size" => item.size = text.parse().ok(),
                    "infohash" => {
                        if !text.is_empty() {
                            let uri = Url::parse(&format!("magnet:?xt=urn:btih:{text}")).ok();
                            item.magnet_uri = uri.clone();
                            item.guid = uri;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) if e.local_name().as_ref() == b"item" => {
                in_item = false;
                if let Some(title) = &item.title {
                    let mut q = query.clone();
                    if !q.match_query_string_and(title, None, None) {
                        continue;
                    }
                }
                item.category = map_cat(caps, "1");
                item.seeders = Some(1);
                item.download_volume_factor = Some(0.0);
                item.upload_volume_factor = Some(1.0);
                if item.guid.is_none() {
                    item.guid = item.link.clone();
                }
                if item.title.is_some() {
                    releases.push(std::mem::take(&mut item));
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow::anyhow!("RSS parse error: {e}")),
            _ => {}
        }
        buf.clear();
    }
    Ok(releases)
}

pub fn parse_anilibria_rss_ids(body: &str) -> Vec<String> {
    let mut reader = Reader::from_str(body);
    reader.trim_text(true);
    let mut ids = Vec::new();
    let mut in_tag = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.local_name().as_ref() == b"torrentId" => in_tag = true,
            Ok(Event::Text(e)) if in_tag => {
                if let Ok(t) = e.unescape() {
                    ids.push(t.to_string());
                }
            }
            Ok(Event::End(e)) if e.local_name().as_ref() == b"torrentId" => in_tag = false,
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    ids
}

pub fn parse_anilibria_detail(
    body: &str,
    caps: &TorznabCapabilities,
) -> Vec<ReleaseInfo> {
    let json: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let rows: Vec<Value> = match &json {
        Value::Array(a) => a.clone(),
        Value::Object(_) => vec![json.clone()],
        _ => return Vec::new(),
    };
    let mut releases = Vec::new();
    for r in rows {
        let mut rel = ReleaseInfo::new();
        let name = v_str(&r, "release.name.main")
            .or_else(|| v_str(&r, "release.name.english"))
            .unwrap_or_default();
        let label = v_str(&r, "label").unwrap_or_default();
        rel.title = Some(format!("{name} {label}").trim().to_string());
        let hash = v_str(&r, "hash").unwrap_or_default();
        if !hash.is_empty() {
            let uri = Url::parse(&format!("magnet:?xt=urn:btih:{hash}")).ok();
            rel.magnet_uri = uri.clone();
            rel.guid = uri;
        }
        rel.size = v_i64(&r, "size");
        rel.seeders = v_i64(&r, "seeders");
        rel.peers = v_i64(&r, "leechers").map(|l| l + rel.seeders.unwrap_or(0));
        rel.grabs = v_i64(&r, "completed_times");
        rel.publish_date = v_i64(&r, "created_at").and_then(unix_time);
        let kind = v_str(&r, "release.type.value").unwrap_or_default();
        rel.category = map_cat(caps, &kind);
        rel.download_volume_factor = Some(0.0);
        rel.upload_volume_factor = Some(1.0);
        if rel.title.as_deref().map(|t| !t.trim().is_empty()).unwrap_or(false) {
            releases.push(rel);
        }
    }
    releases
}

pub fn parse_ptp_response(
    body: &str,
    site_link: &str,
    caps: &TorznabCapabilities,
) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
    let json: Value = serde_json::from_str(body)?;
    let movies = json.get("Movies").and_then(|v| v.as_array());
    let empty = Vec::new();
    let mut releases = Vec::new();
    for m in movies.unwrap_or(&empty) {
        let group_id = v_str(m, "GroupId").unwrap_or_default();
        let cat_id = v_str(m, "CategoryId").unwrap_or_default();
        let title = v_str(m, "Title").unwrap_or_default();
        let year = v_str(m, "Year").unwrap_or_default();
        let torrents = m.get("Torrents").and_then(|v| v.as_array());
        for t in torrents.unwrap_or(&empty) {
            let id = v_str(t, "Id").unwrap_or_default();
            let mut rel = ReleaseInfo::new();
            let release_name = v_str(t, "ReleaseName").unwrap_or_else(|| title.clone());
            rel.title = Some(if year.is_empty() {
                release_name
            } else {
                format!("{release_name} ({year})")
            });
            rel.size = v_str(t, "Size").and_then(|s| s.parse().ok());
            rel.seeders = v_str(t, "Seeders").and_then(|s| s.parse().ok());
            rel.peers = v_str(t, "Leechers").and_then(|s| s.parse::<i64>().ok())
                .map(|l| l + rel.seeders.unwrap_or(0));
            rel.grabs = v_str(t, "Snatched").and_then(|s| s.parse().ok());
            rel.publish_date = v_str(t, "UploadTime").and_then(|s| DateTimeUtil::from_unknown(&s));
            let details = Url::parse(&format!(
                "{site_link}/torrents.php?id={group_id}&torrentid={id}"
            ))
            .ok();
            rel.details = details.clone();
            rel.guid = details;
            rel.link = Url::parse(&format!("{site_link}/torrents.php?action=download&id={id}")).ok();
            rel.category = map_cat(caps, &cat_id);
            if let Some(imdb) = v_str(m, "ImdbId") {
                let digits: String = imdb.chars().filter(|c| c.is_ascii_digit()).collect();
                rel.imdb = digits.parse().ok();
            }
            let free = v_str(t, "FreeleechType").unwrap_or_default();
            rel.download_volume_factor = Some(if free.is_empty() || free == "NORMAL" { 1.0 } else { 0.0 });
            rel.upload_volume_factor = Some(if free == "NEUTRAL LEECH" { 0.0 } else { 1.0 });
            releases.push(rel);
        }
    }
    Ok(releases)
}

pub fn parse_beyondhd_response(
    body: &str,
    caps: &TorznabCapabilities,
) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
    if body.trim_start().starts_with('<') || body.contains("Invalid API Key") {
        anyhow::bail!("Invalid BeyondHD API key / non-JSON response");
    }
    let json: Value = serde_json::from_str(body)?;
    if json.get("status_code").and_then(|v| v.as_i64()) == Some(0) {
        anyhow::bail!("{}", v_str(&json, "status_message").unwrap_or_default());
    }
    let empty = Vec::new();
    let rows = json.get("results").and_then(|v| v.as_array()).unwrap_or(&empty);
    let mut releases = Vec::new();
    for r in rows {
        let (Some(link), Some(details)) = (
            v_str(r, "download_url"),
            v_str(r, "url"),
        ) else {
            continue;
        };
        let mut rel = ReleaseInfo::new();
        rel.title = v_str(r, "name");
        rel.link = Url::parse(&link).ok();
        rel.details = Url::parse(&details).ok();
        rel.guid = rel.details.clone();
        rel.info_hash = v_str(r, "info_hash");
        rel.size = v_i64(r, "size");
        rel.seeders = v_i64(r, "seeders");
        rel.peers = v_i64(r, "leechers").map(|l| l + rel.seeders.unwrap_or(0));
        rel.grabs = v_i64(r, "times_completed");
        rel.publish_date = v_str(r, "created_at").and_then(|s| DateTimeUtil::from_unknown(&s));
        if let Some(cat) = v_str(r, "category") {
            rel.category = caps.categories.map_tracker_cat_desc_to_newznab(&cat);
        }
        if let Some(imdb) = v_str(r, "imdb_id") {
            let digits: String = imdb.chars().filter(|c| c.is_ascii_digit()).collect();
            rel.imdb = digits.parse().ok();
        }
        rel.tmdb = v_i64(r, "tmdb_id");
        rel.languages = v_str(r, "audios").map(|s| split_csv(&s)).unwrap_or_default();
        rel.subs = v_str(r, "subtitles").map(|s| split_csv(&s)).unwrap_or_default();
        let promo = v_str(r, "promo").unwrap_or_default();
        rel.download_volume_factor = Some(match promo.as_str() {
            "Freeleech" => 0.0,
            "Limited" => 0.75,
            "Promo50" => 0.5,
            "Promo25" => 0.25,
            _ => 1.0,
        });
        rel.upload_volume_factor = Some(1.0);
        releases.push(rel);
    }
    Ok(releases)
}

pub fn parse_hdbits_response(
    body: &str,
    site_link: &str,
    passkey: &str,
    caps: &TorznabCapabilities,
) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
    let json: Value = serde_json::from_str(body)?;
    if json.get("status").and_then(|v| v.as_i64()).unwrap_or(0) != 0 {
        anyhow::bail!("{}", v_str(&json, "message").unwrap_or_default());
    }
    let empty = Vec::new();
    let rows = json.get("data").and_then(|v| v.as_array()).unwrap_or(&empty);
    let mut releases = Vec::new();
    for r in rows {
        let mut rel = ReleaseInfo::new();
        rel.title = v_str(r, "name").or_else(|| v_str(r, "filename"));
        let id = v_str(r, "id").unwrap_or_default();
        rel.details = Url::parse(&format!("{site_link}/details.php?id={id}")).ok();
        rel.guid = rel.details.clone();
        let filename = v_str(r, "filename").unwrap_or_default();
        rel.link = Url::parse(&format!(
            "{site_link}/download.php/{filename}?id={id}&passkey={passkey}"
        ))
        .ok();
        rel.category = v_str(r, "type_category")
            .map(|c| map_cat(caps, &c))
            .unwrap_or_default();
        rel.size = v_i64(r, "size");
        rel.files = v_i64(r, "numfiles");
        rel.grabs = v_i64(r, "times_completed");
        rel.seeders = v_i64(r, "seeders");
        rel.peers = v_i64(r, "leechers").map(|l| l + rel.seeders.unwrap_or(0));
        rel.publish_date = v_i64(r, "utadded").and_then(unix_time);
        rel.imdb = v_i64(r, "imdb.id");
        rel.tvdb_id = v_i64(r, "tvdb.id");
        rel.download_volume_factor = Some(if v_str(r, "freeleech").as_deref() == Some("yes") { 0.0 } else { 1.0 });
        rel.upload_volume_factor = Some(1.0);
        releases.push(rel);
    }
    Ok(releases)
}

pub fn parse_mteamtp_response(
    body: &str,
    caps: &TorznabCapabilities,
) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
    let json: Value = serde_json::from_str(body)?;
    let rows = value_path(&json, "data.data")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut releases = Vec::new();
    for r in rows {
        let mut rel = ReleaseInfo::new();
        let name = v_str(&r, "name").unwrap_or_default();
        static WS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
        rel.title = Some(WS.replace_all(&name, " ").trim().to_string());
        rel.description = v_str(&r, "smallDescr");
        if let Some(cat) = v_str(&r, "category") {
            rel.category = map_cat(caps, &cat);
        }
        rel.files = v_str(&r, "numfiles").and_then(|s| s.parse().ok());
        rel.size = v_str(&r, "size").and_then(|s| s.parse().ok());
        let id = v_str(&r, "id").unwrap_or_default();
        rel.details = Url::parse(&format!("https://m-team.cc/detail/{id}")).ok();
        rel.guid = rel.details.clone();
        rel.publish_date = v_str(&r, "status.createdDate")
            .and_then(|s| DateTimeUtil::from_unknown(&s));
        rel.grabs = v_i64(&r, "status.timesCompleted");
        rel.seeders = v_i64(&r, "status.seeders");
        rel.peers = v_i64(&r, "status.leechers").map(|l| l + rel.seeders.unwrap_or(0));
        let discount = v_str(&r, "status.discount").unwrap_or_default();
        rel.download_volume_factor = Some(match discount.as_str() {
            "FREE" => 0.0,
            "_50_PERCENT_" => 0.5,
            "_30_PERCENT_" => 0.3,
            _ => 1.0,
        });
        rel.upload_volume_factor = Some(if discount.contains("_2X_") { 2.0 } else { 1.0 });
        releases.push(rel);
    }
    Ok(releases)
}

pub fn parse_nebulance_response(
    body: &str,
    caps: &TorznabCapabilities,
) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
    let json: Value = serde_json::from_str(body)?;
    if let Some(err) = json.get("error") {
        if !err.is_null() {
            anyhow::bail!("{}", v_str(&json, "error.message").unwrap_or_default());
        }
    }
    let empty = Vec::new();
    let rows = json.get("items").and_then(|v| v.as_array()).unwrap_or(&empty);
    let mut releases = Vec::new();
    for r in rows {
        let mut rel = ReleaseInfo::new();
        rel.title = v_str(r, "rls_name").or_else(|| v_str(r, "group_name"));
        let group_id = v_str(r, "group_id").unwrap_or_default();
        rel.details = Url::parse(&format!("https://nebulance.io/torrents.php?id={group_id}")).ok();
        rel.guid = rel.details.clone();
        rel.link = v_str(r, "download").and_then(|s| Url::parse(&s).ok());
        rel.size = v_i64(r, "size");
        rel.seeders = v_i64(r, "seed");
        rel.peers = v_i64(r, "leech").map(|l| l + rel.seeders.unwrap_or(0));
        rel.grabs = v_i64(r, "snatch");
        rel.publish_date = v_i64(r, "rls_utc").and_then(unix_time);
        rel.files = r.get("file_list").and_then(|v| v.as_array()).map(|a| a.len() as i64);
        if let (Some(imdb), Some(tvmaze)) = (v_i64(r, "imdb_id"), v_i64(r, "tvmaze_id")) {
            rel.imdb = Some(imdb);
            rel.tvmaze_id = Some(tvmaze);
        }
        // Category from cat + resolution tags.
        let cat = v_str(r, "cat").unwrap_or_default();
        let mut mapped = map_cat(caps, &cat);
        if mapped.is_empty() {
            if let Some(tags) = r.get("tags").and_then(|v| v.as_array()) {
                for t in tags {
                    if let Some(s) = t.as_str() {
                        mapped.extend(map_cat(caps, &s.to_ascii_lowercase()));
                    }
                }
            }
        }
        rel.category = mapped;
        rel.download_volume_factor = Some(0.0);
        rel.upload_volume_factor = Some(1.0);
        rel.minimum_ratio = Some(0.0);
        rel.minimum_seed_time = Some(if cat.eq_ignore_ascii_case("SEASON") { 432000 } else { 86400 });
        releases.push(rel);
    }
    Ok(releases)
}

fn split_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Static site registry — mirrors the C# `IndexerBase` constructors.
// ---------------------------------------------------------------------------

const TV_Q_SEASON_EP: &[TvSearchParam] = &[
    TvSearchParam::Q,
    TvSearchParam::Season,
    TvSearchParam::Ep,
];
const MOVIE_Q: &[MovieSearchParam] = &[MovieSearchParam::Q];
const MUSIC_Q: &[MusicSearchParam] = &[MusicSearchParam::Q];
const BOOK_Q: &[BookSearchParam] = &[BookSearchParam::Q];
const NO_TV: &[TvSearchParam] = &[];
const NO_MOVIE: &[MovieSearchParam] = &[];
const NO_MUSIC: &[MusicSearchParam] = &[];
const NO_BOOK: &[BookSearchParam] = &[];

const SUBSPLEASE_CATS: &[CategoryDef] = &[
    ("1", "TV/Anime", ""),
    ("2", "Movies/Other", ""),
];
const TORRENTSCSV_CATS: &[CategoryDef] = &[("1", "Other", "")];
const MAGNETICO_CATS: &[CategoryDef] = &[("1", "Other", "")];
const ERAIRAWS_CATS: &[CategoryDef] = &[("1", "TV/Anime", "Anime - Sub")];

const KNABEN_CATS: &[CategoryDef] = &[
    ("1000000", "Audio", "Audio"),
    ("1001000", "Audio/MP3", "MP3"),
    ("1002000", "Audio/Lossless", "Lossless"),
    ("1003000", "Audio/Audiobook", "Audiobook"),
    ("1004000", "Audio/Video", "Audio Video"),
    ("1005000", "Audio/Other", "Radio"),
    ("1006000", "Audio/Other", "Audio Other"),
    ("2000000", "TV", "TV"),
    ("2001000", "TV/HD", "TV HD"),
    ("2002000", "TV/SD", "TV SD"),
    ("2003000", "TV/UHD", "TV UHD"),
    ("2004000", "TV/Documentary", "Documentary"),
    ("2005000", "TV/Foreign", "TV Foreign"),
    ("2006000", "TV/Sport", "Sport"),
    ("2007000", "TV/Other", "Cartoon"),
    ("2008000", "TV/Other", "TV Other"),
    ("3000000", "Movies", "Movies"),
    ("3001000", "Movies/HD", "Movies HD"),
    ("3002000", "Movies/SD", "Movies SD"),
    ("3003000", "Movies/UHD", "Movies UHD"),
    ("3004000", "Movies/DVD", "Movies DVD"),
    ("3005000", "Movies/Foreign", "Movies Foreign"),
    ("3006000", "Movies/Foreign", "Movies Bollywood"),
    ("3007000", "Movies/3D", "Movies 3D"),
    ("3008000", "Movies/Other", "Movies Other"),
    ("4000000", "PC", "PC"),
    ("4001000", "PC/Games", "Games"),
    ("4002000", "PC/0day", "Software"),
    ("4003000", "PC/Mac", "Mac"),
    ("4004000", "PC/ISO", "Unix"),
    ("5000000", "XXX", "XXX"),
    ("5001000", "XXX/x264", "XXX Video"),
    ("5002000", "XXX/ImageSet", "XXX ImageSet"),
    ("5003000", "XXX/Other", "XXX Games"),
    ("5004000", "XXX/Other", "XXX Hentai"),
    ("5005000", "XXX/Other", "XXX Other"),
    ("6000000", "TV/Anime", "Anime"),
    ("6001000", "TV/Anime", "Anime Subbed"),
    ("6002000", "TV/Anime", "Anime Dubbed"),
    ("6003000", "TV/Anime", "Anime Dual audio"),
    ("6004000", "TV/Anime", "Anime Raw"),
    ("6005000", "Audio/Video", "Music Video"),
    ("6006000", "Books/Other", "Literature"),
    ("6007000", "Audio/Other", "Music"),
    ("6008000", "TV/Anime", "Anime non-english translated"),
    ("7000000", "Console", "Console"),
    ("7001000", "Console/PS4", "PS4"),
    ("7002000", "Console/PS3", "PS3"),
    ("7003000", "Console/PS3", "PS2"),
    ("7004000", "Console/PS3", "PS1"),
    ("7005000", "Console/PS Vita", "PS Vita"),
    ("7006000", "Console/PSP", "PSP"),
    ("7007000", "Console/XBox 360", "Xbox 360"),
    ("7008000", "Console/XBox", "Xbox"),
    ("7009000", "Console/NDS", "Switch"),
    ("7010000", "Console/NDS", "NDS"),
    ("7011000", "Console/Wii", "Wii"),
    ("7012000", "Console/WiiU", "WiiU"),
    ("7013000", "Console/3DS", "3DS"),
    ("7014000", "Console/Wii", "GameCube"),
    ("7015000", "Console/Other", "Other"),
    ("8000000", "PC/Mobile-Other", "Mobile"),
    ("8001000", "PC/Mobile-Android", "Android"),
    ("8002000", "PC/Mobile-iOS", "IOS"),
    ("8003000", "PC/Mobile-Other", "PC Other"),
    ("9000000", "Books", "Books"),
    ("9001000", "Books/EBook", "EBooks"),
    ("9002000", "Books/Comics", "Comics"),
    ("9003000", "Books/Mags", "Magazines"),
    ("9004000", "Books/Technical", "Technical"),
    ("9005000", "Books/Other", "Books Other"),
    ("10000000", "Other", "Other"),
    ("10001000", "Other/Misc", "Other Misc"),
];

const ANILIBRIA_CATS: &[CategoryDef] = &[
    ("TV", "TV/Anime", "Аниме TV"),
    ("MOVIE", "TV/Anime", "Аниме Фильмы"),
    ("OVA", "TV/Anime", "Аниме OVA"),
    ("ONA", "TV/Anime", "Аниме ONA"),
    ("SPECIAL", "TV/Anime", "Аниме Спешл"),
    ("WEB", "TV/Anime", "Аниме WEB"),
    ("OAD", "TV/Anime", "Аниме OAD"),
    ("DORAMA", "TV", "Дорамы"),
];

const PTP_CATS: &[CategoryDef] = &[
    ("1", "Movies", "Feature Film"),
    ("2", "Movies", "Short Film"),
    ("3", "Movies", "Miniseries"),
    ("4", "Movies", "Stand-up Comedy"),
    ("5", "Movies", "Live Performance"),
    ("6", "Movies", "Movie Collection"),
];

const BEYONDHD_CATS: &[CategoryDef] = &[
    ("1", "Movies", "Movies"),
    ("2", "TV", "TV"),
];

const HDBITS_CATS: &[CategoryDef] = &[
    ("1", "Movies", "Movie"),
    ("2", "TV", "TV"),
    ("3", "TV/Documentary", "Documentary"),
    ("4", "Audio", "Music"),
    ("5", "TV/Sport", "Sport"),
    ("6", "Audio", "Audio Track"),
    ("7", "XXX", "XXX"),
    ("8", "Other", "Misc/Demo"),
];

const MTEAMTP_CATS: &[CategoryDef] = &[
    ("401", "Movies/SD", "Movie(電影)/SD"),
    ("419", "Movies/HD", "Movie(電影)/HD"),
    ("420", "Movies/DVD", "Movie(電影)/DVDiSo"),
    ("421", "Movies/BluRay", "Movie(電影)/Blu-Ray"),
    ("439", "Movies/HD", "Movie(電影)/Remux"),
    ("403", "TV/SD", "TV Series(影劇/綜藝)/SD"),
    ("402", "TV/HD", "TV Series(影劇/綜藝)/HD"),
    ("438", "TV/HD", "TV Series(影劇/綜藝)/BD"),
    ("435", "TV/SD", "TV Series(影劇/綜藝)/DVDiSo"),
    ("404", "TV/Documentary", "紀錄教育"),
    ("434", "Audio", "Music(無損)"),
    ("406", "Audio/Video", "MV(演唱)"),
    ("423", "PC/Games", "PCGame(PC遊戲)"),
    ("448", "Console/Other", "TvGame(TV遊戲)"),
    ("405", "TV/Anime", "Anime(動畫)"),
    ("453", "TV/Anime", "Anime(動畫)/BluRay"),
    ("407", "TV/Sport", "Sports(運動)"),
    ("427", "Books/EBook", "Study/Edu ebook(教育書面)"),
    ("422", "PC/0day", "Software(軟體)"),
    ("442", "Audio/Audiobook", "Study/Edu audio(教育音檔)"),
    ("451", "Movies/Other", "教育影片"),
    ("409", "Other", "Misc(其他)"),
    ("410", "XXX", "AV(有碼)/HD Censored"),
    ("424", "XXX/SD", "AV(有碼)/SD Censored"),
    ("437", "XXX/DVD", "AV(有碼)/DVDiSo Censored"),
    ("431", "XXX", "AV(有碼)/Blu-Ray Censored"),
    ("429", "XXX", "AV(無碼)/HD Uncensored"),
    ("430", "XXX/SD", "AV(無碼)/SD Uncensored"),
    ("426", "XXX/DVD", "AV(無碼)/DVDiSo Uncensored"),
    ("432", "XXX", "AV(無碼)/Blu-Ray Uncensored"),
    ("436", "XXX", "AV(網站)/0Day"),
    ("440", "XXX", "AV(Gay)/HD"),
    ("425", "XXX", "IV(寫真影集)/Video Collection"),
    ("433", "XXX/ImageSet", "IV(寫真圖集)/Picture Collection"),
    ("411", "XXX", "H-Game(遊戲)"),
    ("412", "XXX", "H-Anime(動畫)"),
    ("413", "XXX", "H-Comic(漫畫)"),
];

const NEBULANCE_CATS: &[CategoryDef] = &[
    ("tv", "TV", "tv"),
    ("sd", "TV/SD", "sd"),
    ("hd", "TV/HD", "hd"),
    ("uhd", "TV/UHD", "uhd"),
    ("4k", "TV/UHD", "4k"),
    ("480p", "TV/SD", "480p"),
    ("720p", "TV/HD", "720p"),
    ("1080i", "TV/HD", "1080i"),
    ("1080p", "TV/HD", "1080p"),
    ("2160p", "TV/UHD", "2160p"),
];

const TV_QSE_IMDB_TMDB: &[TvSearchParam] = &[
    TvSearchParam::Q,
    TvSearchParam::Season,
    TvSearchParam::Ep,
    TvSearchParam::ImdbId,
    TvSearchParam::TmdbId,
];
const MOVIE_Q_IMDB_TMDB: &[MovieSearchParam] = &[
    MovieSearchParam::Q,
    MovieSearchParam::ImdbId,
    MovieSearchParam::TmdbId,
];
const MOVIE_Q_IMDB: &[MovieSearchParam] = &[MovieSearchParam::Q, MovieSearchParam::ImdbId];
const TV_QSE_TVDB: &[TvSearchParam] = &[
    TvSearchParam::Q,
    TvSearchParam::Season,
    TvSearchParam::Ep,
    TvSearchParam::TvdbId,
];
const TV_QSE_IMDB: &[TvSearchParam] = &[
    TvSearchParam::Q,
    TvSearchParam::Season,
    TvSearchParam::Ep,
    TvSearchParam::ImdbId,
];
const TV_QSE_TVMAZE: &[TvSearchParam] = &[
    TvSearchParam::Q,
    TvSearchParam::Season,
    TvSearchParam::Ep,
    TvSearchParam::TvmazeId,
];

/// All simple (JSON/RSS/API, no browser login) bespoke indexers.
pub static SIMPLE_SPECS: &[SimpleSpec] = &[
    SimpleSpec {
        id: "subsplease",
        name: "SubsPlease",
        description: "SubsPlease is a Public site. A better HorribleSubs/Erai replacement",
        site_link: "https://subsplease.org/",
        language: "en-US",
        indexer_type: "public",
        kind: SimpleKind::SubsPlease,
        auth: AuthKind::None,
        required: &[],
        tv_params: TV_Q_SEASON_EP,
        movie_params: MOVIE_Q,
        music_params: NO_MUSIC,
        book_params: NO_BOOK,
        categories: SUBSPLEASE_CATS,
        limits_default: None,
        limits_max: None,
        supports_raw_search: false,
    },
    SimpleSpec {
        id: "torrentscsv",
        name: "Torrents.csv",
        description: "Torrents.csv is a self-hostable, open source torrent search engine and database",
        site_link: "https://torrents-csv.com/",
        language: "en-US",
        indexer_type: "public",
        kind: SimpleKind::TorrentsCsv,
        auth: AuthKind::None,
        required: &[],
        tv_params: TV_Q_SEASON_EP,
        movie_params: MOVIE_Q,
        music_params: NO_MUSIC,
        book_params: NO_BOOK,
        categories: TORRENTSCSV_CATS,
        limits_default: None,
        limits_max: None,
        supports_raw_search: false,
    },
    SimpleSpec {
        id: "knaben",
        name: "Knaben",
        description: "Knaben is a Public torrent meta-search engine",
        site_link: "https://knaben.org/",
        language: "en-US",
        indexer_type: "public",
        kind: SimpleKind::Knaben,
        auth: AuthKind::None,
        required: &[],
        tv_params: TV_Q_SEASON_EP,
        movie_params: MOVIE_Q,
        music_params: MUSIC_Q,
        book_params: BOOK_Q,
        categories: KNABEN_CATS,
        limits_default: None,
        limits_max: None,
        supports_raw_search: false,
    },
    SimpleSpec {
        id: "magnetico",
        name: "Magnetico (Local DHT)",
        description: "Magnetico is a self-hosted BitTorrent DHT search engine",
        site_link: "http://127.0.0.1:8080/",
        language: "en-US",
        indexer_type: "semi-private",
        kind: SimpleKind::Magnetico,
        auth: AuthKind::Basic,
        required: &["username", "password"],
        tv_params: TV_Q_SEASON_EP,
        movie_params: MOVIE_Q,
        music_params: MUSIC_Q,
        book_params: BOOK_Q,
        categories: MAGNETICO_CATS,
        limits_default: None,
        limits_max: None,
        supports_raw_search: false,
    },
    SimpleSpec {
        id: "erai-raws",
        name: "Erai-Raws",
        description: "Erai-Raws is a Semi-Private team release site for Anime subtitles.",
        site_link: "https://www.erai-raws.info/",
        language: "en-US",
        indexer_type: "semi-private",
        kind: SimpleKind::EraiRaws,
        auth: AuthKind::None,
        required: &["rss_key"],
        tv_params: &[TvSearchParam::Q],
        movie_params: NO_MOVIE,
        music_params: NO_MUSIC,
        book_params: NO_BOOK,
        categories: ERAIRAWS_CATS,
        limits_default: None,
        limits_max: None,
        supports_raw_search: false,
    },
    SimpleSpec {
        id: "anilibria",
        name: "Anilibria",
        description: "Anilibria is a russian-language anime distribution platform",
        site_link: "https://aniliberty.top/",
        language: "ru-RU",
        indexer_type: "public",
        kind: SimpleKind::Anilibria,
        auth: AuthKind::None,
        required: &[],
        tv_params: TV_Q_SEASON_EP,
        movie_params: NO_MOVIE,
        music_params: NO_MUSIC,
        book_params: NO_BOOK,
        categories: ANILIBRIA_CATS,
        limits_default: None,
        limits_max: None,
        supports_raw_search: false,
    },
    SimpleSpec {
        id: "passthepopcorn",
        name: "PassThePopcorn",
        description: "PassThePopcorn (PTP) is a Private site for MOVIES",
        site_link: "https://passthepopcorn.me/",
        language: "en-US",
        indexer_type: "private",
        kind: SimpleKind::PassThePopcorn,
        auth: AuthKind::ApiUserKey,
        required: &["api_user", "api_key"],
        tv_params: NO_TV,
        movie_params: MOVIE_Q_IMDB,
        music_params: NO_MUSIC,
        book_params: NO_BOOK,
        categories: PTP_CATS,
        limits_default: Some(50),
        limits_max: Some(50),
        supports_raw_search: false,
    },
    SimpleSpec {
        id: "beyond-hd-api",
        name: "Beyond-HD (API)",
        description: "Beyond-HD is a Private site. Without BeyondHD, your HDTV is just a TV",
        site_link: "https://beyond-hd.me/",
        language: "en-US",
        indexer_type: "private",
        kind: SimpleKind::BeyondHd,
        auth: AuthKind::None,
        required: &["api_key", "rss_key"],
        tv_params: TV_QSE_IMDB_TMDB,
        movie_params: MOVIE_Q_IMDB_TMDB,
        music_params: NO_MUSIC,
        book_params: NO_BOOK,
        categories: BEYONDHD_CATS,
        limits_default: Some(100),
        limits_max: Some(100),
        supports_raw_search: false,
    },
    SimpleSpec {
        id: "hdbitsapi",
        name: "HDBits (API)",
        description: "HDBits is a Private site for the HighDefinition Bittorrent Community",
        site_link: "https://hdbits.org/",
        language: "en-US",
        indexer_type: "private",
        kind: SimpleKind::Hdbits,
        auth: AuthKind::None,
        required: &["username", "passkey"],
        tv_params: TV_QSE_TVDB,
        movie_params: MOVIE_Q_IMDB,
        music_params: NO_MUSIC,
        book_params: NO_BOOK,
        categories: HDBITS_CATS,
        limits_default: None,
        limits_max: None,
        supports_raw_search: false,
    },
    SimpleSpec {
        id: "mteamtp",
        name: "M-Team - TP",
        description: "M-Team - TP (MTTP) is a CHINESE Private Torrent Tracker for MOVIES / TV / GENERAL",
        site_link: "https://kp.m-team.cc/",
        language: "zh-CN",
        indexer_type: "private",
        kind: SimpleKind::MteamTp,
        auth: AuthKind::ApiKeyHeader("x-api-key"),
        required: &["api_key"],
        tv_params: TV_QSE_IMDB,
        movie_params: MOVIE_Q_IMDB,
        music_params: MUSIC_Q,
        book_params: BOOK_Q,
        categories: MTEAMTP_CATS,
        limits_default: None,
        limits_max: None,
        supports_raw_search: false,
    },
    SimpleSpec {
        id: "nebulanceapi",
        name: "NebulanceAPI",
        description: "Nebulance is a Private site. At Nebulance we will change the way you think about TV. Using API.",
        site_link: "https://nebulance.io/",
        language: "en-US",
        indexer_type: "private",
        kind: SimpleKind::Nebulance,
        auth: AuthKind::None,
        required: &["api_key"],
        tv_params: TV_QSE_TVMAZE,
        movie_params: NO_MOVIE,
        music_params: NO_MUSIC,
        book_params: NO_BOOK,
        categories: NEBULANCE_CATS,
        limits_default: Some(100),
        limits_max: Some(1000),
        supports_raw_search: true,
    },
];

pub fn find_simple_spec(id: &str) -> Option<&'static SimpleSpec> {
    SIMPLE_SPECS.iter().find(|s| s.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps_for(id: &str) -> TorznabCapabilities {
        find_simple_spec(id).unwrap().caps()
    }

    #[test]
    fn all_simple_specs_build() {
        assert_eq!(SIMPLE_SPECS.len(), 11);
        for spec in SIMPLE_SPECS {
            let cfg = SimpleConfig {
                username: "u".into(),
                password: "p".into(),
                api_key: "k".into(),
                api_user: "au".into(),
                rss_key: "r".into(),
                passkey: "pk".into(),
            };
            let idx = spec.build(cfg).expect("build failed");
            assert!(idx.is_configured(), "{} not configured", spec.id);
            assert_eq!(idx.id(), spec.id);
            assert!(!idx.torznab_caps().categories.get_tracker_categories().is_empty());
        }
    }

    #[test]
    fn public_specs_need_no_credentials() {
        for spec in SIMPLE_SPECS.iter().filter(|s| !s.requires_credentials()) {
            assert!(SimpleConfig::default().is_complete(spec.required));
        }
        assert!(find_simple_spec("hdbitsapi")
            .unwrap()
            .requires_credentials());
        assert!(!SimpleConfig::default().is_complete(&["passkey"]));
    }

    #[test]
    fn caps_match_csharp() {
        let knaben = caps_for("knaben");
        assert_eq!(knaben.categories.get_tracker_categories().len(), 73);
        assert!(knaben
            .categories
            .map_tracker_cat_to_newznab("2001000")
            .contains(&5040));
        let mteam = caps_for("mteamtp");
        assert_eq!(mteam.categories.get_tracker_categories().len(), 37);
        assert!(mteam
            .categories
            .map_tracker_cat_to_newznab("419")
            .contains(&2040));
        let neb = caps_for("nebulanceapi");
        assert!(neb.supports_raw_search);
        assert_eq!(neb.limits_max, Some(1000));
        let ptp = caps_for("passthepopcorn");
        assert_eq!(ptp.limits_default, Some(50));
        assert!(ptp.tv_search_params.is_empty());
    }

    #[test]
    fn parses_subsplease_json() {
        let body = r#"{"Show - 01":{"show":"Show","episode":"01","release_date":"2024-01-02 12:00:00","downloads":[{"res":"1080p","magnet":"magnet:?xt=urn:btih:ABCD&xl=123456"}]}}"#;
        let out = parse_subsplease_response(body, &caps_for("subsplease")).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title.as_deref(), Some("[SubsPlease] Show - 01 (1080p)"));
        assert_eq!(out[0].size, Some(123456));
        assert!(out[0].category.contains(&5070));
    }

    #[test]
    fn parses_torrentscsv_and_magnetico() {
        let csv = r#"{"torrents":[{"name":"Some.Release","infohash":"AAAABBBB","size_bytes":"999","created_unix":1700000000,"seeders":3,"leechers":1,"completed":7}]}"#;
        let out = parse_torrentscsv_response(csv, &caps_for("torrentscsv")).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].size, Some(999));
        assert_eq!(out[0].peers, Some(4));
        assert_eq!(out[0].seeders, Some(3));

        let mag = r#"[{"name":"DHT hit","infoHash":"CCCCDDDD","size":42,"nFiles":2,"discoveredOn":1700000000}]"#;
        let out = parse_magnetico_response(mag, &caps_for("magnetico")).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].files, Some(2));
        assert_eq!(out[0].category, vec![8000]);
    }

    #[test]
    fn parses_knaben_and_beyondhd() {
        let knaben = r#"{"hits":[{"title":"A","details":"https://knaben.org/t/1","link":"https://x/1.torrent","magnetUrl":"magnet:?xt=urn:btih:EEEE","bytes":100,"seeders":2,"peers":5,"hash":"EEEE","categoryId":[2001000],"date":"2024-01-01"}]}"#;
        let out = parse_knaben_response(knaben, &caps_for("knaben")).unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].category.contains(&5040));
        assert_eq!(out[0].size, Some(100));

        let bhd = r#"{"status_code":1,"results":[{"name":"BHD.Release","download_url":"https://beyond-hd.me/dl/1","url":"https://beyond-hd.me/t/1","info_hash":"FFFF","size":1000,"seeders":9,"leechers":1,"times_completed":3,"created_at":"2024-01-01 00:00:00","category":"Movies","imdb_id":"tt1234567","tmdb_id":42,"audios":"English, French","subtitles":"English","promo":"Freeleech"}]}"#;
        let out = parse_beyondhd_response(bhd, &caps_for("beyond-hd-api")).unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].category.contains(&2000));
        assert_eq!(out[0].download_volume_factor, Some(0.0));
        assert_eq!(out[0].languages, vec!["English", "French"]);
        assert_eq!(out[0].imdb, Some(1234567));
    }

    #[test]
    fn parses_ptp_hdbits_mteam_nebulance() {
        let caps_ptp = caps_for("passthepopcorn");
        let ptp = r#"{"Movies":[{"GroupId":"10","CategoryId":"1","Title":"A Film","Year":"2020","ImdbId":"tt0000001","Torrents":[{"Id":"20","ReleaseName":"A.Film.2020.1080p","Size":"1000","Seeders":"5","Leechers":"2","Snatched":"9","UploadTime":"2024-01-01 00:00:00","FreeleechType":"FREE"}]}]}"#;
        let out = parse_ptp_response(ptp, "https://passthepopcorn.me", &caps_ptp).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title.as_deref(), Some("A.Film.2020.1080p (2020)"));
        assert!(out[0].category.contains(&2000));
        assert_eq!(out[0].download_volume_factor, Some(0.0));

        let hdb = r#"{"status":0,"data":[{"id":"5","name":"HD.Release","filename":"HD.Release.torrent","type_category":"1","size":2000,"numfiles":3,"times_completed":4,"seeders":6,"leechers":2,"utadded":1700000000,"imdb":{"id":7654321},"tvdb":{"id":88},"freeleech":"yes"}]}"#;
        let out = parse_hdbits_response(hdb, "https://hdbits.org", "PK", &caps_for("hdbitsapi")).unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].category.contains(&2000));
        assert_eq!(out[0].imdb, Some(7654321));
        assert_eq!(out[0].download_volume_factor, Some(0.0));

        let mt = r#"{"data":{"data":[{"id":"99","name":"  MT   Release ","category":"419","numfiles":"4","size":"5000","status":{"createdDate":"2024-01-01 00:00:00","timesCompleted":2,"seeders":7,"leechers":1,"discount":"FREE"}}]}}"#;
        let out = parse_mteamtp_response(mt, &caps_for("mteamtp")).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title.as_deref(), Some("MT Release"));
        assert!(out[0].category.contains(&2040));
        assert_eq!(out[0].download_volume_factor, Some(0.0));

        let neb = r#"{"items":[{"rls_name":"Show.S01E01","group_id":"7","download":"https://nebulance.io/dl/1","size":3000,"seed":4,"leech":1,"snatch":6,"rls_utc":1700000000,"file_list":[],"cat":"hd","tags":["720p"]}]}"#;
        let out = parse_nebulance_response(neb, &caps_for("nebulanceapi")).unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].category.contains(&5040));
        assert_eq!(out[0].minimum_seed_time, Some(86400));
    }

    #[test]
    fn parses_erairaws_rss_and_anilibria_detail() {
        let rss = r#"<?xml version="1.0"?><rss><channel><item><title>[SubsPlease] Show - 01</title><link>https://erai-raws.info/1</link><pubDate>Tue, 02 Jan 2024 12:00:00 +0000</pubDate><size>1234</size><infohash>AABBCCDD</infohash></item></channel></rss>"#;
        let q = TorznabQuery::new();
        let out = parse_erairaws_response(rss, &caps_for("erai-raws"), &q).unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].category.contains(&5070));
        assert_eq!(out[0].size, Some(1234));

        let detail = r#"[{"release":{"name":{"main":"Anime","english":"Anime EN"},"type":{"value":"TV"}},"label":"[1]","hash":"11223344","size":4321,"seeders":3,"leechers":1,"completed_times":9,"created_at":1700000000}]"#;
        let out = parse_anilibria_detail(detail, &caps_for("anilibria"));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title.as_deref(), Some("Anime [1]"));
        assert!(out[0].category.contains(&5070));
    }
}
