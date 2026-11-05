//! Mirror of `applyFilters` in CardigannIndexer.cs
//!
//! Applies Cardigann filter chains (`filters: [name: args, ...]`) to strings.
//! Debug filters (hexdump/strdump) log via `tracing` instead of NLog.

use base64::Engine;
use once_cell::sync::Lazy;
use regex::Regex;

use super::template::{apply_go_template_text, TemplateVars};
use crate::models::indexer_definition::FilterBlock;

/// Mirror of `DateTimeUtil.Rfc1123ZPattern` output format.
pub const RFC1123_Z: &str = "%a, %d %b %Y %H:%M:%S %z";

fn rfc1123z(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.format(RFC1123_Z).to_string()
}

pub fn apply_filters(data: &str, filters: Option<&Vec<FilterBlock>>, variables: Option<&TemplateVars>) -> String {
    let Some(filters) = filters else {
        return data.to_string();
    };

    let mut data = data.to_string();

    for filter in filters {
        let name = filter.name.as_deref().unwrap_or("");
        // C# used `dynamic Args`: either a scalar string or a list
        let arg_str = |i: usize| -> Option<String> {
            match &filter.args {
                serde_yaml::Value::String(s) if i == 0 => Some(s.clone()),
                serde_yaml::Value::Sequence(seq) => seq
                    .get(i)
                    .map(|v| match v {
                        serde_yaml::Value::String(s) => s.clone(),
                        other => serde_yaml::to_string(other)
                            .unwrap_or_default()
                            .trim()
                            .to_string(),
                    }),
                _ => None,
            }
        };

        match name {
            "querystring" => {
                if let Some(param) = arg_str(0) {
                    data = get_argument_from_query_string(&data, &param);
                }
            }
            "timeparse" | "dateparse" => {
                if let Some(layout) = arg_str(0) {
                    if let Some(dt) = crate::utils::date_time_util::DateTimeUtil::parse_golang(&data, &layout) {
                        data = rfc1123z(&dt);
                    }
                }
            }
            "regexp" => {
                if let Some(pattern) = arg_str(0) {
                    if let Ok(re) = Regex::new(&pattern) {
                        if let Some(c) = re.captures(&data) {
                            data = c.get(1).map(|g| g.as_str().to_string()).unwrap_or_default();
                        }
                    }
                }
            }
            "re_replace" => {
                if let (Some(pattern), Some(replacement)) = (arg_str(0), arg_str(1)) {
                    let replacement = apply_go_template_text(&replacement, variables, None);
                    if let Ok(re) = Regex::new(&pattern) {
                        data = re.replace_all(&data, replacement.as_str()).to_string();
                    }
                }
            }
            "split" => {
                if let (Some(sep), Some(pos)) = (arg_str(0), arg_str(1)) {
                    if let Ok(mut pos_int) = pos.parse::<i64>() {
                        let parts: Vec<&str> = data.split(sep.chars().next().unwrap_or(' ')).collect();
                        if pos_int < 0 {
                            pos_int += parts.len() as i64;
                        }
                        if pos_int >= 0 && (pos_int as usize) < parts.len() {
                            data = parts[pos_int as usize].to_string();
                        }
                    }
                }
            }
            "replace" => {
                if let (Some(from), Some(to)) = (arg_str(0), arg_str(1)) {
                    let to = apply_go_template_text(&to, variables, None);
                    data = data.replace(&from, &to);
                }
            }
            "trim" => match arg_str(0) {
                Some(cutset) if !cutset.is_empty() => {
                    let cut: Vec<char> = cutset.chars().collect();
                    data = data.trim_matches(|c| cut.contains(&c)).to_string();
                }
                _ => data = data.trim().to_string(),
            },
            "prepend" => {
                if let Some(prepend) = arg_str(0) {
                    data = format!("{}{}", apply_go_template_text(&prepend, variables, None), data);
                }
            }
            "append" => {
                if let Some(s) = arg_str(0) {
                    data += &apply_go_template_text(&s, variables, None);
                }
            }
            "tolower" => data = data.to_lowercase(),
            "toupper" => data = data.to_uppercase(),
            "base64decode" => {
                // mirror of WebEncoders.Base64UrlDecode (url-safe base64)
                let normalized = data.replace(['-', '_'], "+/");
                let padded = match normalized.len() % 4 {
                    2 => format!("{}==", normalized),
                    3 => format!("{}=", normalized),
                    _ => normalized,
                };
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&padded) {
                    data = String::from_utf8_lossy(&bytes).to_string();
                }
            }
            "base64encode" => {
                data = base64::engine::general_purpose::STANDARD
                    .encode(data.as_bytes())
                    .replace('+', "-")
                    .replace('/', "_")
                    .trim_end_matches('=')
                    .to_string();
            }
            "urldecode" => data = url_decode(&data),
            "urlencode" => data = super::template::url_encode_component(&data),
            "htmldecode" => data = html_decode(&data),
            "htmlencode" => data = html_encode(&data),
            "timeago" | "reltime" => {
                if let Some(dt) = crate::utils::date_time_util::DateTimeUtil::from_time_ago(&data) {
                    data = rfc1123z(&dt);
                }
            }
            "fuzzytime" => {
                if let Some(dt) = crate::utils::date_time_util::DateTimeUtil::from_unknown(&data) {
                    data = rfc1123z(&dt);
                }
            }
            "validfilename" => {
                data = data
                    .chars()
                    .filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
                    .collect();
            }
            "diacritics" => {
                if arg_str(0).as_deref() == Some("replace") {
                    // strip combining marks after Unicode decomposition
                    use unicode_normalization::UnicodeNormalization;
                    data = data.nfd().filter(|c| !is_combining_mark(*c)).nfc().collect();
                } else {
                    tracing::error!("unsupported diacritics filter argument");
                }
            }
            "jsonjoinarray" => {
                if let (Some(json_path), Some(separator)) = (arg_str(0), arg_str(1)) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) {
                        if let Some(arr) = select_token(&json, &json_path).and_then(|v| v.as_array()) {
                            let strings: Vec<String> = arr
                                .iter()
                                .map(|j| match j {
                                    serde_json::Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                })
                                .collect();
                            data = strings.join(&separator);
                        }
                    }
                }
            }
            "hexdump" => {
                // for debugging invisible special char issues
                let hex: String = data
                    .chars()
                    .map(|c| format!("{}({:02X})", c, c as u32))
                    .collect();
                tracing::debug!("strdump: {}", hex);
            }
            "strdump" => {
                let debug_data = data
                    .replace('\r', "\\r")
                    .replace('\n', "\\n")
                    .replace('\u{A0}', "\\xA0");
                tracing::debug!("strdump: {}", debug_data);
            }
            "validate" => {
                static DELIMITERS: [char; 14] = [
                    ',', ' ', '/', ')', '(', '.', ';', '[', ']', '"', '|', ':', '-', '\'',
                ];
                if let Some(args) = arg_str(0) {
                    let valid: std::collections::HashSet<String> = args
                        .to_lowercase()
                        .split(|c: char| DELIMITERS.contains(&c))
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .collect();
                    let intersect: Vec<String> = data
                        .to_lowercase()
                        .split(|c: char| DELIMITERS.contains(&c))
                        .filter(|s| !s.is_empty() && valid.contains(*s))
                        .map(|s| s.to_string())
                        .collect();
                    data = intersect.join(",");
                }
            }
            _ => {}
        }
    }

    data
}

