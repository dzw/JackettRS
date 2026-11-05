//! Mirror of Jackett.Common/Indexers/Definitions/Abstract/SpeedAppTracker.cs —
//! the base for the SpeedApp family (SpeedApp, RetroFlix). REST/JSON API:
//! `POST api/login` (username=email + password) yields a Bearer token, then
//! `GET api/torrent` returns a top-level JSON array.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, ACCEPT};
use serde::Deserialize;
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

/// Credentials for a SpeedApp-family tracker.
#[derive(Clone, Default)]
pub struct SpeedAppConfig {
    /// Note: the C# config calls this "Email" but posts it as `username`.
    pub email: String,
    pub password: String,
    pub freeleech_only: bool,
}

/// Data-driven spec for a SpeedApp-family site.
pub struct SpeedAppSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub site_link: &'static str,
    pub language: &'static str,
    pub minimum_seed_time: i64,
    pub tv_params: &'static [TvSearchParam],
    pub movie_params: &'static [MovieSearchParam],
    pub music_params: &'static [MusicSearchParam],
    pub book_params: &'static [BookSearchParam],
    /// `(tracker category id, Torznab category name)`
    pub categories: &'static [(i32, &'static str)],
}

impl SpeedAppSpec {
    fn caps(&self) -> TorznabCapabilities {
        let mut caps = TorznabCapabilities::new();
        caps.tv_search_params = self.tv_params.to_vec();
        caps.movie_search_params = self.movie_params.to_vec();
        caps.music_search_params = self.music_params.to_vec();
        caps.book_search_params = self.book_params.to_vec();
        for (id, name) in self.categories {
            if let Some(cat) = TorznabCatType::get_cat_by_name(name) {
                caps.categories
                    .add_category_mapping_int(*id, cat.clone(), None);
            }
        }
        caps
    }

    pub fn build(&self, config: SpeedAppConfig) -> anyhow::Result<Arc<dyn IIndexer>> {
        let client = reqwest::Client::builder()
            .user_agent(BrowserUtil::default_user_agent())
            .build()?;
        Ok(Arc::new(SpeedAppTracker {
            id: self.id.to_string(),
            name: self.name.to_string(),
            description: self.description.to_string(),
            site_link: self.site_link.trim_end_matches('/').to_string(),
            language: self.language.to_string(),
            minimum_seed_time: self.minimum_seed_time,
            caps: self.caps(),
            client,
            config,
            token: Mutex::new(None),
        }))
    }
}

pub struct SpeedAppTracker {
    id: String,
    name: String,
    description: String,
    site_link: String,
    language: String,
    minimum_seed_time: i64,
    caps: TorznabCapabilities,
    client: reqwest::Client,
    config: SpeedAppConfig,
    token: Mutex<Option<String>>,
}

impl SpeedAppTracker {
    fn login_url(&self) -> String {
        format!("{}/api/login", self.site_link)
    }
    fn search_url(&self) -> String {
        format!("{}/api/torrent", self.site_link)
    }

    async fn ensure_token(&self) -> anyhow::Result<String> {
        {
            let g = self.token.lock().unwrap();
            if let Some(t) = g.as_ref() {
                return Ok(t.clone());
            }
        }
        let t = self.renew_token().await?;
        *self.token.lock().unwrap() = Some(t.clone());
        Ok(t)
    }

    async fn renew_token(&self) -> Result<String, anyhow::Error> {
        if self.config.email.trim().is_empty() || self.config.password.trim().is_empty() {
            anyhow::bail!("Please, check the indexer configuration.");
        }
        let body = serde_json::json!({
            "username": self.config.email.trim(),
            "password": self.config.password.trim(),
        });
        let resp = self
            .client
            .post(self.login_url())
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .json(&body)
            .send()
            .await?;
        let text = resp.text().await?;
        #[derive(Deserialize, Default)]
        struct LoginResp {
            token: Option<String>,
            message: Option<String>,
        }
        let r: LoginResp = serde_json::from_str(&text).unwrap_or_default();
        r.token.ok_or_else(|| {
            anyhow::anyhow!(r.message.unwrap_or_else(|| "Invalid login response".into()))
        })
    }

