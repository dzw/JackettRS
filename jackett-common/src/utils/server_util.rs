/// Mirror of Jackett.Common/Utils/ServerUtil.cs
pub struct ServerUtil;

impl ServerUtil {
    /// Mirror of `GetApplicationVersion` — from the compile-time version.
    pub fn get_application_version() -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    /// Mirror of `GetStartupFolder` — folder containing the running binary.
    pub fn get_startup_folder() -> Option<std::path::PathBuf> {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
    }
}
