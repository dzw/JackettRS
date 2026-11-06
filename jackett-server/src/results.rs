//! Mirror of Jackett.Server/Controllers/ResultsController.cs — the per-indexer
//! results endpoints: the Torznab search alias (`/indexers/:id/results`),
//! TorrentPotato (`/results/potato`) and ManualSearch (`/results/manualsearch`).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use jackett_common::models::config::proxy_type::ProxyType;
use jackett_common::models::dto::manual_search_result::{
    ManualSearchResult, ManualSearchResultIndexer, ManualSearchResultIndexerStatus,
};
use jackett_common::models::dto::torrent_potato_request::TorrentPotatoRequest;
use jackett_common::models::dto::torrent_potato_response::TorrentPotatoResponse;
use jackett_common::models::dto::torrent_potato_response_item::TorrentPotatoResponseItem;
use jackett_common::models::tracker_cache_result::TrackerCacheResult;

use crate::startup::SharedState;
use crate::torznab::{aggregate_releases, build_query, check_api_key, json_response, run_search_rss};

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

/// GET /api/v2.0/indexers/all/results/manualsearch — aggregate variant over
/// every `is_configured()` indexer (public trackers with no login count as
/// configured, so they are included even with an empty Configured Indexer
/// list). NOTE: the desktop shell does NOT use this endpoint — it fans out
/// over `/desktop/stream`, which searches all public indexers directly and is
/// independent of the configured-indexer concept. This route is only for the
/// standard Jackett admin UI.
pub async fn manualsearch_all(
    State(state): State<SharedState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if let Err(r) = check_api_key(&state, &params).await {
        return r;
    }
    let mut query = build_query("search", &params);
    query.interactive_search = true;
    let start = Instant::now();

    let per = aggregate_releases(&state, query.clone()).await;
    let mut results: Vec<TrackerCacheResult> = Vec::new();
    let mut indexer_results: Vec<ManualSearchResultIndexer> = Vec::new();

    for a in per {
        if !a.releases.is_empty() {
            state.cache_service.cache_result(&a.id, &query, &a.releases);
        }
        results.extend(a.releases.iter().map(|release| TrackerCacheResult {
            release: release.clone(),
            first_seen: None,
            tracker: Some(a.name.clone()),
            tracker_id: Some(a.id.clone()),
            tracker_type: None,
            category_desc: None,
            blackhole_link: None,
        }));
        indexer_results.push(ManualSearchResultIndexer {
            id: Some(a.id.clone()),
            name: Some(a.name.clone()),
            status: ManualSearchResultIndexerStatus::OK,
            results: a.releases.len() as i32,
            error: None,
            elapsed_time: 0,
        });
    }

    tracing::info!(
        "aggregate manualsearch: {} indexers, {} results in {}ms",
        indexer_results.len(),
        results.len(),
        start.elapsed().as_millis()
    );
    json_response(&ManualSearchResult {
        results,
        indexers: indexer_results,
    })
}

// ---------------------------------------------------------------------------
// Desktop pure-search API
//
// The desktop shell is a P2PSearcher-style client: it has no configuration UI
// and does not care about the "configured indexers" concept at all. Every
// request here is scoped to the `public` indexers loaded from Definitions/
// (84 at the current baseline), so a fresh install can search immediately.
// Results are deliberately compact — the desktop table only needs a handful of
// columns — and one request covers one indexer so the UI can stream rows in as
// each site answers instead of waiting for the slowest one.
// ---------------------------------------------------------------------------

/// Hands a magnet/torrent URL or a local `.torrent` path to the OS shell.
fn os_open(target: &str) -> std::io::Result<()> {
    if target.contains('"') || target.contains('\n') || target.contains('\r') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid open target",
        ));
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        // Build the cmd line verbatim: Rust's normal argument quoting would
        // leave `&` in a magnet link unquoted, and cmd reads that as a command
        // separator. `start "" "<target>"` keeps the empty title slot and
        // passes the whole link to the registered torrent client.
        // CREATE_NO_WINDOW avoids a console flash on every download.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("cmd")
            .raw_arg(format!("/C start \"\" \"{target}\""))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
    }
    #[cfg(not(target_os = "windows"))]
    {
        use std::process::Command;
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        Command::new(opener).arg(target).spawn().map(|_| ())
    }
}

fn os_open_response(target: &str) -> Response {
    match os_open(target) {
        Ok(_) => (StatusCode::OK, "opened").into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("open failed: {e}"),
        )
            .into_response(),
    }
}

