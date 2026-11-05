/// Mirror of Jackett.Common/Utils/Variants.cs
///
/// C# is an enum of web-client variants (automatic/httpclient/httpclient2)
/// used to select the request implementation at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Variants {
    #[default]
    Automatic,
    HttpClient,
    HttpClient2,
}

impl Variants {
    pub fn parse(input: &str) -> Option<Self> {
        Some(match input.to_lowercase().as_str() {
            "automatic" => Self::Automatic,
            "httpclient" => Self::HttpClient,
            "httpclient2" => Self::HttpClient2,
            _ => return None,
        })
    }
}
