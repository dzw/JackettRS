//! Mirror of `Jackett.Updater` + `Jackett.Common/Services/UpdateService`.
//!
//! The C# split is: `UpdateService` checks GitHub, downloads and unpacks a
//! release into a temp directory, then launches `JackettUpdater` which copies
//! the files over `--Path`, kills the old processes and restarts. This binary
//! implements both halves:
//!
//! * `--check`                 query GitHub releases, print the latest version
//! * `--update-to vX.Y.Z`      check + download + unpack + apply + restart
//! * `--path <dir> ...`        apply an already-unpacked update (updater role)
//!
//! `--type`, `--args`, `--kill-pids`, `--no-restart`, `--start-tray` mirror
//! `UpdaterConsoleOptions`.

use std::cmp::Ordering;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

const RELEASES_URL: &str = "https://api.github.com/repos/Jackett/Jackett/releases";
const USER_AGENT: &str = "Jackett-Rust-Updater";
const UPDATER_LOG: &str = "updater.txt";

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Action {
    #[default]
    Apply,
    Check,
    UpdateTo,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppType {
    Console,
    WindowsService,
}

#[derive(Debug, Clone)]
struct Options {
    action: Action,
    path: Option<PathBuf>,
    app_type: AppType,
    args: String,
    no_restart: bool,
    start_tray: bool,
    kill_pids: Vec<u32>,
    update_to: Option<String>,
    prerelease: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            action: Action::Apply,
            path: None,
            app_type: AppType::Console,
            args: String::new(),
            no_restart: false,
            start_tray: false,
            kill_pids: Vec::new(),
            update_to: None,
            prerelease: false,
        }
    }
}

impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut opts = Options::default();
        let mut it = args.into_iter().skip(1).peekable();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--check" => opts.action = Action::Check,
                "--update-to" => {
                    opts.action = Action::UpdateTo;
                    opts.update_to = Some(value(&mut it, &arg)?);
                }
                "-h" | "--help" => opts.action = Action::Help,
                "-p" | "--path" => opts.path = Some(PathBuf::from(value(&mut it, &arg)?)),
                "-t" | "--type" => {
                    opts.app_type = match value(&mut it, &arg)?.to_ascii_lowercase().as_str() {
                        "windowsservice" => AppType::WindowsService,
                        "console" => AppType::Console,
                        other => bail!("--type expects Console|WindowsService, got {other}"),
                    };
                }
                "-a" | "--args" => opts.args = value(&mut it, &arg)?,
                "--no-restart" => opts.no_restart = true,
                "--start-tray" => opts.start_tray = true,
                "--prerelease" => opts.prerelease = true,
                "--kill-pids" => {
                    opts.kill_pids = value(&mut it, &arg)?
                        .split(',')
                        .filter_map(|p| p.trim().parse::<u32>().ok())
                        .collect();
                }
                other => bail!("unknown argument: {other}\n{}", usage()),
            }
        }
        Ok(opts)
    }
}

fn value<I: Iterator<Item = String>>(it: &mut I, flag: &str) -> Result<String> {
    it.next().with_context(|| format!("{flag} requires a value"))
}

fn usage() -> &'static str {
    concat!(
        "Usage: jackett-updater [action] [options]\n",
        "Actions: --check | --update-to <version> | --path <dir> (apply) | --help\n",
        "Options: --path <dir> --type <Console|WindowsService> --args <string>\n",
        "         --kill-pids <csv> --no-restart --start-tray --prerelease"
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
        eprintln!("jackett-updater failed: {e:#}");
        std::process::exit(1);
    }
}

