// Mirror of Jackett.Common/Models/TorznabQuery.cs
use once_cell::sync::Lazy;
use regex::Regex;

use crate::models::torznab_cat_type::TorznabCatType;

#[derive(Debug, Clone, Default)]
pub struct TorznabQuery {
    pub interactive_search: bool,
    pub query_type: Option<String>,
    pub categories: Vec<i32>,
    pub extended: i32,
    pub api_key: Option<String>,
    pub limit: i32,
    pub offset: i32,
    pub rage_id: Option<i32>,
    pub tvdb_id: Option<i32>,
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<i32>,
    pub tvmaze_id: Option<i32>,
    pub trakt_id: Option<i32>,
    pub douban_id: Option<i32>,
    pub cache: bool, // default true, set in Default/new
    pub season: Option<i32>,
    pub episode: Option<String>,
    pub search_term: Option<String>,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub label: Option<String>,
    pub track: Option<String>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub author: Option<String>,
    pub title: Option<String>,
    pub publisher: Option<String>,
    pub is_test: bool,

    /// Lazily computed query-string parts (C# `QueryStringParts` cache).
    #[doc(hidden)]
    pub query_string_parts: Option<Vec<String>>,
}

impl TorznabQuery {
    pub fn new() -> Self {
        Self {
            categories: Vec::new(),
            cache: true,
            is_test: false,
            ..Default::default()
        }
    }

    pub fn imdb_id_short(&self) -> Option<String> {
        self.imdb_id
            .as_ref()
            .map(|s| s.trim_start_matches('t').to_string())
    }

    pub fn is_search(&self) -> bool {
        self.query_type.as_deref() == Some("search")
    }

    pub fn is_tv_search(&self) -> bool {
        self.query_type.as_deref() == Some("tvsearch")
    }

