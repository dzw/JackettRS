/// Mirror of Jackett.Common/Utils/EnvironmentUtil.cs
pub struct EnvironmentUtil;

impl EnvironmentUtil {
    /// Mirror of `GetHomeDir` (resolves HOME on Unix, USERPROFILE on Windows).
    pub fn get_home_dir() -> Option<std::path::PathBuf> {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(std::path::PathBuf::from)
    }

    /// Mirror of `GetApplicationFolder` (folder containing the running binary).
    pub fn get_application_folder() -> Option<std::path::PathBuf> {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
    }

    pub fn is_windows() -> bool {
        cfg!(windows)
    }

    pub fn is_linux() -> bool {
        cfg!(target_os = "linux")
    }

    pub fn is_mac() -> bool {
        cfg!(target_os = "macos")
    }
}