fn is_combining_mark(c: char) -> bool {
    use unicode_categories::UnicodeCategories;
    // Mn (non-spacing mark) or Mc (spacing combining mark)
    c.is_mark_nonspacing() || c.is_mark_spacing_combining()
}

/// Mirror of `ParseUtil.GetArgumentFromQueryString`.
pub fn get_argument_from_query_string(input: &str, param: &str) -> String {
    let Some(query) = input.split_once('?').map(|(_, q)| q) else {
        return String::new();
    };
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == param {
            return url_decode(v);
        }
    }
    String::new()
}

/// Mirror of `ParseUtil.NormalizeSpace`: trims and collapses inner whitespace.
pub fn normalize_space(input: &str) -> String {
    static SPACES: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
    SPACES.replace_all(input.trim(), " ").to_string()
}

fn url_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn html_decode(input: &str) -> String {
    static NAMED: Lazy<std::collections::HashMap<&'static str, char>> = Lazy::new(|| {
        [
            ("amp", '&'),
            ("lt", '<'),
            ("gt", '>'),
            ("quot", '"'),
            ("apos", '\''),
            ("nbsp", '\u{A0}'),
            ("hellip", '…'),
            ("mdash", '—'),
            ("ndash", '–'),
        ]
        .into_iter()
        .collect()
    });
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find(';');
        match end {
            Some(end) => {
                let entity = &after[..end];
                if let Some(stripped) = entity.strip_prefix('#') {
                    let code = stripped
                        .strip_prefix('x')
                        .or_else(|| stripped.strip_prefix('X'))
                        .map(|h| u32::from_str_radix(h, 16))
                        .unwrap_or_else(|| u32::from_str_radix(stripped, 10));
                    match code {
                        Ok(c) if char::from_u32(c).is_some() => {
                            out.push(char::from_u32(c).unwrap());
                        }
                        _ => out.push_str(&rest[start..=start + end + 1]),
                    }
                } else if let Some(c) = NAMED.get(entity) {
                    out.push(*c);
                } else {
                    out.push_str(&rest[start..=start + end + 1]);
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

fn html_encode(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Mirrors `JToken.SelectToken` — dot/bracket JSON-path subset.
pub fn select_token<'a>(json: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let path = path.strip_prefix('$').unwrap_or(path);
    let path = path.strip_prefix('.').unwrap_or(path);
    if path.is_empty() {
        return Some(json);
    }
    let mut current = json;
    // split on '.' and '[n]' segments
    for segment in path.split('.').filter(|s| !s.is_empty()) {
        if let Some(caps) = segment.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let index: usize = caps.parse().ok()?;
            current = current.get(index)?;
        } else {
            current = current.get(segment)?;
        }
    }
    Some(current)
}