    pub fn is_movie_search(&self) -> bool {
        self.query_type.as_deref() == Some("movie")
            || (self.query_type.as_deref() == Some("TorrentPotato")
                && self
                    .search_term
                    .as_ref()
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false))
    }

    pub fn is_music_search(&self) -> bool {
        self.query_type.as_deref() == Some("music")
    }

    pub fn is_book_search(&self) -> bool {
        self.query_type.as_deref() == Some("book")
    }

    pub fn is_tv_rage_query(&self) -> bool {
        self.rage_id.is_some()
    }

    pub fn is_tvdb_query(&self) -> bool {
        self.tvdb_id.is_some()
    }

    pub fn is_imdb_query(&self) -> bool {
        self.imdb_id.is_some()
    }

    pub fn is_tmdb_query(&self) -> bool {
        self.tmdb_id.is_some()
    }

    pub fn is_tvmaze_query(&self) -> bool {
        self.tvmaze_id.is_some()
    }

    pub fn is_trakt_query(&self) -> bool {
        self.trakt_id.is_some()
    }

    pub fn is_douban_query(&self) -> bool {
        self.douban_id.is_some()
    }

    pub fn is_genre_query(&self) -> bool {
        self.genre.is_some()
    }

    fn is_not_blank(s: &Option<String>) -> bool {
        s.as_ref().map(|v| !v.trim().is_empty()).unwrap_or(false)
    }

    pub fn is_rss_search(&self) -> bool {
        !Self::is_not_blank(&self.search_term) && !self.is_id_search()
    }

    pub fn is_id_search(&self) -> bool {
        Self::is_not_blank(&self.episode)
            || self.season.map(|s| s > 0).unwrap_or(false)
            || self.is_imdb_query()
            || self.is_tvdb_query()
            || self.is_tv_rage_query()
            || self.is_trakt_query()
            || self.is_tvmaze_query()
            || self.is_tmdb_query()
            || self.is_douban_query()
            || Self::is_not_blank(&self.album)
            || Self::is_not_blank(&self.artist)
            || Self::is_not_blank(&self.label)
            || Self::is_not_blank(&self.genre)
            || Self::is_not_blank(&self.track)
            || Self::is_not_blank(&self.author)
            || Self::is_not_blank(&self.title)
            || Self::is_not_blank(&self.publisher)
            || self.year.is_some()
    }

    pub fn has_specified_categories(&self) -> bool {
        !self.categories.is_empty()
    }

    pub fn sanitized_search_term(&self) -> String {
        static STANDARDIZE_DASHES: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"\p{Pd}+").unwrap());
        static STANDARDIZE_SINGLE_QUOTES: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"[\u{0060}\u{00B4}\u{2018}\u{2019}]").unwrap());

        let term = self.search_term.as_deref().unwrap_or("");
        let term = STANDARDIZE_DASHES.replace_all(term, "-");
        let term = STANDARDIZE_SINGLE_QUOTES.replace_all(&term, "'");

        term.chars()
            .filter(|c| {
                c.is_alphanumeric()
                    || c.is_whitespace()
                    || matches!(
                        c,
                        '-' | '.' | '_' | '(' | ')' | '@' | '/' | '\'' | '[' | ']' | '+' | '%'
                    )
            })
            .collect()
    }

    pub fn create_fallback(&self, search: &str) -> TorznabQuery {
        let mut ret = self.clone();
        if ret.categories.is_empty() {
            ret.categories = vec![
                TorznabCatType::movies().id,
                TorznabCatType::get_cat_by_name("Movies/Foreign").map(|c| c.id).unwrap_or(2010),
                TorznabCatType::get_cat_by_name("Movies/Other").map(|c| c.id).unwrap_or(2020),
                TorznabCatType::get_cat_by_name("Movies/SD").map(|c| c.id).unwrap_or(2030),
                TorznabCatType::get_cat_by_name("Movies/HD").map(|c| c.id).unwrap_or(2040),
                TorznabCatType::get_cat_by_name("Movies/3D").map(|c| c.id).unwrap_or(2060),
                TorznabCatType::get_cat_by_name("Movies/BluRay").map(|c| c.id).unwrap_or(2050),
                TorznabCatType::get_cat_by_name("Movies/DVD").map(|c| c.id).unwrap_or(2070),
                TorznabCatType::get_cat_by_name("Movies/WEB-DL").map(|c| c.id).unwrap_or(2080),
                TorznabCatType::get_cat_by_name("Movies/UHD").map(|c| c.id).unwrap_or(2045),
            ];
        }
        ret.search_term = Some(search.to_string());
        ret
    }

    pub fn get_query_string(&self) -> String {
        format!(
            "{} {}",
            self.sanitized_search_term(),
            self.get_episode_search_string()
        )
        .trim()
        .to_string()
    }

    /// Some trackers don't support AND logic for search terms resulting in unwanted
    /// results. Using this method we can AND filter it within jackett.
    /// With "limit" we can limit the amount of characters which should be compared
    /// (use it if a tracker doesn't return the full title).
    pub fn match_query_string_and(
        &mut self,
        title: &str,
        limit: Option<usize>,
        query_string_override: Option<&str>,
    ) -> bool {
        const COMMON_WORDS: [&str; 3] = ["and", "the", "an"];

        if self.query_string_parts.is_none() {
            let mut query_string = match query_string_override {
                Some(o) if !o.trim().is_empty() => o.to_string(),
                _ => self.get_query_string(),
            };

            if let Some(limit) = limit.filter(|l| *l > 0) {
                if limit < query_string.len() {
                    query_string.truncate(limit);
                }
            }

            static SPLIT_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"[^\w]+").unwrap());

            self.query_string_parts = Some(
                SPLIT_REGEX
                    .split(&query_string)
                    .filter(|p| !p.trim().is_empty() && p.len() > 1 && !contains_ignore_case(COMMON_WORDS.iter().copied(), p))
                    .map(|p| p.to_string())
                    .collect(),
            );
        }

        self.query_string_parts
            .as_ref()
            .unwrap()
            .iter()
            .all(|part| contains_ignore_case(std::iter::once(title), part))
    }

    pub fn get_episode_search_string(&self) -> String {
        let season = match self.season {
            None | Some(0) => return String::new(),
            Some(s) => s,
        };

        let episode = self.episode.as_deref().unwrap_or("");

        // C# tries DateTime.TryParseExact("{Season} {Episode}", "yyyy MM/dd", ...)
        if let Some(date_str) = try_parse_air_date(season, episode) {
            return date_str;
        }

        if episode.trim().is_empty() {
            format!("S{:02}", season)
        } else {
            match coerce_int(episode) {
                Ok(ep) => format!("S{:02}E{:02}", season, ep),
                Err(_) => format!("S{:02}E{}", season, episode), // e.g. seaching for S01E01A
            }
        }
    }
}

fn contains_ignore_case<'a>(mut haystacks: impl Iterator<Item = &'a str>, needle: &str) -> bool {
    let needle_lower = needle.to_lowercase();
    haystacks.any(|h| h.to_lowercase().contains(&needle_lower))
}

fn try_parse_air_date(season: i32, episode: &str) -> Option<String> {
    use chrono::NaiveDate;
    // "yyyy MM/dd" format: season is the year, episode is "MM/dd"
    let raw = format!("{} {}", season, episode);
    NaiveDate::parse_from_str(&raw, "%Y %m/%d")
        .ok()
        .map(|d| d.format("%Y.%m.%d").to_string())
}

fn coerce_int(s: &str) -> Result<i32, ()> {
    s.trim().parse::<i32>().map_err(|_| ())
}
