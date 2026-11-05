//! Mirror of Jackett.Common/Indexers/Definitions/Abstract/AvistazTracker.cs — the
//! abstract base for the Avistaz family (Avistaz/AsiaTorrents, PrivateHD,
//! ExoticaZ, CinemaZ, ...). Unlike the Gazelle trackers, these use a JSON API:
//! `POST api/v1/jackett/auth` (username/password/pid) yields a Bearer token,
//! then `GET api/v1/jackett/torrents` returns the results.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
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

/// Credentials for an Avistaz-family tracker.
#[derive(Clone)]
pub struct AvistazConfig {
    pub username: String,
    pub password: String,
    pub pid: String,
    pub freeleech: bool,
}

impl Default for AvistazConfig {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            pid: String::new(),
            freeleech: false,
        }
    }
}

const HD_RESOLUTIONS: [&str; 3] = ["1080p", "1080i", "720p"];

/// Site-specific variations of the Avistaz API.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AvistazFlavour {
    /// AvistaZ / PrivateHD / CinemaZ: `in`/`type`/`video_quality[]`, categories
    /// derived from `type` + `video_quality`.
    Standard,
    /// AnimeZ: `format[]` query params, categories mapped from `format`,
    /// title from `release_title` when present.
    AnimeZ,
    /// ExoticaZ: `category=` query param, categories mapped from the
    /// `category` object's keys.
    ExoticaZ,
}

/// Reusable Avistaz-API indexer. Constructed by concrete indexers
/// (`build_avistaz`, `build_privatehd`, ...).
pub struct AvistazTracker {
    id: String,
    name: String,
    description: String,
    site_link: String,
    language: String,
    caps: TorznabCapabilities,
    client: reqwest::Client,
    config: AvistazConfig,
    /// Avistaz publishes episodes without a season (e.g. `E323`); when true the
    /// episode search term uses that `E`-prefix form.
    episode_e_prefix: bool,
    flavour: AvistazFlavour,
    token: Mutex<Option<String>>,
}

