//! Mirror of `Jackett.Tray`.
//!
//! Provides the tray UI (Windows), the context-menu actions (Open Web UI,
//! Start/Stop background service, Auto-start on boot, Shutdown, Close tray
//! icon), single-instance protection, and the `--UpdatedVersion` handshake
//! used by the updater. Non-Windows builds keep the same actions reachable
//! from the command line and run a headless status loop instead.

#[cfg(not(windows))]
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};
use tracing_subscriber::EnvFilter;

use jackett_common::models::config::runtime_settings::RuntimeSettings;
use jackett_common::services::configuration_service::ConfigurationService;

const DEFAULT_PORT: u16 = 9117;
const DEFAULT_HOST: &str = "127.0.0.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Action {
    #[default]
    Run,
    Open,
    StartService,
    StopService,
    AutostartOn,
    AutostartOff,
    Status,
    Shutdown,
    Help,
}

#[derive(Debug, Clone)]
struct Options {
    action: Action,
    updated_version: Option<String>,
    port: Option<u16>,
    host: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            action: Action::Run,
            updated_version: None,
            port: None,
            host: None,
        }
    }
}

impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut opts = Options::default();
        let mut it = args.into_iter().skip(1).peekable();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--UpdatedVersion" | "--updated-version" => {
                    opts.updated_version = Some(it.next().context("--UpdatedVersion needs a value")?);
                }
                "--open" => opts.action = Action::Open,
                "--start-service" => opts.action = Action::StartService,
                "--stop-service" => opts.action = Action::StopService,
                "--autostart-on" => opts.action = Action::AutostartOn,
                "--autostart-off" => opts.action = Action::AutostartOff,
                "--status" => opts.action = Action::Status,
                "--shutdown" => opts.action = Action::Shutdown,
                "-h" | "--help" => opts.action = Action::Help,
                "--port" => {
                    opts.port = Some(it.next().context("--port needs a value")?.parse()?);
                }
                "--host" => opts.host = Some(it.next().context("--host needs a value")?),
                other => bail!("unknown argument: {other}\n{}", usage()),
            }
        }
        Ok(opts)
    }

    /// Reads `ServerConfig.json` for the Web UI address, like `Main.cs`.
    fn web_ui_url(&self) -> String {
        let config = ConfigurationService::new(RuntimeSettings::default()).load();
        let (host, port) = match config {
            Ok(c) => (c.local_bind_address, c.port as u16),
            Err(_) => (DEFAULT_HOST.to_string(), DEFAULT_PORT),
        };
        let host = self.host.clone().unwrap_or(host);
        let port = self.port.unwrap_or(port);
        let host = if host == "0.0.0.0" || host == "*" || host == "+" {
            DEFAULT_HOST.to_string()
        } else {
            host
        };
        format!("http://{host}:{port}/")
    }
}

fn usage() -> &'static str {
    concat!(
        "Usage: jackett-tray [action] [options]\n",
        "Actions: (default) show tray | --open | --start-service | --stop-service |\n",
        "         --autostart-on | --autostart-off | --status | --shutdown | --help\n",
        "Options: --UpdatedVersion <version> --port <n> --host <address>"
    )
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let opts = match Options::parse(std::env::args()) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(2);
        }
    };
    if let Err(e) = dispatch(opts) {
        eprintln!("jackett-tray failed: {e:#}");
        std::process::exit(1);
    }
}

fn dispatch(opts: Options) -> Result<()> {
    match opts.action {
        Action::Help => println!("{}", usage()),
        Action::Open => open_web_ui(&opts)?,
        Action::StartService => start_service()?,
        Action::StopService => stop_service()?,
        Action::AutostartOn => set_autostart(true)?,
        Action::AutostartOff => set_autostart(false)?,
        Action::Status => println!("{}", status_line(&opts)),
        Action::Shutdown => {
            stop_service()?;
            println!("Jackett shut down");
        }
        Action::Run => run_tray(opts)?,
    }
    Ok(())
}

fn status_line(opts: &Options) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let state = if service_running() {
        "Jackett is running as a background service"
    } else {
        "Jackett is not running"
    };
    format!("Jackett v{version} — {state} — Web UI: {}", opts.web_ui_url())
}

// ---------------------------------------------------------------------------
// Menu actions
// ---------------------------------------------------------------------------

fn open_web_ui(opts: &Options) -> Result<()> {
    let url = opts.web_ui_url();
    tracing::info!("opening {url}");
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("cmd.exe");
        c.args(["/C", "start", "", &url]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(&url);
        c
    };
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(&url);
        c
    };
    cmd.spawn()
        .with_context(|| format!("failed to open browser for {url}"))?;
    Ok(())
}

/// Resolve a sibling Jackett binary (falls back to PATH lookup).
fn sibling_binary(name: &str) -> PathBuf {
    let name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(&name);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from(name)
}

