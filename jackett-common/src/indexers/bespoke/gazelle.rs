//! Mirror of Jackett.Common/Indexers/Definitions/Abstract/GazelleTracker.cs — the
//! abstract base for the large family of Gazelle-API private trackers (AlphaRatio,
//! Redacted/Orpheus, PassThePopcorn, etc.). A concrete indexer supplies its
//! `TorznabCapabilities` (category mappings) and optional hooks.
//!
//! The Gazelle API exposes `ajax.php?action=browse` returning a JSON document; we
//! parse it into `ReleaseInfo` and build the torrent download/info links.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue};
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

/// Credentials/behaviour for a Gazelle tracker instance.
#[derive(Clone, Default)]
pub struct GazelleConfig {
    /// API key sent via the `Authorization` header.
    pub api_key: Option<String>,
    /// Raw `Cookie` header (alternative to api key login).
    pub cookie: Option<String>,
    /// `torrent_pass` appended to download links.
    pub passkey: Option<String>,
    /// `authkey` appended to download links.
    pub authkey: Option<String>,
    /// Whether to append `&usetoken=1` (freeleech tokens) on download.
    pub use_tokens: bool,
    /// When true, only freeleech results are kept.
    pub freeleech_only: bool,
    /// When true, only "freeload" (neutral leech) results are kept.
    pub freeload_only: bool,
    /// Download endpoint: `true` => `ajax.php?action=download` (Redacted/Orpheus
    /// style, no passkey/authkey); `false` => `torrents.php?action=download`.
    pub download_via_ajax: bool,
    /// Header name carrying the API key (default `Authorization`; GGn uses
    /// `X-API-Key`).
    pub auth_header: Option<String>,
    /// Format string for the API key header value (default `{0}`; Orpheus/Simurg
    /// use `token {0}`).
    pub auth_format: Option<String>,
    /// API endpoint path (default `ajax.php`; GGn uses `api.php`).
    pub api_path: Option<String>,
    /// GreatPosterWall: put the IMDb id into `searchstr` rather than a dedicated
    /// catalogue/tag search.
    pub imdb_in_search_term: bool,
    /// Filter results by requiring all query words in the title (DICMusic,
    /// SecretCinema, Redacted).
    pub match_query_string_and: bool,
    /// Uniform `MinimumRatio` applied to every release.
    pub minimum_ratio: Option<f64>,
    /// Uniform `MinimumSeedTime` (seconds) applied to every release.
    pub minimum_seed_time: Option<i64>,
}

/// Per-indexer post-parse hook: tweak/reject a parsed release. Return `false` to
/// drop the release (mirrors C#'s `ReleaseInfoPostParse` returning false). The
/// query is exposed so trackers can AND-filter titles (e.g. Redacted).
pub type GazellePostParse = Arc<dyn Fn(&mut ReleaseInfo, &mut TorznabQuery) -> bool + Send + Sync>;

/// Hook with access to the raw Gazelle JSON (torrent + group), for trackers whose
/// `ReleaseInfoPostParse` reads extra fields (GreatPosterWall, Simurg,
/// SecretCinema). Return `false` to drop the release.
pub type GazelleJsonPostParse = Arc<
    dyn Fn(&mut ReleaseInfo, &mut TorznabQuery, &GazelleResult, &GazelleResult) -> bool
        + Send
        + Sync,
>;

/// Reusable Gazelle-API indexer. Constructed by concrete indexers (e.g.
/// `build_alpharatio`).
pub struct GazelleTracker {
    id: String,
    name: String,
    description: String,
    site_link: String,
    language: String,
    caps: TorznabCapabilities,
    client: reqwest::Client,
    config: GazelleConfig,
    imdb_in_tags: bool,
    post_parse: Option<GazellePostParse>,
    json_post_parse: Option<GazelleJsonPostParse>,
}

