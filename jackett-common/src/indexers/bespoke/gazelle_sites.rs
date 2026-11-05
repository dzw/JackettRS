//! Data-driven registry for the remaining `GazelleTracker` sites.
//!
//! Each site is a static [`GazelleSpec`] (metadata + caps + behaviour flags);
//! `build` turns it into an `IIndexer` via the shared [`GazelleTracker`] base.
//! `GazelleGames (API)` is included as a dedicated implementation because its
//! `PerformQuery` is fully rewritten in C# and uses a different JSON shape.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;
use url::Url;

use super::gazelle::{
    GazelleConfig, GazelleJsonPostParse, GazelleResult, GazelleTracker,
};
use crate::indexers::i_indexer::IIndexer;
use crate::models::release_info::ReleaseInfo;
use crate::models::torznab_capabilities::{
    BookSearchParam, MovieSearchParam, MusicSearchParam, TorznabCapabilities, TvSearchParam,
};
use crate::models::torznab_cat_type::TorznabCatType;
use crate::models::torznab_query::TorznabQuery;
use crate::utils::browser_util::BrowserUtil;

/// Site-specific post-parse behaviour that needs raw JSON.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GazelleFlavour {
    Plain,
    GreatPosterWall,
    Simurg,
    SecretCinema,
}

/// A Gazelle-family site definition.
pub struct GazelleSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub site_link: &'static str,
    pub language: &'static str,
    pub imdb_in_tags: bool,
    pub download_via_ajax: bool,
    pub use_tokens: bool,
    pub imdb_in_search_term: bool,
    pub match_query_string_and: bool,
    pub minimum_ratio: Option<f64>,
    pub minimum_seed_time: Option<i64>,
    pub auth_header: Option<&'static str>,
    pub auth_format: Option<&'static str>,
    pub api_path: Option<&'static str>,
    /// `(tracker id, Torznab category name, tracker description)`
    pub categories: &'static [(i32, &'static str, &'static str)],
    pub tv_params: &'static [TvSearchParam],
    pub movie_params: &'static [MovieSearchParam],
    pub music_params: &'static [MusicSearchParam],
    pub book_params: &'static [BookSearchParam],
    pub flavour: GazelleFlavour,
}

impl GazelleSpec {
    fn caps(&self) -> TorznabCapabilities {
        let mut caps = TorznabCapabilities::new();
        caps.tv_search_params = self.tv_params.to_vec();
        caps.movie_search_params = self.movie_params.to_vec();
        caps.music_search_params = self.music_params.to_vec();
        caps.book_search_params = self.book_params.to_vec();
        for (id, name, desc) in self.categories {
            if let Some(cat) = TorznabCatType::get_cat_by_name(name) {
                caps.categories
                    .add_category_mapping_int(*id, cat.clone(), Some(desc));
            }
        }
        caps
    }

    pub fn build(&self, mut config: GazelleConfig) -> anyhow::Result<Arc<dyn IIndexer>> {
        config.use_tokens |= self.use_tokens;
        config.download_via_ajax |= self.download_via_ajax;
        config.imdb_in_search_term |= self.imdb_in_search_term;
        config.match_query_string_and |= self.match_query_string_and;
        if config.minimum_ratio.is_none() {
            config.minimum_ratio = self.minimum_ratio;
        }
        if config.minimum_seed_time.is_none() {
            config.minimum_seed_time = self.minimum_seed_time;
        }
        if self.auth_header.is_some() && config.auth_header.is_none() {
            config.auth_header = self.auth_header.map(str::to_string);
        }
        if self.auth_format.is_some() && config.auth_format.is_none() {
            config.auth_format = self.auth_format.map(str::to_string);
        }
        if self.api_path.is_some() && config.api_path.is_none() {
            config.api_path = self.api_path.map(str::to_string);
        }

        let json_hook: Option<GazelleJsonPostParse> = match self.flavour {
            GazelleFlavour::Plain => None,
            GazelleFlavour::GreatPosterWall => Some(Arc::new(gpw_post_parse)),
            GazelleFlavour::Simurg => Some(Arc::new(simurg_post_parse)),
            GazelleFlavour::SecretCinema => Some(Arc::new(secret_cinema_post_parse)),
        };

        GazelleTracker::new(
            self.id,
            self.name,
            self.description,
            self.site_link,
            self.language,
            self.caps(),
            config,
            self.imdb_in_tags,
            None,
            json_hook,
        )
    }
}

