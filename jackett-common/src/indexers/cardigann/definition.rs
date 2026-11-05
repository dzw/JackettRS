use once_cell::sync::Lazy;
use regex::Regex;

use crate::models::indexer_definition::IndexerDefinition;

/// Mirror of the CardigannIndexer constructor's defaults/normalization:
/// fills missing mandatory attributes the C# code applied at construction time.
pub fn apply_definition_defaults(mut def: IndexerDefinition) -> IndexerDefinition {
    // Add default settings if necessary
    if def.settings.is_empty() {
        def.settings = vec![
            crate::models::indexer_definition::SettingsField {
                name: Some("username".into()),
                label: Some("Username".into()),
                type_: Some("text".into()),
                ..Default::default()
            },
            crate::models::indexer_definition::SettingsField {
                name: Some("password".into()),
                label: Some("Password".into()),
                type_: Some("password".into()),
                ..Default::default()
            },
        ];
    }

    if def.encoding.is_none() {
        def.encoding = Some("UTF-8".into());
    }

    if let Some(login) = &mut def.login {
        if login.method.is_none() {
            login.method = Some("form".into());
        }
    }

    // convert definitions with a single search Path to a Paths entry
    if let Some(search) = &mut def.search {
        if search.paths.is_empty() {
            if let Some(path) = search.path.clone() {
                search.paths = vec![crate::models::indexer_definition::SearchPathBlock {
                    path: Some(path),
                    inherit_inputs: true,
                    ..Default::default()
                }];
            }
        }
        // ensure the single search path converted above is not duplicated on reload
        if search.path.is_some() && !search.paths.is_empty() {
            search.path = None;
        }
    }

    // DefaultSiteLink must end with "/"
    if let Some(first) = def.links.first() {
        if !first.ends_with('/') {
            def.links[0] = format!("{}/", first);
        }
    }

    def
}

/// Loads and validates a definition from YAML text.
/// Mirror of the YAML parsing + the constructor-time validation.
pub fn parse_definition(yaml: &str) -> anyhow::Result<IndexerDefinition> {
    // Some definitions ship with a UTF-8 BOM, which serde_yaml rejects
    // ("invalid type: string \"--- ...\"); strip it before parsing.
    let yaml = yaml.strip_prefix('\u{feff}').unwrap_or(yaml);
    let def: IndexerDefinition = serde_yaml::from_str(yaml)?;
    validate_definition(&def)?;
    Ok(apply_definition_defaults(def))
}

pub fn validate_definition(def: &IndexerDefinition) -> anyhow::Result<()> {
    if def.id.is_none() {
        anyhow::bail!("Cardigann definition is missing the id");
    }
    if def.name.is_none() {
        anyhow::bail!("Cardigann definition is missing the name");
    }
    if def.type_.is_none() {
        anyhow::bail!("Cardigann definition is missing the type");
    }
    if def.language.is_none() {
        anyhow::bail!("Cardigann definition is missing the language");
    }
    if def.links.is_empty() {
        anyhow::bail!("Cardigann definition is missing the links");
    }
    let caps = def
        .caps
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Cardigann definition is missing the caps"))?;
    if caps.modes.is_empty() {
        anyhow::bail!("At least one search mode is required");
    }
    let search = def
        .search
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Cardigann definition is missing the search block"))?;
    if search.rows.is_none() {
        anyhow::bail!("Cardigann definition search is missing the rows block");
    }
    Ok(())
}

/// Loads a definition from the Definitions folder (one YAML file per tracker).
pub fn load_definition_file(path: &std::path::Path) -> anyhow::Result<IndexerDefinition> {
    let yaml = std::fs::read_to_string(path)?;
    parse_definition(&yaml)
}

/// Static regexes shared by the Cardigann engine (mirror of the C# static fields).
pub static LOGIC_FUNCTION_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"\b(and|or|eq|ne)(?:\s+(\(?\.[^\)\s]+\)?|"[^"]+")){2,}"#,
    )
    .unwrap()
});

/// The C# mirror used `:(?<filter>.+?)\((?<key>.+?)\)(?=:|\z)` with a
/// look-ahead, which the Rust regex crate does not support. The equivalent
/// semantics are reimplemented as a scanner in
/// [`json_selector_segments`](super::parsing::json_selector_segments) /
/// [`json_selector_is_match`](super::parsing::json_selector_is_match).

pub static IF_ELSE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"\{\{\s*if\s*(.+?)\s*\}\}(.*?)\{\{\s*else\s*\}\}(.*?)\{\{\s*end\s*\}\}"#).unwrap());

pub static RANGE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"\{\{\s*range\s*(((?<index>\$.+?),)((\s*(?<element>.+?)\s*(:=)\s*)))?(?<variable>.+?)\s*\}\}(?<prefix>.*?)\{\{\s*\.\s*}}(?<postfix>.*?)\{\{\s*end\s*}}"#,
    )
    .unwrap()
});

pub static VARIABLES_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\{\{\s*(\..+?)\s*\}\}").unwrap());

pub static RE_REPLACE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"\{\{\s*re_replace\s+(\..+?)\s+"(.*?)"\s+"(.*?)"\s*\}\}"#).unwrap());

pub static JOIN_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"\{\{\s*join\s+(\..+?)\s+"(.*?)"\s*\}\}"#).unwrap());

/// Optional field names that never abort row parsing (mirror of `OptionalFields`).
pub const OPTIONAL_FIELDS: [&str; 11] = [
    "imdb",
    "imdbid",
    "tmdbid",
    "rageid",
    "tvdbid",
    "tvmazeid",
    "traktid",
    "doubanid",
    "poster",
    "genre",
    "description",
];