fn dispatch(opts: Options) -> Result<()> {
    match opts.action {
        Action::Help => println!("{}", usage()),
        Action::Check => {
            let release = latest_release(opts.prerelease)?;
            println!(
                "{} ({}) -> {}",
                release.version,
                release.created_at.as_deref().unwrap_or("unknown date"),
                release.asset.name
            );
        }
        Action::UpdateTo => {
            let target = opts.update_to.clone().context("--update-to needs a version")?;
            let release = find_release(&target, opts.prerelease)?;
            let install_dir = opts
                .path
                .clone()
                .or_else(installation_dir)
                .context("cannot determine installation directory; pass --path")?;
            let temp = temp_dir(&release.version.to_string());
            fs::create_dir_all(&temp)?;
            let archive = download_asset(&release, &temp)?;
            let extracted = extract_archive(&archive, &temp)?;
            kill_pids(&opts.kill_pids);
            let copied = apply_update(&extracted, &install_dir)?;
            tracing::info!("copied {copied} files into {}", install_dir.display());
            fs::write(install_dir.join(UPDATER_LOG), format!("updated to {}\n", release.version))?;
            let _ = fs::remove_file(install_dir.join(".lock"));
            restart(&opts, &install_dir, &release.version.to_string())?;
            let _ = fs::remove_dir_all(&temp);
        }
        Action::Apply => {
            let install_dir = opts
                .path
                .clone()
                .context("--path is required when applying an unpacked update")?;
            let source = installation_dir().unwrap_or_else(|| PathBuf::from("."));
            kill_pids(&opts.kill_pids);
            let copied = apply_update(&source, &install_dir)?;
            tracing::info!("copied {copied} files into {}", install_dir.display());
            let _ = fs::remove_dir_all(install_dir.join("Content/logos"));
            let _ = fs::remove_file(install_dir.join(".lock"));
            restart(&opts, &install_dir, env!("CARGO_PKG_VERSION"))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// GitHub releases
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    major: u64,
    minor: u64,
    build: u64,
}

impl Version {
    fn parse(text: &str) -> Option<Self> {
        let re = Regex::new(r"v?(\d+)\.(\d+)\.(\d+)").ok()?;
        let caps = re.captures(text)?;
        Some(Self {
            major: caps.get(1)?.as_str().parse().ok()?,
            minor: caps.get(2)?.as_str().parse().ok()?,
            build: caps.get(3)?.as_str().parse().ok()?,
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}.{}.{}", self.major, self.minor, self.build)
    }
}

/// GitHub JSON uses snake_case — C# only worked because Newtonsoft.Json is
/// case-insensitive; serde is not, so the field names must match exactly.
#[derive(Debug, Deserialize)]
struct GithubRelease {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    tag_name: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone)]
struct Release {
    version: Version,
    created_at: Option<String>,
    asset: GithubAsset,
}

fn artifact_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "Jackett.Binaries.Windows.zip"
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "macOSARM64.tar.gz"
        } else {
            "macOS.tar.gz"
        }
    } else if cfg!(target_env = "musl") {
        match std::env::consts::ARCH {
            "aarch64" => "LinuxMuslARM64.tar.gz",
            "arm" => "LinuxMuslARM32.tar.gz",
            _ => "LinuxMuslAMDx64.tar.gz",
        }
    } else {
        match std::env::consts::ARCH {
            "aarch64" => "LinuxARM64.tar.gz",
            "arm" => "LinuxARM32.tar.gz",
            _ => "LinuxAMDx64.tar.gz",
        }
    }
}

fn http_client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(300))
        .build()?)
}

