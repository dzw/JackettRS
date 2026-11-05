use axum::extract::{OriginalUri, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use jackett_common::models::config::server_config::ServerConfig;
use jackett_common::models::config::runtime_settings::RuntimeSettings;
use jackett_common::services::cache_service::CacheService;
use jackett_common::services::configuration_service::ConfigurationService;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::Path;
use jackett_common::indexers::bespoke::registry;
use jackett_common::models::torznab_query::TorznabQuery;
use jackett_common::services::indexer_manager_service::IndexerManagerService;
use jackett_common::services::indexer_manager_service::IndexerUserConfig;
use jackett_common::services::log_cache_service::LogCacheService;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json;
use tokio::sync::RwLock;
use tower_http::services::ServeDir;

/// Mirror of Jackett.Server/Services/ServerService.cs — the shared state
/// the ASP.NET Core DI container used to inject into controllers.
pub struct ServerState {
    pub server_config: RwLock<ServerConfig>,
    pub runtime_settings: RuntimeSettings,
    pub configuration_service: ConfigurationService,
    pub indexer_manager: RwLock<IndexerManagerService>,
    pub cache_service: CacheService,
    pub log_cache_service: LogCacheService,
    /// Folder holding per-indexer user configs (`<data>/Indexers`). Resolved once
    /// at startup from `-d`/`--data-folder` (via `RuntimeSettings::data_folder`)
    /// or the `JACKETT_DATA` override, then shared so every handler writes to the
    /// same place the manager loaded from.
    pub indexer_config_dir: PathBuf,
}

pub type SharedState = Arc<ServerState>;

/// Resolves the folder holding the web UI static assets (mirror of C#'s content
/// folder). Explicit env `JACKETT_WWWROOT` wins, then `<exe_dir>/wwwroot`, then
/// a relative `wwwroot`.
fn resolve_wwwroot() -> PathBuf {
    if let Ok(d) = std::env::var("JACKETT_WWWROOT") {
        return PathBuf::from(d);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.join("wwwroot");
        }
    }
    PathBuf::from("wwwroot")
}

/// Minimal self-contained web UI compiled into the binary. The Rust port has
/// no Angular SPA yet, so without this `ServeDir` would 404 on `/` whenever
/// no external `wwwroot` is present.
pub const BUILTIN_INDEX_HTML: &str = include_str!("../wwwroot/index.html");

/// SPA-style fallback: any non-API path renders the built-in UI, while unknown
/// API/Torznab paths keep returning a real 404 for clients such as Sonarr.
async fn builtin_ui(OriginalUri(uri): OriginalUri) -> Response {
    let path = uri.path();
    if path.starts_with("/api/") || path.starts_with("/torznab/") || path == "/health" {
        return StatusCode::NOT_FOUND.into_response();
    }
    Html(BUILTIN_INDEX_HTML).into_response()
}