impl AvistazTracker {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &str,
        name: &str,
        description: &str,
        site_link: &str,
        language: &str,
        caps: TorznabCapabilities,
        config: AvistazConfig,
        episode_e_prefix: bool,
        flavour: AvistazFlavour,
    ) -> anyhow::Result<Arc<dyn IIndexer>> {
        let client = reqwest::Client::builder()
            .user_agent(BrowserUtil::default_user_agent())
            .build()?;
        Ok(Arc::new(Self {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            site_link: site_link.trim_end_matches('/').to_string(),
            language: language.to_string(),
            caps,
            client,
            config,
            episode_e_prefix,
            flavour,
            token: Mutex::new(None),
        }))
    }

    fn auth_url(&self) -> String {
        format!("{}/api/v1/jackett/auth", self.site_link)
    }

    fn search_url(&self) -> String {
        format!("{}/api/v1/jackett/torrents", self.site_link)
    }

    async fn ensure_token(&self) -> anyhow::Result<String> {
        {
            let guard = self.token.lock().unwrap();
            if let Some(t) = guard.as_ref() {
                return Ok(t.clone());
            }
        }
        let t = self.renew_token().await?;
        *self.token.lock().unwrap() = Some(t.clone());
        Ok(t)
    }

    async fn renew_token(&self) -> Result<String, anyhow::Error> {
        let body = serde_json::json!({
            "username": self.config.username,
            "password": self.config.password,
            "pid": self.config.pid,
        });
        let resp = self
            .client
            .post(self.auth_url())
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        if status.as_u16() == 429 {
            anyhow::bail!("Rate limited by indexer");
        }
        let text = resp.text().await?;
        #[derive(Deserialize)]
        struct AuthResp {
            token: Option<String>,
            message: Option<String>,
        }
        let ar: AuthResp = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("Invalid response from AvistaZ (not JSON): {e}"))?;
        ar.token.ok_or_else(|| {
            anyhow::anyhow!(ar.message.unwrap_or_else(|| "Unauthorized request to indexer".into()))
        })
    }

    fn get_episode_search_term(&self, query: &TorznabQuery) -> String {
        if self.episode_e_prefix
            && (query.season.is_none() || query.season == Some(0))
            && query.episode.is_some()
        {
            return format!("E{}", query.episode.as_ref().unwrap());
        }
        query.get_episode_search_string()
    }

    fn get_search_term(&self, query: &TorznabQuery) -> String {
        let ep = self.get_episode_search_term(query);
        format!(
            "{} {}",
            query.search_term.clone().unwrap_or_default(),
            ep
        )
        .trim()
        .to_string()
    }

    fn get_search_params(&self, query: &TorznabQuery) -> Vec<(String, String)> {
        if self.flavour == AvistazFlavour::AnimeZ {
            return self.get_search_params_animez(query);
        }
        if self.flavour == AvistazFlavour::ExoticaZ {
            return self.get_search_params_exoticaz(query);
        }
        self.get_search_params_standard(query)
    }

    /// AnimeZ: `format[]` per requested tracker category, no `in`/`type`.
    fn get_search_params_animez(&self, query: &TorznabQuery) -> Vec<(String, String)> {
        let mut p: Vec<(String, String)> = Vec::new();
        p.push(("limit".into(), "50".into()));
        let mut seen = std::collections::HashSet::new();
        for cat in self.caps.categories.map_torznab_caps_to_trackers(query, true) {
            if seen.insert(cat.clone()) {
                p.push(("format[]".into(), cat));
            }
        }
        p.push(("search".into(), self.get_search_term(query)));
        if query.limit > 0 && query.offset > 0 {
            p.push(("page".into(), (query.offset / query.limit + 1).to_string()));
        }
        if self.config.freeleech {
            p.push(("freeleech".into(), "1".into()));
        }
        p
    }

    /// ExoticaZ: `category=` (single or `0`), no `type`/resolution filters.
    fn get_search_params_exoticaz(&self, query: &TorznabQuery) -> Vec<(String, String)> {
        let mut p: Vec<(String, String)> = Vec::new();
        p.push(("in".into(), "1".into()));
        let tracker_cats = self.caps.categories.map_torznab_caps_to_trackers(query, true);
        let category = if tracker_cats.len() == 1 {
            tracker_cats[0].clone()
        } else {
            "0".into()
        };
        p.push(("category".into(), category));
        p.push(("limit".into(), "50".into()));
        p.push(("search".into(), self.get_search_term(query)));
        if query.limit > 0 && query.offset > 0 {
            p.push(("page".into(), (query.offset / query.limit + 1).to_string()));
        }
        if self.config.freeleech {
            p.push(("discount[]".into(), "1".into()));
        }
        p
    }

    fn get_search_params_standard(&self, query: &TorznabQuery) -> Vec<(String, String)> {
        let mut p: Vec<(String, String)> = Vec::new();
        p.push(("in".into(), "1".into()));

        let tracker_cats = self.caps.categories.map_torznab_caps_to_trackers(query, true);
        let type_ = if tracker_cats.len() == 1 {
            tracker_cats[0].clone()
        } else {
            "0".into()
        };
        p.push(("type".into(), type_));
        p.push(("limit".into(), "50".into()));

        if query.limit > 0 && query.offset > 0 {
            let page = query.offset / query.limit + 1;
            p.push(("page".into(), page.to_string()));
        }

        let cats = &query.categories;
        // Resolution filter improves the search (mirrors C#): only when no broad
        // Movies/TV/Audio category was requested.
        if !cats.contains(&2000) && !cats.contains(&5000) && !cats.contains(&3000) {
            if cats.contains(&2045) || cats.contains(&5045) {
                p.push(("video_quality[]".into(), "6".into())); // 2160p
            }
            if cats.contains(&2040) || cats.contains(&5040) {
                p.push(("video_quality[]".into(), "2".into())); // 720p
                p.push(("video_quality[]".into(), "7".into())); // 1080i
                p.push(("video_quality[]".into(), "3".into())); // 1080p
            }
            if cats.contains(&2030) || cats.contains(&5030) {
                p.push(("video_quality[]".into(), "1".into())); // SD
            }
        }

        if query.is_imdb_query() {
            if let Some(imdb) = &query.imdb_id {
                p.push(("imdb".into(), imdb.clone()));
            }
            p.push(("search".into(), self.get_episode_search_term(query)));
        } else if query.is_tmdb_query() {
            if let Some(tmdb) = query.tmdb_id {
                p.push(("tmdb".into(), tmdb.to_string()));
            }
            p.push(("search".into(), self.get_episode_search_term(query)));
        } else if query.is_tvdb_query() {
            if let Some(tvdb) = query.tvdb_id {
                p.push(("tvdb".into(), tvdb.to_string()));
            }
            p.push(("search".into(), self.get_episode_search_term(query)));
        } else {
            p.push(("search".into(), self.get_search_term(query)));
        }

        if let Some(genre) = &query.genre {
            if !genre.is_empty() {
                p.push(("tags".into(), genre.clone()));
            }
        }
        if self.config.freeleech {
            p.push(("discount[]".into(), "1".into()));
        }
        p
    }

    async fn run_query(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let token = self.ensure_token().await?;
        let params = self.get_search_params(query);
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))
                .unwrap_or_else(|_| HeaderValue::from_static("")),
        );

        let mut resp = self
            .client
            .get(self.search_url())
            .query(&params)
            .headers(headers.clone())
            .send()
            .await?;
        let status = resp.status();

        // Token expired / forbidden -> re-auth and retry once.
        if status.as_u16() == 401 || status.as_u16() == 412 {
            let token = self.renew_token().await?;
            *self.token.lock().unwrap() = Some(token.clone());
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {token}"))
                    .unwrap_or_else(|_| HeaderValue::from_static("")),
            );
            resp = self
                .client
                .get(self.search_url())
                .query(&params)
                .headers(headers)
                .send()
                .await?;
        }

        let status = resp.status();
        if status.as_u16() == 404 {
            return Ok(Vec::new()); // no results
        }
        if status.as_u16() == 429 {
            anyhow::bail!("Rate limited by indexer");
        }
        if status.as_u16() >= 400 {
            anyhow::bail!("Invalid status code {status} received from indexer");
        }

        let body = resp.text().await?;
        parse_avistaz_response_with(&body, self.flavour, &self.caps)
    }
}

