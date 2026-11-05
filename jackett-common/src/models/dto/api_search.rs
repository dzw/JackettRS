use once_cell::sync::Lazy;
use regex::Regex;

use crate::models::torznab_query::TorznabQuery;
use crate::utils::parse_util::ParseUtil;

/// Mirror of Jackett.Common/Models/DTO/ApiSearch.cs
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct ApiSearch {
    pub query: Option<String>,
    #[serde(default)]
    pub category: Vec<i32>,
    #[serde(default)]
    pub tracker: Vec<String>,
    pub imdb_id: Option<String>,
    #[serde(rename = "DoubanID")]
    pub douban_id: Option<i32>,
    #[serde(rename = "RageID")]
    pub rage_id: Option<i32>,
    #[serde(rename = "TvdbId")]
    pub tvdb_id: Option<i32>,
    #[serde(rename = "TmdbId")]
    pub tmdb_id: Option<i32>,
    #[serde(rename = "TvMazeId")]
    pub tv_maze_id: Option<i32>,
    #[serde(rename = "TraktID")]
    pub trakt_id: Option<i32>,
}

impl ApiSearch {
    pub fn to_torznab_query(request: &ApiSearch) -> TorznabQuery {
        static SEASON_EPISODE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"\bS(\d{2,4})E(\d{2,4}[A-Za-z]?)$").unwrap());
        static EPISODE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bE(\d{2,4}[A-Za-z]?)$").unwrap());
        static SEASON: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bS(\d{2,4})$").unwrap());

        let mut string_query = TorznabQuery {
            query_type: Some("search".to_string()),
            ..Default::default()
        };

        let mut query_str = request.query.clone().unwrap_or_default().trim().to_string();

        if !query_str.trim().is_empty() {
            if let Some(m) = SEASON_EPISODE.captures(&query_str) {
                string_query.season = m.get(1).unwrap().as_str().parse::<i32>().ok();
                string_query.episode = Some(
                    m.get(2)
                        .unwrap()
                        .as_str()
                        .trim_start_matches('0')
                        .to_string(),
                );
                let full = m.get(0).unwrap();
                query_str = format!(
                    "{}{}",
                    &query_str[..full.start()],
                    &query_str[full.end()..]
                )
                .trim()
                .to_string();
            } else {
                if let Some(m) = EPISODE.captures(&query_str) {
                    string_query.episode = Some(
                        m.get(1)
                            .unwrap()
                            .as_str()
                            .trim_start_matches('0')
                            .to_string(),
                    );
                    let full = m.get(0).unwrap();
                    query_str = format!(
                        "{}{}",
                        &query_str[..full.start()],
                        &query_str[full.end()..]
                    )
                    .trim()
                    .to_string();
                }

                if let Some(m) = SEASON.captures(&query_str) {
                    string_query.season = m.get(1).unwrap().as_str().parse::<i32>().ok();
                    let full = m.get(0).unwrap();
                    query_str = format!(
                        "{}{}",
                        &query_str[..full.start()],
                        &query_str[full.end()..]
                    )
                    .trim()
                    .to_string();
                }
            }

            query_str = query_str.trim().to_string();
        }

        string_query.search_term = Some(query_str);
        string_query.categories = request.category.clone();
        string_query.douban_id = request.douban_id;
        string_query.rage_id = request.rage_id;
        string_query.trakt_id = request.trakt_id;
        string_query.tmdb_id = request.tmdb_id;
        string_query.tvdb_id = request.tvdb_id;
        string_query.imdb_id = request.imdb_id.clone();
        string_query.tvmaze_id = request.tv_maze_id;

        // try to build an IMDB Query (tt plus 6 to 8 digits)
        let sanitized = string_query.sanitized_search_term();
        if sanitized.starts_with("tt") && sanitized.len() <= 10 {
            if let Some(imdb_id) = ParseUtil::get_full_imdb_id(&sanitized) {
                return TorznabQuery {
                    query_type: Some("search".to_string()),
                    imdb_id: Some(imdb_id),
                    categories: string_query.categories.clone(),
                    season: string_query.season,
                    episode: string_query.episode.clone(),
                    ..Default::default()
                };
            }
        }

        string_query
    }
}