/// Mirror of Jackett.Server/Startup.cs — builds the router ("pipeline").
pub fn build_router(state: SharedState) -> Router {
    let api = Router::new()
        // Mirror of AdminController endpoints (subset wired; more controllers ported incrementally)
        .route("/api/v2.0/server/config", get(get_server_config))
        .route("/api/v2.0/server/config", post(update_server_config))
        .route("/api/v2.0/server/logs", get(get_logs))
        .route("/api/v2.0/indexers", get(get_indexers))
        .route("/api/v2.0/cache", get(get_cache))
        // Torznab API (Cardigann) — both the v2.0 path and the legacy C# path
        .route("/api/v2.0/torznab/:indexer_id/api", get(crate::torznab::torznab_api))
        .route(
            "/api/v2.0/torznab/:indexer_id/download",
            get(crate::torznab::torznab_download),
        )
        .route("/torznab/:indexer_id/api", get(crate::torznab::torznab_api))
        .route(
            "/torznab/:indexer_id/download",
            get(crate::torznab::torznab_download),
        )
        // C# standard per-indexer feed paths (ResultsController.cs): the URLs
        // the dashboard "Copy Torznab/RSS/Potato Feed" buttons hand out.
        .route(
            "/api/v2.0/indexers/:indexer_id/results/torznab/api",
            get(crate::torznab::torznab_api),
        )
        .route(
            "/api/v2.0/indexers/:indexer_id/results/torznab/download",
            get(crate::torznab::torznab_download),
        )
        .route(
            "/api/v2.0/indexers/:indexer_id/results/torznab/",
            get(crate::torznab::torznab_api),
        )
        .route(
            "/api/v2.0/indexers/:indexer_id/results/potato/",
            get(crate::results::potato),
        )
        // Results controller (mirror of ResultsController.cs)
        .route(
            "/api/v2.0/indexers/:indexer_id/results",
            get(crate::results::results_search),
        )
        .route(
            "/api/v2.0/indexers/:indexer_id/results/potato",
            get(crate::results::potato),
        )
        .route(
            "/api/v2.0/indexers/:indexer_id/results/manualsearch",
            get(crate::results::manualsearch),
        )
        // Mirror of `IndexerApiController.Test` (the #jackett-test-all target).
        .route(
            "/api/v2.0/indexers/:indexer_id/Test",
            post(test_indexer_endpoint),
        )
        .route("/health", get(|| async { "OK" }))
    .route("/api/v2.0/indexers/available", get(get_available_indexers))
    .route("/api/v2.0/indexers", post(post_indexer))
    .route("/api/v2.0/indexers/:indexer_id", delete(delete_indexer))
    .route("/api/v2.0/indexers/:indexer_id/config", get(get_indexer_config));

    // An external `wwwroot` (JACKETT_WWWROOT) wins; otherwise serve the
    // compiled-in UI so `/` and SPA deep links never 404.
    let wwwroot = resolve_wwwroot();
    if wwwroot.join("index.html").is_file() {
        api.fallback_service(
            ServeDir::new(wwwroot).append_index_html_on_directories(true),
        )
        .with_state(state)
    } else {
        api.fallback(builtin_ui).with_state(state)
    }
}

/// GET /api/v2.0/server/config — mirror of AdminController.GetServerConfig.
async fn get_server_config(State(state): State<SharedState>) -> impl IntoResponse {
    let config = state.server_config.read().await;
    let dto = jackett_common::models::dto::server_config::ServerConfig::new(
        Vec::new(),
        &config,
        jackett_common::utils::server_util::ServerUtil::get_application_version().as_str(),
        true,
    );
    (StatusCode::OK, Json(dto))
}