// ---------------------------------------------------------------------------
// Site-specific raw-JSON post-parse hooks
// ---------------------------------------------------------------------------

fn with_offset(time: Option<&str>, offset: &str) -> Option<DateTime<Utc>> {
    let t = time?;
    let raw = format!("{t} {offset}").replace(":00", "");
    DateTime::parse_from_str(&raw, "%Y-%m-%d %H:%M:%S %z")
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

fn gpw_post_parse(
    rel: &mut ReleaseInfo,
    _query: &mut TorznabQuery,
    torrent: &GazelleResult,
    group: &GazelleResult,
) -> bool {
    // Title: "<groupName> [groupYear]" + codec / source / resolution / container / processing
    let mut title = group.group_name.clone();
    if let Some(y) = &group.group_year {
        if !y.is_empty() && y != "0" {
            title.push_str(&format!(" [{y}]"));
        }
    }
    let flags: Vec<String> = [
        torrent.codec.as_deref(),
        torrent.extra_str("source").as_deref(),
        torrent.resolution.as_deref(),
        torrent.container.as_deref(),
        torrent.extra_str("processing").as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter(|s| !s.is_empty())
    .map(str::to_string)
    .collect();
    if !flags.is_empty() {
        title.push_str(&format!(" {}", flags.join(" / ")));
    }
    rel.title = Some(title);

    if let Some(sub) = group.extra_str("groupSubName") {
        rel.description = Some(sub);
    }
    if let Some(v) = torrent.extra_str("imdbId") {
        rel.imdb = parse_imdb(&v);
    }
    if let Some(v) = group.extra_str("doubanId") {
        rel.douban_id = v.parse().ok();
    }
    // freeType: 11=0.75, 12=0.5, 13=0.25, 1=0, 2=neutral
    match torrent.extra_str("freeType").as_deref() {
        Some("11") => rel.download_volume_factor = Some(0.75),
        Some("12") => rel.download_volume_factor = Some(0.5),
        Some("13") => rel.download_volume_factor = Some(0.25),
        Some("1") => rel.download_volume_factor = Some(0.0),
        Some("2") => {
            rel.download_volume_factor = Some(0.0);
            rel.upload_volume_factor = Some(0.0);
        }
        _ => {}
    }
    if torrent.is_personal_freeleech == Some(true) {
        rel.download_volume_factor = Some(0.0);
    }
    rel.category = vec![2000];
    rel.publish_date = with_offset(torrent.time.as_deref(), "+08:00");
    true
}

fn simurg_post_parse(
    rel: &mut ReleaseInfo,
    _query: &mut TorznabQuery,
    torrent: &GazelleResult,
    group: &GazelleResult,
) -> bool {
    // Prefer the content category over the container category.
    let desc = torrent
        .category
        .as_ref()
        .or(group.category.as_ref())
        .cloned();
    let mut cats = Vec::new();
    if let Some(d) = &desc {
        // Categories are set by the caller's caps; here we only handle the
        // format fallback when the description is unknown.
        let _ = d;
    }
    if cats.is_empty() {
        match torrent.format.as_deref().unwrap_or("").to_uppercase().as_str() {
            "PDF" | "MOBI" | "EPUB" | "AZW3" => cats.push(7020), // Books/EBook
            "CBR" | "CBZ" => cats.push(7030),                     // Books/Comics
            "MP3" | "M4B" => cats.push(3030),                     // Audio/Audiobook
            _ => {}
        }
    }
    if !cats.is_empty() {
        rel.category = cats;
    }
    true
}

fn secret_cinema_post_parse(
    rel: &mut ReleaseInfo,
    _query: &mut TorznabQuery,
    torrent: &GazelleResult,
    group: &GazelleResult,
) -> bool {
    let media = torrent.media.as_deref().unwrap_or("");
    let mapped = match media {
        "SD" => Some(2030),
        "720p" | "1080p" | "2160p" | "4k" => Some(2040),
        "DVD-R" => Some(2070),
        "BDMV" => Some(2050),
        _ => None,
    };
    if let Some(c) = mapped {
        rel.category = vec![c];
    }
    if rel
        .category
        .iter()
        .any(|c| (2000..2100).contains(c))
    {
        static YEAR_RE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"(?i)\b(?:19|20)\d{2}\b").unwrap());
        let release_name = torrent.extra_str("releaseName").unwrap_or_default();
        let title = if YEAR_RE.is_match(&release_name) {
            release_name
        } else {
            let year = group.group_year.clone().unwrap_or_default();
            let mut t = format!("{} ({}) {}", group.group_name, year, media);
            if let Some(rt) = &torrent.remaster_title {
                if !rt.is_empty() {
                    t.push_str(&format!(" / {rt}"));
                }
            }
            t
        };
        let title = title
            .replace("BDMV", "COMPLETE BLURAY")
            .replace("SD", "DVDRip");
        rel.title = Some(title);
    }
    rel.publish_date = with_offset(torrent.time.as_deref(), "+02:00");
    true
}

