//! Mirror of `Jackett.Service` + `WindowsServiceConfigService`.
//!
//! `Jackett.Service` is a thin Windows SCM host that launches the console
//! server. The Rust port keeps the same shape but runs the axum server in
//! process (there is no second binary to supervise):
//!
//! * `--service`      Windows SCM entry point (installed as `binPath`)
//! * `--install`      `sc.exe create Jackett ... start= auto`
//! * `--uninstall`    stop then `sc.exe delete Jackett`
//! * `--start/stop/status`
//! * `--reserve-urls add|delete` (`netsh http`)
//! * `--write-unit`   emit a systemd/launchd unit on non-Windows platforms
//! * no arguments     foreground supervisor (systemd / nssm / terminal)
//!
//! On non-Windows platforms the SCM dispatcher is replaced by a plain long
//! running process; install/uninstall manage a user unit file instead.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tracing_subscriber::EnvFilter;

use jackett_server::{RuntimeServerConfig, RuntimeSettings};

const SERVICE_NAME: &str = "Jackett";
const SERVICE_DESCRIPTION: &str = "API Support for your favorite torrent trackers";
const DEFAULT_PORT: u16 = 9117;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Action {
    #[default]
    Run,
    Service,
    Install,
    Uninstall,
    Start,
    Stop,
    Status,
    ReserveUrls,
    WriteUnit,
    Help,
}

#[derive(Debug, Clone, Default)]
struct Options {
    action: Action,
    port: Option<u16>,
    bind_address: Option<String>,
    data_folder: Option<String>,
    definitions_dir: Option<PathBuf>,
    pid_file: Option<String>,
    no_restart: bool,
    no_updates: bool,
    /// `add`, `delete`, or a unit-file path for `--write-unit`.
    argument: Option<String>,
}

impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut opts = Options::default();
        let mut it = args.into_iter().skip(1).peekable();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--service" => opts.action = Action::Service,
                "-i" | "--install" => opts.action = Action::Install,
                "-u" | "--uninstall" => opts.action = Action::Uninstall,
                "-s" | "--start" => opts.action = Action::Start,
                "-k" | "--stop" => opts.action = Action::Stop,
                "--status" => opts.action = Action::Status,
                "-r" | "--reserve-urls" => {
                    opts.action = Action::ReserveUrls;
                    opts.argument = it.next();
                }
                "--write-unit" => {
                    opts.action = Action::WriteUnit;
                    opts.argument = it.next();
                }
                "-h" | "--help" => opts.action = Action::Help,
                "-p" | "--port" => {
                    opts.port = Some(take_value(&mut it, &arg)?.parse().context("invalid port")?);
                }
                "-d" | "--data-folder" => opts.data_folder = Some(take_value(&mut it, &arg)?),
                "--definitions-dir" => {
                    opts.definitions_dir = Some(PathBuf::from(take_value(&mut it, &arg)?))
                }
                "-x" | "--listen-public" => opts.bind_address = Some("0.0.0.0".into()),
                "-z" | "--listen-private" => opts.bind_address = Some("127.0.0.1".into()),
                "--pid-file" => opts.pid_file = Some(take_value(&mut it, &arg)?),
                "--no-restart" => opts.no_restart = true,
                "--no-updates" => opts.no_updates = true,
                other => bail!("unknown argument: {other}\n{}", usage()),
            }
        }
        Ok(opts)
    }

    fn runtime_settings(&self) -> RuntimeSettings {
        RuntimeSettings {
            custom_data_folder: self.data_folder.clone(),
            pid_file: self.pid_file.clone(),
            no_restart: self.no_restart,
            no_updates: self.no_updates,
            custom_log_file_name: Some("ServiceLog.txt".into()),
            ..Default::default()
        }
    }

    fn server_config(&self) -> RuntimeServerConfig {
        RuntimeServerConfig {
            port_override: self.port,
            bind_address_override: self.bind_address.clone(),
            definitions_dir: self.definitions_dir.clone(),
        }
    }
}

fn take_value<I: Iterator<Item = String>>(it: &mut I, flag: &str) -> Result<String> {
    it.next().with_context(|| format!("{flag} requires a value"))
}

fn usage() -> &'static str {
    concat!(
        "Usage: jackett-service [action] [options]\n",
        "Actions: --service | --install | --uninstall | --start | --stop | --status |\n",
        "         --reserve-urls <add|delete> | --write-unit <path> | --help\n",
        "         (default: run the server in the foreground)\n",
        "Options: --port <n> --data-folder <path> --definitions-dir <path>\n",
        "         --listen-public | --listen-private --pid-file <path>\n",
        "         --no-restart --no-updates"
    )
}

