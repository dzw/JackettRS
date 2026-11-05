use serde::de::DeserializeOwned;
use serde::Serialize;

/// Mirror of Jackett.Common/Utils/JsonUtil.cs
pub struct JsonUtil;

impl JsonUtil {
    /// C# `Serialize` (used Newtonsoft with NullValueHandling.Ignore by default).
    pub fn serialize<T: Serialize>(value: &T) -> String {
        serde_json::to_string(value).unwrap_or_default()
    }

    pub fn deserialize<T: DeserializeOwned>(json: &str) -> Option<T> {
        serde_json::from_str(json).ok()
    }
}
