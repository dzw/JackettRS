use once_cell::sync::Lazy;
use regex::Regex;

/// Mirror of Jackett.Common/Utils/ParseUtil.cs (core helpers used so far).
pub struct ParseUtil;

impl ParseUtil {
    /// Extracts an IMDB id from a string. Returns the "ttNNNNNNNN" form
    /// (7- or 8-digit numeric part).
    pub fn get_imdb_id_from_string(input: &str) -> Option<String> {
        static IMDB_REGEX: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"(?:tt)?(\d{7,8})").unwrap());
        IMDB_REGEX
            .captures(input)
            .map(|c| format!("tt{}", &c[1]))
    }

    /// Mirror of `GetFullImdbId`: validates "tt" + 7 or 8 digits.
    pub fn get_full_imdb_id(input: &str) -> Option<String> {
        static FULL_IMDB: Lazy<Regex> = Lazy::new(|| Regex::new(r"^tt(\d{7,8})$").unwrap());
        FULL_IMDB.captures(input).map(|c| format!("tt{}", &c[1]))
    }

    /// Mirror of `CoerceInt`: extracts the leading integer from a string
    /// (C# used a regex-based coercion that tolerates surrounding junk).
    pub fn coerce_int(input: &str) -> Result<i32, String> {
        static INT_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\s]*(-?\d+)[\s]*$").unwrap());
        match INT_REGEX.captures(input) {
            Some(c) => c[1]
                .parse::<i32>()
                .map_err(|e| format!("Cannot coerce \"{}\" to int: {}", input, e)),
            None => Err(format!("Cannot coerce \"{}\" to int", input)),
        }
    }

    /// Mirror of `GetLongFromString`: finds the first long in a string.
    pub fn get_long_from_string(input: &str) -> Option<i64> {
        static LONG_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\d+").unwrap());
        LONG_REGEX
            .find(input)
            .and_then(|m| m.as_str().parse::<i64>().ok())
    }

    /// Mirror of `GetSizeInBytes`: parses human-readable sizes like "1.5 GB".
    pub fn get_size_in_bytes(input: &str) -> Option<f64> {
        static SIZE_REGEX: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"(?i)^([\d\.]+)\s*(tib|gib|mib|kib|tb|gb|mb|kb|b|)$").unwrap()
        });
        let captures = SIZE_REGEX.captures(input.trim())?;
        let value: f64 = captures[1].parse().ok()?;
        let unit = captures.get(2).map(|m| m.as_str()).unwrap_or("");
        let multiplier = match unit.to_lowercase().as_str() {
            "tib" => 1024f64.powi(4),
            "gib" => 1024f64.powi(3),
            "mib" => 1024f64.powi(2),
            "kib" => 1024f64,
            "tb" => 1000f64.powi(4),
            "gb" => 1000f64.powi(3),
            "mb" => 1000f64.powi(2),
            "kb" => 1000f64,
            "b" | "" => 1.0,
            _ => return None,
        };
        Some(value * multiplier)
    }
}