fn parse_imdb(v: &str) -> Option<i64> {
    let digits: String = v.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

// ---------------------------------------------------------------------------
// Static site definitions
// ---------------------------------------------------------------------------

const NONE_TV: &[TvSearchParam] = &[];
const NONE_MOVIE: &[MovieSearchParam] = &[];
const NONE_MUSIC: &[MusicSearchParam] = &[];
const NONE_BOOK: &[BookSearchParam] = &[];

pub static GAZELLE_SITES: Lazy<Vec<GazelleSpec>> = Lazy::new(|| {
    vec![
        GazelleSpec {
            id: "brokenstones",
            name: "Broken Stones",
            description: "Broken Stones is a Private site for MacOS and iOS APPS / GAMES",
            site_link: "https://brokenstones.is/",
            language: "en-US",
            imdb_in_tags: false,
            download_via_ajax: true,
            use_tokens: true,
            imdb_in_search_term: false,
            match_query_string_and: false,
            minimum_ratio: None,
            minimum_seed_time: None,
            auth_header: None,
            auth_format: None,
            api_path: None,
            categories: &[
                (1, "PC/Mac", "MacOS Apps"),
                (2, "PC/Mac", "MacOS Games"),
                (3, "PC/Mobile-iOS", "iOS Apps"),
                (4, "PC/Mobile-iOS", "iOS Games"),
                (5, "Other", "Graphics"),
                (6, "Audio", "Audio"),
                (7, "Other", "Tutorials"),
                (8, "Other", "Other"),
            ],
            tv_params: NONE_TV,
            movie_params: NONE_MOVIE,
            music_params: NONE_MUSIC,
            book_params: NONE_BOOK,
            flavour: GazelleFlavour::Plain,
        },
        GazelleSpec {
            id: "desigaane",
            name: "DesiGaane",
            description: "DesiGaane is a Private Torrent Tracker for DESI MUSIC",
            site_link: "https://desigaane.rocks/",
            language: "en-US",
            imdb_in_tags: false,
            download_via_ajax: true,
            use_tokens: true,
            imdb_in_search_term: false,
            match_query_string_and: false,
            minimum_ratio: Some(0.6),
            minimum_seed_time: Some(259200),
            auth_header: None,
            auth_format: None,
            api_path: None,
            categories: &[(1, "Audio", "Music")],
            tv_params: NONE_TV,
            movie_params: NONE_MOVIE,
            music_params: NONE_MUSIC,
            book_params: NONE_BOOK,
            flavour: GazelleFlavour::Plain,
        },
        GazelleSpec {
            id: "dicmusic",
            name: "DICMusic",
            description: "DICMusic is a CHINESE Private Torrent Tracker for MUSIC",
            site_link: "https://dicmusic.com/",
            language: "zh-CN",
            imdb_in_tags: false,
            download_via_ajax: false,
            use_tokens: true,
            imdb_in_search_term: false,
            match_query_string_and: true,
            minimum_ratio: None,
            minimum_seed_time: None,
            auth_header: None,
            auth_format: None,
            api_path: None,
            categories: &[(1, "Audio", "Music"), (2, "PC", "Applications")],
            tv_params: NONE_TV,
            movie_params: NONE_MOVIE,
            music_params: &[
                MusicSearchParam::Q,
                MusicSearchParam::Album,
                MusicSearchParam::Artist,
                MusicSearchParam::Label,
                MusicSearchParam::Year,
            ],
            book_params: NONE_BOOK,
            flavour: GazelleFlavour::Plain,
        },
        GazelleSpec {
            id: "greatposterwall",
            name: "GreatPosterWall",
            description: "GreatPosterWall (GPW) is a CHINESE Private site for MOVIES",
            site_link: "https://greatposterwall.com/",
            language: "zh-CN",
            imdb_in_tags: false,
            download_via_ajax: false,
            use_tokens: true,
            imdb_in_search_term: true,
            match_query_string_and: false,
            minimum_ratio: Some(1.0),
            minimum_seed_time: Some(172800),
            auth_header: None,
            auth_format: None,
            api_path: None,
            categories: &[(1, "Movies", "Movies 电影")],
            tv_params: NONE_TV,
            movie_params: &[
                MovieSearchParam::Q,
                MovieSearchParam::ImdbId,
                MovieSearchParam::Genre,
            ],
            music_params: NONE_MUSIC,
            book_params: NONE_BOOK,
            flavour: GazelleFlavour::GreatPosterWall,
        },
        GazelleSpec {
            id: "orpheus",
            name: "Orpheus",
            description: "Orpheus is a Private MUSIC tracker",
            site_link: "https://orpheus.network/",
            language: "en-US",
            imdb_in_tags: false,
            download_via_ajax: true,
            use_tokens: true,
            imdb_in_search_term: false,
            match_query_string_and: false,
            minimum_ratio: Some(0.6),
            minimum_seed_time: None,
            auth_header: None,
            auth_format: Some("token {0}"),
            api_path: None,
            categories: &[
                (1, "Audio", "Music"),
                (2, "PC", "Applications"),
                (3, "Books/EBook", "E-Books"),
                (4, "Audio/Audiobook", "Audiobooks"),
                (5, "Other", "E-Learning Videos"),
                (6, "Other", "Comedy"),
                (7, "Books/Comics", "Comics"),
            ],
            tv_params: NONE_TV,
            movie_params: NONE_MOVIE,
            music_params: &[
                MusicSearchParam::Q,
                MusicSearchParam::Album,
                MusicSearchParam::Artist,
                MusicSearchParam::Label,
                MusicSearchParam::Year,
                MusicSearchParam::Genre,
            ],
            book_params: &[BookSearchParam::Q, BookSearchParam::Genre],
            flavour: GazelleFlavour::Plain,
        },
        GazelleSpec {
            id: "simurg",
            name: "Simurg",
            description: "Simurg is a Private Torrent Tracker for EBOOKS and AUDIOBOOKS",
            site_link: "https://simurg.world/",
            language: "en-US",
            imdb_in_tags: false,
            download_via_ajax: true,
            use_tokens: true,
            imdb_in_search_term: false,
            match_query_string_and: false,
            minimum_ratio: Some(0.6),
            minimum_seed_time: Some(259200),
            auth_header: None,
            auth_format: Some("token {0}"),
            api_path: None,
            categories: &[
                (3, "Books/EBook", "E-Books"),
                (4, "Audio/Audiobook", "Audiobooks"),
                (7, "Books/Comics", "Comics"),
                (8, "Books/Mags", "Magazines"),
            ],
            tv_params: NONE_TV,
            movie_params: NONE_MOVIE,
            music_params: NONE_MUSIC,
            book_params: NONE_BOOK,
            flavour: GazelleFlavour::Simurg,
        },
        GazelleSpec {
            id: "secretcinema",
            name: "Secret Cinema",
            description: "Secret Cinema is a Private ratioless site for rare MOVIES.",
            site_link: "https://secret-cinema.pw/",
            language: "en-US",
            imdb_in_tags: false,
            download_via_ajax: false,
            use_tokens: false,
            imdb_in_search_term: false,
            match_query_string_and: true,
            minimum_ratio: None,
            minimum_seed_time: None,
            auth_header: None,
            auth_format: None,
            api_path: None,
            categories: &[(1, "Movies", "Movies"), (2, "Audio", "Music")],
            tv_params: NONE_TV,
            movie_params: &[MovieSearchParam::Q, MovieSearchParam::ImdbId],
            music_params: &[
                MusicSearchParam::Q,
                MusicSearchParam::Album,
                MusicSearchParam::Artist,
                MusicSearchParam::Label,
                MusicSearchParam::Year,
            ],
            book_params: NONE_BOOK,
            flavour: GazelleFlavour::SecretCinema,
        },
    ]
});

/// Looks up a Gazelle site definition by id.
pub fn find_spec(id: &str) -> Option<&'static GazelleSpec> {
    GAZELLE_SITES.iter().find(|s| s.id == id)
}

