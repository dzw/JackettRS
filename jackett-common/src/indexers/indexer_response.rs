/// Mirror of Jackett.Common/Indexers/IndexerResponse.cs
pub struct IndexerResponse {
    pub status_code: reqwest::StatusCode,
    pub content: String,
    pub content_bytes: Vec<u8>,
    pub cookies: std::collections::HashMap<String, String>,
    pub headers: reqwest::header::HeaderMap,
    pub request_url: url::Url,
}
