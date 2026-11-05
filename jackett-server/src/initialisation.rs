use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use crate::startup::{build_router, ServerState};
use jackett_common::models::config::runtime_settings::RuntimeSettings;
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

/// Mirror of Jackett.Server/Initialisation.cs + Program.cs:
/// load config, build services, start the Kestrel-equivalent (axum) listener.
pub async fn run_server(
    runtime_settings: RuntimeSettings,
    server_options: RuntimeServerConfig,
) -> Result<()> {
    jackett_common::utils::logging_setup::LoggingSetup::setup(runtime_settings.tracing_enabled);

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

    let port = server_config.port as u16;
    let bind_address = server_config.local_bind_address.clone();

    let mut indexer_manager = IndexerManagerService::new();
    let definitions_dir = resolve_definitions_dir(&server_options);
    let cfg_dir = resolve_indexer_config_dir(&runtime_settings);
    match indexer_manager.init(&definitions_dir, &cfg_dir) {
        Ok(n) => tracing::info!("Loaded {n} indexers from {:?}", definitions_dir),
        Err(e) => tracing::error!("Failed to load indexers: {e:#}"),
    }

    // Restore user-added indexers persisted via the UI/API (mirror of C#'s
    // per-indexer config restore on startup). Hand-coded indexers rebuild
    // through the bespoke registry; Cardigann definitions rebuild from their
    // YAML with the persisted settings.
    if let Ok(entries) = std::fs::read_dir(&cfg_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                match parse_indexer_config(&path, &text) {
                    Some((id, fields)) => {
                        if let Some(idx) = registry::build_indexer(&id, &fields) {
                            indexer_manager.add_indexer(idx);
                        } else if indexer_manager.rebuild_cardigann(&id, &fields) {
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

    let state = Arc::new(ServerState {
        server_config: RwLock::new(server_config),
        runtime_settings,
        configuration_service,
        indexer_manager: RwLock::new(indexer_manager),
        cache_service: CacheService::new(),
        log_cache_service: LogCacheService::new(1000),
        indexer_config_dir: cfg_dir,
    });

    let app = build_router(state);
    let addr = format!("{}:{}", bind_address, port);
    tracing::info!("Jackett (Rust) listening on http://{}", addr);

    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
