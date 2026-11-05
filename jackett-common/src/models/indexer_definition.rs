use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// Mirror of Jackett.Common/Models/IndexerDefinition.cs (Cardigann YAML classes).
///
/// Cardigann YAML keys are lowercase/camelCase; C# used case-insensitive
/// binding, so every field carries an explicit `rename` where needed
/// (`type` is a Rust keyword, hence `type_`).
/// A list allowing the same key multiple times (C# `KeyValuePairList`).
/// YAML accepts both a map (`name: {selector: ...}`) and a list of
/// `{name: ..., selector: ...}` entries.
pub type KeyValuePairList = Vec<(String, SelectorBlock)>;

fn deserialize_key_value_list<'de, D>(deserializer: D) -> Result<KeyValuePairList, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    #[derive(Deserialize)]
    struct NamedSelector {
        name: String,
        #[serde(flatten)]
        selector: SelectorBlock,
    }

    let value = serde_yaml::Value::deserialize(deserializer)?;
    match value {
        serde_yaml::Value::Mapping(map) => {
            let mut out = Vec::new();
            for (k, v) in map {
                let key = k.as_str().ok_or_else(|| D::Error::custom("field name must be a string"))?;
                let selector: SelectorBlock = serde_yaml::from_value(v).map_err(D::Error::custom)?;
                out.push((key.to_string(), selector));
            }
            Ok(out)
        }
        serde_yaml::Value::Sequence(seq) => {
            let mut out = Vec::new();
            for entry in seq {
                let named: NamedSelector =
                    serde_yaml::from_value(entry).map_err(D::Error::custom)?;
                out.push((named.name, named.selector));
            }
            Ok(out)
        }
        serde_yaml::Value::Null => Ok(Vec::new()),
        other => Err(D::Error::custom(format!(
            "fields must be a map or a list, got: {:?}",
            other
        ))),
    }
}

/// Coerces any YAML scalar (string/int/bool) into a string — Cardigann
/// definitions freely use `perPage: 100`, `disablesort: false` etc. where C#
/// binding produced strings.
pub fn yaml_scalar_to_string(value: serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(s) => Some(s),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// `Option<String>` that tolerates YAML scalars (`default: false`, `default: 0`).
fn deserialize_opt_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_yaml::Value::deserialize(deserializer)?;
    match value {
        serde_yaml::Value::Null => Ok(None),
        other => Ok(yaml_scalar_to_string(other)),
    }
}


/// `HashMap<String, String>` whose keys may be YAML booleans
/// (`True: ...` / `False: ...` in Cardigann `case:` blocks).
fn deserialize_case_map<'de, D>(deserializer: D) -> Result<HashMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    let value = serde_yaml::Value::deserialize(deserializer)?;
    let map = match value {
        serde_yaml::Value::Null => return Ok(HashMap::new()),
        serde_yaml::Value::Mapping(m) => m,
        other => return Err(D::Error::custom(format!("expected a map, got {:?}", other))),
    };
    let mut out = HashMap::new();
    for (k, v) in map {
        let key = yaml_scalar_to_string(k.clone())
            .ok_or_else(|| D::Error::custom(format!("case key must be a scalar, got {:?}", k)))?;
        let val = yaml_scalar_to_string(v.clone())
            .ok_or_else(|| D::Error::custom(format!("case value must be a scalar, got {:?}", v)))?;
        out.insert(key, val);
    }
    Ok(out)
}

