use serde::{Deserialize, Serialize};

/// Mirror of Jackett.Common/Models/TorznabCategory.cs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorznabCategory {
    pub id: i32,
    pub name: Option<String>,
    #[serde(default)]
    pub sub_categories: Vec<TorznabCategory>,
}

impl TorznabCategory {
    pub fn new(id: i32, name: impl Into<String>) -> Self {
        Self {
            id,
            name: Some(name.into()),
            sub_categories: Vec::new(),
        }
    }

    pub fn contains(&self, cat: &TorznabCategory) -> bool {
        self.id == cat.id || self.sub_categories.iter().any(|c| c == cat)
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "ID": self.id,
            "Name": self.name,
        })
    }

    pub fn copy_without_sub_categories(&self) -> TorznabCategory {
        TorznabCategory {
            id: self.id,
            name: self.name.clone(),
            sub_categories: Vec::new(),
        }
    }
}

/// Equality mirrors C#: compares by ID only.
impl PartialEq<i32> for TorznabCategory {
    fn eq(&self, other: &i32) -> bool {
        self.id == *other
    }
}
