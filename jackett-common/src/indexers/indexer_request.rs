use std::collections::HashMap;

use url::Url;

/// Mirror of Jackett.Common/Indexers/IndexerRequest.cs
#[derive(Debug, Clone)]
pub struct IndexerRequest {
    pub url: Url,
    pub method: reqwest::Method,
    pub headers: HashMap<String, String>,
    pub cookies: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub timeout: Option<std::time::Duration>,
}
