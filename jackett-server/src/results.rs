//! Mirror of Jackett.Server/Controllers/ResultsController.cs — the per-indexer
//! results endpoints: the Torznab search alias (`/indexers/:id/results`),
//! TorrentPotato (`/results/potato`) and ManualSearch (`/results/manualsearch`).

use std::collections::HashMap;
use std::time::Instant;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use jackett_common::models::dto::manual_search_result::{
    ManualSearchResult, ManualSearchResultIndexer, ManualSearchResultIndexerStatus,
};
use jackett_common::models::dto::torrent_potato_request::TorrentPotatoRequest;
use jackett_common::models::dto::torrent_potato_response::TorrentPotatoResponse;
use jackett_common::models::dto::torrent_potato_response_item::TorrentPotatoResponseItem;
use jackett_common::models::tracker_cache_result::TrackerCacheResult;

use crate::startup::SharedState;
use crate::torznab::{build_query, check_api_key, json_response, run_search_rss};

/// GET /api/v2.0/indexers/{indexer_id}/results
/// Mirror of `ResultsController.Results` — same as a Torznab `t=search` but on
/// the dedicated results route.
pub async fn results_search(
    State(state): State<SharedState>,
    Path(indexer_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if let Err(r) = check_api_key(&state, &params).await {
        return r;
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
    let query = build_query("search", &params);
    run_search_rss(&state, &indexer_id, &query, &indexer).await
}

/// GET /api/v2.0/indexers/{indexer_id}/results/potato
/// Mirror of `ResultsController.Potato`.
pub async fn potato(
    State(state): State<SharedState>,
    Path(indexer_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if let Err(r) = check_api_key(&state, &params).await {
        return r;
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

    let request = TorrentPotatoRequest {
        username: params.get("username").cloned(),
        imdb_id: params
            .get("imdbid")
            .or_else(|| params.get("imdb"))
            .cloned(),
        search: params.get("search").cloned().filter(|s| !s.trim().is_empty()),
    };
    let query = TorrentPotatoRequest::to_torznab_query(&request);

    match indexer.query(&query).await {
        Ok(releases) => {
            let items = releases
                .iter()
                .filter(|r| r.link.is_some() || r.magnet_uri.is_some())
                .map(|r| TorrentPotatoResponseItem {
                    release_name: r
                        .title
                        .clone()
                        .map(|t| format!("{}[{}]", t, indexer.name())),
                    torrent_id: r.guid.as_ref().map(|u| u.to_string()),
                    details_url: r.details.as_ref().map(|u| u.to_string()),
                    download_url: r
                        .link
                        .as_ref()
                        .or(r.magnet_uri.as_ref())
                        .map(|u| u.to_string()),
                    imdb_id: r.imdb.map(|v| format!("tt{:07}", v)),
                    freeleech: r.download_volume_factor == Some(0.0),
                    type_: Some("movie".to_string()),
                    size: r.size.map(|s| s / (1024 * 1024)).unwrap_or(0),
                    leechers: (r.peers.unwrap_or(-1) - r.seeders.unwrap_or(0)).max(-1),
                    seeders: r.seeders.unwrap_or(-1),
                    publish_date: r.publish_date.map(|d| d.to_rfc3339()),
                })
                .collect();
            json_response(&TorrentPotatoResponse { results: items })
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Potato search failed: {e:#}"),
        )
            .into_response(),
    }
}

/// GET /api/v2.0/indexers/{indexer_id}/results/manualsearch
/// Mirror of `ResultsController.ManualSearch` (single-indexer flavour). Network
/// failures are captured per-indexer rather than aborting the whole response.
pub async fn manualsearch(
    State(state): State<SharedState>,
    Path(indexer_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if let Err(r) = check_api_key(&state, &params).await {
        return r;
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

    let mut query = build_query("search", &params);
    query.interactive_search = true;
    let start = Instant::now();

    let (status, results, error): (ManualSearchResultIndexerStatus, Vec<TrackerCacheResult>, Option<String>) =
        match indexer.query(&query).await {
            Ok(releases) => {
                state.cache_service.cache_result(&indexer_id, &query, &releases);
                let results = releases
                    .into_iter()
                    .map(|release| TrackerCacheResult {
                        release,
                        first_seen: None,
                        tracker: Some(indexer.name().to_string()),
                        tracker_id: Some(indexer.id().to_string()),
                        tracker_type: Some(indexer.type_().to_string()),
                        category_desc: None,
                        blackhole_link: None,
                    })
                    .collect();
                (ManualSearchResultIndexerStatus::OK, results, None)
            }
            Err(e) => (
                ManualSearchResultIndexerStatus::Error,
                Vec::new(),
                Some(format!("{e:#}")),
            ),
        };

    let elapsed = start.elapsed().as_millis() as i64;
    let indexer_result = ManualSearchResultIndexer {
        id: Some(indexer.id().to_string()),
        name: Some(indexer.name().to_string()),
        status,
        results: results.len() as i32,
        error,
        elapsed_time: elapsed,
    };

    json_response(&ManualSearchResult {
        results,
        indexers: vec![indexer_result],
    })
}
