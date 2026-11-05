/// Mirror of Jackett.Common/Utils/MagnetUtil.cs
pub struct MagnetUtil;

impl MagnetUtil {
    /// Parses the info hash (btih) out of a magnet URI.
    pub fn info_hash_from_magnet(magnet: &str) -> Option<String> {
        let query = magnet.strip_prefix("magnet:?.")?;
        for param in query.split('&') {
            if let Some(value) = param.strip_prefix("xt=urn:btih:") {
                return Some(value.to_string());
            }
        }
        None
    }

    /// Builds a magnet URI from an info hash and optional display name.
    pub fn magnet_from_info_hash(info_hash: &str, display_name: Option<&str>) -> String {
        match display_name {
            Some(name) => format!(
                "magnet:?xt=urn:btih:{}&dn={}",
                info_hash,
                urlencoding_like::encode(name)
            ),
            None => format!("magnet:?xt=urn:btih:{}", info_hash),
        }
    }
}

/// Minimal percent-encoding used by MagnetUtil.
mod urlencoding_like {
    pub fn encode(input: &str) -> String {
        let mut out = String::new();
        for byte in input.as_bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(*byte as char)
                }
                _ => out.push_str(&format!("%{:02X}", byte)),
            }
        }
        out
    }
}
