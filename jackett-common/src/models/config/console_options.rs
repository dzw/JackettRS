use super::runtime_settings::RuntimeSettings;

/// Mirror of Jackett.Common/Models/Config/ConsoleOptions.cs
/// (C# used CommandLineParser attributes; here we expose plain fields that the
/// CLI layer in jackett-server fills from argv.)
#[derive(Debug, Clone, Default)]
pub struct ConsoleOptions {
    /// -i, --Install: Install Jackett windows service (Must be admin)
    pub install: bool,
    /// -r, --ReserveUrls: (Re)Register windows port reservations
    pub reserve_urls: bool,
    /// -u, --Uninstall: Uninstall Jackett windows service (Must be admin).
    pub uninstall: bool,
    /// -l, --Logging: Log all requests/responses to Jackett
    pub logging: bool,
    /// -t, --Tracing: Enable tracing
    pub tracing: bool,
    /// -c, --UseClient: [automatic(Default)/httpclient/httpclient2]
    pub client: Option<String>,
    /// -s, --Start: Start the Jackett Windows service (Must be admin)
    pub start_service: bool,
    /// -k, --Stop: Stop the Jackett Windows service (Must be admin)
    pub stop_service: bool,
    /// -x, --ListenPublic: Listen publicly
    pub listen_public: bool,
    /// -z, --ListenPrivate: Only allow local access
    pub listen_private: bool,
    /// -p, --Port: Web server port
    pub port: i32,
    /// -n, --IgnoreSslErrors: [true/false] Ignores invalid SSL certificates
    pub ignore_ssl_errors: Option<bool>,
    /// -d, --DataFolder: Specify the location of the data folder
    pub data_folder: Option<String>,
    /// --NoRestart: Don't restart after update
    pub no_restart: bool,
    /// --PIDFile: Specify the location of PID file
    pub pid_file: Option<String>,
    /// --NoUpdates: Disable automatic updates
    pub no_updates: bool,
}

impl ConsoleOptions {
    /// Mirror of `ToRunTimeSettings()`.
    pub fn to_run_time_settings(&self) -> RuntimeSettings {
        let mut runtime_settings = RuntimeSettings::default();

        // Logging
        if self.logging {
            runtime_settings.log_requests = true;
        }

        // Tracing
        if self.tracing {
            runtime_settings.tracing_enabled = true;
        }

        // (C# exits when both ListenPublic and ListenPrivate are set — handled by the CLI layer)

        // Use curl
        if let Some(client) = &self.client {
            runtime_settings.client_override = Some(client.to_lowercase());
        }

        // Ignore SSL errors on Curl
        runtime_settings.ignore_ssl_errors = self.ignore_ssl_errors;

        runtime_settings.no_restart = self.no_restart;
        runtime_settings.no_updates = self.no_updates;

        if let Some(data_folder) = self.data_folder.as_ref().filter(|d| !d.trim().is_empty()) {
            runtime_settings.custom_data_folder = Some(data_folder.clone());
        }

        runtime_settings.pid_file = self.pid_file.clone();

        runtime_settings
    }
}