#[tokio::main]
async fn main() {
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
    if let Err(e) = dispatch(opts).await {
        eprintln!("jackett-service failed: {e:#}");
        std::process::exit(1);
    }
}

async fn dispatch(opts: Options) -> Result<()> {
    match opts.action {
        Action::Help => println!("{}", usage()),
        Action::Install => install()?,
        Action::Uninstall => uninstall()?,
        Action::Start => start_service()?,
        Action::Stop => stop_service()?,
        Action::Status => println!("{}", service_status()?),
        Action::ReserveUrls => reserve_urls(&opts)?,
        Action::WriteUnit => write_unit(&opts)?,
        Action::Service => {
            #[cfg(windows)]
            scm::run(opts)?;
            #[cfg(not(windows))]
            run_foreground(&opts).await?;
        }
        Action::Run => run_foreground(&opts).await?,
    }
    Ok(())
}

/// Runs the server, restarting after a crash unless `--no-restart` was given.
async fn run_foreground(opts: &Options) -> Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_signal = stop.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::info!("shutdown requested");
            stop_signal.store(true, Ordering::SeqCst);
        }
    });
    write_pid_file(opts)?;
    let result = serve(opts, stop).await;
    remove_pid_file(opts);
    result
}

/// Shared supervisor loop used by both the foreground and SCM hosts.
pub(crate) async fn serve(opts: &Options, stop: Arc<AtomicBool>) -> Result<()> {
    loop {
        let rs = opts.runtime_settings();
        let so = opts.server_config();
        let outcome = tokio::select! {
            r = jackett_server::run_server(rs, so) => Some(r),
            _ = wait_for_stop(stop.clone()) => None,
        };
        let Some(result) = outcome else {
            tracing::info!("Jackett service stopping");
            return Ok(());
        };
        if opts.no_restart {
            return result;
        }
        match &result {
            Ok(()) => tracing::error!("server exited; restarting in 2s"),
            Err(e) => tracing::error!("server exited ({e:#}); restarting in 2s"),
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            _ = wait_for_stop(stop.clone()) => return Ok(()),
        }
    }
}

async fn wait_for_stop(stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn write_pid_file(opts: &Options) -> Result<()> {
    let Some(path) = opts.pid_file.as_ref() else {
        return Ok(());
    };
    std::fs::write(path, std::process::id().to_string())
        .with_context(|| format!("failed to write pid file {path}"))
}

fn remove_pid_file(opts: &Options) {
    if let Some(path) = &opts.pid_file {
        let _ = std::fs::remove_file(path);
    }
}

// ---------------------------------------------------------------------------
// Service registration
// ---------------------------------------------------------------------------

fn current_exe() -> Result<PathBuf> {
    std::env::current_exe().context("cannot resolve current executable")
}

/// `sc.exe create Jackett start= auto binpath= "<exe> --service"` — mirrors
/// `WindowsServiceConfigService.Install()`.
fn install() -> Result<()> {
    #[cfg(windows)]
    {
        let exe = current_exe()?;
        let binpath = format!("\"{}\" --service", exe.display());
        sc(&[
            "create",
            SERVICE_NAME,
            "binPath=",
            &binpath,
            "start=",
            "auto",
            "DisplayName=",
            SERVICE_NAME,
        ])?;
        sc(&["description", SERVICE_NAME, SERVICE_DESCRIPTION])?;
        println!("installed service {SERVICE_NAME}");
    }
    #[cfg(not(windows))]
    {
        let unit = unit_path()?;
        std::fs::write(&unit, systemd_unit()?).with_context(|| unit.display().to_string())?;
        println!("wrote {}", unit.display());
        println!("enable with: systemctl --user enable --now jackett");
    }
    Ok(())
}

fn uninstall() -> Result<()> {
    #[cfg(windows)]
    {
        if service_running()? {
            stop_service()?;
        }
        if let Err(e) = reserve_urls_raw(false) {
            tracing::warn!("failed to remove urlacl: {e:#}");
        }
        sc(&["delete", SERVICE_NAME])?;
        println!("uninstalled service {SERVICE_NAME}");
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("systemctl").args(["--user", "stop", "jackett"]).status();
        let unit = unit_path()?;
        std::fs::remove_file(&unit).with_context(|| unit.display().to_string())?;
        println!("removed {}", unit.display());
    }
    Ok(())
}

fn start_service() -> Result<()> {
    #[cfg(windows)]
    {
        sc(&["start", SERVICE_NAME])?;
    }
    #[cfg(not(windows))]
    {
        run("systemctl", &["--user", "start", "jackett"])?;
    }
    println!("started {SERVICE_NAME}");
    Ok(())
}

fn stop_service() -> Result<()> {
    #[cfg(windows)]
    {
        sc(&["stop", SERVICE_NAME])?;
    }
    #[cfg(not(windows))]
    {
        run("systemctl", &["--user", "stop", "jackett"])?;
    }
    println!("stopped {SERVICE_NAME}");
    Ok(())
}

fn service_status() -> Result<String> {
    #[cfg(windows)]
    {
        // `sc query` exits non-zero when the service is absent; that is a
        // normal state, so read the output directly instead of using `sc()`.
        let text = Command::new("sc.exe")
            .args(["query", SERVICE_NAME])
            .output()
            .map(|o| {
                format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                )
            })
            .unwrap_or_default();
        let running = text.contains("RUNNING");
        let exists = !text.is_empty() && !text.contains("1060");
        Ok(format!(
            "{SERVICE_NAME}: {}",
            if !exists {
                "not installed"
            } else if running {
                "running"
            } else {
                "stopped"
            }
        ))
    }
    #[cfg(not(windows))]
    {
        let out = Command::new("systemctl")
            .args(["--user", "is-active", "jackett"])
            .output();
        Ok(match out {
            Ok(o) if o.status.success() => format!("{SERVICE_NAME}: running"),
            _ => format!("{SERVICE_NAME}: stopped"),
        })
    }
}