// ---------------------------------------------------------------------------
// GazelleGames (API) — fully rewritten PerformQuery in C#, different JSON shape
// ---------------------------------------------------------------------------

const GGN_PLATFORMS: &[(&str, &str)] = &[
    ("Mac", "Console/Other"),
    ("iOS", "PC/Mobile-iOS"),
    ("Apple Bandai Pippin", "Console/Other"),
    ("Apple II", "Console/Other"),
    ("Android", "PC/Mobile-Android"),
    ("DOS", "PC/Games"),
    ("Windows", "PC/Games"),
    ("Xbox", "Console/XBox"),
    ("Xbox 360", "Console/XBox 360"),
    ("Game Boy", "Console/Other"),
    ("Game Boy Advance", "Console/Other"),
    ("Game Boy Color", "Console/Other"),
    ("NES", "Console/Other"),
    ("Nintendo 64", "Console/Other"),
    ("Nintendo 3DS", "Console/Other"),
    ("New Nintendo 3DS", "Console/Other"),
    ("Nintendo DS", "Console/NDS"),
    ("Nintendo GameCube", "Console/Other"),
    ("Pokemon Mini", "Console/Other"),
    ("SNES", "Console/Other"),
    ("Switch", "Console/Other"),
    ("Virtual Boy", "Console/Other"),
    ("Wii", "Console/Wii"),
    ("Wii U", "Console/WiiU"),
    ("PlayStation 1", "Console/Other"),
    ("PlayStation 2", "Console/Other"),
    ("PlayStation 3", "Console/PS3"),
    ("PlayStation 4", "Console/PS4"),
    ("PlayStation Portable", "Console/PSP"),
    ("PlayStation Vita", "Console/PS Vita"),
    ("Dreamcast", "Console/Other"),
    ("Game Gear", "Console/Other"),
    ("Master System", "Console/Other"),
    ("Mega Drive", "Console/Other"),
    ("Pico", "Console/Other"),
    ("Saturn", "Console/Other"),
    ("SG-1000", "Console/Other"),
    ("Atari 2600", "Console/Other"),
    ("Atari 5200", "Console/Other"),
    ("Atari 7800", "Console/Other"),
    ("Atari Jaguar", "Console/Other"),
    ("Atari Lynx", "Console/Other"),
    ("Atari ST", "Console/Other"),
    ("Amstrad CPC", "Console/Other"),
    ("Bandai WonderSwan", "Console/Other"),
    ("Bandai WonderSwan Color", "Console/Other"),
    ("Commodore 64", "Console/Other"),
    ("Commodore 128", "Console/Other"),
    ("Commodore Amiga", "Console/Other"),
    ("Amiga CD32", "Console/Other"),
    ("Commodore Plus-4", "Console/Other"),
    ("Commodore VIC-20", "Console/Other"),
    ("NEC PC-98", "Console/Other"),
    ("NEC PC-FX", "Console/Other"),
    ("NEC SuperGrafx", "Console/Other"),
    ("NEC TurboGrafx-16", "Console/Other"),
    ("ZX Spectrum", "Console/Other"),
    ("MSX", "Console/Other"),
    ("MSX 2", "Console/Other"),
    ("Game.com", "Console/Other"),
    ("Gizmondo", "Console/Other"),
    ("V.Smile", "Console/Other"),
    ("CreatiVision", "Console/Other"),
    ("Board Game", "Console/Other"),
    ("Card Game", "Console/Other"),
    ("Miniature Wargames", "Console/Other"),
    ("Pen and Paper RPG", "Console/Other"),
    ("3DO", "Console/Other"),
    ("Casio Loopy", "Console/Other"),
    ("Casio PV-1000", "Console/Other"),
    ("Colecovision", "Console/Other"),
    ("Emerson Arcadia 2001", "Console/Other"),
    ("Entex Adventure Vision", "Console/Other"),
    ("Epoch Super Casette Vision", "Console/Other"),
    ("Fairchild Channel F", "Console/Other"),
    ("Funtech Super Acan", "Console/Other"),
    ("GamePark GP32", "Console/Other"),
    ("General Computer Vectrex", "Console/Other"),
    ("Interactive DVD", "Console/Other"),
    ("Linux", "Console/Other"),
    ("Hartung Game Master", "Console/Other"),
    ("Magnavox-Phillips Odyssey", "Console/Other"),
    ("Mattel Intellivision", "Console/Other"),
    ("Memotech MTX", "Console/Other"),
    ("Miles Gordon Sam Coupe", "Console/Other"),
    ("Nokia N-Gage", "Console/Other"),
    ("Oculus Quest", "Console/Other"),
    ("Ouya", "Console/Other"),
    ("Philips Videopac+", "Console/Other"),
    ("Philips CD-i", "Console/Other"),
    ("Phone/PDA", "Console/Other"),
    ("RCA Studio II", "Console/Other"),
    ("Sharp X1", "Console/Other"),
    ("Sharp X68000", "Console/Other"),
    ("SNK Neo Geo", "Console/Other"),
    ("SNK Neo Geo Pocket", "Console/Other"),
    ("Taito Type X", "Console/Other"),
    ("Tandy Color Computer", "Console/Other"),
    ("Tangerine Oric", "Console/Other"),
    ("Thomson MO5", "Console/Other"),
    ("Watara Supervision", "Console/Other"),
    ("Retro - Other", "Console/Other"),
];