fn service_command(arg: &str) -> Result<String> {
    let output = Command::new(sibling_binary("jackett-service"))
        .arg(arg)
        .output()
        .with_context(|| format!("failed to run jackett-service {arg}"))?;
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn service_running() -> bool {
    service_command("--status")
        .map(|s| s.contains("running"))
        .unwrap_or(false)
}

fn start_service() -> Result<()> {
    let out = service_command("--start")?;
    println!("{}", out.trim());
    Ok(())
}

fn stop_service() -> Result<()> {
    let out = service_command("--stop")?;
    println!("{}", out.trim());
    Ok(())
}

fn autostart_marker() -> Result<PathBuf> {
    if cfg!(windows) {
        Ok(PathBuf::from("Jackett"))
    } else {
        let base = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".config")
            });
        Ok(base.join("autostart/jackett.desktop"))
    }
}

fn set_autostart(enable: bool) -> Result<()> {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe().context("cannot resolve tray executable")?;
        let output = if enable {
            Command::new("reg.exe")
                .args([
                    "add",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    "Jackett",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &exe.to_string_lossy(),
                    "/f",
                ])
                .output()?
        } else {
            Command::new("reg.exe")
                .args([
                    "delete",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    "Jackett",
                    "/f",
                ])
                .output()?
        };
        if !output.status.success() {
            bail!(
                "reg.exe failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    #[cfg(not(windows))]
    {
        let path = autostart_marker()?;
        if enable {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let exe = std::env::current_exe()?;
            std::fs::write(
                &path,
                format!(
                    "[Desktop Entry]\nType=Application\nName=Jackett\nExec={}\nX-GNOME-Autostart-enabled=true\n",
                    exe.display()
                ),
            )?;
        } else if path.exists() {
            std::fs::remove_file(&path)?;
        }
    }
    println!("auto-start on boot: {}", if enable { "on" } else { "off" });
    let _ = autostart_marker();
    Ok(())
}

fn autostart_enabled() -> bool {
    #[cfg(windows)]
    {
        Command::new("reg.exe")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "Jackett",
            ])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        autostart_marker().map(|p| p.exists()).unwrap_or(false)
    }
}

/// Ensures the server is up when the tray starts, mirroring `Main.cs`: prefer
/// the installed service, otherwise launch the console server directly.
fn ensure_server_running() -> Result<()> {
    if service_running() {
        return Ok(());
    }
    if let Ok(out) = service_command("--status") {
        if out.contains("stopped") {
            let _ = start_service();
            return Ok(());
        }
    }
    let console = sibling_binary("jackett-server");
    tracing::info!("no service installed; starting {:?}", console);
    Command::new(console)
        .stdin(std::process::Stdio::null())
        .spawn()
        .context("failed to start jackett-server")?;
    Ok(())
}

/// Single-instance guard (C# enumerates processes in the current session).
struct SingleInstance {
    path: PathBuf,
    _file: std::fs::File,
}

impl SingleInstance {
    fn acquire() -> Result<Self> {
        let path = std::env::temp_dir().join("jackett-tray.lock");
        for _ in 0..2 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    use std::io::Write;
                    let _ = write!(file, "{}", std::process::id());
                    return Ok(Self { path, _file: file });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let pid = std::fs::read_to_string(&path)
                        .unwrap_or_default()
                        .trim()
                        .parse::<u32>()
                        .unwrap_or(0);
                    if pid == 0 || !process_alive(pid) {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    bail!("JackettTray is already running (pid {pid})");
                }
                Err(e) => return Err(e).with_context(|| path.display().to_string()),
            }
        }
        bail!("could not acquire the single-instance lock")
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn process_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        Command::new("tasklist.exe")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        Path::new("/proc").join(pid.to_string()).exists()
    }
}