    fn search_headers(&self, token: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))
                .unwrap_or_else(|_| HeaderValue::from_static("")),
        );
        h
    }

    fn get_search_params(&self, query: &TorznabQuery) -> Vec<(String, String)> {
        let mut p: Vec<(String, String)> = vec![
            ("itemsPerPage".into(), "100".into()),
            ("includingDead".into(), "1".into()),
            ("sort".into(), "torrent.createdAt".into()),
            ("direction".into(), "desc".into()),
        ];
        if query.limit > 0 && query.offset > 0 {
            p.push(("page".into(), (query.offset / query.limit + 1).to_string()));
        }
        for cat in self.caps.categories.map_torznab_caps_to_trackers(query, true) {
            p.push(("categories[]".into(), cat));
        }
        if query.is_imdb_query() {
            if let Some(imdb) = &query.imdb_id {
                p.push(("imdbId".into(), imdb.clone()));
            }
        } else {
            p.push(("search".into(), query.get_query_string()));
        }
        p
    }

    async fn run_query(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let token = self.ensure_token().await?;
        let params = self.get_search_params(query);
        let mut resp = self
            .client
            .get(self.search_url())
            .query(&params)
            .headers(self.search_headers(&token))
            .send()
            .await?;
        if resp.status().as_u16() == 401 {
            let token = self.renew_token().await?;
            *self.token.lock().unwrap() = Some(token.clone());
            resp = self
                .client
                .get(self.search_url())
                .query(&params)
                .headers(self.search_headers(&token))
                .send()
                .await?;
        }
        let status = resp.status();
        let body = resp.text().await?;
        if status.as_u16() != 200 {
            anyhow::bail!("Unknown error in search: {body}");
        }
        parse_speedapp_response(
            &body,
            &self.site_link,
            &self.caps,
            self.config.freeleech_only,
            self.minimum_seed_time,
            query,
        )
    }
}

#[async_trait]
impl IIndexer for SpeedAppTracker {
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
        !self.config.email.is_empty() && !self.config.password.is_empty()
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
        let token = self.ensure_token().await?;
        let mut resp = self
            .client
            .get(link.clone())
            .headers(self.search_headers(&token))
            .send()
            .await?;
        if resp.status().as_u16() == 401 {
            let token = self.renew_token().await?;
            *self.token.lock().unwrap() = Some(token.clone());
            resp = self
                .client
                .get(link.clone())
                .headers(self.search_headers(&token))
                .send()
                .await?;
        } else if resp.status().as_u16() != 200 {
            anyhow::bail!("Unknown error in download: HTTP {}", resp.status());
        }
        Ok(resp.bytes().await?.to_vec())
    }
}