fn service_running() -> Result<bool> {
    Ok(service_status()?.contains("running"))
}

#[cfg(windows)]
fn sc(args: &[&str]) -> Result<String> {
    let output = Command::new("sc.exe")
        .args(args)
        .output()
        .context("failed to run sc.exe (administrator privileges may be required)")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        bail!("sc.exe {} failed: {}", args.join(" "), text.trim());
    }
    Ok(text)
}

#[cfg(not(windows))]
fn run(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("failed to run {program}"))?;
    if !status.success() {
        bail!("{program} {} exited with {status}", args.join(" "));
    }
    Ok(())
}

fn reserve_urls(opts: &Options) -> Result<()> {
    let mode = opts.argument.as_deref().unwrap_or("add");
    match mode {
        "add" => reserve_urls_raw(true),
        "delete" => reserve_urls_raw(false),
        other => bail!("--reserve-urls expects add|delete, got {other}"),
    }
}

fn reserve_urls_raw(add: bool) -> Result<()> {
    #[cfg(windows)]
    {
        let port = DEFAULT_PORT;
        let verb = if add { "add" } else { "delete" };
        let url = format!("url=http://+:{port}/");
        let mut args = vec!["http", verb, "urlacl", &url];
        if add {
            args.push("sddl=D:(A;;GX;;;S-1-1-0)");
        }
        let output = Command::new("netsh.exe")
            .args(&args)
            .output()
            .context("failed to run netsh.exe")?;
        if !output.status.success() {
            bail!(
                "netsh failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        println!("urlacl {verb} for port {port}");
    }
    #[cfg(not(windows))]
    {
        let _ = add;
        println!("urlacl reservation is a Windows-only operation");
    }
    Ok(())
}

#[cfg(not(windows))]
fn unit_path() -> Result<PathBuf> {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".config")
        });
    let dir = base.join("systemd/user");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("jackett.service"))
}

#[cfg(not(windows))]
fn systemd_unit() -> Result<String> {
    let exe = current_exe()?;
    Ok(format!(
        "[Unit]\nDescription={SERVICE_DESCRIPTION}\nAfter=network.target\n\n\
         [Service]\nType=simple\nExecStart={}\nRestart=always\n\
         Environment=DOTNET_EnableDiagnostics=0\n\n\
         [Install]\nWantedBy=default.target\n",
        exe.display()
    ))
}