/// GET /desktop/open?url=magnet:...|https:... — scheme-whitelisted so the
/// endpoint cannot be abused to launch arbitrary handlers.
pub async fn desktop_open(Query(params): Query<HashMap<String, String>>) -> Response {
    let url = params.get("url").cloned().unwrap_or_default();
    let ok = matches!(
        url.split(':').next().unwrap_or(""),
        "magnet" | "http" | "https"
    ) && !url.trim().is_empty();
    if !ok {
        return (StatusCode::BAD_REQUEST, "unsupported url scheme").into_response();
    }
    os_open_response(&url)
}

// ---------------------------------------------------------------------------
// Desktop settings
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct DesktopProxyBody {
    #[serde(default)]
    enabled: bool,
    #[serde(default, rename = "type")]
    proxy_type: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    port: Option<i32>,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
}

fn desktop_proxy_type_name(value: i32) -> &'static str {
    match value {
        v if v == ProxyType::Socks4 as i32 => "socks4",
        v if v == ProxyType::Socks5 as i32 => "socks5",
        _ => "http",
    }
}

/// GET /desktop/settings — current proxy settings for the settings dialog.
pub async fn desktop_settings_get(State(state): State<SharedState>) -> Response {
    let config = state.server_config.read().await;
    let enabled = config.proxy_type != ProxyType::Disabled as i32
        && config
            .proxy_url
            .as_deref()
            .map(|u| !u.trim().is_empty())
            .unwrap_or(false);
    json_response(&serde_json::json!({
        "enabled": enabled,
        "type": desktop_proxy_type_name(config.proxy_type),
        "url": config.proxy_url.clone().unwrap_or_default(),
        "port": config.proxy_port,
        "username": config.proxy_username.clone().unwrap_or_default(),
        "password": config.proxy_password.clone().unwrap_or_default(),
        "proxy_url": config.get_proxy_url(false),
    }))
}

/// POST /desktop/settings — persist the proxy and rebuild every indexer so
/// their reqwest clients pick up the new setting without restarting the app.
pub async fn desktop_settings_save(
    State(state): State<SharedState>,
    Json(body): Json<DesktopProxyBody>,
) -> Response {
    let url = body.url.trim().to_string();
    let enabled = body.enabled && !url.is_empty();
    let raw_host = {
        let raw = match url.find("://") {
            Some(i) => &url[i + 3..],
            None => &url,
        };
        raw.split('/').next().unwrap_or(raw).trim().to_string()
    };
    if enabled && raw_host.is_empty() {
        return (StatusCode::BAD_REQUEST, "invalid proxy address").into_response();
    }
    // Accept a "host:port" pasted into the address field without duplicating
    // the port when get_proxy_url() reassembles the URL.
    let (host, inline_port) = match raw_host.rfind(':') {
        Some(i) if raw_host[i + 1..].chars().all(|c| c.is_ascii_digit()) => (
            raw_host[..i].to_string(),
            raw_host[i + 1..].parse::<i32>().ok(),
        ),
        _ => (raw_host, None),
    };
    let port = inline_port
        .or(body.port)
        .filter(|p| (1..=65535).contains(p));

    {
        let mut config = state.server_config.write().await;
        if !enabled {
            config.proxy_type = ProxyType::Disabled as i32;
            config.proxy_url = None;
            config.proxy_port = None;
            config.proxy_username = None;
            config.proxy_password = None;
        } else {
            config.proxy_type = match body.proxy_type.to_ascii_lowercase().as_str() {
                "socks4" => ProxyType::Socks4 as i32,
                "socks5" => ProxyType::Socks5 as i32,
                _ => ProxyType::Http as i32,
            };
            config.proxy_url = Some(host);
            config.proxy_port = port;
            config.proxy_username = (!body.username.trim().is_empty())
                .then(|| body.username.trim().to_string());
            config.proxy_password =
                (!body.password.trim().is_empty()).then(|| body.password.clone());
        }
        if let Err(e) = state.configuration_service.save(&config) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("save failed: {e}"),
            )
                .into_response();
        }
        crate::initialisation::apply_proxy_env(&config);
    }

    let defs = state.definitions_dir.clone();
    let cfg = state.indexer_config_dir.clone();
    let manager = match tokio::task::spawn_blocking(move || {
        crate::initialisation::build_indexer_manager(&defs, &cfg)
    })
    .await
    {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("reload failed: {e}"),
            )
                .into_response();
        }
    };
    let count = manager.indexer_count();
    *state.indexer_manager.write().await = manager;
    json_response(&serde_json::json!({ "ok": true, "indexers": count }))
}