#[async_trait]
impl IIndexer for AvistazTracker {
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
        !self.config.username.is_empty()
            && !self.config.password.is_empty()
            && !self.config.pid.is_empty()
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
    async fn download(&self, link: &url::Url) -> Result<Vec<u8>, anyhow::Error> {
        let token = self.ensure_token().await?;
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))
                .unwrap_or_else(|_| HeaderValue::from_static("")),
        );
        let resp = self
            .client
            .get(link.clone())
            .headers(headers)
            .send()
            .await?;
        Ok(resp.bytes().await?.to_vec())
    }
}

/// Parses an Avistaz `api/v1/jackett/torrents` JSON response into `ReleaseInfo`s
/// using the standard category/title logic. Kept for offline tests.
pub fn parse_avistaz_response(body: &str) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
    parse_avistaz_response_with(body, AvistazFlavour::Standard, &TorznabCapabilities::new())
}

/// Flavour-aware parser. `caps` is used by AnimeZ/ExoticaZ to map the row's
/// `format` / `category` keys onto Torznab categories.
pub fn parse_avistaz_response_with(
    body: &str,
    flavour: AvistazFlavour,
    caps: &TorznabCapabilities,
) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
    let parsed: AvistazResponse = serde_json::from_str(body)?;
    let mut releases = Vec::new();

    for row in parsed.data {
        let details = match Url::parse(&row.url) {
            Ok(u) => u,
            Err(_) => continue,
        };
        let link = match Url::parse(&row.download) {
            Ok(u) => u,
            Err(_) => continue,
        };

        let mut description = String::new();
        if let Some(audio) = &row.audio {
            let langs: Vec<&str> = audio.iter().map(|a| a.language.as_str()).collect();
            description.push_str(&format!("Audio: {}", langs.join(", ")));
        }
        if let Some(sub) = &row.subtitle {
            if !description.is_empty() {
                description.push_str("<br/>");
            }
            let langs: Vec<&str> = sub.iter().map(|s| s.language.as_str()).collect();
            description.push_str(&format!("Subtitles: {}", langs.join(", ")));
        }

        let mut rel = ReleaseInfo::new();
        let title = match flavour {
            AvistazFlavour::AnimeZ => row
                .release_title
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| row.file_name.clone()),
            _ => row.file_name.clone(),
        };
        rel.title = Some(title);
        rel.link = Some(link);
        rel.details = Some(details.clone());
        rel.guid = Some(details);
        rel.category = match flavour {
            AvistazFlavour::AnimeZ => row
                .format
                .as_deref()
                .map(|f| caps.categories.map_tracker_cat_to_newznab(f))
                .unwrap_or_default(),
            AvistazFlavour::ExoticaZ => {
                let mut cats = Vec::new();
                if let Some(dict) = &row.category {
                    for key in dict.keys() {
                        cats.extend(caps.categories.map_tracker_cat_to_newznab(key));
                    }
                }
                cats
            }
            AvistazFlavour::Standard => parse_categories(&row),
        };
        rel.publish_date = DateTimeUtil::parse_rfc3339(&row.created_at_iso);
        if !description.is_empty() {
            rel.description = Some(description);
        }
        rel.size = row.file_size;
        rel.files = row.file_count;
        rel.grabs = row.completed.map(|c| c as i64);
        rel.seeders = row.seed.map(|s| s as i64);
        rel.peers = row
            .leech
            .map(|l| l as i64 + row.seed.unwrap_or(0) as i64);
        rel.download_volume_factor = row.download_multiply;
        rel.upload_volume_factor = row.upload_multiply;
        rel.minimum_ratio = Some(1.0);
        rel.minimum_seed_time = Some(259200); // 72h default
        rel.info_hash = row.info_hash.clone();
        rel.languages = row
            .audio
            .map(|a| a.into_iter().map(|x| x.language).collect())
            .unwrap_or_default();
        rel.subs = row
            .subtitle
            .map(|s| s.into_iter().map(|x| x.language).collect())
            .unwrap_or_default();

        if let Some(fs) = row.file_size {
            if fs > 0 {
                let size_gb = fs as f64 / (1024.0_f64).powi(3);
                rel.minimum_seed_time = Some(if size_gb > 50.0 {
                    ((100.0 * size_gb.ln() - 219.2023) * 3600.0) as i64
                } else {
                    259200 + (size_gb * 7200.0) as i64
                });
            }
        }

        if let Some(info) = &row.movie_tv {
            rel.imdb = info
                .imdb
                .as_ref()
                .and_then(|s| parse_imdb_id(s));
            rel.tmdb = info
                .tmdb
                .as_ref()
                .and_then(|s| s.trim().parse::<i64>().ok())
                .filter(|v| *v != 0);
            rel.tvdb_id = info
                .tvdb
                .as_ref()
                .and_then(|s| s.trim().parse::<i64>().ok())
                .filter(|v| *v != 0);
        }

        releases.push(rel);
    }

    releases.sort_by(|a, b| b.publish_date.cmp(&a.publish_date));
    Ok(releases)
}