fn fetch_releases() -> Result<Vec<GithubRelease>> {
    let body = http_client()?
        .get(RELEASES_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .context("failed to query GitHub releases")?
        .error_for_status()?
        .text()?;
    Ok(serde_json::from_str(&body).context("failed to parse GitHub releases JSON")?)
}

fn into_release(r: GithubRelease, prerelease: bool) -> Option<Release> {
    if r.prerelease && !prerelease {
        return None;
    }
    let label = r
        .tag_name
        .as_deref()
        .or(r.name.as_deref())
        .unwrap_or_default();
    let version = Version::parse(label)?;
    let wanted = artifact_name();
    let asset = r
        .assets
        .into_iter()
        .find(|a| a.name.ends_with(wanted))?
        .clone();
    Some(Release {
        version,
        created_at: r.created_at,
        asset,
    })
}

fn latest_release(prerelease: bool) -> Result<Release> {
    let mut releases: Vec<Release> = fetch_releases()?
        .into_iter()
        .filter_map(|r| into_release(r, prerelease))
        .collect();
    releases.sort_by(|a, b| b.version.cmp(&a.version));
    releases
        .into_iter()
        .next()
        .context("no Jackett release with a matching platform artifact")
}

fn find_release(target: &str, prerelease: bool) -> Result<Release> {
    let wanted = Version::parse(target).unwrap_or_else(|| Version::parse(&format!("v{target}")).unwrap_or(Version { major: 0, minor: 0, build: 0 }));
    fetch_releases()?
        .into_iter()
        .filter_map(|r| into_release(r, prerelease))
        .find(|r| {
            if wanted == (Version { major: 0, minor: 0, build: 0 }) {
                r.version.to_string().eq_ignore_ascii_case(target)
                    || r.asset.name.contains(target)
            } else {
                r.version == wanted
            }
        })
        .with_context(|| format!("release {target} not found"))
}

fn temp_dir(version: &str) -> PathBuf {
    std::env::temp_dir().join(format!("JackettUpdate-{version}"))
}

fn download_asset(release: &Release, temp: &Path) -> Result<PathBuf> {
    let path = temp.join(&release.asset.name);
    let bytes = http_client()?
        .get(&release.asset.browser_download_url)
        .header("Accept", "application/octet-stream")
        .send()?
        .error_for_status()?
        .bytes()?;
    fs::write(&path, &bytes).with_context(|| path.display().to_string())?;
    Ok(path)
}

fn extract_archive(archive: &Path, temp: &Path) -> Result<PathBuf> {
    let out = temp.join("extracted");
    fs::create_dir_all(&out)?;
    if archive
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.ends_with(".zip"))
        .unwrap_or(false)
    {
        let file = File::open(archive)?;
        let mut zip = zip::ZipArchive::new(file).context("invalid zip archive")?;
        zip.extract(&out).context("failed to extract zip")?;
    } else {
        let file = File::open(archive)?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(decoder);
        tar.unpack(&out).context("failed to extract tar.gz")?;
    }
    #[cfg(unix)]
    for name in ["jackett", "jackett-server", "jackett-updater", "jackett_launcher.sh"] {
        make_executable(&out.join(name));
    }
    Ok(out)
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
    }
}

// ---------------------------------------------------------------------------
// Applying an unpacked update
// ---------------------------------------------------------------------------

fn is_archive(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    name.ends_with(".zip") || name.ends_with(".tar.gz") || name.ends_with(".tgz") || name.ends_with(".gz")
}

/// A handful of definitions removed by the C# updater. The full ~700-entry
/// list is release-specific; stale YAML files are harmless (Cardigann simply
/// loads them), so only the well-known removals are reproduced here.
const OBSOLETE_DEFINITIONS: &[&str] = &[
    "bitmetv.yml",
    "bithdtv.yml",
    "torrentbytes.yml",
    "sceneaccess.yml",
    "pretome.yml",
];

pub fn apply_update(source: &Path, target: &Path) -> Result<usize> {
    if !source.is_dir() {
        bail!("update source {} is not a directory", source.display());
    }
    fs::create_dir_all(target)?;
    let mut count = 0usize;
    for entry in walkdir::WalkDir::new(source).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path == source || is_archive(path) {
            continue;
        }
        let relative = path.strip_prefix(source)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let destination = target.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&destination)?;
            continue;
        }
        copy_with_retry(path, &destination)?;
        count += 1;
    }
    for obsolete in OBSOLETE_DEFINITIONS {
        let _ = fs::remove_file(target.join("Definitions").join(obsolete));
    }
    let _ = fs::remove_dir_all(target.join("Content/logos"));
    let _ = fs::remove_file(target.join(".lock"));
    Ok(count)
}

/// C# `CopyUpdateFile`: retry once after deleting the destination and a 1s
/// backoff — Windows may still hold the old executable open.
fn copy_with_retry(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Err(first) = fs::copy(source, destination) {
        let _ = fs::remove_file(destination);
        sleep(Duration::from_secs(1));
        fs::copy(source, destination).with_context(|| {
            format!(
                "failed to copy {} -> {}: {first}",
                source.display(),
                destination.display()
            )
        })?;
        sleep(Duration::from_secs(1));
    }
    Ok(())
}

fn kill_pids(pids: &[u32]) {
    for pid in pids {
        #[cfg(windows)]
        let result = Command::new("taskkill.exe")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
        #[cfg(not(windows))]
        let result = {
            let _ = Command::new("kill").args(["-15", &pid.to_string()]).status();
            sleep(Duration::from_secs(2));
            Command::new("kill").args(["-9", &pid.to_string()]).status()
        };
        match result {
            Ok(status) if status.success() => tracing::info!("stopped pid {pid}"),
            _ => tracing::warn!("could not stop pid {pid}"),
        }
    }
}

