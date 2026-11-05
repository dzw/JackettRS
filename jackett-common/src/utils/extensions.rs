/// Mirror of Jackett.Common/Extensions (string/enum helpers used by Utils).
///
/// C# extension methods become free functions / traits in Rust.
pub trait StringExtensions {
    /// Mirror of `IsNullOrWhiteSpace`.
    fn is_null_or_whitespace(&self) -> bool;
    /// Mirror of `IsNotNullOrWhiteSpace`.
    fn is_not_null_or_whitespace(&self) -> bool;
    /// Mirror of `ContainsIgnoreCase`.
    fn contains_ignore_case(&self, other: &str) -> bool;
}

impl StringExtensions for str {
    fn is_null_or_whitespace(&self) -> bool {
        self.trim().is_empty()
    }

    fn is_not_null_or_whitespace(&self) -> bool {
        !self.trim().is_empty()
    }

    fn contains_ignore_case(&self, other: &str) -> bool {
        self.to_lowercase().contains(&other.to_lowercase())
    }
}

/// Mirror of `CollectionExtensions.ContainsIgnoreCase`.
pub fn contains_ignore_case(haystack: &[String], needle: &str) -> bool {
    haystack
        .iter()
        .any(|h| h.eq_ignore_ascii_case(needle))
}