/// POST /api/v2.0/server/config — mirror of AdminController.UpdateServerConfig.
/// POST /api/v2.0/server/config — mirror of `UpdateServerConfig`: applies the
/// posted settings onto the core config and persists it. Absent keys keep
/// their current value; `password` is only touched when present (empty string
/// disables the admin password, matching C#'s "Blank to disable").
async fn update_server_config(
    State(state): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let mut config = state.server_config.write().await;
    let mut touched = false;

    // Returns whether the key was present and applied.
    let set_str = |field: &mut Option<String>, key: &str| -> bool {
        match body.get(key) {
            Some(v) if v.is_null() => {
                *field = None;
                true
            }
            Some(v) => match v.as_str() {
                Some(s) if s.is_empty() => {
                    *field = None;
                    true
                }
                Some(s) => {
                    *field = Some(s.to_string());
                    true
                }
                None => false,
            },
            None => false,
        }
    };

    if let Some(v) = body.get("port").and_then(|v| v.as_i64()) {
        config.port = v as i32;
        touched = true;
    }
    if let Some(v) = body.get("api_key").and_then(|v| v.as_str()) {
        if !v.is_empty() {
            config.api_key = Some(v.to_string());
            touched = true;
        }
    }
    if let Some(v) = body.get("password") {
        // C# semantics: blank disables, non-empty sets. Absent key = no change.
        if let Some(s) = v.as_str() {
            config.admin_password = if s.is_empty() { None } else { Some(s.to_string()) };
            touched = true;
        }
    }
    touched |= set_str(&mut config.blackhole_dir, "blackholedir");
    touched |= set_str(&mut config.base_path_override, "basepathoverride");
    touched |= set_str(&mut config.base_url_override, "baseurloverride");
    if let Some(v) = body.get("external").and_then(|v| v.as_bool()) {
        config.allow_external = v;
        touched = true;
    }
    if let Some(v) = body.get("local_bind_address").and_then(|v| v.as_str()) {
        if !v.is_empty() {
            config.local_bind_address = v.to_string();
            touched = true;
        }
    }
    if let Some(v) = body.get("cors").and_then(|v| v.as_bool()) {
        config.allow_cors = v;
        touched = true;
    }
    if let Some(v) = body.get("updatedisabled").and_then(|v| v.as_bool()) {
        config.update_disabled = v;
        touched = true;
    }
    if let Some(v) = body.get("prerelease").and_then(|v| v.as_bool()) {
        config.update_prerelease = v;
        touched = true;
    }
    if let Some(v) = body.get("logging").and_then(|v| v.as_bool()) {
        config.runtime_settings.tracing_enabled = v;
        touched = true;
    }
    if let Some(v) = body.get("cache_enabled").and_then(|v| v.as_bool()) {
        config.cache_enabled = v;
        touched = true;
    }
    if let Some(v) = body.get("cache_ttl").and_then(|v| v.as_i64()) {
        config.cache_ttl = v;
        touched = true;
    }
    if let Some(v) = body.get("cache_max_results_per_indexer").and_then(|v| v.as_i64()) {
        config.cache_max_results_per_indexer = v;
        touched = true;
    }
    touched |= set_str(&mut config.flare_solverr_url, "flaresolverrurl");
    if let Some(v) = body.get("flaresolverr_maxtimeout").and_then(|v| v.as_i64()) {
        config.flare_solverr_max_timeout = v as i32;
        touched = true;
    }
    touched |= set_str(&mut config.omdb_api_key, "omdbkey");
    touched |= set_str(&mut config.omdb_api_url, "omdburl");
    if let Some(v) = body.get("proxy_type").and_then(|v| v.as_i64()) {
        config.proxy_type = v as i32;
        touched = true;
    }
    touched |= set_str(&mut config.proxy_url, "proxy_url");
    if let Some(v) = body.get("proxy_port").and_then(|v| v.as_i64()) {
        config.proxy_port = Some(v as i32);
        touched = true;
    }
    touched |= set_str(&mut config.proxy_username, "proxy_username");
    touched |= set_str(&mut config.proxy_password, "proxy_password");

    if !touched {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "no recognized settings in body" })));
    }
    if let Err(e) = state.configuration_service.save(&config) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() })));
    }
    (StatusCode::OK, Json(serde_json::json!({ "result": "success" })))
}

/// GET /api/v2.0/server/logs — mirror of AdminController.GetLogs.
async fn get_logs(State(state): State<SharedState>) -> impl IntoResponse {
    let logs = state.log_cache_service.logs();
    (StatusCode::OK, Json(serde_json::json!(logs)))
}

/// GET /api/v2.0/indexers — mirror of IndexerApiController.GetIndexers.
/// Returns only the indexers the user has configured (added via the UI /
/// persisted `*.json` config), not every Cardigann definition loaded at
/// startup. This matches C#'s "Configured Indexers" dashboard tab.
async fn get_indexers(State(state): State<SharedState>) -> impl IntoResponse {
    let manager = state.indexer_manager.read().await;
    let dtos: Vec<jackett_common::models::dto::indexer::Indexer> = manager
        .configured_indexers()
        .iter()
        .map(|i| jackett_common::models::dto::indexer::Indexer::from_indexer(i.as_ref()))
        .collect();
    (StatusCode::OK, Json(dtos))
}