fn installation_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
}

fn sibling_binary(name: &str, dir: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let candidate = dir.join(&name);
    if candidate.exists() {
        candidate
    } else {
        PathBuf::from(name)
    }
}

fn restart(opts: &Options, install_dir: &Path, version: &str) -> Result<()> {
    if opts.no_restart {
        println!("update applied; --no-restart set, leaving Jackett stopped");
        return Ok(());
    }

    if opts.app_type == AppType::WindowsService {
        let service = sibling_binary("jackett-service", install_dir);
        let output = Command::new(service).arg("--start").output();
        match output {
            Ok(o) if o.status.success() => {
                println!("restarted Jackett service");
                return Ok(());
            }
            _ => {
                // C# elevates `JackettConsole.exe --Start` when not admin.
                let output = Command::new("sc.exe").args(["start", "Jackett"]).output();
                if output.map(|o| o.status.success()).unwrap_or(false) {
                    println!("restarted Jackett service");
                    return Ok(());
                }
                bail!("failed to restart the Jackett service");
            }
        }
    }

    if opts.start_tray || cfg!(windows) && tray_was_running() {
        // C# hands off to jackett_launcher.bat when the update was triggered
        // from a background service: the script waits for the updater to exit
        // before starting the tray on the interactive desktop (issue #10068).
        #[cfg(windows)]
        {
            let launcher = install_dir.join("jackett_launcher.bat");
            if launcher.exists() {
                match Command::new("cmd.exe").arg("/C").arg(&launcher).spawn() {
                    Ok(_) => {
                        println!("scheduled tray restart via {}", launcher.display());
                        return Ok(());
                    }
                    Err(e) => tracing::warn!("failed to run {}: {e}", launcher.display()),
                }
            }
        }
        let tray = sibling_binary("jackett-tray", install_dir);
        match Command::new(tray)
            .args(["--UpdatedVersion", version])
            .spawn()
        {
            Ok(_) => {
                println!("restarted Jackett tray");
                return Ok(());
            }
            Err(e) => tracing::warn!("failed to start tray: {e}"),
        }
    }

    let console = sibling_binary("jackett-server", install_dir);
    let mut cmd = Command::new(&console);
    for arg in opts.args.split_whitespace() {
        cmd.arg(arg);
    }
    cmd.stdin(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("failed to start {}", console.display()))?;
    println!("restarted {}", console.display());
    Ok(())
}

