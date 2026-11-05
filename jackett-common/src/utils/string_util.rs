use once_cell::sync::Lazy;
use regex::Regex;

/// Mirror of Jackett.Common/Utils/StringUtil.cs (core helpers).
pub struct StringUtil;

impl StringUtil {
    /// Mirror of `CleanTitle`: removes common release-group noise and normalizes
    /// separators for comparison purposes.
    pub fn clean_title(title: &str) -> String {
        static STRIP: Lazy<Regex> = Lazy::new(|| {
            Regex::new(
                r"(?i)[\(（]?\b(19|20)\d{2}\b[\)）]?|\b(bluray|blu-ray|brrip|bdrip|dvdrip|webrip|web[- ]?dl|hdtv|x264|x265|h\.?264|h\.?265|hevc|aac|aac2|dts|ddp|5\.1|7\.1|proper|repack|extended|unrated|remastered|internal)\b|[\[\](){}]",
            )
            .unwrap()
        });
        static SPACES: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
        let cleaned = STRIP.replace_all(title, " ");
        SPACES.replace_all(cleaned.trim(), " ").to_lowercase()
    }

    /// Mirror of `ToTitleCase` (best-effort for ASCII; C# used the invariant culture).
    pub fn to_title_case(input: &str) -> String {
        input
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Mirror of `RemoveIllegalChars`: strips filesystem-hostile characters.
    pub fn remove_illegal_chars(input: &str) -> String {
        input
            .chars()
            .filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
            .collect()
    }

    /// Mirror of `TrimEnd`.
    pub fn trim_end<'a>(input: &'a str, suffix: &str) -> &'a str {
        input.strip_suffix(suffix).unwrap_or(input)
    }

    /// Mirror of `ReplaceFirst`.
    pub fn replace_first(input: &str, from: &str, to: &str) -> String {
        input.replacen(from, to, 1)
    }
}