impl GazelleTracker {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &str,
        name: &str,
        description: &str,
        site_link: &str,
        language: &str,
        caps: TorznabCapabilities,
        config: GazelleConfig,
        imdb_in_tags: bool,
        post_parse: Option<GazellePostParse>,
        json_post_parse: Option<GazelleJsonPostParse>,
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
            imdb_in_tags,
            post_parse,
            json_post_parse,
        }))
    }

    fn auth_headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(ak) = &self.config.api_key {
            let value = match &self.config.auth_format {
                Some(fmt) => fmt.replace("{0}", ak),
                None => ak.clone(),
            };
            if let Ok(v) = HeaderValue::from_str(&value) {
                let name = self.config.auth_header.as_deref().unwrap_or("Authorization");
                h.insert(
                    reqwest::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                    v,
                );
            }
        }
        if let Some(c) = &self.config.cookie {
            if let Ok(v) = HeaderValue::from_str(c) {
                h.insert("Cookie", v);
            }
        }
        h
    }

    fn get_info_url(&self, torrent_id: i64, group_id: Option<i64>) -> Url {
        let g = group_id
            .filter(|x| *x > 0)
            .map(|x| format!("&id={x}"))
            .unwrap_or_default();
        Url::parse(&format!(
            "{}/torrents.php?torrentid={}{}",
            self.site_link, torrent_id, g
        ))
        .unwrap()
    }

    fn get_download_url(&self, torrent_id: i64, can_use_token: bool) -> Url {
        let endpoint = if self.config.download_via_ajax {
            "ajax.php"
        } else {
            "torrents.php"
        };
        let mut u = format!("{}/{}?action=download", self.site_link, endpoint);
        if self.config.use_tokens && can_use_token {
            u.push_str("&usetoken=1");
        }
        // API-key trackers (Redacted/Orpheus) authenticate via the Authorization
        // header and don't need the passkey/authkey appended to the link.
        if !self.config.download_via_ajax {
            if let Some(pk) = &self.config.passkey {
                u.push_str(&format!("&torrent_pass={}", pk));
            }
            if let Some(ak) = &self.config.authkey {
                u.push_str(&format!("&authkey={}", ak));
            }
        }
        u.push_str(&format!("&id={}", torrent_id));
        Url::parse(&u).unwrap()
    }

    fn should_skip(&self, t: &GazelleResult) -> bool {
        if self.config.freeleech_only && t.is_freeleech != Some(true) {
            return true;
        }
        if self.config.freeload_only && t.is_freeload != Some(true) {
            return true;
        }
        false
    }

    fn fill_from_json(&self, rel: &mut ReleaseInfo, t: &GazelleResult) {
        if let Some(tid) = t.torrent_id {
            if let Some(time) = &t.time {
                rel.publish_date = parse_torrent_time(time);
            }

            let mut flags: Vec<String> = Vec::new();
            if let Some(f) = &t.format {
                if !f.is_empty() {
                    flags.push(f.clone());
                }
            }
            if let Some(e) = &t.encoding {
                if !e.is_empty() {
                    flags.push(e.clone());
                }
            }
            if t.has_log == Some(true) {
                let score = t.log_score.clone().unwrap_or_default();
                flags.push(format!("Log ({}%)", score));
            }
            if t.has_cue == Some(true) {
                flags.push("Cue".to_string());
            }
            if let Some(l) = &t.lang {
                if !l.is_empty() && l != "---" {
                    flags.push(l.clone());
                }
            }
            if let Some(m) = &t.media {
                if !m.is_empty() {
                    flags.push(m.clone());
                }
            }
            if let Some(r) = &t.resolution {
                if !r.is_empty() {
                    flags.push(r.clone());
                }
            }
            if let Some(c) = &t.container {
                if !c.is_empty() {
                    flags.push(c.clone());
                }
            }
            if let Some(c) = &t.codec {
                if !c.is_empty() {
                    flags.push(c.clone());
                }
            }
            if let Some(a) = &t.audio {
                if !a.is_empty() {
                    flags.push(a.clone());
                }
            }
            if let Some(s) = &t.subbing {
                if !s.is_empty() && s != "---" {
                    flags.push(s.clone());
                }
            }
            if t.remastered == Some(true) {
                let ry = t.remaster_year.clone().unwrap_or_default();
                let rt = t.remaster_title.clone().unwrap_or_default();
                let remastered = format!(
                    "{} {}",
                    if ry != "0" { ry.as_str() } else { "" },
                    rt
                )
                .trim()
                .to_string();
                if !remastered.is_empty() {
                    flags.push(remastered);
                }
            }
            if !flags.is_empty() {
                rel.title = Some(format!(
                    "{} {}",
                    rel.title.clone().unwrap_or_default(),
                    flags.join(" / ")
                ));
            }

            rel.size = t.size;
            rel.seeders = t.seeders;
            rel.peers = t
                .leechers
                .map(|l| l + t.seeders.unwrap_or(0));
            let info = self.get_info_url(tid, None);
            rel.details = Some(info.clone());
            rel.guid = Some(info);
            let can_use_token = rel.download_volume_factor != Some(0.0);
            rel.link = Some(self.get_download_url(tid, can_use_token));

            rel.category = match &t.category {
                None => self.caps.categories.map_tracker_cat_to_newznab("1"),
                Some(c) if c.contains("Select Category") => {
                    self.caps.categories.map_tracker_cat_to_newznab("1")
                }
                Some(c) => self.caps.categories.map_tracker_cat_desc_to_newznab(c),
            };
            rel.files = t.file_count;
            rel.grabs = t.snatches;
            rel.download_volume_factor = Some(1.0);
            rel.upload_volume_factor = Some(1.0);
            if t.is_freeleech == Some(true) {
                rel.download_volume_factor = Some(0.0);
            }
            if t.is_personal_freeleech == Some(true) {
                rel.download_volume_factor = Some(0.0);
            }
            if t.is_neutral_leech == Some(true) || t.is_freeload == Some(true) {
                rel.download_volume_factor = Some(0.0);
                rel.upload_volume_factor = Some(0.0);
            }
        }
    }

    async fn run_query(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let url = self.build_query_url(query);
        let headers = self.auth_headers();
        let response = self
            .client
            .get(url)
            .headers(headers)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;

        if status.as_u16() >= 400 {
            anyhow::bail!("Invalid status code {} received from indexer", status);
        }
        if self.config.api_key.is_some() && body.contains("failure") {
            if let Ok(json) = serde_json::from_str::<GazelleError>(&body) {
                if !json.error.is_empty() {
                    anyhow::bail!("Unexpected response from indexer: \"{}\"", json.error);
                }
            }
        }

        let mut q = query.clone();
        parse_gazelle_response(&body, self, &mut q)
    }

    fn build_query_url(&self, query: &TorznabQuery) -> Url {
        let api_path = self.config.api_path.as_deref().unwrap_or("ajax.php");
        let mut url = Url::parse(&format!("{}/{}", self.site_link, api_path)).unwrap();
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("action", "browse");
            q.append_pair("order_by", "time");
            q.append_pair("order_way", "desc");
            if let Some(g) = &query.genre {
                if !g.is_empty() {
                    q.append_pair("taglist", g);
                }
            }
            if let Some(imdb) = &query.imdb_id {
                if self.config.imdb_in_search_term {
                    q.append_pair("searchstr", imdb);
                } else if self.imdb_in_tags {
                    q.append_pair("taglist", imdb);
                } else {
                    q.append_pair("cataloguenumber", imdb);
                }
            } else if let Some(s) = &query.search_term {
                if !s.is_empty() {
                    q.append_pair("searchstr", s);
                }
            }
            if let Some(a) = &query.artist {
                if !a.is_empty() && a != "VA" {
                    q.append_pair("artistname", a);
                }
            }
            if let Some(l) = &query.label {
                if !l.is_empty() {
                    q.append_pair("recordlabel", l);
                }
            }
            if let Some(y) = query.year {
                q.append_pair("year", &y.to_string());
            }
            if let Some(al) = &query.album {
                if !al.is_empty() {
                    q.append_pair("groupname", al);
                }
            }
            for cat in self
                .caps
                .categories
                .map_torznab_caps_to_trackers(query, true)
            {
                q.append_pair(&format!("filter_cat[{}]", cat), "1");
            }
            if self.config.freeleech_only {
                q.append_pair("freetorrent", "1");
            }
        }
        url
    }
}

