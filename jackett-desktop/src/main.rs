#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Desktop shell: embeds the jackett-server on an ephemeral localhost port and
//! shows its Web UI in a native window. The port is bound before the window is
//! created so the WebView can navigate straight to the real URL.

use axum::extract::Path as AxumPath;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use jackett_server::{build_app_state, RuntimeServerConfig, RuntimeSettings};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// The desktop UI is a standalone P2PSearcher-style page (no Add Indexer /
/// Configured Indexers, no settings) compiled into the shell itself. Serving it
/// over the embedded HTTP server keeps the page pure HTML/JS — no Tauri IPC.
async fn desktop_ui() -> Html<&'static str> {
    Html(include_str!("../wwwroot/index.html"))
}

/// QR / contact images embedded into the binary at compile time, so the desktop
/// build needs no external files (works in both dev and packaged installs).
/// The `file` names must match the entries in index.html's `QR_CODES` array.
const QR_IMAGES: &[(&str, &[u8])] = &[
    ("qr_weibo.png", include_bytes!("../assets/qr_weibo.png")),
    ("qr_app.png", include_bytes!("../assets/qr_app.png")),
    ("qr_xianyu.png", include_bytes!("../assets/qr_xianyu.png")),
];

/// Serves the embedded QR images under `/desktop/assets/<file>`.
async fn desktop_asset(AxumPath(rel): AxumPath<String>) -> Response {
    for (name, bytes) in QR_IMAGES {
        if *name == rel {
            return ([(header::CONTENT_TYPE, "image/png")], bytes.to_vec()).into_response();
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

fn main() {
    // Own tokio runtime drives the embedded server; tauri is told to use it
    // for async tasks as well so everything lives on one loop.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let (port, tokio_handle) = rt.block_on(start_embedded_server());
    // Keep the runtime alive for the whole process lifetime.
    std::mem::forget(rt);
    tauri::async_runtime::set(tokio_handle);

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }
        }))
        .setup(move |app| {
            let url: tauri::Url = format!("http://127.0.0.1:{port}/desktop")
                .parse()
                .expect("server url");
            let win = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("JackettRS — 种子搜索")
                .inner_size(1000.0, 680.0)
                .min_inner_size(760.0, 520.0)
                .build()?;
            win.show()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Builds the app state (loads config, initialises the indexer manager against
/// the app-data folder and the bundled Definitions) and serves it on
/// 127.0.0.1:0. Returns the bound port once the listener is live.
async fn start_embedded_server() -> (u16, tokio::runtime::Handle) {
    let data_dir = default_data_dir();
    std::fs::create_dir_all(&data_dir).expect("create data dir");

    let definitions_dir = bundled_definitions_dir();

    let runtime_settings = RuntimeSettings {
        custom_data_folder: Some(data_dir.display().to_string()),
        tracing_enabled: false,
        ..RuntimeSettings::default()
    };
    let server_options = RuntimeServerConfig {
        definitions_dir: Some(definitions_dir),
        port_override: Some(0),
        bind_address_override: Some("127.0.0.1".into()),
    };

    let (state, app) = build_app_state(runtime_settings, &server_options)
        .await
        .expect("build app state");
    // Explicit routes take priority over the router's SPA fallback, so this
    // works both from a dev checkout and from a packaged install where no
    // external wwwroot folder exists.
    let app = app
        .route("/desktop", get(desktop_ui))
        .route("/desktop/assets/:file", get(desktop_asset));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();

    {
        let mut cfg = state.server_config.write().await;
        cfg.port = port as i32;
    }

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("embedded server error: {e}");
        }
    });

    let tokio_handle = tokio::runtime::Handle::current();
    (port, tokio_handle)
}

/// App-data directory for the desktop shell (Data lives here, matching the
/// `-d <dir>` layout of headless deployments).
fn default_data_dir() -> std::path::PathBuf {
    let base = std::env::var("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| dirs_like_home());
    base.join("jackett-desktop")
        .join("Data")
}

fn dirs_like_home() -> std::path::PathBuf {
    std::env::var("USERPROFILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// In a packaged build the Definitions folder ships as a Tauri resource next
/// to the executable; in dev mode fall back to the repository copy.
fn bundled_definitions_dir() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // NSIS layout: <install>\Definitions\...
            let candidate = dir.join("Definitions");
            if candidate.join("nyaasi.yml").is_file() {
                return candidate;
            }
            let resource = dir.join("resources").join("Definitions");
            if resource.join("nyaasi.yml").is_file() {
                return resource;
            }
        }
    }
    // Dev checkout fallback.
    let mut p = std::env::current_dir().unwrap_or_default();
    loop {
        if p.join("Definitions").join("nyaasi.yml").is_file() {
            return p.join("Definitions");
        }
        if !p.pop() {
            break;
        }
    }
    std::path::PathBuf::from("Definitions")
}