fn parse_categories(row: &AvistazRelease) -> Vec<i32> {
    let vq = row.video_quality.as_deref().unwrap_or("");
    match row.type_.to_uppercase().as_str() {
        "MOVIE" => vec![if HD_RESOLUTIONS.contains(&vq) {
            2040 // Movies/HD
        } else if vq == "2160p" {
            2045 // Movies/UHD
        } else {
            2030 // Movies/SD
        }],
        "TV-SHOW" => vec![if HD_RESOLUTIONS.contains(&vq) {
            5040 // TV/HD
        } else if vq == "2160p" {
            5045 // TV/UHD
        } else {
            5030 // TV/SD
        }],
        "MUSIC" => vec![3000], // Audio
        _ => Vec::new(),
    }
}

// ----------------------------------------------------------------------------
// Avistaz JSON response shape
// ----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct AvistazResponse {
    data: Vec<AvistazRelease>,
}

#[derive(Debug, Deserialize)]
struct AvistazRelease {
    url: String,
    download: String,
    #[serde(rename = "movie_tv")]
    movie_tv: Option<AvistazIdInfo>,
    #[serde(rename = "created_at_iso")]
    created_at_iso: String,
    #[serde(rename = "file_name")]
    file_name: String,
    #[serde(rename = "release_title")]
    #[allow(dead_code)] // kept for future ParseTitle overrides (mirrors C# model)
    release_title: Option<String>,
    #[serde(rename = "info_hash")]
    info_hash: Option<String>,
    leech: Option<i32>,
    completed: Option<i32>,
    seed: Option<i32>,
    #[serde(rename = "file_size")]
    file_size: Option<i64>,
    #[serde(rename = "file_count")]
    file_count: Option<i64>,
    #[serde(rename = "download_multiply")]
    download_multiply: Option<f64>,
    #[serde(rename = "upload_multiply")]
    upload_multiply: Option<f64>,
    #[serde(rename = "video_quality")]
    video_quality: Option<String>,
    #[serde(rename = "type")]
    type_: String,
    /// AnimeZ uses `format` as the category key; also present on other sites.
    format: Option<String>,
    /// ExoticaZ returns a dictionary whose keys are tracker category ids.
    category: Option<serde_json::Map<String, serde_json::Value>>,
    audio: Option<Vec<AvistazLanguage>>,
    subtitle: Option<Vec<AvistazLanguage>>,
}