/// Parses a Gazelle browse JSON response into `ReleaseInfo`s using the base's
/// category mappings / hooks. Kept as a free function so it can be unit-tested
/// without a live client.
pub fn parse_gazelle_response(
    body: &str,
    base: &GazelleTracker,
    query: &mut TorznabQuery,
) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
    let parsed: GazelleResponse = serde_json::from_str(body)?;
    let mut releases = Vec::new();

    for group in parsed.response.results {
        let mut base_release = ReleaseInfo::new();
        base_release.publish_date = parse_group_time(&group.group_time);

        let mut title = String::new();
        if let Some(a) = &group.artist {
            if !a.is_empty() {
                title.push_str(a);
                title.push_str(" - ");
            }
        }
        title.push_str(&group.group_name);
        if let Some(gy) = &group.group_year {
            if !gy.is_empty() && gy != "0" {
                title.push_str(&format!(" [{}]", gy));
            }
        }
        if let Some(rt) = &group.release_type {
            if !rt.is_empty() && rt != "Unknown" {
                title.push_str(&format!(" [{}]", rt));
            }
        }
        base_release.title = Some(title);

        if let Some(tags) = &group.tags {
            if !tags.is_empty() && !tags[0].is_empty() {
                base_release.description = Some(format!("Tags: {}\n", tags.join(", ")));
                base_release.genres = tags.clone();
            }
        }
        if let Some(cover) = &group.cover {
            if !cover.is_empty() {
                let poster = if cover.starts_with("http") {
                    Url::parse(cover).ok()
                } else {
                    Url::parse(&format!("{}{}", base.site_link, cover)).ok()
                };
                base_release.poster = poster;
            }
        }
        if base.imdb_in_tags {
            if let Some(tags) = &group.tags {
                base_release.imdb = tags.iter().find_map(|t| parse_imdb_id(t));
            }
        }

        let torrents = group
            .torrents
            .clone()
            .unwrap_or_else(|| vec![group.clone()]);
        for t in torrents {
            if base.should_skip(&t) {
                continue;
            }
            let mut rel = base_release.clone();
            base.fill_from_json(&mut rel, &t);

            if let Some(ratio) = base.config.minimum_ratio {
                rel.minimum_ratio = Some(ratio);
            }
            if let Some(seed_time) = base.config.minimum_seed_time {
                rel.minimum_seed_time = Some(seed_time);
            }

            let mut keep = match &base.post_parse {
                Some(hook) => hook(&mut rel, query),
                None => true,
            };
            if keep {
                if let Some(hook) = &base.json_post_parse {
                    keep = hook(&mut rel, query, &t, &group);
                }
            }
            // AND-filter titles for trackers that ignore the search term.
            if keep && base.config.match_query_string_and {
                keep = query.match_query_string_and(rel.title.as_deref().unwrap_or(""), None, None);
            }
            if keep {
                releases.push(rel);
            }
        }
    }

    releases.sort_by(|a, b| b.publish_date.cmp(&a.publish_date));
    Ok(releases)
}