fn run_tray(opts: Options) -> Result<()> {
    let _guard = SingleInstance::acquire()?;
    if let Some(version) = &opts.updated_version {
        tracing::info!("Jackett updated to {version}");
    }
    ensure_server_running()?;
    println!("{}", status_line(&opts));

    #[cfg(windows)]
    tray::run(&opts)?;
    #[cfg(not(windows))]
    {
        // No shell tray icon off Windows: keep the controller alive so it can
        // supervise the console server, and stop on Ctrl+C.
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            tracing::info!("running headless (no system tray available)");
            let _ = tokio::signal::ctrl_c().await;
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Windows tray icon
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod tray {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, TrayIconBuilder, TrayIconEvent};

    /// Simple generated 32x32 icon so no external `.ico` resource is needed.
    fn default_icon() -> Result<Icon> {
        const N: usize = 32;
        let mut rgba = vec![0u8; N * N * 4];
        for y in 0..N {
            for x in 0..N {
                let i = (y * N + x) * 4;
                let border = x < 3 || y < 3 || x >= N - 3 || y >= N - 3;
                let glyph = (x >= 18 && x < 23 && y >= 7 && y < 24)
                    || (y >= 20 && y < 24 && x >= 10 && x < 23)
                    || (x >= 10 && x < 14 && y >= 17 && y < 22);
                let (r, g, b) = if glyph {
                    (255, 255, 255)
                } else if border {
                    (40, 110, 190)
                } else {
                    (18, 52, 92)
                };
                rgba[i] = r;
                rgba[i + 1] = g;
                rgba[i + 2] = b;
                rgba[i + 3] = 255;
            }
        }
        Icon::from_rgba(rgba, N as u32, N as u32)
            .map_err(|e| anyhow::anyhow!("invalid tray icon: {e}"))
    }

    pub fn run(opts: &Options) -> Result<()> {
        let event_loop = EventLoopBuilder::new().build();
        let menu = Menu::new();
        let open = MenuItem::new("Open Web UI", true, None);
        let state = MenuItem::new(
            if service_running() {
                "Jackett is running as a background service"
            } else {
                "Jackett will run in the foreground"
            },
            false,
            None,
        );
        let start = MenuItem::new("Start background service", true, None);
        let stop = MenuItem::new("Stop background service", true, None);
        let autostart = CheckMenuItem::new("Auto-start on boot", true, autostart_enabled(), None);
        let shutdown = MenuItem::new("Shutdown", true, None);
        let close = MenuItem::new("Close tray icon", true, None);
        menu.append_items(&[&open, &state, &start, &stop, &autostart])?;
        menu.append(&shutdown)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&close)?;

        let _tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(format!("Jackett v{}", env!("CARGO_PKG_VERSION")))
            .with_icon(default_icon()?)
            .build()?;

        let menu_rx = MenuEvent::receiver();
        let tray_rx = TrayIconEvent::receiver();
        let url = opts.web_ui_url();
        let autostart_on = Arc::new(AtomicBool::new(autostart_enabled()));
        let autostart_id = autostart.id().clone();
        let open_id = open.id().clone();
        let start_id = start.id().clone();
        let stop_id = stop.id().clone();
        let shutdown_id = shutdown.id().clone();
        let close_id = close.id().clone();

        event_loop.run(move |_, _, control_flow| {
            *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(200));

            while let Ok(event) = menu_rx.try_recv() {
                let id = &event.id;
                let result = if id == &open_id {
                    open_web_ui_url(&url)
                } else if id == &start_id {
                    start_service()
                } else if id == &stop_id {
                    stop_service()
                } else if id == &shutdown_id {
                    stop_service()
                } else if id == &autostart_id {
                    let next = !autostart_on.load(Ordering::SeqCst);
                    let r = set_autostart(next);
                    if r.is_ok() {
                        autostart_on.store(next, Ordering::SeqCst);
                    }
                    r
                } else if id == &close_id {
                    *control_flow = ControlFlow::Exit;
                    Ok(())
                } else {
                    Ok(())
                };
                if let Err(e) = result {
                    tracing::warn!("tray action failed: {e:#}");
                }
            }

            while let Ok(event) = tray_rx.try_recv() {
                if let TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } = event
                {
                    if let Err(e) = open_web_ui_url(&url) {
                        tracing::warn!("failed to open Web UI: {e:#}");
                    }
                }
            }
        });
    }

    fn open_web_ui_url(url: &str) -> Result<()> {
        Command::new("cmd.exe")
            .args(["/C", "start", "", url])
            .spawn()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_actions() {
        let opts = Options::parse(
            ["jackett-tray", "--UpdatedVersion", "v0.22.1", "--port", "9118"]
                .iter()
                .map(|s| s.to_string()),
        )
        .unwrap();
        assert_eq!(opts.action, Action::Run);
        assert_eq!(opts.updated_version.as_deref(), Some("v0.22.1"));
        assert_eq!(opts.port, Some(9118));
        assert_eq!(
            Options::parse(["jackett-tray", "--open"].iter().map(|s| s.to_string()))
                .unwrap()
                .action,
            Action::Open
        );
    }

    #[test]
    fn rejects_bad_arguments() {
        assert!(Options::parse(["jackett-tray", "--nope"].iter().map(|s| s.to_string())).is_err());
        assert!(Options::parse(["jackett-tray", "--port"].iter().map(|s| s.to_string())).is_err());
        assert!(Options::parse(["jackett-tray", "--port", "abc"].iter().map(|s| s.to_string())).is_err());
    }

    #[test]
    fn web_ui_url_uses_defaults_and_overrides() {
        let mut opts = Options::default();
        let url = opts.web_ui_url();
        assert!(url.starts_with("http://127.0.0.1:"));
        opts.host = Some("192.168.1.5".into());
        opts.port = Some(9118);
        assert_eq!(opts.web_ui_url(), "http://192.168.1.5:9118/");
    }

    #[test]
    fn status_line_mentions_web_ui() {
        let line = status_line(&Options::default());
        assert!(line.contains("Jackett v"));
        assert!(line.contains("Web UI: http://"));
    }
}