fn tray_was_running() -> bool {
    Command::new("tasklist.exe")
        .args(["/FI", "IMAGENAME eq jackett-tray.exe", "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("jackett-tray.exe"))
        .unwrap_or(false)
}

/// Writes the `.lock` marker used by `UpdateService.CheckUpdaterLock()`.
#[allow(dead_code)]
fn write_lock(install_dir: &Path) -> Result<()> {
    let mut f = File::create(install_dir.join(".lock"))?;
    f.write_all(b"Jackett update in progress")?;
    Ok(())
}

#[allow(dead_code)]
fn compare_versions(a: &str, b: &str) -> Ordering {
    match (Version::parse(a), Version::parse(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        _ => a.cmp(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cli() {
        let opts = Options::parse(
            [
                "jackett-updater",
                "--path",
                "C:/Jackett",
                "--type",
                "WindowsService",
                "--args",
                " --NoUpdates ",
                "--kill-pids",
                "12, 34,invalid",
                "--no-restart",
                "--start-tray",
            ]
            .iter()
            .map(|s| s.to_string()),
        )
        .unwrap();
        assert_eq!(opts.action, Action::Apply);
        assert_eq!(opts.app_type, AppType::WindowsService);
        assert_eq!(opts.path, Some(PathBuf::from("C:/Jackett")));
        assert_eq!(opts.kill_pids, vec![12, 34]);
        assert!(opts.no_restart && opts.start_tray);
        assert_eq!(opts.args, " --NoUpdates ");
    }

    #[test]
    fn rejects_bad_cli() {
        assert!(Options::parse(["jackett-updater", "--wat"].iter().map(|s| s.to_string())).is_err());
        assert!(Options::parse(["jackett-updater", "--path"].iter().map(|s| s.to_string())).is_err());
        assert!(Options::parse(
            ["jackett-updater", "--type", "bogus"].iter().map(|s| s.to_string())
        )
        .is_err());
    }

    #[test]
    fn versions_parse_and_sort() {
        assert_eq!(Version::parse("v0.22.1917"), Some(Version { major: 0, minor: 22, build: 1917 }));
        assert_eq!(Version::parse("Jackett 1.2.3"), Some(Version { major: 1, minor: 2, build: 3 }));
        assert!(Version::parse("0.21.0").unwrap() < Version::parse("v0.22.0").unwrap());
        assert_eq!(compare_versions("v0.20.1", "v0.20.10"), Ordering::Less);
        assert_eq!(Version::parse("not a version"), None);
    }

    #[test]
    fn artifact_name_is_platform_specific() {
        let name = artifact_name();
        assert!(!name.is_empty());
        assert!(name.ends_with(".zip") || name.ends_with(".tar.gz"));
    }

    #[test]
    fn parses_github_release_json_with_snake_case_fields() {
        let json = r#"[{
            "name": "v0.22.100",
            "tag_name": "v0.22.100",
            "created_at": "2025-01-02T03:04:05Z",
            "prerelease": false,
            "assets": [{"name": "Jackett.Binaries.Windows.zip", "browser_download_url": "https://example/win.zip"}]
        }]"#;
        let releases: Vec<GithubRelease> = serde_json::from_str(json).unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].created_at.as_deref(), Some("2025-01-02T03:04:05Z"));
        assert_eq!(releases[0].assets[0].name, "Jackett.Binaries.Windows.zip");
        assert_eq!(Version::parse(releases[0].tag_name.as_deref().unwrap()).unwrap().build, 100);
    }

    #[test]
    fn into_release_filters_prerelease_and_platform() {
        let wanted = artifact_name();
        let json = format!(
            r#"[{{"tag_name":"v0.22.1","prerelease":true,"assets":[{{"name":"{wanted}","browser_download_url":"u"}}]}},
                 {{"tag_name":"v0.22.2","prerelease":false,"assets":[{{"name":"unrelated.txt","browser_download_url":"u"}}]}},
                 {{"tag_name":"v0.22.3","prerelease":false,"assets":[{{"name":"{wanted}","browser_download_url":"https://example/a"}}]}}]"#
        );
        let releases: Vec<GithubRelease> = serde_json::from_str(&json).unwrap();
        let found: Vec<Release> = releases
            .into_iter()
            .filter_map(|r| into_release(r, false))
            .collect();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].version.to_string(), "v0.22.3");
    }

    #[test]
    fn archives_are_skipped() {
        assert!(is_archive(Path::new("Update.zip")));
        assert!(is_archive(Path::new("Jackett.Binaries.LinuxAMDx64.tar.gz")));
        assert!(!is_archive(Path::new("jackett-service.exe")));
        assert!(!is_archive(Path::new("Definitions/site.yml")));
    }

    #[test]
    fn applies_an_unpacked_update() {
        let root = std::env::temp_dir().join(format!("jackett-updater-test-{}", std::process::id()));
        let src = root.join("src");
        let dst = root.join("dst");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(src.join("Definitions")).unwrap();
        fs::create_dir_all(src.join("Content/logos")).unwrap();
        fs::create_dir_all(&dst).unwrap();
        fs::create_dir_all(dst.join("Content/logos")).unwrap();
        fs::write(src.join("jackett-server.exe"), b"new binary").unwrap();
        fs::write(src.join("Definitions/site.yml"), b"site: 1").unwrap();
        fs::write(src.join("Update.zip"), b"archive").unwrap();
        fs::write(dst.join("jackett-server.exe"), b"old binary").unwrap();
        fs::write(dst.join("Content/logos/old.png"), b"x").unwrap();
        fs::write(dst.join(".lock"), b"lock").unwrap();

        let count = apply_update(&src, &dst).unwrap();
        assert_eq!(count, 2);
        assert_eq!(fs::read_to_string(dst.join("jackett-server.exe")).unwrap(), "new binary");
        assert_eq!(fs::read_to_string(dst.join("Definitions/site.yml")).unwrap(), "site: 1");
        assert!(!dst.join("Update.zip").exists());
        assert!(!dst.join(".lock").exists());
        assert!(!dst.join("Content/logos").exists());

        let _ = fs::remove_dir_all(&root);
    }
}