/// GET /api/v2.0/cache — mirror of `CacheController.GetCachedResults`.
async fn get_cache(State(state): State<SharedState>) -> impl IntoResponse {
    Json(state.cache_service.get_all())
}

/// Directory holding per-indexer user configs, mirroring C#'s
/// `RuntimeSettings.DataFolder/Indexers`. Resolution order:
///   1. `JACKETT_DATA` env override (back-compat)
///   2. the resolved data folder (`-d`/`--data-folder`, else the C# default
///      `C:\ProgramData\Jackett` on Windows / `$XDG_CONFIG_HOME/Jackett` on Unix)
///
/// By default this is the SAME folder a co-installed C# Jackett uses
/// (`C:\ProgramData\Jackett\Indexers`), so the two share indexer configs. The
/// config loader tolerates both the Rust-native `{"id","fields"}` format and C#'s
/// array-of-settings format, so existing C# configs load without parse errors.
pub fn resolve_indexer_config_dir(rt: &RuntimeSettings) -> PathBuf {
    if let Ok(d) = std::env::var("JACKETT_DATA") {
        return PathBuf::from(d).join("Indexers");
    }
    rt.data_folder().join("Indexers")
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

/// GET /api/v2.0/indexers/available — mirror of `IndexerApiController.AddIndexer`
/// (the definition-discovery half). Served from the manager's definition cache
/// with the configured flag resolved from persisted user configs.
async fn get_available_indexers(State(state): State<SharedState>) -> impl IntoResponse {
    Json(state.indexer_manager.read().await.available())
}

/// POST /api/v2.0/indexers — add (or reconfigure) an indexer from a field map,
/// persist it, and register it with the manager. Hand-coded indexers resolve
/// through the bespoke registry; Cardigann definitions rebuild from their YAML
/// with the posted settings.
async fn post_indexer(
    State(state): State<SharedState>,
    Json(req): Json<IndexerUserConfig>,
) -> impl IntoResponse {
    let bespoke = registry::build_indexer(&req.id, &req.fields);
    let mut manager = state.indexer_manager.write().await;
    if bespoke.is_none() && !manager.has_definition(&req.id) {
        return (StatusCode::BAD_REQUEST, "{\"error\":\"unknown or invalid indexer\"}").into_response();
    }
    let dir = state.indexer_config_dir.clone();
    let path = dir.join(format!("{}.json", sanitize_id(&req.id)));
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| {
        std::fs::write(&path, serde_json::to_string_pretty(&req).unwrap_or_default())
    }) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{{\"error\":\"persist failed: {e}\"}}"),
        )
            .into_response();
    }
    match bespoke {
        Some(idx) => manager.add_indexer(idx),
        None => {
            if !manager.rebuild_cardigann(&req.id, &req.fields) {
                let _ = std::fs::remove_file(&path);
                return (
                    StatusCode::BAD_REQUEST,
                    "{\"error\":\"invalid indexer definition\"}",
                )
                    .into_response();
            }
        }
    }
    manager.mark_configured(&req.id);
    (StatusCode::OK, "{\"ok\":true}").into_response()
}

/// DELETE /api/v2.0/indexers/:indexer_id — unregister and drop its config file.
async fn delete_indexer(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    state.indexer_manager.write().await.remove_indexer(&id);
    let path = state.indexer_config_dir.join(format!("{}.json", sanitize_id(&id)));
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    state.indexer_manager.write().await.unmark_configured(&id);
    (StatusCode::OK, "{\"ok\":true}").into_response()
}

/// GET /api/v2.0/indexers/:indexer_id/config — mirror of
/// `IndexerApiController.Config`: returns the editable settings schema, current
/// values and capabilities used to render the C#-style edit modal.
async fn get_indexer_config(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let dir = state.indexer_config_dir.clone();
    match state.indexer_manager.read().await.indexer_config(&id, &dir) {
        Some(cfg) => (StatusCode::OK, Json(cfg)).into_response(),
        None => (StatusCode::NOT_FOUND, "{\"error\":\"unknown indexer\"}").into_response(),
    }
}

