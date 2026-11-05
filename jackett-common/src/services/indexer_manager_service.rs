use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::indexers::bespoke::registry;
use crate::indexers::bespoke::registry::AvailableIndexer;
use crate::indexers::bespoke::newznab::NewznabIndexer;
use crate::indexers::cardigann::engine::{CardigannConfig, CardigannIndexer};
use crate::indexers::cardigann::parse_definition;
use crate::indexers::i_indexer::IIndexer;
use crate::models::indexer_definition::IndexerDefinition;

/// Mirror of Jackett.Common/Services/IndexerManagerService.cs
///
/// C# registered all indexer factories and dispatched queries by id;
/// here the registry holds boxed trait objects.
pub struct IndexerManagerService {
    indexers: HashMap<String, Arc<dyn IIndexer>>,
    /// Definition metadata backing the available-indexers API
    /// (bespoke specs + Cardigann YAML definitions).
    available: Vec<AvailableIndexer>,
    /// ids that have a persisted user config — C# semantics for "configured":
    /// such indexers disappear from the Add Indexer list.
    configured_ids: HashSet<String>,
    /// Cardigann definition file path per id, for on-demand rebuild when the
    /// user POSTs new settings.
    definition_paths: HashMap<String, PathBuf>,
}

/// Maps a parsed Cardigann definition to the available-indexers metadata row.
fn definition_to_available(def: &IndexerDefinition) -> AvailableIndexer {
    let mut categories: Vec<String> = def
        .caps
        .as_ref()
        .map(|c| c.categories.values().cloned().collect())
        .unwrap_or_default();
    categories.sort();
    categories.dedup();
    AvailableIndexer {
        id: def.id.clone().unwrap_or_default(),
        name: def.name.clone().unwrap_or_default(),
        kind: "cardigann".into(),
        required_fields: def
            .settings
            .iter()
            .filter_map(|s| s.name.clone())
            .collect(),
        type_: def.type_.clone(),
        description: def.description.clone(),
        language: def.language.clone(),
        categories,
        site_link: def.links.first().cloned().unwrap_or_default(),
        configured: false,
    }
}

/// Bespoke specs and Cardigann YAML definitions can cover the same tracker id
/// (two engine implementations for one site). Keep the first occurrence (the
/// bespoke entry, which is also what `registry::build_indexer` prefers) and
/// fill its missing metadata from the later duplicate.
fn merge_available(all: Vec<AvailableIndexer>) -> Vec<AvailableIndexer> {
    let mut out: Vec<AvailableIndexer> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for a in all {
        match index.get(&a.id) {
            Some(&i) => {
                let e = &mut out[i];
                if e.categories.is_empty() {
                    e.categories = a.categories;
                }
                if e.language.is_none() {
                    e.language = a.language;
                }
                if e.description.is_none() {
                    e.description = a.description;
                }
                if e.site_link.is_empty() {
                    e.site_link = a.site_link;
                }
                if e.type_.is_none() {
                    e.type_ = a.type_;
                }
            }
            None => {
                index.insert(a.id.clone(), out.len());
                out.push(a);
            }
        }
    }
    out
}

/// A single editable configuration item, mirroring C#'s `SetupItem` rendered by
/// `WebIndexerApiConfig`. `type` drives which control the UI shows.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigSettingDto {
    pub id: String,
    /// `inputstring` | `inputpassword` | `inputbool` | `inputselect` |
    /// `displayinfo` | `inputtags` — matches C#'s `data-type` attribute.
    #[serde(rename = "type")]
    pub type_: String,
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub options: HashMap<String, String>,
}

/// One capability category row for the "Capabilities" table in the edit modal.
#[derive(Debug, Clone, Serialize)]
pub struct CapCatDto {
    pub id: String,
    pub desc: String,
}

/// C#-style per-indexer configuration payload served by
/// `GET /api/v2.0/indexers/:id/config` and rendered in the edit modal.
#[derive(Debug, Clone, Serialize)]
pub struct IndexerConfigDto {
    pub id: String,
    pub name: String,
    pub link: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub settings: Vec<ConfigSettingDto>,
    pub caps: Vec<CapCatDto>,
}

/// Identity of a persisted indexer config as written by this port's
/// `POST /api/v2.0/indexers` handler (`<data>/Indexers/<id>.json`). Distinct from
/// C#'s on-disk format (a JSON array of setting items) — `parse_indexer_config`
/// tolerates both.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexerUserConfig {
    pub id: String,
    #[serde(default)]
    pub fields: HashMap<String, String>,
}

/// One setting item as serialized by C# Jackett into
/// `<DataFolder>/Indexers/<id>.json` — a JSON array of these.
#[derive(Debug, Clone, Deserialize)]
struct CSharpSetting {
    pub id: String,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
}

