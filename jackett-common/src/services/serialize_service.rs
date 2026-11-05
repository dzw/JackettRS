/// Mirror of Jackett.Common/Services/SerializeService.cs
pub struct SerializeService;

impl SerializeService {
    pub fn to_json<T: serde::Serialize>(value: &T) -> String {
        serde_json::to_string(value).unwrap_or_default()
    }

    pub fn from_json<T: serde::de::DeserializeOwned>(json: &str) -> Option<T> {
        serde_json::from_str(json).ok()
    }

    pub fn to_yaml<T: serde::Serialize>(value: &T) -> String {
        serde_yaml::to_string(value).unwrap_or_default()
    }

    pub fn from_yaml<T: serde::de::DeserializeOwned>(yaml: &str) -> Option<T> {
        serde_yaml::from_str(yaml).ok()
    }
}