/// Parses the top-level JSON array returned by `api/torrent` (offline-testable).
pub fn parse_speedapp_response(
    body: &str,
    site_link: &str,
    caps: &TorznabCapabilities,
    freeleech_only: bool,
    minimum_seed_time: i64,
    query: &TorznabQuery,
) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(body)?;
    let mut releases = Vec::new();
    let mut query = query.clone();

    for row in rows {
        let dvf = row
            .get("download_volume_factor")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);
        if freeleech_only && dvf != 0.0 {
            continue;
        }

        let raw_title = row.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let title = clean_title(raw_title);
        if !query.is_imdb_query() && !query.match_query_string_and(&title, None, None) {
            continue;
        }

        let mut rel = ReleaseInfo::new();
        if let Some(id) = row.get("id").and_then(|v| v.as_str()) {
            rel.link = Url::parse(&format!("{site_link}/api/torrent/{id}/download")).ok();
        }
        if let Some(u) = row.get("url").and_then(|v| v.as_str()) {
            if let Ok(details) = Url::parse(u) {
                rel.details = Some(details.clone());
                rel.guid = Some(details);
            }
        }
        rel.publish_date = row
            .get("created_at")
            .and_then(|v| v.as_str())
            .and_then(DateTimeUtil::from_unknown);
        let cat_id = row
            .get("category")
            .and_then(|c| c.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        rel.category = caps.categories.map_tracker_cat_to_newznab(cat_id);

        if let Some(sd) = row.get("short_description").and_then(|v| v.as_str()) {
            static SPLIT: Lazy<Regex> =
                Lazy::new(|| Regex::new(r#"[, /)().;\[\]"|:]+"#).unwrap());
            let genres: Vec<String> = SPLIT
                .split(sd)
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim().to_string())
                .collect();
            if !genres.is_empty() {
                rel.description = Some(genres.join(", "));
                rel.genres = genres;
            }
        }
        if let Some(poster) = row.get("poster").and_then(|v| v.as_str()) {
            rel.poster = Url::parse(poster).ok();
        }
        rel.title = Some(title);
        rel.size = row.get("size").and_then(|v| v.as_i64());
        rel.grabs = row.get("times_completed").and_then(|v| v.as_i64());
        let seeders = row.get("seeders").and_then(|v| v.as_i64());
        let leechers = row.get("leechers").and_then(|v| v.as_i64());
        rel.seeders = seeders;
        rel.peers = leechers.map(|l| l + seeders.unwrap_or(0));
        rel.download_volume_factor = Some(dvf);
        rel.upload_volume_factor = row
            .get("upload_volume_factor")
            .and_then(|v| v.as_f64())
            .or(Some(1.0));
        rel.minimum_ratio = Some(1.0);
        rel.minimum_seed_time = Some(minimum_seed_time);

        releases.push(rel);
    }
    Ok(releases)
}

fn clean_title(title: &str) -> String {
    static REQUEST_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)\[request(ed)?\]").unwrap());
    REQUEST_RE
        .replace_all(title, "")
        .trim_matches(|c| c == ' ' || c == '.')
        .to_string()
}

// ---------------------------------------------------------------------------
// Static site definitions
// ---------------------------------------------------------------------------

const SP_TV: &[TvSearchParam] = &[
    TvSearchParam::Q,
    TvSearchParam::Season,
    TvSearchParam::Ep,
    TvSearchParam::ImdbId,
];
const SP_MOVIE: &[MovieSearchParam] = &[MovieSearchParam::Q, MovieSearchParam::ImdbId];
const SP_MUSIC: &[MusicSearchParam] = &[MusicSearchParam::Q];
const SP_BOOK: &[BookSearchParam] = &[BookSearchParam::Q];

pub static SPEEDAPP_SITES: Lazy<Vec<SpeedAppSpec>> = Lazy::new(|| {
    vec![
        SpeedAppSpec {
            id: "speedapp",
            name: "SpeedApp",
            description: "SpeedApp is a ROMANIAN Private Torrent Tracker for MOVIES / TV / GENERAL",
            site_link: "https://speedapp.io/",
            language: "ro-RO",
            minimum_seed_time: 172800, // 48h
            tv_params: SP_TV,
            movie_params: SP_MOVIE,
            music_params: SP_MUSIC,
            book_params: SP_BOOK,
            categories: &[
                (38, "Movies"), (10, "Movies/SD"), (35, "Movies/SD"),
                (8, "Movies/HD"), (29, "Movies/HD"), (7, "Movies/DVD"), (2, "Movies/DVD"),
                (17, "Movies/BluRay"), (24, "Movies/BluRay"), (59, "Movies"),
                (57, "Movies/UHD"), (61, "Movies/UHD"), (41, "TV"), (66, "TV"),
                (45, "TV/SD"), (46, "TV/SD"), (43, "TV/HD"), (44, "TV/HD"), (60, "TV"),
                (11, "PC/Games"), (52, "Console"), (1, "PC/0day"), (14, "PC"),
                (37, "PC/Mac"), (19, "PC/Mobile-Other"), (62, "TV"), (3, "TV/Anime"),
                (6, "Books/EBook"), (5, "Audio"), (64, "Audio/Video"), (68, "Audio/Lossless"),
                (18, "Other"), (22, "TV/Sport"), (58, "TV/Sport"), (9, "TV/Documentary"),
                (63, "TV/Documentary"), (65, "Other"), (67, "Other/Misc"),
                (15, "XXX"), (47, "XXX"), (48, "XXX"), (49, "XXX/ImageSet"),
                (50, "XXX"), (51, "XXX"),
            ],
        },
        SpeedAppSpec {
            id: "retroflix",
            name: "RetroFlix",
            description: "RetroFlix is a Private site for Classic Movies / TV / General Releases",
            site_link: "https://retroflix.net/",
            language: "en-US",
            minimum_seed_time: 432000, // 120h
            tv_params: SP_TV,
            movie_params: SP_MOVIE,
            music_params: SP_MUSIC,
            book_params: SP_BOOK,
            categories: &[
                (401, "Movies"), (402, "TV"), (406, "Audio/Video"),
                (407, "TV/Sport"), (409, "Books"), (408, "Audio"),
            ],
        },
    ]
});