/// One row of the desktop results table. Only the fields the UI renders are
/// serialised; `link` is kept so the download endpoint can resolve a plain
/// `.torrent` URL through its indexer when no magnet is available.
#[derive(serde::Serialize)]
struct DesktopRow {
    tracker: String,
    tracker_id: String,
    title: String,
    size: Option<i64>,
    seeders: Option<i64>,
    peers: Option<i64>,
    /// Snatch / completion count (private trackers). `None` for most public
    /// indexers, rendered as "—" by the client.
    grabs: Option<i64>,
    /// Download volume factor: `0` = freeleech, `1` = normal, `2` = double.
    download_volume_factor: Option<f64>,
    /// Upload volume factor: `0` = neutral (no upload required), `1` = normal.
    upload_volume_factor: Option<f64>,
    category: Vec<i32>,
    magnet: Option<String>,
    link: Option<String>,
    details: Option<String>,
    publish_date: Option<String>,
}

#[derive(serde::Serialize)]
struct DesktopIndexer {
    id: String,
    name: String,
    site: String,
}

fn is_public(indexer: &dyn jackett_common::indexers::i_indexer::IIndexer) -> bool {
    indexer.type_().eq_ignore_ascii_case("public")
}

fn desktop_rows(
    releases: Vec<jackett_common::models::release_info::ReleaseInfo>,
    tracker: &str,
    tracker_id: &str,
) -> Vec<DesktopRow> {
    releases
        .into_iter()
        .filter(|r| r.title.is_some() || r.magnet_uri.is_some() || r.link.is_some())
        .map(|r| DesktopRow {
            tracker: tracker.to_string(),
            tracker_id: tracker_id.to_string(),
            title: r.title.unwrap_or_default(),
            size: r.size,
            seeders: r.seeders,
            peers: r.peers,
            grabs: r.grabs,
            download_volume_factor: r.download_volume_factor,
            upload_volume_factor: r.upload_volume_factor,
            category: r.category,
            magnet: r.magnet_uri.map(|u| u.to_string()),
            link: r.link.map(|u| u.to_string()),
            details: r.details.map(|u| u.to_string()),
            publish_date: r.publish_date.map(|d| d.to_rfc3339()),
        })
        .collect()
}

/// GET /desktop/indexers — public indexers, no configuration required.
pub async fn desktop_indexers(State(state): State<SharedState>) -> Response {
    let mut list: Vec<DesktopIndexer> = {
        let manager = state.indexer_manager.read().await;
        manager
            .get_all_indexers()
            .iter()
            .filter(|i| is_public(i.as_ref()))
            .map(|i| DesktopIndexer {
                id: i.id().to_string(),
                name: i.name().to_string(),
                site: i.site_link().to_string(),
            })
            .collect()
    };
    list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    json_response(&serde_json::json!({ "total": list.len(), "indexers": list }))
}

/// GET /desktop/search?id=<indexer>&q=<term>&cat=<ids> — search one public
/// indexer with a bounded timeout. The desktop UI runs these concurrently so
/// fast sites show up immediately and a dead site cannot stall the list.
pub async fn desktop_search(
    State(state): State<SharedState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let id = params.get("id").cloned().unwrap_or_default();
    let indexer = {
        let manager = state.indexer_manager.read().await;
        manager.get_indexer(&id)
    };
    let Some(indexer) = indexer else {
        return (StatusCode::NOT_FOUND, "unknown indexer").into_response();
    };
    if !is_public(indexer.as_ref()) {
        return (StatusCode::BAD_REQUEST, "not a public indexer").into_response();
    }

    let mut query = build_query("search", &params);
    query.interactive_search = true;
    let start = Instant::now();
    let outcome = tokio::time::timeout(Duration::from_secs(20), indexer.query(&query)).await;
    let elapsed = start.elapsed().as_millis() as i64;

    let (rows, error) = match outcome {
        Ok(Ok(releases)) => (desktop_rows(releases, indexer.name(), &id), None),
        Ok(Err(e)) => (Vec::new(), Some(format!("{e:#}"))),
        Err(_) => (Vec::new(), Some("timeout".to_string())),
    };

    json_response(&serde_json::json!({
        "id": id,
        "name": indexer.name(),
        "rows": rows,
        "error": error,
        "elapsed": elapsed,
    }))
}