#[async_trait]
impl IIndexer for GazelleTracker {
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
        self.config.api_key.is_some() || self.config.cookie.is_some()
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
        let headers = self.auth_headers();
        let response = self
            .client
            .get(link.clone())
            .headers(headers.clone())
            .send()
            .await?;
        let bytes = response.bytes().await?;
        let request_link = link.to_string();
        // If the token download returned an HTML error page, retry without the token.
        if bytes.len() >= 1
            && bytes[0] != b'd'
            && request_link.contains("usetoken=1")
        {
            let html = String::from_utf8_lossy(&bytes);
            if html.contains("You do not have any freeleech tokens left.")
                || html.contains("You do not have enough freeleech tokens")
                || html.contains("This torrent is too large.")
                || html.contains("You cannot use tokens here")
            {
                if let Some(new_link) = request_link
                    .replace("&usetoken=1", "")
                    .parse::<Url>()
                    .ok()
                {
                    let resp2 = self
                        .client
                        .get(new_link)
                        .headers(headers)
                        .send()
                        .await?;
                    return Ok(resp2.bytes().await?.to_vec());
                }
            }
        }
        Ok(bytes.to_vec())
    }
}

/// Adds one AlphaRatio category mapping (tracker id -> Torznab cat, with desc).
fn add_alpharatio_cat(caps: &mut TorznabCapabilities, id: i32, name: &str, desc: &str) {
    if let Some(cat) = TorznabCatType::get_cat_by_name(name) {
        caps.categories
            .add_category_mapping_int(id, cat.clone(), Some(desc));
    }
}

