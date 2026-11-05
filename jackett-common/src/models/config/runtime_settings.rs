/// Mirror of Jackett.Common/Models/Config/RuntimeSettings.cs
#[derive(Debug, Clone, Default)]
pub struct RuntimeSettings {
    pub tracing_enabled: bool,
    pub log_requests: bool,
    pub client_override: Option<String>,
    pub ignore_ssl_errors: Option<bool>,
    pub custom_data_folder: Option<String>,
    pub base_path: Option<String>,
    pub no_restart: bool,
    pub custom_log_file_name: Option<String>,
    pub pid_file: Option<String>,
    pub no_updates: bool,
}

impl RuntimeSettings {
    /// Mirrors the C# `DataFolder` computed property:
    /// - Unix: `$XDG_CONFIG_HOME` (or `~/.config`) + `/Jackett`
    /// - Windows: `%PROGRAMDATA%` + `/Jackett`
    pub fn data_folder(&self) -> std::path::PathBuf {
        if let Some(custom) = self
            .custom_data_folder
            .as_ref()
            .filter(|c| !c.trim().is_empty())
        {
            return std::path::PathBuf::from(custom);
        }

        if cfg!(unix) {
            let base = std::env::var("XDG_CONFIG_HOME")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .or_else(|| {
                    std::env::var("HOME")
                        .ok()
                        .map(|h| std::path::Path::new(&h).join(".config").to_string_lossy().into_owned())
                })
                .unwrap_or_default();
            std::path::Path::new(&base).join("Jackett")
        } else {
            let base = std::env::var("PROGRAMDATA").unwrap_or_default();
            std::path::Path::new(&base).join("Jackett")
        }
    }
}
