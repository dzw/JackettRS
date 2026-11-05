/// Mirror of Jackett.Common/Utils/CookieUtil.cs (core helpers).
pub struct CookieUtil;

impl CookieUtil {
    /// Parses a Cookie header string ("k1=v1; k2=v2") into a map.
    pub fn parse_cookie_header(header: &str) -> std::collections::HashMap<String, String> {
        header
            .split(';')
            .filter_map(|pair| {
                let pair = pair.trim();
                let (k, v) = pair.split_once('=')?;
                Some((k.trim().to_string(), v.trim().to_string()))
            })
            .collect()
    }

    /// Joins a cookie map back into a header string.
    pub fn cookies_to_header(cookies: &std::collections::HashMap<String, String>) -> String {
        cookies
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("; ")
    }
}