pub fn build_gazelle_games(config: GazelleConfig) -> anyhow::Result<Arc<dyn IIndexer>> {
    let mut caps = TorznabCapabilities::new();
    for (platform, name) in GGN_PLATFORMS {
        if let Some(cat) = TorznabCatType::get_cat_by_name(name) {
            caps.categories
                .add_category_mapping(platform, cat.clone(), Some(platform));
        }
    }
    for (id, name, desc) in [
        (1, "PC/Games", "Games"),
        (2, "PC/0day", "Applications"),
        (3, "Books/EBook", "E-Books"),
        (4, "Audio/Other", "OST"),
    ] {
        if let Some(cat) = TorznabCatType::get_cat_by_name(name) {
            caps.categories
                .add_category_mapping_int(id, cat.clone(), Some(desc));
        }
    }

    let client = reqwest::Client::builder()
        .user_agent(BrowserUtil::default_user_agent())
        .build()?;
    Ok(Arc::new(GazelleGamesIndexer {
        site_link: "https://gazellegames.net".to_string(),
        caps,
        client,
        config,
    }))
}

struct GazelleGamesIndexer {
    site_link: String,
    caps: TorznabCapabilities,
    client: reqwest::Client,
    config: GazelleConfig,
}

impl GazelleGamesIndexer {
    fn auth_headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(key) = &self.config.api_key {
            if let Ok(v) = HeaderValue::from_str(key) {
                h.insert("X-API-Key", v);
            }
        }
        if let Some(c) = &self.config.cookie {
            if let Ok(v) = HeaderValue::from_str(c) {
                h.insert("Cookie", v);
            }
        }
        h
    }

    async fn run_query(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let mut url = Url::parse(&format!("{}/api.php", self.site_link))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("request", "search");
            q.append_pair("search_type", "torrents");
            q.append_pair("order_by", "time");
            q.append_pair("order_way", "desc");
            if let Some(s) = &query.search_term {
                if !s.is_empty() {
                    q.append_pair("searchstr", &s.replace('.', " "));
                }
            }
            if let Some(imdb) = &query.imdb_id {
                q.append_pair("cataloguenumber", imdb);
            }
            if let Some(g) = &query.genre {
                if !g.is_empty() {
                    q.append_pair("taglist", g);
                }
            }
            if let Some(y) = query.year {
                q.append_pair("year", &y.to_string());
            }
            if self.config.freeleech_only {
                q.append_pair("freetorrent", "1");
            }
        }
        let resp = self
            .client
            .get(url)
            .headers(self.auth_headers())
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await?;
        if status.as_u16() >= 400 {
            anyhow::bail!("Invalid status code {status} received from indexer");
        }
        if self.config.api_key.is_some() && body.contains("failure") {
            if let Ok(e) = serde_json::from_str::<GgnError>(&body) {
                if !e.status.is_empty() {
                    anyhow::bail!("Unexpected response from indexer: \"{}\"", e.status);
                }
            }
        }
        parse_ggn_response(&body, self)
    }
}