#[derive(Debug, Deserialize)]
struct AvistazIdInfo {
    tmdb: Option<String>,
    tvdb: Option<String>,
    imdb: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AvistazLanguage {
    #[allow(dead_code)]
    id: i32,
    language: String,
}

fn parse_imdb_id(tag: &str) -> Option<i64> {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"tt(\d+)").unwrap());
    RE.captures(tag)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<i64>().ok())
}

/// Data-driven spec for an Avistaz-family site.
pub struct AvistazSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub site_link: &'static str,
    pub language: &'static str,
    pub episode_e_prefix: bool,
    pub flavour: AvistazFlavour,
    pub supports_raw_search: bool,
    pub tv_search_imdb_available: bool,
    pub tv_params: &'static [TvSearchParam],
    pub movie_params: &'static [MovieSearchParam],
    pub music_params: &'static [MusicSearchParam],
    pub book_params: &'static [BookSearchParam],
    /// `(tracker category key, Torznab category name)`
    pub categories: &'static [(&'static str, &'static str)],
}

impl AvistazSpec {
    fn caps(&self) -> TorznabCapabilities {
        let mut caps = TorznabCapabilities::new();
        caps.limits_default = Some(50);
        caps.limits_max = Some(50);
        caps.tv_search_params = self.tv_params.to_vec();
        caps.movie_search_params = self.movie_params.to_vec();
        caps.music_search_params = self.music_params.to_vec();
        caps.book_search_params = self.book_params.to_vec();
        caps.supports_raw_search = self.supports_raw_search;
        caps.tv_search_imdb_available = self.tv_search_imdb_available;
        for (key, name) in self.categories {
            if let Some(cat) = TorznabCatType::get_cat_by_name(name) {
                caps.categories.add_category_mapping(key, cat.clone(), None);
            }
        }
        caps
    }

    pub fn build(&self, config: AvistazConfig) -> anyhow::Result<Arc<dyn IIndexer>> {
        AvistazTracker::new(
            self.id,
            self.name,
            self.description,
            self.site_link,
            self.language,
            self.caps(),
            config,
            self.episode_e_prefix,
            self.flavour,
        )
    }
}