/// Builds AlphaRatio (`alpharatio`) — the reference Gazelle tracker.
pub fn build_alpharatio(
    config: GazelleConfig,
) -> anyhow::Result<Arc<dyn IIndexer>> {
    let mut caps = TorznabCapabilities::new();
    caps.search_available = true;
    caps.tv_search_params = vec![TvSearchParam::Q, TvSearchParam::Season, TvSearchParam::Ep];
    caps.movie_search_params = vec![MovieSearchParam::Q, MovieSearchParam::ImdbId];

    add_alpharatio_cat(&mut caps, 1, "TV/SD", "TvSD");
    add_alpharatio_cat(&mut caps, 2, "TV/HD", "TvHD");
    add_alpharatio_cat(&mut caps, 3, "TV/UHD", "TvUHD");
    add_alpharatio_cat(&mut caps,4, "TV/SD", "TvDVDRip");
    add_alpharatio_cat(&mut caps,5, "TV/SD", "TvPackSD");
    add_alpharatio_cat(&mut caps,6, "TV/HD", "TvPackHD");
    add_alpharatio_cat(&mut caps,7, "TV/UHD", "TvPackUHD");
    add_alpharatio_cat(&mut caps,8, "Movies/SD", "MovieSD");
    add_alpharatio_cat(&mut caps,9, "Movies/HD", "MovieHD");
    add_alpharatio_cat(&mut caps,10, "Movies/UHD", "MovieUHD");
    add_alpharatio_cat(&mut caps,11, "Movies/SD", "MoviePackSD");
    add_alpharatio_cat(&mut caps,12, "Movies/HD", "MoviePackHD");
    add_alpharatio_cat(&mut caps,13, "Movies/UHD", "MoviePackUHD");
    add_alpharatio_cat(&mut caps,14, "XXX", "MovieXXX");
    add_alpharatio_cat(&mut caps,15, "Movies/BluRay", "Bluray");
    add_alpharatio_cat(&mut caps,16, "TV/Anime", "AnimeSD");
    add_alpharatio_cat(&mut caps,17, "TV/Anime", "AnimeHD");
    add_alpharatio_cat(&mut caps,18, "PC/Games", "GamesPC");
    add_alpharatio_cat(&mut caps,19, "Console/XBox", "GamesxBox");
    add_alpharatio_cat(&mut caps,20, "Console/PS4", "GamesPS");
    add_alpharatio_cat(&mut caps,21, "Console/Wii", "GamesNin");
    add_alpharatio_cat(&mut caps,22, "PC/0day", "AppsWindows");
    add_alpharatio_cat(&mut caps,23, "PC/Mac", "AppsMAC");
    add_alpharatio_cat(&mut caps,24, "PC/0day", "AppsLinux");
    add_alpharatio_cat(&mut caps,25, "PC/Mobile-Other", "AppsMobile");
    add_alpharatio_cat(&mut caps,26, "XXX", "0dayXXX");
    add_alpharatio_cat(&mut caps,27, "Books/EBook", "eBook");
    add_alpharatio_cat(&mut caps,28, "Audio/Audiobook", "AudioBook");
    add_alpharatio_cat(&mut caps,29, "Audio/Other", "Music");
    add_alpharatio_cat(&mut caps,30, "Other", "Misc");

    let post_parse: GazellePostParse = Arc::new(|rel: &mut ReleaseInfo, _query: &mut TorznabQuery| {
        rel.minimum_ratio = Some(1.0);
        rel.minimum_seed_time = Some(259200);
        true
    });

    GazelleTracker::new(
        "alpharatio",
        "AlphaRatio",
        "AlphaRatio (AR) is a Private Torrent Tracker for 0DAY / GENERAL",
        "https://alpharatio.cc/",
        "en-US",
        caps,
        config,
        true, // imdbInTags
        Some(post_parse),
        None,
    )
}

