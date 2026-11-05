/// Mirror of Jackett.Common/ExceptionWithConfigData.cs
#[derive(Debug)]
pub struct ExceptionWithConfigData {
    pub message: String,
    pub config_data: Option<serde_json::Value>,
}

impl ExceptionWithConfigData {
    pub fn new(message: impl Into<String>, config_data: Option<serde_json::Value>) -> Self {
        Self {
            message: message.into(),
            config_data,
        }
    }
}

impl std::fmt::Display for ExceptionWithConfigData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ExceptionWithConfigData {}

/// Mirror of Jackett.Common/IndexerException.cs
#[derive(Debug)]
pub struct IndexerException {
    pub inner: ExceptionWithConfigData,
    /// Indexer id the failure came from.
    pub indexer_id: Option<String>,
}

impl IndexerException {
    pub fn new(inner: ExceptionWithConfigData) -> Self {
        Self {
            inner,
            indexer_id: None,
        }
    }

    pub fn with_indexer(mut self, indexer_id: impl Into<String>) -> Self {
        self.indexer_id = Some(indexer_id.into());
        self
    }

    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            inner: ExceptionWithConfigData::new(message, None),
            indexer_id: None,
        }
    }
}

impl std::fmt::Display for IndexerException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl std::error::Error for IndexerException {}