/// POST /api/v2.0/indexers/:indexer_id/Test — mirror of
/// `IndexerApiController.Test`. Runs a *real* test search (empty `SearchTerm`,
/// `is_test: true`) exactly like C#'s `TestIndexer`: a successful search with
/// >=1 result returns `status: OK`, while zero results (or a query failure)
/// surfaces as an error — C# throws and the dashboard shows `LastError`.
///
/// The indexer `Arc` is cloned under a brief read lock and the lock released
/// before the (potentially slow) network query, so the test never blocks
/// add/delete/config writes like the old whole-manager read-lock did.
async fn test_indexer_endpoint(
    State(state): State<SharedState>,
    Path(indexer_id): Path<String>,
) -> Response {
    let (indexer, name) = {
        let mgr = state.indexer_manager.read().await;
        match mgr.get_indexer(&indexer_id) {
            Some(i) => {
                let name = i.name().to_string();
                (i, name)
            }
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": format!("Unknown indexer: {}", indexer_id) })),
                )
                    .into_response()
            }
        }
    };
    let query = TorznabQuery {
        query_type: Some("search".to_string()),
        search_term: Some(String::new()),
        is_test: true,
        ..Default::default()
    };
    match indexer.query(&query).await {
        Ok(results) if !results.is_empty() => (
            StatusCode::OK,
            Json(json!({ "status": "OK", "results": results.len() })),
        )
            .into_response(),
        Ok(_) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": format!(
                    "Test search in {} => Found no results while trying to browse this tracker. \
                     This may be an issue with the indexer, or other indexer settings such as search freeleech only etc.",
                    name
                )
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": format!("Test search in {} failed: {:#}", name, e) })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::Service;

    fn req(uri: &str) -> Request<Body> {
        Request::builder().uri(uri).body(Body::empty()).unwrap()
    }

    fn test_state() -> SharedState {
        let runtime = RuntimeSettings::default();
        let configuration_service = ConfigurationService::new(runtime.clone());
        Arc::new(ServerState {
            server_config: RwLock::new(ServerConfig::new(runtime.clone())),
            runtime_settings: runtime,
            configuration_service,
            indexer_manager: RwLock::new(IndexerManagerService::new()),
            cache_service: CacheService::new(),
            log_cache_service: LogCacheService::new(10),
        })
    }

    #[test]
    fn builtin_ui_is_self_contained() {
        assert!(BUILTIN_INDEX_HTML.contains("<html"));
        assert!(BUILTIN_INDEX_HTML.contains("/api/v2.0/indexers"));
        assert!(BUILTIN_INDEX_HTML.contains("results/manualsearch"));
        // Add Indexer / Manual Search / Cached Releases each open as a modal popup;
        // results and cache tables use C#-style sortable columns.
        assert!(BUILTIN_INDEX_HTML.contains("add-modal"));
        assert!(BUILTIN_INDEX_HTML.contains("search-modal"));
        assert!(BUILTIN_INDEX_HTML.contains("cache-modal"));
        assert!(BUILTIN_INDEX_HTML.contains("add-list"));
        assert!(BUILTIN_INDEX_HTML.contains("add-row"));
        assert!(BUILTIN_INDEX_HTML.contains("add-do"));
        assert!(BUILTIN_INDEX_HTML.contains("results-table"));
        assert!(BUILTIN_INDEX_HTML.contains("cache-table"));
        assert!(BUILTIN_INDEX_HTML.contains("data-key=\"published\""));
        assert!(!BUILTIN_INDEX_HTML.contains("<script src="));
    }

    #[tokio::test]
    async fn root_serves_ui_and_unknown_api_stays_404() {
        let mut app = build_router(test_state());

        let res = app.call(req("/")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let content_type = res
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(content_type.starts_with("text/html"));
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("Jackett"));

        let res = app.call(req("/UI/Dashboard")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let res = app.call(req("/api/v2.0/does-not-exist")).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}