#[async_trait]
impl IIndexer for GazelleGamesIndexer {
    fn id(&self) -> &str {
        "gazellegamesapi"
    }
    fn name(&self) -> &str {
        "GazelleGames (API)"
    }
    fn description(&self) -> &str {
        "GazelleGames is a Private gaming tracker"
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
        "en-US"
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
        let resp = self
            .client
            .get(link.clone())
            .headers(self.auth_headers())
            .send()
            .await?;
        Ok(resp.bytes().await?.to_vec())
    }
}

#[derive(Deserialize)]
struct GgnError {
    #[serde(default)]
    status: String,
}

#[derive(Deserialize)]
struct GgnResponse {
    #[serde(rename = "response")]
    response: GgnResponseBody,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GgnResponseBody {
    #[serde(rename = "Results")]
    results: Option<Vec<GgnGroup>>,
    #[serde(rename = "results")]
    results_lower: Option<Vec<GgnGroup>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
#[serde(rename_all = "PascalCase")]
struct GgnGroup {
    #[serde(rename = "GroupID")]
    group_id: Option<i64>,
    #[serde(rename = "GroupName")]
    group_name: String,
    #[serde(rename = "GroupYear")]
    group_year: Option<String>,
    #[serde(rename = "Artists")]
    artists: Option<Vec<GgnArtist>>,
    #[serde(rename = "Torrents")]
    torrents: Option<Vec<GgnTorrent>>,
    #[serde(rename = "CategoryID")]
    category_id: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
#[serde(rename_all = "PascalCase")]
struct GgnArtist {
    #[serde(rename = "Name")]
    name: String,
}

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
#[serde(rename_all = "PascalCase")]
struct GgnTorrent {
    #[serde(rename = "ID")]
    id: Option<i64>,
    #[serde(rename = "ReleaseTitle")]
    release_title: String,
    #[serde(rename = "RemasterTitle")]
    remaster_title: Option<String>,
    #[serde(rename = "RemasterYear")]
    remaster_year: Option<String>,
    #[serde(rename = "Size")]
    size: Option<i64>,
    #[serde(rename = "Seeders")]
    seeders: Option<i64>,
    #[serde(rename = "Leechers")]
    leechers: Option<i64>,
    #[serde(rename = "Snatched")]
    snatches: Option<i64>,
    #[serde(rename = "FreeTorrent")]
    free_torrent: Option<i64>,
    #[serde(rename = "Time")]
    time: Option<String>,
    #[serde(rename = "Format")]
    format: Option<String>,
    #[serde(rename = "Encoding")]
    encoding: Option<String>,
    #[serde(rename = "Region")]
    region: Option<String>,
    #[serde(rename = "Language")]
    language: Option<String>,
    #[serde(rename = "Scene")]
    scene: Option<String>,
    #[serde(rename = "Miscellaneous")]
    miscellaneous: Option<String>,
    #[serde(rename = "GameDOXType")]
    game_dox_type: Option<String>,
    #[serde(rename = "GameDOXVers")]
    game_dox_vers: Option<String>,
    #[serde(rename = "Dupable")]
    dupable: Option<String>,
}

/// Parses a GazelleGames `api.php` search response (offline-testable).
pub fn parse_ggn_response(
    body: &str,
    base: &GazelleGamesIndexer,
) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
    let parsed: GgnResponse = serde_json::from_str(body)?;
    let groups = parsed
        .response
        .results
        .or(parsed.response.results_lower)
        .unwrap_or_default();
    let mut releases = Vec::new();

    for group in groups {
        for t in group.torrents.clone().unwrap_or_default() {
            let Some(id) = t.id else { continue };

            let mut title = t.release_title.clone();
            if !title.contains(&group.group_year.clone().unwrap_or_default()) {
                if let Some(y) = &group.group_year {
                    if !y.is_empty() && y != "0" {
                        title.push_str(&format!(" ({y})"));
                    }
                }
            }
            let remaster = [t.remaster_title.as_deref(), t.remaster_year.as_deref()]
                .into_iter()
                .flatten()
                .filter(|s| !s.is_empty() && *s != "0")
                .collect::<Vec<_>>()
                .join(" ");
            if !remaster.is_empty() {
                title.push_str(&format!(" [{remaster}]"));
            }
            let mut tags: Vec<String> = group
                .artists
                .as_ref()
                .map(|a| a.iter().map(|x| x.name.clone()).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default();
            for f in [
                t.format.as_deref(),
                t.encoding.as_deref(),
                t.region.as_deref(),
                t.language.as_deref(),
                t.scene.as_deref(),
                t.miscellaneous.as_deref(),
                t.game_dox_type.as_deref(),
                t.game_dox_vers.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                if !f.is_empty() {
                    tags.push(f.to_string());
                }
            }
            if t.dupable.as_deref() == Some("1") {
                tags.push("Trumpable".to_string());
            }
            if !tags.is_empty() {
                title.push_str(&format!(" [{}]", tags.join(" ")));
            }

            let mut rel = ReleaseInfo::new();
            rel.title = Some(title);
            rel.size = t.size;
            rel.seeders = t.seeders;
            rel.peers = t.leechers.map(|l| l + t.seeders.unwrap_or(0));
            rel.grabs = t.snatches;
            let details = Url::parse(&format!(
                "{}/torrents.php?id={}",
                base.site_link,
                group.group_id.unwrap_or(id)
            ))?;
            rel.details = Some(details.clone());
            rel.guid = Some(details);
            let mut link = Url::parse(&format!("{}/torrents.php?action=download&id={id}", base.site_link))?;
            if base.config.use_tokens {
                link.query_pairs_mut().append_pair("usetoken", "1");
            }
            rel.link = Some(link);
            rel.publish_date = t
                .time
                .as_deref()
                .and_then(crate::utils::date_time_util::DateTimeUtil::from_unknown);
            rel.minimum_ratio = Some(1.0);
            rel.minimum_seed_time = Some(288000);
            rel.download_volume_factor = Some(1.0);
            rel.upload_volume_factor = Some(1.0);
            match t.free_torrent {
                Some(1) => rel.download_volume_factor = Some(0.0),
                Some(2) => {
                    rel.download_volume_factor = Some(0.0);
                    rel.upload_volume_factor = Some(0.0);
                }
                _ => {}
            }
            if base.config.freeleech_only && rel.download_volume_factor != Some(0.0) {
                continue;
            }
            // Map artists (platform names) to Torznab categories, else CategoryID.
            let mut cats = Vec::new();
            if let Some(artists) = &group.artists {
                for a in artists {
                    cats.extend(base.caps.categories.map_tracker_cat_to_newznab(&a.name));
                }
            }
            if cats.is_empty() {
                if let Some(cid) = &group.category_id {
                    cats.extend(base.caps.categories.map_tracker_cat_to_newznab(cid));
                }
            }
            rel.category = cats;

            releases.push(rel);
        }
    }
    releases.sort_by(|a, b| b.publish_date.cmp(&a.publish_date));
    Ok(releases)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_all_new_gazelle_sites() {
        assert_eq!(GAZELLE_SITES.len(), 7);
        for id in [
            "brokenstones",
            "desigaane",
            "dicmusic",
            "greatposterwall",
            "orpheus",
            "simurg",
            "secretcinema",
        ] {
            let spec = find_spec(id).unwrap_or_else(|| panic!("missing spec {id}"));
            assert!(!spec.caps().categories.get_tracker_categories().is_empty(), "{id}");
        }
    }

    #[test]
    fn gpw_and_orpheus_build() {
        let gpw = find_spec("greatposterwall")
            .unwrap()
            .build(GazelleConfig {
                api_key: Some("k".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(gpw.id(), "greatposterwall");
        assert!(gpw.torznab_caps().movie_search_params.len() == 3);

        let orpheus = find_spec("orpheus")
            .unwrap()
            .build(GazelleConfig {
                api_key: Some("k".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(orpheus.id(), "orpheus");
        assert_eq!(orpheus.torznab_caps().music_search_params.len(), 6);
    }

    #[test]
    fn ggn_caps_cover_platforms() {
        let idx = build_gazelle_games(GazelleConfig::default()).unwrap();
        assert_eq!(idx.id(), "gazellegamesapi");
        assert!(idx.torznab_caps().categories.get_tracker_categories().len() > 90);
    }
}
