/// Mirror of Jackett.Common/Utils/BrowserUtil.cs
pub struct BrowserUtil;

impl BrowserUtil {
    /// Firefox/Chrome-like default User-Agent used for tracker requests
    /// (C# built it from a Windows Chrome version string).
    pub fn default_user_agent() -> String {
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string()
    }

    /// Removes browser-control noise from a cookie value (C# `CleanUselessCookies`).
    pub fn clean_useless_cookies(cookies: &mut std::collections::HashMap<String, String>) {
        static USELESS: [&str; 6] = [
            "__cfduid",
            "__cf_bm",
            "cf_chl_prog",
            "cf_chl_seq",
            "cf_chl_opt",
            "cf_chl_2",
        ];
        for key in USELESS {
            cookies.remove(key);
        }
    }
}
