use crate::models::torznab_cat_type::TorznabCatType;
use crate::models::torznab_query::TorznabQuery;

/// Mirror of Jackett.Common/Models/DTO/TorznabRequest.cs
#[derive(Debug, Clone, Default)]
pub struct TorznabRequest {
    pub q: Option<String>,
    pub imdb_id: Option<String>,
    pub ep: Option<String>,
    pub t: Option<String>,
    pub extended: Option<String>,
    pub limit: Option<String>,
    pub offset: Option<String>,
    pub cache: Option<String>,
    pub cat: Option<String>,
    pub season: Option<String>,
    pub rid: Option<String>,
    pub tvdb_id: Option<String>,
    pub tmdb_id: Option<String>,
    pub tvmaze_id: Option<String>,
    pub trakt_id: Option<String>,
    pub douban_id: Option<String>,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub label: Option<String>,
    pub track: Option<String>,
    pub year: Option<String>,
    pub genre: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub configured: Option<String>,
}

fn not_blank(s: &Option<String>) -> bool {
    s.as_ref().map(|v| !v.trim().is_empty()).unwrap_or(false)
}

impl TorznabRequest {
    pub fn to_torznab_query(request: &TorznabRequest) -> Result<TorznabQuery, String> {
        let mut query = TorznabQuery {
            query_type: Some("search".to_string()),
            search_term: request.q.clone(),
            imdb_id: request.imdb_id.clone(),
            episode: request.ep.clone(),
            ..Default::default()
        };
        if let Some(t) = &request.t {
            query.query_type = Some(t.clone());
        }
        if not_blank(&request.extended) {
            query.extended = coerce(request.extended.as_deref().unwrap())?;
        }
        if not_blank(&request.limit) {
            query.limit = coerce(request.limit.as_deref().unwrap())?;
        }
        if not_blank(&request.offset) {
            query.offset = coerce(request.offset.as_deref().unwrap())?;
        }

        if let Some(cache) = &request.cache {
            if let Ok(parsed) = cache.parse::<bool>() {
                query.cache = parsed;
            }
        }

        if let Some(cat) = &request.cat {
            query.categories = cat
                .split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim().parse::<i32>().map_err(|e| e.to_string()))
                .collect::<Result<Vec<_>, _>>()?;
        } else if query.query_type.as_deref() == Some("movie") && not_blank(&request.imdb_id) {
            query.categories = vec![TorznabCatType::movies().id];
        } else {
            query.categories = Vec::new();
        }

        if not_blank(&request.season) {
            query.season = request.season.as_deref().unwrap().parse::<i32>().ok();
        }
        if not_blank(&request.rid) {
            query.rage_id = request.rid.as_deref().unwrap().parse::<i32>().ok();
        }
        if not_blank(&request.tvdb_id) {
            query.tvdb_id = request.tvdb_id.as_deref().unwrap().parse::<i32>().ok();
        }
        if not_blank(&request.tvmaze_id) {
            query.tvmaze_id = request.tvmaze_id.as_deref().unwrap().parse::<i32>().ok();
        }

        if not_blank(&request.tmdb_id) {
            query.tmdb_id = request.tmdb_id.as_deref().unwrap().parse::<i32>().ok();
        }
        if not_blank(&request.trakt_id) {
            query.trakt_id = request.trakt_id.as_deref().unwrap().parse::<i32>().ok();
        }
        if not_blank(&request.douban_id) {
            query.douban_id = request.douban_id.as_deref().unwrap().parse::<i32>().ok();
        }

        if not_blank(&request.album) {
            query.album = request.album.clone();
        }
        if not_blank(&request.artist) {
            query.artist = request.artist.clone();
        }
        if not_blank(&request.label) {
            query.label = request.label.clone();
        }
        if not_blank(&request.track) {
            query.track = request.track.clone();
        }
        if not_blank(&request.year) {
            query.year = request.year.as_deref().unwrap().parse::<i32>().ok();
        }
        if not_blank(&request.genre) {
            query.genre = request.genre.clone();
        }

        if not_blank(&request.title) {
            query.title = request.title.clone();
        }
        if not_blank(&request.author) {
            query.author = request.author.clone();
        }
        if not_blank(&request.publisher) {
            // NOTE: mirrors the C# source, which assigns `Author` here (upstream quirk)
            query.author = request.publisher.clone();
        }

        Ok(query)
    }
}

fn coerce(s: &str) -> Result<i32, String> {
    s.trim()
        .parse::<i32>()
        .map_err(|_| format!("Cannot coerce \"{}\" to int", s))
}