/// GET /desktop/stream?q=<term>&cat=<ids> — one SSE connection fans the query
/// out to every public indexer concurrently on the server. A browser would cap
/// ~89 per-indexer fetches at six HTTP/1.1 connections, so streaming over a
/// single connection is both faster and lets rows appear as each site answers.
///
/// Events: `start` {total}, `rows` {id,name,rows,error} x N, `done` {}.
pub async fn desktop_stream(
    State(state): State<SharedState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use futures_util::stream::unfold;
    use std::convert::Infallible;

    let indexers: Vec<(String, String)> = {
        let manager = state.indexer_manager.read().await;
        manager
            .get_all_indexers()
            .iter()
            .filter(|i| is_public(i.as_ref()))
            .map(|i| (i.id().to_string(), i.name().to_string()))
            .collect()
    };
    let total = indexers.len();
    let mut query = build_query("search", &params);
    query.interactive_search = true;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Event>();
    let _ = tx.send(
        Event::default()
            .event("start")
            .data(serde_json::json!({ "total": total }).to_string()),
    );

    tokio::spawn(async move {
        let mut set = tokio::task::JoinSet::new();
        for (id, name) in indexers {
            let st = state.clone();
            let q = query.clone();
            let tx = tx.clone();
            set.spawn(async move {
                let indexer = {
                    let manager = st.indexer_manager.read().await;
                    manager.get_indexer(&id)
                };
                let Some(indexer) = indexer else { return };
                let outcome = tokio::time::timeout(Duration::from_secs(20), indexer.query(&q)).await;
                let (rows, error) = match outcome {
                    Ok(Ok(releases)) => (desktop_rows(releases, &name, &id), None),
                    Ok(Err(e)) => (Vec::new(), Some(format!("{e:#}"))),
                    Err(_) => (Vec::new(), Some("timeout".to_string())),
                };
                let payload = serde_json::json!({
                    "id": id, "name": name, "rows": rows, "error": error,
                });
                let _ = tx.send(Event::default().event("rows").data(payload.to_string()));
            });
        }
        while set.join_next().await.is_some() {}
        let _ = tx.send(Event::default().event("done").data("{}"));
    });

    let stream = unfold(rx, |mut rx| async move {
        rx.recv().await.map(|ev| (Ok::<Event, Infallible>(ev), rx))
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// GET /desktop/download?id=<indexer>&url=<magnet|http link> — resolve the
/// link (a plain torrent URL needs the indexer's own download flow) and hand
/// the result to the torrent client registered in the OS.
pub async fn desktop_download(
    State(state): State<SharedState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let id = params.get("id").cloned().unwrap_or_default();
    let link = params.get("url").cloned().unwrap_or_default();
    if link.starts_with("magnet:") {
        return os_open_response(&link);
    }
    let url = match url::Url::parse(&link) {
        Ok(u) if matches!(u.scheme(), "http" | "https") => u,
        _ => return (StatusCode::BAD_REQUEST, "unsupported link").into_response(),
    };
    let indexer = {
        let manager = state.indexer_manager.read().await;
        manager.get_indexer(&id)
    };
    let Some(indexer) = indexer else {
        return (StatusCode::NOT_FOUND, "unknown indexer").into_response();
    };
    if !is_public(indexer.as_ref()) {
        return (StatusCode::BAD_REQUEST, "not a public indexer").into_response();
    }

    let bytes = match tokio::time::timeout(Duration::from_secs(20), indexer.download(&url)).await {
        Ok(Ok(b)) => b,
        Ok(Err(e)) => {
            tracing::warn!("desktop download {id}: {e:#}; opening link directly");
            return os_open_response(&link);
        }
        Err(_) => return os_open_response(&link),
    };

    // Some definitions resolve a detail page into a magnet link rather than a
    // .torrent payload.
    if bytes.starts_with(b"magnet:") {
        return os_open_response(String::from_utf8_lossy(&bytes).trim());
    }
    if bytes.first() != Some(&b'd') {
        // Not a bencoded torrent (usually an HTML error page) — let the browser
        // show the original page instead of writing a bogus .torrent file.
        return os_open_response(&link);
    }

    let dir = std::env::temp_dir().join("jackett-desktop");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("temp dir failed: {e}"),
        )
            .into_response();
    }
    let path = dir.join(format!(
        "jackett_{}.torrent",
        chrono::Utc::now().timestamp_millis()
    ));
    if let Err(e) = std::fs::write(&path, &bytes) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("write failed: {e}"),
        )
            .into_response();
    }
    os_open_response(&path.to_string_lossy())
}