/// Adds one Redacted category mapping (tracker id -> Torznab cat, with desc).
fn add_redacted_cat(caps: &mut TorznabCapabilities, id: i32, name: &str, desc: &str) {
    if let Some(cat) = TorznabCatType::get_cat_by_name(name) {
        caps.categories
            .add_category_mapping_int(id, cat.clone(), Some(desc));
    }
}

/// Builds Redacted (`redacted`) — the flagship Gazelle music tracker.
///
/// Redacted authenticates with an API key (Authorization header) and downloads
/// via `ajax.php?action=download` (no passkey/authkey). It also AND-filters
/// results by the query string and pins a minimum ratio of 0.6.
pub fn build_redacted(config: GazelleConfig) -> anyhow::Result<Arc<dyn IIndexer>> {
    let mut caps = TorznabCapabilities::new();
    caps.search_available = true;
    caps.movie_search_params = vec![MovieSearchParam::Q, MovieSearchParam::Genre];
    caps.music_search_params = vec![
        MusicSearchParam::Q,
        MusicSearchParam::Album,
        MusicSearchParam::Artist,
        MusicSearchParam::Label,
        MusicSearchParam::Year,
        MusicSearchParam::Genre,
    ];
    caps.book_search_params = vec![BookSearchParam::Q, BookSearchParam::Genre];

    add_redacted_cat(&mut caps, 1, "Audio", "Music");
    add_redacted_cat(&mut caps, 2, "PC", "Applications");
    add_redacted_cat(&mut caps, 3, "Books", "E-Books");
    add_redacted_cat(&mut caps, 4, "Audio/Audiobook", "Audiobooks");
    add_redacted_cat(&mut caps, 5, "Movies", "E-Learning Videos");
    add_redacted_cat(&mut caps, 6, "Audio", "Comedy");
    add_redacted_cat(&mut caps, 7, "Books", "Comics");

    let post_parse: GazellePostParse = Arc::new(|rel: &mut ReleaseInfo, query: &mut TorznabQuery| {
        // Redacted uses a proportional ratio system; default MR to the maximum
        // required to cover the whole range (same as the C# port).
        rel.minimum_ratio = Some(0.6);
        query.match_query_string_and(rel.title.as_deref().unwrap_or(""), None, None)
    });

    let mut config = config;
    config.download_via_ajax = true;
    config.use_tokens = true;

    GazelleTracker::new(
        "redacted",
        "Redacted",
        "Redacted is a Private site for MUSIC",
        "https://redacted.sh/",
        "en-US",
        caps,
        config,
        false, // imdbInTags
        Some(post_parse),
        None,
    )
}