pub fn find_speedapp_spec(id: &str) -> Option<&'static SpeedAppSpec> {
    SPEEDAPP_SITES.iter().find(|s| s.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[
      {
        "id": "12345",
        "url": "https://speedapp.io/torrents/12345",
        "created_at": "2024-01-02 11:22:33",
        "category": {"id": "8", "name": "Movies: HD"},
        "short_description": "Action, Adventure / Drama",
        "poster": "https://speedapp.io/poster.jpg",
        "name": "[REQUESTED] Big Buck Bunny 1080p",
        "size": 1610612736,
        "times_completed": 100,
        "seeders": 42,
        "leechers": 3,
        "download_volume_factor": 0.0,
        "upload_volume_factor": 1.0
      }
    ]"#;

    #[test]
    fn both_specs_build() {
        assert_eq!(SPEEDAPP_SITES.len(), 2);
        for spec in SPEEDAPP_SITES.iter() {
            let idx = spec
                .build(SpeedAppConfig {
                    email: "u@example.com".into(),
                    password: "p".into(),
                    freeleech_only: false,
                })
                .unwrap();
            assert_eq!(idx.id(), spec.id);
            assert!(idx.torznab_caps().categories.get_tracker_categories().len() >= 6);
        }
        assert_eq!(find_speedapp_spec("speedapp").unwrap().minimum_seed_time, 172800);
        assert_eq!(find_speedapp_spec("retroflix").unwrap().minimum_seed_time, 432000);
    }

    #[test]
    fn parses_speedapp_response_offline() {
        let spec = find_speedapp_spec("speedapp").unwrap();
        let caps = spec.caps();
        let mut q = TorznabQuery {
            search_term: Some("Bunny".into()),
            ..Default::default()
        };
        let rels = parse_speedapp_response(
            SAMPLE,
            "https://speedapp.io",
            &caps,
            false,
            172800,
            &q,
        )
        .unwrap();
        assert_eq!(rels.len(), 1);
        let r = &rels[0];
        assert_eq!(r.title.as_deref(), Some("Big Buck Bunny 1080p"));
        assert!(r.category.contains(&2040), "cat: {:?}", r.category);
        assert_eq!(r.size, Some(1610612736));
        assert_eq!(r.seeders, Some(42));
        assert_eq!(r.peers, Some(45));
        assert_eq!(r.download_volume_factor, Some(0.0));
        assert_eq!(r.minimum_seed_time, Some(172800));
        assert!(r
            .link
            .as_ref()
            .unwrap()
            .to_string()
            .contains("/api/torrent/12345/download"));
        assert_eq!(r.genres.len(), 3);
        assert!(r.publish_date.is_some());
        let _ = &mut q;
    }
}
