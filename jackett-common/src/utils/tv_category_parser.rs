use once_cell::sync::Lazy;
use regex::Regex;

/// Mirror of Jackett.Common/Utils/TvCategoryParser.cs (simplified core:
/// detects the TV category (season/episode) from a release title).
pub struct TvCategoryParser;

impl TvCategoryParser {
    /// Returns true when the title looks like a TV episode release.
    pub fn is_tv(title: &str) -> bool {
        static PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
            vec![
                // S01E02 / S01E02E03
                Regex::new(r"(?i)\bS\d{1,3}[E ]{1,2}\d{1,3}([E ]\d{1,3})*\b").unwrap(),
                // 1x02
                Regex::new(r"(?i)\b\d{1,2}x\d{1,3}\b").unwrap(),
                // season/episode keywords
                Regex::new(r"(?i)\b(season|saison|staffel)[ ._-]?\d{1,3}\b").unwrap(),
                Regex::new(r"(?i)\b(episode|ep)\d{1,3}\b").unwrap(),
            ]
        });
        PATTERNS.iter().any(|re| re.is_match(title))
    }
}