// ----------------------------------------------------------------------------
// Gazelle JSON shape (camelCase fields). Both group- and torrent-level fields are
// flattened into one struct because a group may carry torrent fields directly
// when it has no `torrents` array.
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct GazelleResult {
    pub group_time: String,
    pub group_name: String,
    pub artist: Option<String>,
    pub cover: Option<String>,
    pub tags: Option<Vec<String>>,
    pub group_year: Option<String>,
    pub release_type: Option<String>,
    pub torrents: Option<Vec<GazelleResult>>,
    pub torrent_id: Option<i64>,
    pub size: Option<i64>,
    pub seeders: Option<i64>,
    pub leechers: Option<i64>,
    pub category: Option<String>,
    pub file_count: Option<i64>,
    pub snatches: Option<i64>,
    pub is_freeleech: Option<bool>,
    pub is_personal_freeleech: Option<bool>,
    pub is_neutral_leech: Option<bool>,
    pub is_freeload: Option<bool>,
    pub time: Option<String>,
    pub remastered: Option<bool>,
    pub remaster_year: Option<String>,
    pub remaster_title: Option<String>,
    pub format: Option<String>,
    pub encoding: Option<String>,
    pub has_log: Option<bool>,
    pub log_score: Option<String>,
    pub has_cue: Option<bool>,
    pub lang: Option<String>,
    pub media: Option<String>,
    pub resolution: Option<String>,
    pub container: Option<String>,
    pub codec: Option<String>,
    pub audio: Option<String>,
    pub subbing: Option<String>,
    /// Site-specific JSON fields not covered above (e.g. `freeType`,
    /// `groupSubName`, `imdbId`, `source`, `processing`, `fileName`).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl GazelleResult {
    /// Reads a site-specific string field from the flattened JSON map.
    pub fn extra_str(&self, key: &str) -> Option<String> {
        self.extra.get(key).and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
    }
}

impl Default for GazelleResult {
    fn default() -> Self {
        Self {
            group_time: String::new(),
            group_name: String::new(),
            artist: None,
            cover: None,
            tags: None,
            group_year: None,
            release_type: None,
            torrents: None,
            torrent_id: None,
            size: None,
            seeders: None,
            leechers: None,
            category: None,
            file_count: None,
            snatches: None,
            is_freeleech: None,
            is_personal_freeleech: None,
            is_neutral_leech: None,
            is_freeload: None,
            time: None,
            remastered: None,
            remaster_year: None,
            remaster_title: None,
            format: None,
            encoding: None,
            has_log: None,
            log_score: None,
            has_cue: None,
            lang: None,
            media: None,
            resolution: None,
            container: None,
            codec: None,
            audio: None,
            subbing: None,
            extra: serde_json::Map::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct GazelleResponse {
    #[serde(rename = "response")]
    response: GazelleResponseInner,
}

#[derive(Debug, Deserialize)]
struct GazelleResponseInner {
    results: Vec<GazelleResult>,
}

#[derive(Debug, Deserialize)]
struct GazelleError {
    #[serde(default)]
    error: String,
}

fn parse_group_time(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if let Ok(secs) = s.parse::<i64>() {
        if let Some(dt) = DateTime::<Utc>::from_timestamp(secs, 0) {
            return Some(dt);
        }
    }
    DateTimeUtil::from_unknown(s)
}

fn parse_torrent_time(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if s.len() >= 19 {
        if let Ok(dt) = NaiveDateTime::parse_from_str(&s[..19], "%Y-%m-%d %H:%M:%S") {
            return Some(Utc.from_utc_datetime(&dt));
        }
    }
    DateTimeUtil::from_unknown(s)
}

fn parse_imdb_id(tag: &str) -> Option<i64> {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"tt(\d+)").unwrap());
    RE.captures(tag)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<i64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "status": "success",
      "response": {
        "results": [
          {
            "groupName": "Big Buck Bunny",
            "artist": "Blender",
            "groupYear": "2008",
            "releaseType": "Movie",
            "groupTime": "1700000000",
            "cover": "https://alpharatio.cc/covers/123.jpg",
            "tags": ["imdb:tt1254207", "Animation"],
            "torrents": [
              {
                "torrentId": 555,
                "size": 1610612736,
                "seeders": 42,
                "leechers": 3,
                "category": "MovieHD",
                "fileCount": 1,
                "snatches": 100,
                "isFreeleech": true,
                "format": "x264",
                "encoding": "1080p",
                "time": "2023-11-14 22:13:37 +0000"
              }
            ]
          }
        ]
      }
    }"#;