/// Parses a persisted indexer config file into `(indexer_id, fields)`, tolerating
/// both formats found on disk:
///   • native: `{"id": "...", "fields": {"k": "v", ...}}`
///   • C# Jackett: a JSON array of `{"id","value"}` setting items, where the
///     indexer id is the file name (e.g. `animetosho.json` -> `animetosho`).
///
/// The C# `value` may be a string, bool or null; it is coerced to a string
/// (`null` -> "", `bool` -> "true"/"false") to fit `fields: HashMap<String,String>`.
/// Returns `None` when the file matches neither format (caller should skip + warn).
pub fn parse_indexer_config(
    path: &Path,
    text: &str,
) -> Option<(String, HashMap<String, String>)> {
    // Native format first.
    if let Ok(cfg) = serde_json::from_str::<IndexerUserConfig>(text) {
        if !cfg.id.is_empty() {
            return Some((cfg.id, cfg.fields));
        }
    }
    // C# format: array of setting items; the indexer id is the file name.
    if let Ok(items) = serde_json::from_str::<Vec<CSharpSetting>>(text) {
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if !stem.is_empty() && !items.is_empty() {
                let fields: HashMap<String, String> = items
                    .iter()
                    .map(|s| {
                        let v = match &s.value {
                            Some(serde_json::Value::String(st)) => st.clone(),
                            Some(serde_json::Value::Bool(b)) => b.to_string(),
                            Some(other) => other.to_string(),
                            None => String::new(),
                        };
                        (s.id.clone(), v)
                    })
                    .collect();
                return Some((stem.to_string(), fields));
            }
        }
    }
    None
}

impl IndexerManagerService {
    pub fn new() -> Self {
        Self {
            indexers: HashMap::new(),
            available: Vec::new(),
            configured_ids: HashSet::new(),
            definition_paths: HashMap::new(),
        }
    }

    /// Mirror of `AddIndexer`.
    pub fn add_indexer(&mut self, indexer: Arc<dyn IIndexer>) {
        self.indexers.insert(indexer.id().to_string(), indexer);
    }

    /// Marks `id` as having a persisted user config.
    pub fn mark_configured(&mut self, id: &str) {
        self.configured_ids.insert(id.to_string());
    }

    /// Clears the persisted-config marker (config file was deleted).
    pub fn unmark_configured(&mut self, id: &str) {
        self.configured_ids.remove(id);
    }

    /// Whether a Cardigann YAML definition exists for `id`.
    pub fn has_definition(&self, id: &str) -> bool {
        self.definition_paths.contains_key(id)
    }

    /// Available-indexers metadata (bespoke + Cardigann definitions) with the
    /// configured flag resolved from the persisted-config set.
    pub fn available(&self) -> Vec<AvailableIndexer> {
        self.available
            .iter()
            .map(|a| {
                let mut a = a.clone();
                a.configured = self.configured_ids.contains(&a.id);
                a
            })
            .collect()
    }

    /// Indexers the user has actually configured (added via the UI / persisted
    /// `*.json` config). Mirrors C#'s "Configured Indexers" dashboard tab: the
    /// in-memory registry holds every Cardigann definition loaded at startup,
    /// but only configured ones belong in this list.
    pub fn configured_indexers(&self) -> Vec<Arc<dyn IIndexer>> {
        self.indexers
            .iter()
            .filter(|(id, _)| self.configured_ids.contains(id.as_str()))
            .map(|(_, i)| i.clone())
            .collect()
    }

