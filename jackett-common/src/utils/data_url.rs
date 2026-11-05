use base64::Engine;

/// Mirror of Jackett.Common/Utils/DataUrl.cs
pub struct DataUrl;

impl DataUrl {
    /// Builds a `data:` URL (C# used mimeType + byte[]).
    pub fn build_data_url(mime_type: &str, data: &[u8]) -> String {
        format!(
            "data:{};base64,{}",
            mime_type,
            base64::engine::general_purpose::STANDARD.encode(data)
        )
    }

    /// Parses a `data:` URL back into (mime_type, bytes).
    pub fn parse_data_url(url: &str) -> Option<(String, Vec<u8>)> {
        let rest = url.strip_prefix("data:")?;
        let (meta, data) = rest.split_once(',')?;
        let mime = meta
            .strip_suffix(";base64")
            .unwrap_or(meta)
            .trim_end_matches(';')
            .to_string();
        let bytes = base64::engine::general_purpose::STANDARD.decode(data).ok()?;
        Some((mime, bytes))
    }
}