    #[test]
    fn parses_gazelle_response_offline() {
        let base = GazelleTracker {
            id: "alpharatio".into(),
            name: "AlphaRatio".into(),
            description: "test".into(),
            site_link: "https://alpharatio.cc".into(),
            language: "en-US".into(),
            caps: {
                let mut c = TorznabCapabilities::new();
                if let Some(cat) = TorznabCatType::get_cat_by_name("Movies/HD") {
                    c.categories.add_category_mapping_int(9, cat.clone(), Some("MovieHD"));
                }
                c
            },
            client: reqwest::Client::new(),
            config: GazelleConfig {
                use_tokens: true,
                ..Default::default()
            },
            imdb_in_tags: true,
            post_parse: Some(Arc::new(|r, _q: &mut TorznabQuery| {
                r.minimum_ratio = Some(1.0);
                true
            })),
            json_post_parse: None,
        };

        let mut q = TorznabQuery::default();
        let rels = parse_gazelle_response(SAMPLE, &base, &mut q).expect("parse");
        assert_eq!(rels.len(), 1);
        let r = &rels[0];
        let title = r.title.as_deref().unwrap();
        assert!(title.starts_with("Blender - Big Buck Bunny [2008] [Movie]"));
        assert!(title.contains("x264 / 1080p"), "title was: {title}");
        assert_eq!(r.size, Some(1610612736));
        assert_eq!(r.seeders, Some(42));
        assert_eq!(r.peers, Some(45));
        assert!(r.category.contains(&2040), "category was: {:?}", r.category); // MovieHD (+ custom 100009)
        assert_eq!(r.download_volume_factor, Some(0.0)); // isFreeleech
        assert_eq!(r.imdb, Some(1254207));
        assert_eq!(r.minimum_ratio, Some(1.0));
        assert!(r.details.is_some());
        let link = r.link.as_ref().unwrap().to_string();
        assert!(link.contains("action=download"));
        assert!(link.contains("usetoken=1"));
    }

    #[test]
    fn redacted_uses_ajax_download_and_pins_ratio() {
        // Redacted authenticates via api key and downloads through ajax.php (no
        // torrent_pass / authkey on the link), and pins MinimumRatio = 0.6.
        let idx = build_redacted(GazelleConfig {
            api_key: Some("deadbeef".into()),
            ..Default::default()
        })
        .expect("build");

        assert_eq!(idx.id(), "redacted");
        assert!(idx.torznab_caps().music_search_params.len() >= 6);

        let mut q = TorznabQuery {
            search_term: Some("bunny".into()),
            ..Default::default()
        };
        let rels = parse_gazelle_response(SAMPLE, &base_for_redacted(), &mut q).expect("parse");
        assert_eq!(rels.len(), 1);
        let r = &rels[0];
        assert_eq!(r.minimum_ratio, Some(0.6));
        let link = r.link.as_ref().unwrap().to_string();
        assert!(link.contains("ajax.php?action=download"), "link: {link}");
        assert!(!link.contains("torrent_pass"), "link: {link}");
    }

    // Build a GazelleTracker mimicking Redacted's parsing (audio/Music category) for
    // the offline test above without needing a live client.
    fn base_for_redacted() -> GazelleTracker {
        let mut caps = TorznabCapabilities::new();
        if let Some(cat) = TorznabCatType::get_cat_by_name("Audio") {
            caps.categories.add_category_mapping_int(1, cat.clone(), Some("Music"));
        }
        GazelleTracker {
            id: "redacted".into(),
            name: "Redacted".into(),
            description: "test".into(),
            site_link: "https://redacted.sh".into(),
            language: "en-US".into(),
            caps,
            client: reqwest::Client::new(),
            config: GazelleConfig {
                api_key: Some("deadbeef".into()),
                download_via_ajax: true,
                use_tokens: true,
                ..Default::default()
            },
            imdb_in_tags: false,
            post_parse: Some(Arc::new(|rel: &mut ReleaseInfo, query: &mut TorznabQuery| {
                rel.minimum_ratio = Some(0.6);
                query.match_query_string_and(rel.title.as_deref().unwrap_or(""), None, None)
            })),
            json_post_parse: None,
        }
    }
}