const AV_TV_FULL: &[TvSearchParam] = &[
    TvSearchParam::Q,
    TvSearchParam::Season,
    TvSearchParam::Ep,
    TvSearchParam::ImdbId,
    TvSearchParam::TvdbId,
    TvSearchParam::Genre,
];
const AV_TV_SHORT: &[TvSearchParam] = &[TvSearchParam::Q, TvSearchParam::Season, TvSearchParam::Ep];
const AV_MOVIE_FULL: &[MovieSearchParam] = &[
    MovieSearchParam::Q,
    MovieSearchParam::ImdbId,
    MovieSearchParam::TmdbId,
    MovieSearchParam::Genre,
];
const AV_MOVIE_Q: &[MovieSearchParam] = &[MovieSearchParam::Q];
const AV_BOOK_Q: &[BookSearchParam] = &[BookSearchParam::Q];
const AV_NONE_TV: &[TvSearchParam] = &[];
const AV_NONE_MOVIE: &[MovieSearchParam] = &[];
const AV_NONE_MUSIC: &[MusicSearchParam] = &[];
const AV_NONE_BOOK: &[BookSearchParam] = &[];

const AV_CATS_MOVIES_TV: &[(&str, &str)] = &[
    ("1", "Movies"),
    ("1", "Movies/UHD"),
    ("1", "Movies/HD"),
    ("1", "Movies/SD"),
    ("2", "TV"),
    ("2", "TV/UHD"),
    ("2", "TV/HD"),
    ("2", "TV/SD"),
];

pub static AVISTAZ_SITES: Lazy<Vec<AvistazSpec>> = Lazy::new(|| {
    vec![
        AvistazSpec {
            id: "avistaz",
            name: "AvistaZ",
            description: "AvistaZ (AsiaTorrents) is a Private Torrent Tracker for ASIAN MOVIES / TV / GENERAL",
            site_link: "https://avistaz.to/",
            language: "en-US",
            episode_e_prefix: true,
            flavour: AvistazFlavour::Standard,
            supports_raw_search: true,
            tv_search_imdb_available: true,
            tv_params: AV_TV_FULL,
            movie_params: AV_MOVIE_FULL,
            music_params: AV_NONE_MUSIC,
            book_params: AV_NONE_BOOK,
            categories: &[
                ("1", "Movies"),
                ("1", "Movies/UHD"),
                ("1", "Movies/HD"),
                ("1", "Movies/SD"),
                ("2", "TV"),
                ("2", "TV/UHD"),
                ("2", "TV/HD"),
                ("2", "TV/SD"),
                ("3", "Audio"),
            ],
        },
        AvistazSpec {
            id: "privatehd",
            name: "PrivateHD",
            description: "PrivateHD is a Private BitTorrent site for High Quality, High Definition (HD) MOVIES and TV Shows",
            site_link: "https://privatehd.to/",
            language: "en-US",
            episode_e_prefix: false,
            flavour: AvistazFlavour::Standard,
            supports_raw_search: false,
            tv_search_imdb_available: true,
            tv_params: AV_TV_FULL,
            movie_params: AV_MOVIE_FULL,
            music_params: AV_NONE_MUSIC,
            book_params: AV_NONE_BOOK,
            categories: AV_CATS_MOVIES_TV,
        },
        AvistazSpec {
            id: "cinemaz",
            name: "CinemaZ",
            description: "CinemaZ (EuTorrents) is a Private Torrent Tracker for FOREIGN NON-ASIAN MOVIES / TV",
            site_link: "https://cinemaz.to/",
            language: "en-US",
            episode_e_prefix: false,
            flavour: AvistazFlavour::Standard,
            supports_raw_search: false,
            tv_search_imdb_available: true,
            tv_params: AV_TV_FULL,
            movie_params: AV_MOVIE_FULL,
            music_params: AV_NONE_MUSIC,
            book_params: AV_NONE_BOOK,
            categories: AV_CATS_MOVIES_TV,
        },
        AvistazSpec {
            id: "animez",
            name: "AnimeZ",
            description: "AnimeZ (ex-AnimeTorrents) is a Private Torrent Tracker for ANIME / MANGA",
            site_link: "https://animez.to/",
            language: "en-US",
            episode_e_prefix: false,
            flavour: AvistazFlavour::AnimeZ,
            supports_raw_search: false,
            tv_search_imdb_available: false,
            tv_params: AV_TV_SHORT,
            movie_params: AV_MOVIE_Q,
            music_params: AV_NONE_MUSIC,
            book_params: AV_BOOK_Q,
            categories: &[
                ("TV", "TV/Anime"),
                ("TV_SHORT", "TV/Anime"),
                ("MOVIE", "Movies"),
                ("SPECIAL", "TV/Anime"),
                ("OVA", "TV/Anime"),
                ("ONA", "TV/Anime"),
                ("MUSIC", "TV/Anime"),
                ("MANGA", "Books/Comics"),
                ("NOVEL", "Books/Foreign"),
                ("ONE_SHOT", "Books/Foreign"),
            ],
        },
        AvistazSpec {
            id: "exoticaz",
            name: "ExoticaZ",
            description: "ExoticaZ (YourExotic) is a Private Torrent Tracker for 3X",
            site_link: "https://exoticaz.to/",
            language: "en-US",
            episode_e_prefix: false,
            flavour: AvistazFlavour::ExoticaZ,
            supports_raw_search: false,
            tv_search_imdb_available: false,
            tv_params: AV_NONE_TV,
            movie_params: AV_NONE_MOVIE,
            music_params: AV_NONE_MUSIC,
            book_params: AV_NONE_BOOK,
            categories: &[
                ("1", "XXX/x264"),
                ("2", "XXX/Pack"),
                ("3", "XXX/Pack"),
                ("4", "XXX/Pack"),
                ("5", "XXX/DVD"),
                ("6", "XXX/x264"),
                ("7", "XXX/ImageSet"),
                ("8", "XXX/ImageSet"),
            ],
        },
    ]
});

