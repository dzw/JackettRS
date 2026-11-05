//! Mirror of Jackett.Server/Controllers/TorznabController.cs (the public Torznab
//! API surface). Handles `t=caps`, the various search modes, `t=download` and
//! the virtual "all" aggregate indexer used by qBittorrent's jackett plugin.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use serde_json;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use jackett_common::indexers::i_indexer::IIndexer;
use jackett_common::models::channel_info::ChannelInfo;
use jackett_common::models::release_info::ReleaseInfo;
use jackett_common::models::result_page::ResultPage;
use jackett_common::models::torznab_query::TorznabQuery;

use crate::startup::SharedState;

/// GET /api/v2.0/torznab/{indexer_id}/api
/// GET /torznab/{indexer_id}/api  (legacy C# route)
pub async fn torznab_api(
    State(state): State<SharedState>,
    Path(indexer_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if let Err(resp) = check_api_key(&state, &params).await {
        return resp;
    }

    // Virtual "all" indexer: fan the search out across every configured
    // indexer and merge the results (qBittorrent's jackett plugin searches
    // /indexers/all/results/torznab/api).
    if indexer_id == "all" {
        return aggregate_search(&state, &params).await;
    }

    let manager = state.indexer_manager.read().await;
    let indexer = match manager.get_indexer(&indexer_id) {
        Some(i) => i,
        None => {
            return (
                StatusCode::NOT_FOUND,
                format!("Unknown indexer: {}", indexer_id),
            )
                .into_response()
        }
    };
    drop(manager);

    let t = params
        .get("t")
        .map(|s| s.as_str())
        .unwrap_or("search")
        .to_string();

    match t.as_str() {
        "caps" => {
            let xml = indexer.torznab_caps().to_xml();
            xml_response(StatusCode::OK, &xml)
        }
        "search"
        | "tvsearch"
        | "movie"
        | "music"
        | "book"
        | "TorrentPotato" => {
            let query = build_query(&t, &params);
            return run_search_rss(&state, &indexer_id, &query, &indexer).await;
        }
        other => (
            StatusCode::BAD_REQUEST,
            format!("Unsupported torznab operation: t={}", other),
        )
            .into_response(),
    }
}

/// Runs a search against `indexer` and renders the result as a Torznab RSS
/// document. Shared by the `/torznab/:id/api` (t=search) and
/// `/indexers/:id/results` endpoints.
pub(crate) async fn run_search_rss(
    state: &SharedState,
    indexer_id: &str,
    query: &TorznabQuery,
    indexer: &Arc<dyn IIndexer>,
) -> Response {
    match indexer.query(query).await {
        Ok(releases) => {
            state.cache_service.cache_result(indexer_id, query, &releases);
            let ci = ChannelInfo {
                title: Some(indexer.name().to_string()),
                description: Some(indexer.description().to_string()),
                link: indexer.site_link().parse().ok(),
                language: Some(indexer.language().to_string()),
                category: query.query_type.clone(),
            };
            let page = ResultPage {
                channel_info: ci,
                releases,
            };
            let cfg = state.server_config.read().await;
            let port = cfg.port as u16;
            let bind = cfg.local_bind_address.clone();
            drop(cfg);
            let self_url = format!(
                "http://{}:{}/api/v2.0/torznab/{}/api",
                bind, port, indexer_id
            );
            let xml = page.to_xml(&self_url.parse().unwrap_or_else(|_| {
                url::Url::parse("http://localhost/").unwrap()
            }));
            xml_response(StatusCode::OK, &xml)
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Search failed: {e:#}"),
        )
            .into_response(),
    }
}

/// Mirror of the C# virtual "all" indexer: fans a search out across every
/// configured indexer concurrently, merges the releases (sorted by seeders)
/// and renders one Torznab RSS document. Per-indexer results time out so a
/// slow or hung tracker cannot stall the aggregate response.
async fn aggregate_search(state: &SharedState, params: &HashMap<String, String>) -> Response {
    const PER_INDEXER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

    let t = params
        .get("t")
        .map(|s| s.as_str())
        .unwrap_or("search")
        .to_string();

    if t == "caps" {
        // Minimal capabilities document — clients searching "all" never need
        // the per-tracker category trees.
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<caps><server title=\"Jackett\" /><limits default=\"100\" max=\"100\" />\
<searching><search available=\"yes\" supportedParams=\"q\" /></searching></caps>";
        return xml_response(StatusCode::OK, xml);
    }
    if !matches!(
        t.as_str(),
        "search" | "tvsearch" | "movie" | "music" | "book" | "TorrentPotato"
    ) {
        return (
            StatusCode::BAD_REQUEST,
            format!("Unsupported torznab operation: t={}", t),
        )
            .into_response();
    }

    // Targets under a short read lock, then fan out without holding it.
    let ids: Vec<String> = {
        let manager = state.indexer_manager.read().await;
        manager
            .get_all_indexers()
            .iter()
            .filter(|i| i.is_configured())
            .map(|i| i.id().to_string())
            .collect()
    };

    let mut set = tokio::task::JoinSet::new();
    let base_query = build_query(&t, params);
    for id in ids {
        let st = state.clone();
        let q = base_query.clone();
        set.spawn(async move {
            let fut = async {
                let manager = st.indexer_manager.read().await;
                match manager.get_indexer(&id) {
                    Some(indexer) => indexer.query(&q).await.unwrap_or_default(),
                    None => Vec::new(),
                }
            };
            match tokio::time::timeout(PER_INDEXER_TIMEOUT, fut).await {
                Ok(releases) => releases,
                Err(_) => Vec::new(),
            }
        });
    }

    let mut releases: Vec<ReleaseInfo> = Vec::new();
    while let Some(joined) = set.join_next().await {
        if let Ok(per_indexer) = joined {
            releases.extend(per_indexer);
        }
    }
    releases.sort_by(|a, b| b.seeders.unwrap_or(0).cmp(&a.seeders.unwrap_or(0)));

    let ci = ChannelInfo {
        title: Some("Jackett (all)".to_string()),
        description: Some("Aggregate search across all configured indexers".to_string()),
        link: "http://localhost:9117".parse().ok(),
        language: None,
        category: Some(t),
    };
    let page = ResultPage {
        channel_info: ci,
        releases,
    };
    let cfg = state.server_config.read().await;
    let port = cfg.port as u16;
    let bind = cfg.local_bind_address.clone();
    drop(cfg);
    let self_url = format!("http://{}:{}/api/v2.0/torznab/all/api", bind, port);
    let xml = page.to_xml(&self_url.parse().unwrap_or_else(|_| {
        url::Url::parse("http://localhost/").unwrap()
    }));
    xml_response(StatusCode::OK, &xml)
}

/// GET /api/v2.0/torznab/{indexer_id}/download
/// GET /torznab/{indexer_id}/download  (legacy C# route)
pub async fn torznab_download(
    State(state): State<SharedState>,
    Path(indexer_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if let Err(resp) = check_api_key(&state, &params).await {
        return resp;
    }

    let link = params
        .get("link")
        .or_else(|| params.get("path"))
        .or_else(|| params.get("file"))
        .cloned();
    let link = match link {
        Some(l) => l,
        None => return (StatusCode::BAD_REQUEST, "Missing link parameter").into_response(),
    };

    // Magnet URIs are not parseable by the url crate — deliver them directly
    // (to the blackhole folder or the client) without touching the indexer.
    if link.starts_with("magnet:") {
        return deliver_payload(&state, link.into_bytes()).await;
    }

    let url = match url::Url::parse(&link) {
        Ok(u) => u,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Invalid link: {e}"),
            )
                .into_response()
        }
    };

    let manager = state.indexer_manager.read().await;
    let indexer = match manager.get_indexer(&indexer_id) {
        Some(i) => i,
        None => {
            return (
                StatusCode::NOT_FOUND,
                format!("Unknown indexer: {}", indexer_id),
            )
                .into_response()
        }
    };
    drop(manager);

    match indexer.download(&url).await {
        Ok(bytes) => deliver_payload(&state, bytes).await,
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Download failed: {e:#}"),
        )
            .into_response(),
    }
}

/// Delivers a download payload: into the blackhole watched folder when
/// configured (.torrent binary / .magnet text for qBittorrent 4.5+), otherwise
/// back to the client as an attachment.
async fn deliver_payload(state: &SharedState, payload: Vec<u8>) -> Response {
    // Blackhole mode (mirror of C#): when configured, save the payload into
    // the watched folder and answer briefly instead of streaming it.
    let blackhole = {
        let cfg = state.server_config.read().await;
        cfg.blackhole_dir
            .clone()
            .filter(|d| !d.trim().is_empty())
    };
    if let Some(dir) = blackhole {
        let (ext, payload) = if payload.first() == Some(&b'd') {
            (".torrent", payload)
        } else {
            (".magnet", payload)
        };
        let fname = format!("jackett_{}{}", chrono::Utc::now().timestamp_millis(), ext);
        let path = std::path::Path::new(&dir).join(fname);
        return match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, payload)) {
            Ok(_) => (StatusCode::OK, format!("saved to {}", path.display())).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Blackhole write failed: {e}"),
            )
                .into_response(),
        };
    }
    (
        [
            (header::CONTENT_TYPE, "application/x-bittorrent".to_string()),
            (header::CONTENT_DISPOSITION, "attachment".to_string()),
        ],
        payload,
    )
        .into_response()
}