fn deserialize_string_map<'de, D>(deserializer: D) -> Result<HashMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    let value = serde_yaml::Value::deserialize(deserializer)?;
    let map = match value {
        serde_yaml::Value::Null => return Ok(HashMap::new()),
        serde_yaml::Value::Mapping(m) => m,
        other => return Err(D::Error::custom(format!("expected a map, got {:?}", other))),
    };
    let mut out = HashMap::new();
    for (k, v) in map {
        let key = k
            .as_str()
            .ok_or_else(|| D::Error::custom("input name must be a string"))?
            .to_string();
        let val = yaml_scalar_to_string(v.clone())
            .ok_or_else(|| D::Error::custom(format!("input value must be a scalar, got {:?}", v)))?;
        out.insert(key, val);
    }
    Ok(out)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexerDefinition {
    pub id: Option<String>,
    #[serde(default)]
    pub replaces: Vec<String>,
    #[serde(default)]
    pub settings: Vec<SettingsField>,
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub language: Option<String>,
    pub encoding: Option<String>,
    #[serde(rename = "requestDelay")]
    pub request_delay: Option<f64>,
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default)]
    pub legacy_links: Vec<String>,
    #[serde(default, rename = "followredirect")]
    pub follow_redirect: bool,
    #[serde(rename = "testlinktorrent", default = "default_true")]
    pub test_link_torrent: bool,
    #[serde(default)]
    pub certificates: Vec<String>,
    pub caps: Option<CapabilitiesBlock>,
    pub login: Option<LoginBlock>,
    pub ratio: Option<RatioBlock>,
    pub search: Option<SearchBlock>,
    pub download: Option<DownloadBlock>,
    // IndexerDefinitionStats not needed/implemented
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SettingsField {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub label: Option<String>,
    #[serde(rename = "default")]
    #[serde(default, deserialize_with = "deserialize_opt_string")]
    pub default: Option<String>,
    #[serde(default)]
    pub defaults: Vec<String>,
    #[serde(default)]
    pub options: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CategorymappingBlock {
    pub id: Option<String>,
    pub cat: Option<String>,
    pub desc: Option<String>,
    #[serde(rename = "Default", default)]
    pub default: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitiesBlock {
    #[serde(default)]
    pub categories: HashMap<String, String>,
    #[serde(default, rename = "categorymappings")]
    pub category_mappings: Vec<CategorymappingBlock>,
    #[serde(default)]
    pub modes: HashMap<String, Vec<String>>,
    #[serde(default, rename = "allowrawsearch")]
    pub allow_raw_search: bool,
    #[serde(default, rename = "allowtvsearchimdb")]
    pub allow_tv_search_imdb: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaptchaBlock {
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub selector: Option<String>,
    pub input: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoginBlock {
    pub path: Option<String>,
    #[serde(rename = "submitpath")]
    pub submit_path: Option<String>,
    #[serde(default)]
    pub cookies: Vec<String>,
    pub method: Option<String>,
    pub form: Option<String>,
    #[serde(default)]
    pub selectors: bool,
    #[serde(default, deserialize_with = "deserialize_string_map")]
    pub inputs: HashMap<String, String>,
    #[serde(default, rename = "selectorinputs")]
    pub selector_inputs: HashMap<String, SelectorBlock>,
    #[serde(default, rename = "getselectorinputs")]
    pub get_selector_inputs: HashMap<String, SelectorBlock>,
    #[serde(default)]
    pub error: Vec<ErrorBlock>,
    #[serde(default)]
    pub test: Option<PageTestBlock>,
    #[serde(default)]
    pub captcha: Option<CaptchaBlock>,
    #[serde(default)]
    pub headers: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ErrorBlock {
    pub path: Option<String>,
    pub selector: Option<String>,
    pub message: Option<SelectorBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SelectorBlock {
    pub selector: Option<String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default, deserialize_with = "deserialize_opt_string")]
    pub default: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string")]
    pub text: Option<String>,
    pub attribute: Option<String>,
    pub remove: Option<String>,
    #[serde(default)]
    pub filters: Vec<FilterBlock>,
    #[serde(default, deserialize_with = "deserialize_case_map")]
    pub case: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FilterBlock {
    pub name: Option<String>,
    /// C# used `dynamic Args`; Cardigann YAML args can be a scalar, list or map.
    #[serde(default)]
    pub args: serde_yaml::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageTestBlock {
    pub path: Option<String>,
    pub selector: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RatioBlock {
    pub path: Option<String>,
    pub selector: Option<String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default, deserialize_with = "deserialize_opt_string")]
    pub default: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string")]
    pub text: Option<String>,
    pub attribute: Option<String>,
    pub remove: Option<String>,
    #[serde(default)]
    pub filters: Vec<FilterBlock>,
    #[serde(default, deserialize_with = "deserialize_case_map")]
    pub case: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchBlock {
    pub path: Option<String>,
    #[serde(default)]
    pub paths: Vec<SearchPathBlock>,
    #[serde(default)]
    pub headers: HashMap<String, Vec<String>>,
    #[serde(default, rename = "keywordsfilters")]
    pub keywords_filters: Vec<FilterBlock>,
    #[serde(rename = "allowEmptyInputs", default)]
    pub allow_empty_inputs: bool,
    #[serde(default, deserialize_with = "deserialize_string_map")]
    pub inputs: HashMap<String, String>,
    #[serde(default)]
    pub error: Vec<ErrorBlock>,
    #[serde(default, rename = "preprocessingfilters")]
    pub preprocessing_filters: Vec<FilterBlock>,
    #[serde(default)]
    pub rows: Option<RowsBlock>,
    #[serde(default, deserialize_with = "deserialize_key_value_list")]
    pub fields: KeyValuePairList,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RowsBlock {
    // selectorBlock fields
    pub selector: Option<String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default, deserialize_with = "deserialize_opt_string")]
    pub default: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string")]
    pub text: Option<String>,
    pub attribute: Option<String>,
    pub remove: Option<String>,
    #[serde(default)]
    pub filters: Vec<FilterBlock>,
    #[serde(default, deserialize_with = "deserialize_case_map")]
    pub case: HashMap<String, String>,
    // rowsBlock fields
    #[serde(default)]
    pub after: i32,
    #[serde(default, rename = "dateheaders")]
    pub date_headers: Option<SelectorBlock>,
    #[serde(default)]
    pub count: Option<SelectorBlock>,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default, rename = "missingAttributeEqualsNoResults")]
    pub missing_attribute_equals_no_results: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchPathBlock {
    // requestBlock fields
    pub path: Option<String>,
    pub method: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_map")]
    pub inputs: HashMap<String, String>,
    #[serde(rename = "queryseparator", default = "default_query_separator")]
    pub query_separator: String,
    // searchPathBlock fields
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default = "default_true")]
    pub inherit_inputs: bool,
    #[serde(default, rename = "followredirect")]
    pub follow_redirect: bool,
    #[serde(default)]
    pub response: Option<ResponseBlock>,
}

fn default_query_separator() -> String {
    "&".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestBlock {
    pub path: Option<String>,
    pub method: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_map")]
    pub inputs: HashMap<String, String>,
    #[serde(rename = "queryseparator", default = "default_query_separator")]
    pub query_separator: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BeforeBlock {
    pub path: Option<String>,
    pub method: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_map")]
    pub inputs: HashMap<String, String>,
    #[serde(rename = "queryseparator", default = "default_query_separator")]
    pub query_separator: String,
    #[serde(rename = "pathselector")]
    pub path_selector: Option<SelectorField>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InfohashBlock {
    pub hash: Option<SelectorField>,
    pub title: Option<SelectorField>,
    #[serde(default, rename = "usebeforeresponse")]
    pub use_before_response: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DownloadBlock {
    #[serde(default)]
    pub selectors: Vec<SelectorField>,
    pub method: Option<String>,
    #[serde(default)]
    pub before: Option<BeforeBlock>,
    #[serde(default)]
    pub infohash: Option<InfohashBlock>,
    #[serde(default)]
    pub headers: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SelectorField {
    pub selector: Option<String>,
    pub attribute: Option<String>,
    #[serde(default, rename = "usebeforeresponse")]
    pub use_before_response: bool,
    #[serde(default)]
    pub filters: Vec<FilterBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResponseBlock {
    #[serde(rename = "type")]
    pub type_: Option<String>,
    #[serde(rename = "noResultsMessage")]
    pub no_results_message: Option<String>,
}