pub fn find_avistaz_spec(id: &str) -> Option<&'static AvistazSpec> {
    AVISTAZ_SITES.iter().find(|s| s.id == id)
}

/// Backwards-compatible builders used by existing callers/tests.
pub fn build_avistaz(config: AvistazConfig) -> anyhow::Result<Arc<dyn IIndexer>> {
    find_avistaz_spec("avistaz").unwrap().build(config)
}

pub fn build_privatehd(config: AvistazConfig) -> anyhow::Result<Arc<dyn IIndexer>> {
    find_avistaz_spec("privatehd").unwrap().build(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "data": [
        {
          "url": "https://avistaz.to/torrent/123",
          "download": "https://avistaz.to/api/v1/jackett/download/123",
          "created_at_iso": "2023-11-14T22:13:37.000000Z",
          "file_name": "Big Buck Bunny 2023 1080p.mkv",
          "info_hash": "abc123",
          "leech": 5, "completed": 100, "seed": 42,
          "file_size": 1610612736, "file_count": 1,
          "download_multiply": 1.0, "upload_multiply": 1.0,
          "video_quality": "1080p", "type": "MOVIE", "format": "x264",
          "audio": [{"id": 1, "language": "English"}],
          "subtitle": [{"id": 2, "language": "Chinese"}],
          "movie_tv": {"imdb": "tt1254207", "tmdb": "12345", "tvdb": "76543"}
        },
        {
          "url": "https://avistaz.to/torrent/124",
          "download": "https://avistaz.to/api/v1/jackett/download/124",
          "created_at_iso": "2023-10-01T10:00:00Z",
          "file_name": "Show S01E01 480p.mkv",
          "info_hash": "def456",
          "leech": 2, "completed": 50, "seed": 10,
          "file_size": 500000000, "file_count": 1,
          "download_multiply": 0.0, "upload_multiply": 1.0,
          "video_quality": "480p", "type": "TV-SHOW", "format": "x264"
        }
      ]
    }"#;

    #[test]
    fn parses_avistaz_response_offline() {
        let rels = parse_avistaz_response(SAMPLE).expect("parse");
        assert_eq!(rels.len(), 2);

        let movie = &rels[0];
        assert_eq!(movie.title.as_deref(), Some("Big Buck Bunny 2023 1080p.mkv"));
        assert!(movie.category.contains(&2040), "cat: {:?}", movie.category); // Movies/HD
        assert_eq!(movie.size, Some(1610612736));
        assert_eq!(movie.seeders, Some(42));
        assert_eq!(movie.peers, Some(47));
        assert_eq!(movie.download_volume_factor, Some(1.0));
        assert_eq!(movie.imdb, Some(1254207));
        assert_eq!(movie.tmdb, Some(12345));
        assert_eq!(movie.tvdb_id, Some(76543));
        assert_eq!(movie.languages, vec!["English".to_string()]);
        assert_eq!(movie.subs, vec!["Chinese".to_string()]);
        assert_eq!(movie.info_hash.as_deref(), Some("abc123"));
        // size_gb = 1.5 -> 259200 + 1.5*7200 = 270000
        assert_eq!(movie.minimum_seed_time, Some(270000));
        assert!(movie.publish_date.is_some());

        let tv = &rels[1];
        assert!(tv.category.contains(&5030), "cat: {:?}", tv.category); // TV/SD
        assert_eq!(tv.download_volume_factor, Some(0.0));
        assert!(tv.minimum_seed_time.unwrap() > 259200);
    }

    #[test]
    fn all_avistaz_specs_build() {
        assert_eq!(AVISTAZ_SITES.len(), 5);
        for spec in AVISTAZ_SITES.iter() {
            let idx = spec
                .build(AvistazConfig {
                    username: "u".into(),
                    password: "p".into(),
                    pid: "pid".into(),
                    freeleech: false,
                })
                .unwrap_or_else(|_| panic!("failed to build {}", spec.id));
            assert_eq!(idx.id(), spec.id);
            assert!(
                !idx.torznab_caps().categories.get_tracker_categories().is_empty(),
                "{} has no categories",
                spec.id
            );
        }
    }

    #[test]
    fn animez_uses_format_category_and_release_title() {
        let spec = find_avistaz_spec("animez").unwrap();
        let caps = spec.caps();
        let body = r#"{"data":[{
          "url":"https://animez.to/t/1","download":"https://animez.to/d/1",
          "created_at_iso":"2024-01-01T00:00:00Z","file_name":"raw-filename.mkv",
          "release_title":"Nice Release Title","file_size":1000,"seed":1,"leech":0,
          "format":"MOVIE","type":"TV-SHOW"
        }]}"#;
        let rels = parse_avistaz_response_with(body, AvistazFlavour::AnimeZ, &caps).unwrap();
        assert_eq!(rels[0].title.as_deref(), Some("Nice Release Title"));
        assert!(rels[0].category.contains(&2000), "cat: {:?}", rels[0].category);
    }

    #[test]
    fn exoticaz_uses_category_dict_keys() {
        let spec = find_avistaz_spec("exoticaz").unwrap();
        let caps = spec.caps();
        let body = r#"{"data":[{
          "url":"https://exoticaz.to/t/1","download":"https://exoticaz.to/d/1",
          "created_at_iso":"2024-01-01T00:00:00Z","file_name":"clip.mkv",
          "file_size":1000,"seed":1,"leech":0,"type":"MOVIE",
          "category":{"1": "Video Clip", "7": "Photo Pack"}
        }]}"#;
        let rels = parse_avistaz_response_with(body, AvistazFlavour::ExoticaZ, &caps).unwrap();
        assert_eq!(rels.len(), 1);
        assert!(rels[0].category.len() >= 2, "cat: {:?}", rels[0].category);
    }

    #[test]
    fn hd_resolutions_map_to_hd_categories() {
        let hd = r#"{"data":[{
          "url":"https://avistaz.to/t/1","download":"https://avistaz.to/d/1",
          "created_at_iso":"2024-01-01T00:00:00Z","file_name":"Show S01E01 720p.mkv",
          "file_size":1000,"seed":1,"leech":0,"video_quality":"720p","type":"TV-SHOW"
        }]}"#;
        let rels = parse_avistaz_response(hd).unwrap();
        assert!(rels[0].category.contains(&5040), "cat: {:?}", rels[0].category);

        let uhd = r#"{"data":[{
          "url":"https://avistaz.to/t/2","download":"https://avistaz.to/d/2",
          "created_at_iso":"2024-01-01T00:00:00Z","file_name":"Movie 2160p.mkv",
          "file_size":1000,"seed":1,"leech":0,"video_quality":"2160p","type":"MOVIE"
        }]}"#;
        let rels = parse_avistaz_response(uhd).unwrap();
        assert!(rels[0].category.contains(&2045), "cat: {:?}", rels[0].category);
    }
}