fn write_unit(opts: &Options) -> Result<()> {
    let path = PathBuf::from(
        opts.argument
            .clone()
            .context("--write-unit requires a destination path")?,
    );
    let exe = current_exe()?;
    let content = if cfg!(target_os = "macos") && path.extension().and_then(|e| e.to_str()) == Some("plist") {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict>\n\
             <key>Label</key><string>org.user.Jackett</string>\n\
             <key>ProgramArguments</key><array><string>{}</string>\
             <string>--NoRestart</string></array>\n\
             <key>KeepAlive</key><true/>\n\
             </dict></plist>\n",
            exe.display()
        )
    } else {
        format!(
            "[Unit]\nDescription={SERVICE_DESCRIPTION}\nAfter=network.target\n\n\
             [Service]\nType=simple\nExecStart={}\nRestart=always\n\
             Environment=DOTNET_EnableDiagnostics=0\n\n\
             [Install]\nWantedBy=default.target\n",
            exe.display()
        )
    };
    std::fs::write(&path, content).with_context(|| path.display().to_string())?;
    println!("wrote {}", path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Windows SCM dispatcher (`--service`)
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod scm {
    use super::*;
    use std::ffi::OsString;
    use std::sync::OnceLock;
    use windows_service::service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service_dispatcher;
    use windows_service::define_windows_service;

    static OPTIONS: OnceLock<Options> = OnceLock::new();

    define_windows_service!(ffi_service_main, service_main);

    pub fn run(opts: Options) -> Result<()> {
        let _ = OPTIONS.set(opts);
        service_dispatcher::start(SERVICE_NAME, ffi_service_main)
            .context("StartServiceCtrlDispatcher failed (was --service launched by the SCM?)")
    }

    fn service_main(_args: Vec<OsString>) {
        let opts = OPTIONS.get().cloned().unwrap_or_default();
        if let Err(e) = inner(opts) {
            tracing::error!("service host failed: {e:#}");
        }
    }

    fn inner(opts: Options) -> Result<()> {
        let stop = Arc::new(AtomicBool::new(false));
        let handler_stop = stop.clone();
        let status_handle = service_control_handler::register(SERVICE_NAME, move |control| {
            match control {
                ServiceControl::Stop | ServiceControl::Shutdown => {
                    handler_stop.store(true, Ordering::SeqCst);
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        })
        .context("failed to register the service control handler")?;

        status_handle
            .set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: ServiceState::Running,
                controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
                exit_code: ServiceExitCode::Win32(0),
                checkpoint: 0,
                wait_hint: Duration::from_secs(5),
                process_id: None,
            })
            .context("failed to report SERVICE_RUNNING")?;

        write_pid_file(&opts)?;
        let rt = tokio::runtime::Runtime::new()?;
        let result = rt.block_on(serve(&opts, stop));
        remove_pid_file(&opts);

        status_handle
            .set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: ServiceState::Stopped,
                controls_accepted: ServiceControlAccept::empty(),
                exit_code: match &result {
                    Ok(()) => ServiceExitCode::Win32(0),
                    Err(_) => ServiceExitCode::ServiceSpecific(1),
                },
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            })
            .context("failed to report SERVICE_STOPPED")?;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_actions_and_options() {
        let opts = Options::parse(
            ["jackett-service", "--install", "--port", "9118", "--no-restart"]
                .iter()
                .map(|s| s.to_string()),
        )
        .unwrap();
        assert_eq!(opts.action, Action::Install);
        assert_eq!(opts.port, Some(9118));
        assert!(opts.no_restart);
    }

    #[test]
    fn rejects_unknown_and_missing_values() {
        assert!(Options::parse(["jackett-service", "--bogus"].iter().map(|s| s.to_string())).is_err());
        assert!(Options::parse(["jackett-service", "--port"].iter().map(|s| s.to_string())).is_err());
        assert!(Options::parse(["jackett-service", "--reserve-urls"].iter().map(|s| s.to_string())).is_ok());
    }

    #[test]
    fn runtime_options_are_forwarded() {
        let opts = Options::parse(
            [
                "jackett-service",
                "--port",
                "9000",
                "--data-folder",
                "C:/JackettData",
                "--listen-public",
                "--no-updates",
                "--pid-file",
                "svc.pid",
            ]
            .iter()
            .map(|s| s.to_string()),
        )
        .unwrap();
        let rs = opts.runtime_settings();
        assert_eq!(rs.custom_data_folder.as_deref(), Some("C:/JackettData"));
        assert_eq!(rs.custom_log_file_name.as_deref(), Some("ServiceLog.txt"));
        assert!(rs.no_updates);
        let so = opts.server_config();
        assert_eq!(so.port_override, Some(9000));
        assert_eq!(so.bind_address_override.as_deref(), Some("0.0.0.0"));
    }

    #[test]
    fn unit_content_contains_exec_and_description() {
        #[cfg(not(windows))]
        {
            let unit = systemd_unit().unwrap();
            assert!(unit.contains(SERVICE_DESCRIPTION));
            assert!(unit.contains("ExecStart="));
            assert!(unit.contains("Restart=always"));
        }
    }
}
