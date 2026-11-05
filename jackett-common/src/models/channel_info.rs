/// Mirror of Jackett.Common/Models/ChannelInfo.cs
#[derive(Debug, Clone)]
pub struct ChannelInfo {
    pub title: Option<String>,
    pub description: Option<String>,
    pub link: Option<url::Url>,
    pub language: Option<String>,
    pub category: Option<String>,
}

impl Default for ChannelInfo {
    fn default() -> Self {
        // Set default values
        Self {
            title: None,
            description: None,
            link: None,
            language: Some("en-US".to_string()),
            category: Some("search".to_string()),
        }
    }
}

impl ChannelInfo {
    pub fn new() -> Self {
        Self::default()
    }
}
