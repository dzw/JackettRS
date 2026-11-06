use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::startup::{build_router, ServerState, SharedState};
use jackett_common::models::config::runtime_settings::RuntimeSettings;
use jackett_common::models::config::server_config::ServerConfig;
use jackett_common::services::cache_service::CacheService;
use jackett_common::services::configuration_service::ConfigurationService;
use crate::startup::resolve_indexer_config_dir;
use jackett_common::indexers::bespoke::registry;
use jackett_common::services::indexer_manager_service::IndexerManagerService;
use jackett_common::services::indexer_manager_service::parse_indexer_config;
use jackett_common::services::log_cache_service::LogCacheService;
use tokio::net::TcpListener;
use tokio::sync::RwLock;

/// Options controlling `run_server` (mirror of what C# Initialisation threaded
/// through Main/Startup/ServerService).
#[derive(Debug, Clone, Default)]
pub struct RuntimeServerConfig {
    /// Overrides ServerConfig.Port when set (from --Port).
    pub port_override: Option<u16>,
    /// Overrides ServerConfig.LocalBindAddress when set (from --ListenPrivate).
    pub bind_address_override: Option<String>,
    /// Folder holding the Cardigann `*.yml` definitions. Resolved automatically
    /// when `None` (env `JACKETT_DEFINITIONS_DIR`, then `<exe_dir>/Definitions`).
    pub definitions_dir: Option<PathBuf>,
}

/// Resolves the Definitions directory the same way the C# server located its
/// config/data folders: explicit option first, then `JACKETT_DEFINITIONS_DIR`,
/// then `<current_exe_dir>/Definitions`.
fn resolve_definitions_dir(opts: &RuntimeServerConfig) -> PathBuf {
    if let Some(d) = &opts.definitions_dir {
        return d.clone();
    }
    if let Ok(d) = std::env::var("JACKETT_DEFINITIONS_DIR") {
        return PathBuf::from(d);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.join("Definitions");
        }
    }
    PathBuf::from("Definitions")
}

/// Proxy keys reqwest reads when constructing a client.
const PROXY_ENV_KEYS: [&str; 6] = [
    "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY",
    "http_proxy", "https_proxy", "all_proxy",
];

/// Remembers the environment as it was before JackettRS touched it, so a
/// headless/service install that relies on a system-wide HTTP_PROXY keeps
/// working when no proxy is configured in serverconfig.json.
static ORIGINAL_PROXY_ENV: std::sync::OnceLock<Vec<(&'static str, Option<String>)>> =
    std::sync::OnceLock::new();

/// Applies the persisted proxy to the process environment. reqwest's default
/// client builder reads HTTP_PROXY / ALL_PROXY at construction time, so this
/// must run before any indexer (and its HTTP client) is created.
pub fn apply_proxy_env(config: &ServerConfig) {
    let original = ORIGINAL_PROXY_ENV.get_or_init(|| {
        PROXY_ENV_KEYS
            .iter()
            .map(|key| (*key, std::env::var(key).ok()))
            .collect()
    });
    match config.get_proxy_url(true) {
        Some(url) => {
            for key in PROXY_ENV_KEYS {
                std::env::set_var(key, &url);
            }
            if let Some(public_url) = config.get_proxy_url(false) {
                tracing::info!("proxy enabled: {public_url}");
            }
        }
        None => {
            // Restore rather than clear: disabling our proxy must not silently
            // remove a proxy configured by the OS / container environment.
            for (key, value) in original {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

/// Loads all Cardigann definitions and restores user-configured indexers.
/// Shared by startup and the desktop settings endpoint: changing the proxy
/// requires rebuilding every indexer so their reqwest clients pick it up.
pub fn build_indexer_manager(
    definitions_dir: &Path,
    config_dir: &Path,
) -> IndexerManagerService {
    let mut manager = IndexerManagerService::new();
    match manager.init(definitions_dir, config_dir) {
        Ok(n) => tracing::info!("Loaded {n} indexers from {:?}", definitions_dir),
        Err(e) => tracing::error!("Failed to load indexers: {e:#}"),
    }

    // Hand-coded indexers rebuild through the bespoke registry; Cardigann
    // definitions rebuild from their YAML with the persisted settings.
    if let Ok(entries) = std::fs::read_dir(config_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                match parse_indexer_config(&path, &text) {
                    Some((id, fields)) => {
                        if let Some(idx) = registry::build_indexer(&id, &fields) {
                            manager.add_indexer(idx);
                        } else if manager.rebuild_cardigann(&id, &fields) {
                            // rebuilt from its Cardigann definition
                        } else {
                            tracing::warn!("skipping persisted indexer {}: build failed", id);
                        }
                    }
                    None => tracing::warn!("skip {}: unrecognized config format", path.display()),
                }
            }
        }
    }
    manager
}

/// Mirror of Jackett.Server/Initialisation.cs + Program.cs:
/// load config, build services, start the Kestrel-equivalent (axum) listener.
pub async fn run_server(
    runtime_settings: RuntimeSettings,
    server_options: RuntimeServerConfig,
) -> Result<()> {
    jackett_common::utils::logging_setup::LoggingSetup::setup(runtime_settings.tracing_enabled);

    let (state, app) = build_app_state(runtime_settings, &server_options).await?;

    let cfg = state.server_config.read().await;
    let addr = format!("{}:{}", cfg.local_bind_address, cfg.port);
    drop(cfg);
    tracing::info!("Jackett (Rust) listening on http://{}", addr);

    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Loads configuration, initialises all services and builds the shared
/// application state plus router. Shared by `run_server` (binds the configured
/// port) and the Tauri desktop shell (binds an ephemeral port so the WebView
/// can navigate to the actual local URL).
pub async fn build_app_state(
    runtime_settings: RuntimeSettings,
    server_options: &RuntimeServerConfig,
) -> Result<(SharedState, axum::Router)> {
    let configuration_service = ConfigurationService::new(runtime_settings.clone());
    let mut server_config = configuration_service.load()?;
    server_config.on_deserialized();
    server_config.runtime_settings = runtime_settings.clone();

    if let Some(port) = server_options.port_override {
        server_config.port = port as i32;
    }
    if let Some(bind) = server_options.bind_address_override.clone() {
        server_config.local_bind_address = bind;
    }

    // reqwest reads the proxy environment when a Client is *constructed*, so
    // the persisted proxy must be applied before any indexer is built.
    apply_proxy_env(&server_config);

    let definitions_dir = resolve_definitions_dir(server_options);
    let cfg_dir = resolve_indexer_config_dir(&runtime_settings);
    let indexer_manager = build_indexer_manager(&definitions_dir, &cfg_dir);

    let state: SharedState = Arc::new(ServerState {
        server_config: RwLock::new(server_config),
        runtime_settings,
        configuration_service,
        indexer_manager: RwLock::new(indexer_manager),
        cache_service: CacheService::new(),
        log_cache_service: LogCacheService::new(1000),
        indexer_config_dir: cfg_dir.clone(),
        definitions_dir,
    });

    let app = build_router(state.clone());
    Ok((state, app))
}
