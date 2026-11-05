use tracing_subscriber::EnvFilter;

/// Mirror of Jackett.Common/Utils/LoggingSetup.cs
pub struct LoggingSetup;

impl LoggingSetup {
    /// Configures the global `tracing` subscriber (C# wired NLog).
    pub fn setup(tracing_enabled: bool) {
        let filter = if tracing_enabled {
            EnvFilter::try_new("debug").expect("valid filter")
        } else {
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
        };
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .try_init();
    }
}
