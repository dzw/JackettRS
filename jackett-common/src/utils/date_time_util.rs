use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use once_cell::sync::Lazy;

/// Mirror of Jackett.Common/Utils/DateTimeUtil.cs (core helpers).
pub struct DateTimeUtil;

impl DateTimeUtil {
    /// Mirror of `UnixTimestampToDateTime` (seconds).
    pub fn unix_timestamp_to_date_time(timestamp: i64) -> Option<DateTime<Utc>> {
        Utc.timestamp_opt(timestamp, 0).single()
    }

    /// Mirror of `DateTimeToUnixTimestamp` (seconds).
    pub fn date_time_to_unix_timestamp(dt: &DateTime<Utc>) -> i64 {
        dt.timestamp()
    }

    /// Mirror of `FromUnixTime` (seconds, convenience).
    pub fn from_unix_time(timestamp: i64) -> Option<DateTime<Utc>> {
        Self::unix_timestamp_to_date_time(timestamp)
    }

    /// Parses common RFC formats used in tracker RSS/pages.
    pub fn parse_rfc2822(input: &str) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc2822(input)
            .ok()
            .map(|d| d.with_timezone(&Utc))
    }

    pub fn parse_rfc3339(input: &str) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(input)
            .ok()
            .map(|d| d.with_timezone(&Utc))
    }

    /// Mirror of `ParseDateTimeGoLang` — parses using Go reference-time layouts
    /// ("2006-01-02 15:04:05" style). Supports the layouts commonly found in
    /// Cardigann definitions.
    pub fn parse_golang(input: &str, layout: &str) -> Option<DateTime<Utc>> {
        let input = input.trim();
        // map Go layout fragments to chrono format strings, longest first
        let go_fragments: &[(&str, &str)] = &[
            ("2006", "%Y"),
            ("01", "%m"),
            ("02", "%d"),
            ("15:04:05", "%H:%M:%S"),
            ("15:04", "%H:%M"),
            ("15", "%H"),
            ("04", "%M"),
            ("05", "%S"),
            ("Jan", "%b"),
            ("January", "%B"),
            ("Mon", "%a"),
            ("Monday", "%A"),
            ("PM", "%p"),
            ("pm", "%P"),
            ("-0700", "%z"),
            ("-07:00", "%:z"),
            ("Z07:00", "%:z"),
            ("MST", "%Z"),
            (".000", "%.3f"),
            (".999999999", "%.f"),
        ];
        let mut sorted: Vec<&(&str, &str)> = go_fragments.iter().collect();
        sorted.sort_by_key(|(g, _)| std::cmp::Reverse(g.len()));

        let mut fmt = layout.to_string();
        let mut consumed: Vec<(usize, usize)> = Vec::new();
        for (go, chrono_frag) in sorted {
            let mut search_from = 0;
            while let Some(pos) = fmt[search_from..].find(go) {
                let abs = search_from + pos;
                if consumed.iter().any(|(s, e)| abs >= *s && abs < *e) {
                    search_from = abs + go.len();
                    continue;
                }
                consumed.push((abs, abs + go.len()));
                fmt.replace_range(abs..abs + go.len(), chrono_frag);
                search_from = abs + chrono_frag.len();
            }
        }

        // literal "T" etc. must be escaped for chrono
        let mut escaped = String::new();
        for c in fmt.chars() {
            if c.is_alphabetic() && !escaped.ends_with('%') {
                escaped.push_str(&format!("'{}'", c));
            } else {
                escaped.push(c);
            }
        }

        NaiveDateTime::parse_from_str(input, &escaped)
            .map(|d| d.and_utc())
            .ok()
            .or_else(|| DateTime::parse_from_rfc3339(input).ok().map(|d| d.with_timezone(&Utc)))
    }

    /// Mirror of `FromTimeAgo` — parses relative times like "2 hours ago",
    /// "vor 3 Tagen", "1h 30m", "just now".
    pub fn from_time_ago(input: &str) -> Option<DateTime<Utc>> {
        static NUM: Lazy<regex::Regex> =
            Lazy::new(|| regex::Regex::new(r"(?i)(\d+)\s*([a-zäöü]+)").unwrap());
        let lower = input.trim().to_lowercase();
        let mut seconds: i64 = 0;
        for caps in NUM.captures_iter(&lower) {
            let n: i64 = caps.get(1)?.as_str().parse().ok()?;
            let unit = caps.get(2)?.as_str();
            let mult = if unit.starts_with("s") || unit.starts_with("sec") || unit.starts_with("sek")
                || unit.starts_with("秒")
            {
                1
            } else if unit.starts_with("m") && !unit.starts_with("mo") || unit.starts_with("min")
                || unit.starts_with("分")
            {
                60
            } else if unit.starts_with("h") || unit.starts_with("hour") || unit.starts_with("std")
                || unit.starts_with("小时")
            {
                3600
            } else if unit.starts_with("d") || unit.starts_with("day") || unit.starts_with("tag")
                || unit.starts_with("天")
            {
                86400
            } else if unit.starts_with("w") || unit.starts_with("week") || unit.starts_with("wo")
                || unit.starts_with("周")
            {
                604800
            } else if unit.starts_with("mo") || unit.starts_with("month") || unit.starts_with("monat")
            {
                2592000
            } else if unit.starts_with("y") || unit.starts_with("year") || unit.starts_with("jahr")
                || unit.starts_with("年")
            {
                31536000
            } else {
                0
            };
            seconds += n * mult;
        }
        if seconds == 0 {
            return Self::from_unknown(input);
        }
        Some(Utc::now() - chrono::Duration::seconds(seconds))
    }

    /// Mirror of `FromUnknown` — tries several known date formats.
    pub fn from_unknown(input: &str) -> Option<DateTime<Utc>> {
        let input = input.trim();
        if input.is_empty() {
            return None;
        }
        for parser in [
            DateTime::parse_from_rfc3339,
            DateTime::parse_from_rfc2822,
        ] {
            if let Ok(dt) = parser(input) {
                return Some(dt.with_timezone(&Utc));
            }
        }
        let naive_formats = [
            "%Y-%m-%d %H:%M:%S",
            "%Y-%m-%dT%H:%M:%S",
            "%Y-%m-%d %H:%M",
            "%Y-%m-%d",
            "%d/%m/%Y %H:%M:%S",
            "%d/%m/%Y",
            "%m/%d/%Y %H:%M:%S",
            "%m/%d/%Y",
            "%d.%m.%Y %H:%M:%S",
            "%d.%m.%Y",
            "%Y/%m/%d %H:%M:%S",
            "%d %b %Y %H:%M:%S",
            "%d %b %Y",
            "%b %d, %Y %H:%M:%S",
            "%b %d, %Y",
            "%B %d, %Y %H:%M:%S",
            "%B %d, %Y",
        ];
        for fmt in naive_formats {
            if let Ok(d) = chrono::NaiveDateTime::parse_from_str(input, fmt) {
                return Some(d.and_utc());
            }
            if let Ok(d) = chrono::NaiveDate::parse_from_str(input, fmt) {
                return Some(d.and_hms_opt(0, 0, 0)?.and_utc());
            }
        }
        // unix timestamps
        if let Ok(ts) = input.parse::<i64>() {
            return Utc.timestamp_opt(ts, 0).single();
        }
        None
    }
}