    /// Rebuilds a Cardigann indexer from its YAML definition with the given
    /// user settings, replacing the previous instance. Returns false when no
    /// definition (or a broken one) exists for `id`.
    pub fn rebuild_cardigann(&mut self, id: &str, fields: &HashMap<String, String>) -> bool {
        let Some(path) = self.definition_paths.get(id).cloned() else {
            return false;
        };
        let Ok(yaml) = std::fs::read_to_string(&path) else {
            tracing::warn!("rebuild {}: cannot read definition file", id);
            return false;
        };
        let def = match parse_definition(&yaml) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("rebuild {}: parse error: {e:#}", id);
                return false;
            }
        };
        let mut cfg = CardigannConfig::default();
        cfg.settings = fields.clone();
        match CardigannIndexer::new(def, cfg) {
            Ok(idx) => {
                self.add_indexer(Arc::new(idx));
                true
            }
            Err(e) => {
                tracing::warn!("rebuild {}: build error: {e:#}", id);
                false
            }
        }
    }

    /// Mirror of `RemoveIndexer` — drops the indexer from the in-memory registry.
    pub fn remove_indexer(&mut self, id: &str) {
        self.indexers.remove(id);
    }

    /// Mirror of `Init` (Cardigann branch): scan the Definitions folder, parse
    /// every `*.yml`, build a `CardigannIndexer` and register it. Failures on a
    /// single definition are logged and skipped (C# does the same so a bad
    /// tracker can't take down the whole server). `config_dir` holds the
    /// persisted per-indexer `*.json` user configs used to mark ids configured.
    pub fn init(&mut self, definitions_dir: &Path, config_dir: &Path) -> anyhow::Result<usize> {
        // Persisted user configs decide which definitions show as configured
        // (and therefore hidden from the Add Indexer list).
        if let Ok(entries) = std::fs::read_dir(config_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if let Some((id, _)) = parse_indexer_config(&path, &text) {
                        self.configured_ids.insert(id);
                    }
                }
            }
        }

        // Bespoke indexers do not depend on the YAML definitions folder, so
        // register them first — an absent Definitions directory must not hide
        // every hand-coded indexer from the API.
        let mut loaded = self.register_bespoke();
        self.available.extend(registry::available_indexers());
        if !definitions_dir.exists() {
            tracing::warn!(
                "Definitions directory {:?} does not exist; only bespoke indexers loaded",
                definitions_dir
            );
            return Ok(loaded);
        }
        for entry in std::fs::read_dir(definitions_dir)? {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("read_dir error: {e}");
                    continue;
                }
            };
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("yml") {
                continue;
            }
            let yaml = match std::fs::read_to_string(&path) {
                Ok(y) => y,
                Err(e) => {
                    tracing::warn!("skip {}: read error: {e}", path.display());
                    continue;
                }
            };
            let def = match parse_definition(&yaml) {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("skip {}: parse error: {e:#}", path.display());
                    continue;
                }
            };
            if let Some(id) = def.id.clone() {
                self.definition_paths.insert(id, path.clone());
                self.available.push(definition_to_available(&def));
            }
            match CardigannIndexer::new(def, CardigannConfig::default()) {
                Ok(idx) => {
                    // Top-level category names from the resolved capabilities —
                    // the YAML caps block alone misses categorymappings.
                    if let Some(row) = self.available.iter_mut().rev().find(|r| r.id == idx.id()) {
                        if row.categories.is_empty() {
                            let mut cats: Vec<String> = idx
                                .torznab_caps()
                                .categories
                                .get_torznab_category_tree(false)
                                .into_iter()
                                .filter_map(|c| c.name.clone())
                                .collect();
                            cats.sort();
                            cats.dedup();
                            row.categories = cats;
                        }
                    }
                    tracing::debug!("loaded indexer {}", idx.id());
                    self.add_indexer(Arc::new(idx));
                    loaded += 1;
                }
                Err(e) => {
                    tracing::warn!("skip {}: build error: {e:#}", path.display());
                }
            }
        }
        self.available = merge_available(std::mem::take(&mut self.available));
        tracing::info!("IndexerManagerService initialised with {loaded} indexers");
        Ok(loaded)
    }

    /// Registers the configured bespoke indexers. The generic Newznab proxy is
    /// enabled by `JACKETT_NEWZNAB_URL`; all other coded indexers are handled by
    /// [`registry::build_all_from_env`]. Returns the number added.
    fn register_bespoke(&mut self) -> usize {
        let mut added = 0usize;

        for idx in registry::build_all_from_env() {
            tracing::info!("loaded bespoke indexer {}", idx.id());
            self.add_indexer(idx);
            added += 1;
        }

        if let Ok(url) = std::env::var("JACKETT_NEWZNAB_URL") {
            if !url.trim().is_empty() {
                let id =
                    std::env::var("JACKETT_NEWZNAB_ID").unwrap_or_else(|_| "newznab".to_string());
                let name = std::env::var("JACKETT_NEWZNAB_NAME")
                    .unwrap_or_else(|_| "Newznab".to_string());
                let apikey = std::env::var("JACKETT_NEWZNAB_APIKEY").unwrap_or_default();
                let cats = std::env::var("JACKETT_NEWZNAB_CATS")
                    .ok()
                    .and_then(|s| {
                        let v: Vec<i32> =
                            s.split(',').filter_map(|c| c.trim().parse().ok()).collect();
                        if v.is_empty() {
                            None
                        } else {
                            Some(v)
                        }
                    })
                    .unwrap_or_else(|| {
                        vec![2000, 5000, 1000, 3000, 4000, 6000, 7000, 8000]
                    });
                let lang =
                    std::env::var("JACKETT_NEWZNAB_LANG").unwrap_or_else(|_| "en".to_string());
                match NewznabIndexer::new(&id, &name, &url, &apikey, &cats, &lang) {
                    Ok(idx) => {
                        tracing::info!("loaded bespoke indexer {}", idx.id());
                        self.add_indexer(idx);
                        added += 1;
                    }
                    Err(e) => tracing::warn!("failed to build Newznab indexer: {e:#}"),
                }
            }
        }

        added
    }

    pub fn get_indexer(&self, id: &str) -> Option<Arc<dyn IIndexer>> {
        self.indexers.get(id).cloned()
    }

    pub fn get_all_indexers(&self) -> Vec<Arc<dyn IIndexer>> {
        self.indexers.values().cloned().collect()
    }

    pub fn indexer_count(&self) -> usize {
        self.indexers.len()
    }

    /// Mirror of `TestIndexer` — runs a basic query and reports the outcome.
    pub async fn test_indexer(&self, id: &str) -> Result<usize, anyhow::Error> {
        let indexer = self
            .get_indexer(id)
            .ok_or_else(|| anyhow::anyhow!("Unknown indexer: {}", id))?;
        let query = crate::models::torznab_query::TorznabQuery {
            query_type: Some("search".to_string()),
            search_term: Some(String::new()),
            is_test: true,
            ..Default::default()
        };
        let results = indexer.query(&query).await?;
        Ok(results.len())
    }

    /// Returns the editable configuration schema for `id`, mirroring
    /// `IndexerApiController.Config`. Cardigann definitions yield the full
    /// settings list with current values plus a capabilities table; bespoke
    /// indexers (no YAML definition) return only name/link/caps with an empty
    /// settings list (they are env-driven and not UI-editable).
    pub fn indexer_config(&self, id: &str, config_dir: &Path) -> Option<IndexerConfigDto> {
        // Current persisted field values, if a config file exists for this id.
        let mut current = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(config_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&p) {
                    if let Some((file_id, fields)) = parse_indexer_config(&p, &text) {
                        if file_id == id {
                            current = fields;
                            break;
                        }
                    }
                }
            }
        }

        // Cardigann definition path.
        if let Some(path) = self.definition_paths.get(id) {
            let yaml = std::fs::read_to_string(path).ok()?;
            let def = parse_definition(&yaml).ok()?;
            let settings: Vec<ConfigSettingDto> = def
                .settings
                .iter()
                .map(|s| {
                    let name = s.name.clone().unwrap_or_default();
                    let type_ = match s.type_.as_deref() {
                        Some("checkbox") => "inputbool",
                        Some("select") => "inputselect",
                        Some("info") => "displayinfo",
                        Some("tags") => "inputtags",
                        Some("password") => "inputpassword",
                        _ => "inputstring",
                    };
                    let value = current
                        .get(&name)
                        .cloned()
                        .or_else(|| s.default.clone())
                        .unwrap_or_default();
                    ConfigSettingDto {
                        id: name.clone(),
                        type_: type_.into(),
                        label: s.label.clone().unwrap_or_else(|| name.clone()),
                        value,
                        options: s.options.clone(),
                    }
                })
                .collect();
            let mut caps: Vec<CapCatDto> = Vec::new();
            if let Some(c) = &def.caps {
                for (k, v) in c.categories.iter() {
                    caps.push(CapCatDto { id: k.clone(), desc: v.clone() });
                }
                for m in c.category_mappings.iter() {
                    if let (Some(mid), Some(desc)) = (&m.id, &m.desc) {
                        if !caps.iter().any(|x| x.id == *mid) {
                            caps.push(CapCatDto { id: mid.clone(), desc: desc.clone() });
                        }
                    }
                }
            }
            return Some(IndexerConfigDto {
                id: id.to_string(),
                name: def.name.clone().unwrap_or_else(|| id.to_string()),
                link: def.links.first().cloned().unwrap_or_default(),
                description: def.description.clone(),
                settings,
                caps,
            });
        }

        // Bespoke / non-Cardigann indexer: capabilities from the live instance.
        let idx = self.get_indexer(id)?;
        let caps = idx
            .torznab_caps()
            .categories
            .get_torznab_category_tree(false)
            .into_iter()
            .filter_map(|c| {
                c.name
                    .clone()
                    .map(|n| CapCatDto { id: c.id.to_string(), desc: n })
            })
            .collect();
        Some(IndexerConfigDto {
            id: id.to_string(),
            name: idx.name().to_string(),
            link: String::new(),
            description: None,
            settings: Vec::new(),
            caps,
        })
    }
}

impl Default for IndexerManagerService {
    fn default() -> Self {
        Self::new()
    }
}