/// Mirrors the C# API-key gate: when a server key is configured, the client
/// must supply it via the `apikey` query parameter (or `X-Api-Key` header).
pub(crate) async fn check_api_key(
    state: &SharedState,
    params: &HashMap<String, String>,
) -> Result<(), Response> {
    let cfg = state.server_config.read().await;
    let key = match &cfg.api_key {
        Some(k) if !k.is_empty() => k.clone(),
        _ => return Ok(()),
    };
    drop(cfg);

    let provided = params.get("apikey").cloned().unwrap_or_default();
    if provided == key {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, "Invalid API key").into_response())
    }
}

/// Builds a `TorznabQuery` from the standard Torznab request parameters
/// (mirror of `TorznabController.CreateQuery`).
pub(crate) fn build_query(t: &str, params: &HashMap<String, String>) -> TorznabQuery {
    let mut q = TorznabQuery::new();
    q.query_type = Some(t.to_string());
    q.api_key = params.get("apikey").cloned();
    q.search_term = params
        .get("q")
        .cloned()
        .filter(|s| !s.trim().is_empty());
    if let Some(cat) = params.get("cat") {
        q.categories = cat
            .split(',')
            .filter_map(|c| c.trim().parse::<i32>().ok())
            .collect();
    }
    if let Some(limit) = params.get("limit").and_then(|s| s.parse::<i32>().ok()) {
        q.limit = limit;
    }
    if let Some(offset) = params.get("offset").and_then(|s| s.parse::<i32>().ok()) {
        q.offset = offset;
    }
    if let Some(ext) = params.get("extended").and_then(|s| s.parse::<i32>().ok()) {
        q.extended = ext;
    }
    q.imdb_id = params
        .get("imdbid")
        .or_else(|| params.get("imdb"))
        .cloned()
        .filter(|s| !s.trim().is_empty());
    if let Some(s) = params.get("season").and_then(|s| s.parse::<i32>().ok()) {
        q.season = Some(s);
    }
    if let Some(e) = params.get("ep") {
        q.episode = Some(e.clone());
    }
    if let Some(id) = params.get("tvdbid").and_then(|s| s.parse::<i32>().ok()) {
        q.tvdb_id = Some(id);
    }
    if let Some(id) = params.get("tvmazeid").and_then(|s| s.parse::<i32>().ok()) {
        q.tvmaze_id = Some(id);
    }
    if let Some(id) = params.get("traktid").and_then(|s| s.parse::<i32>().ok()) {
        q.trakt_id = Some(id);
    }
    if let Some(id) = params.get("tmdbid").and_then(|s| s.parse::<i32>().ok()) {
        q.tmdb_id = Some(id);
    }
    if let Some(id) = params.get("rid").and_then(|s| s.parse::<i32>().ok()) {
        q.rage_id = Some(id);
    }
    if let Some(y) = params.get("year").and_then(|s| s.parse::<i32>().ok()) {
        q.year = Some(y);
    }
    q.genre = params.get("genre").cloned().filter(|s| !s.trim().is_empty());
    q.artist = params.get("artist").cloned().filter(|s| !s.trim().is_empty());
    q.album = params.get("album").cloned().filter(|s| !s.trim().is_empty());
    q.author = params.get("author").cloned().filter(|s| !s.trim().is_empty());
    q.title = params.get("title").cloned().filter(|s| !s.trim().is_empty());
    q.publisher = params.get("pub").cloned().filter(|s| !s.trim().is_empty());
    q
}

fn xml_response(status: StatusCode, body: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/rss+xml".to_string())],
        body.to_string(),
    )
        .into_response()
}

/// Renders any `Serialize` payload as a `application/json` response.
pub(crate) fn json_response<T: serde::Serialize>(body: &T) -> Response {
    match serde_json::to_string(body) {
        Ok(s) => (
            [
                (header::CONTENT_TYPE, "application/json".to_string()),
            ],
            s,
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("JSON error: {e}"),
        )
            .into_response(),
    }
}
