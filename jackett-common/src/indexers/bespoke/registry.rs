//! Central, env-driven registration of bespoke (hand-coded) indexers.
//!
//! The Rust port has no configuration UI/persistence yet, so each bespoke
//! indexer is enabled by setting environment variables named after its id:
//!
//! * `JACKETT_<ID>_APIKEY` or `JACKETT_<ID>_COOKIE` — API-key / cookie trackers
//! * `JACKETT_<ID>_USERNAME`, `..._PASSWORD`, `..._PID` — Avistaz-family
//! * `JACKETT_<ID>_EMAIL`, `..._PASSWORD` — SpeedApp-family
//! * Optional `JACKETT_<ID>_FREELEECH=1`, `..._USETOKENS=0`
//!
//! `<ID>` is the indexer id uppercased with non-alphanumerics replaced by `_`.
//! Returns the list of indexers to add; the service owns insertion.

use std::collections::HashMap;
use std::sync::Arc;

use crate::indexers::i_indexer::IIndexer;

use super::avistaz::{AvistazConfig, AVISTAZ_SITES};
use super::gazelle::{build_alpharatio, build_redacted, GazelleConfig};
use super::gazelle_sites::{build_gazelle_games, find_spec, GAZELLE_SITES};
use super::public_brazilian::PUBLIC_BRAZILIAN_SITES;
use super::simple::{SimpleConfig, SIMPLE_SPECS};
use super::speedapp::{SpeedAppConfig, SPEEDAPP_SITES};

/// Builds every bespoke indexer that has credentials in the environment.
pub fn build_all_from_env() -> Vec<Arc<dyn IIndexer>> {
    let mut out = Vec::new();
    out.extend(build_gazelle_from_env());
    out.extend(build_avistaz_from_env());
    out.extend(build_speedapp_from_env());
    out.extend(build_couchpotato_from_env());
    out.extend(build_public_sites());
    out.extend(build_simple_from_env());
    out
}

fn env(prefix: &str, suffix: &str) -> Option<String> {
    std::env::var(format!("{prefix}_{suffix}"))
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn env_flag(prefix: &str, suffix: &str, default: bool) -> bool {
    match env(prefix, suffix) {
        Some(v) => matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        None => default,
    }
}

fn prefix_for(id: &str) -> String {
    format!(
        "JACKETT_{}",
        id.to_ascii_uppercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
    )
}

/// Common API-key / cookie credentials.
fn gazelle_credentials(prefix: &str) -> Option<GazelleConfig> {
    let api_key = env(prefix, "APIKEY");
    let cookie = env(prefix, "COOKIE");
    if api_key.is_none() && cookie.is_none() {
        return None;
    }
    Some(GazelleConfig {
        api_key,
        cookie,
        passkey: env(prefix, "PASSKEY"),
        authkey: env(prefix, "AUTHKEY"),
        use_tokens: env_flag(prefix, "USETOKENS", true),
        freeleech_only: env_flag(prefix, "FREELEECH", false),
        freeload_only: env_flag(prefix, "FREELOAD", false),
        ..Default::default()
    })
}

fn push_result(
    out: &mut Vec<Arc<dyn IIndexer>>,
    id: &str,
    result: anyhow::Result<Arc<dyn IIndexer>>,
) {
    match result {
        Ok(idx) => {
            tracing::info!("loaded bespoke indexer {}", idx.id());
            out.push(idx);
        }
        Err(e) => tracing::warn!("failed to build {id} indexer: {e:#}"),
    }
}

/// All Gazelle-family sites (existing builders + data-driven specs + GGn).
fn build_gazelle_from_env() -> Vec<Arc<dyn IIndexer>> {
    let mut out = Vec::new();

    // The two hand-written builders from the original port.
    for (id, builder) in [
        (
            "alpharatio",
            super::gazelle::build_alpharatio as fn(GazelleConfig) -> anyhow::Result<Arc<dyn IIndexer>>,
        ),
        ("redacted", super::gazelle::build_redacted),
    ] {
        let prefix = prefix_for(id);
        if let Some(config) = gazelle_credentials(&prefix) {
            push_result(&mut out, id, builder(config));
        }
    }

    // Data-driven specs (7 sites).
    for spec in GAZELLE_SITES.iter() {
        let prefix = prefix_for(spec.id);
        if let Some(config) = gazelle_credentials(&prefix) {
            push_result(&mut out, spec.id, spec.build(config));
        }
    }

    // GazelleGames (API) uses a dedicated implementation.
    let ggn_prefix = prefix_for("gazellegamesapi");
    if let Some(config) = gazelle_credentials(&ggn_prefix) {
        push_result(&mut out, "gazellegamesapi", build_gazelle_games(config));
    }

    out
}

/// Avistaz-family sites (username / password / pid).
fn build_avistaz_from_env() -> Vec<Arc<dyn IIndexer>> {
    let mut out = Vec::new();
    for spec in AVISTAZ_SITES.iter() {
        let prefix = prefix_for(spec.id);
        let Some(config) = avistaz_credentials(&prefix) else {
            continue;
        };
        push_result(&mut out, spec.id, spec.build(config));
    }
    out
}

pub(crate) fn avistaz_credentials(prefix: &str) -> Option<AvistazConfig> {
    let username = env(prefix, "USERNAME")?;
    let password = env(prefix, "PASSWORD")?;
    let pid = env(prefix, "PID")?;
    Some(AvistazConfig {
        username,
        password,
        pid,
        freeleech: env_flag(prefix, "FREELEECH", false),
    })
}

/// SpeedApp-family sites (email / password).
fn build_speedapp_from_env() -> Vec<Arc<dyn IIndexer>> {
    let mut out = Vec::new();
    for spec in super::speedapp::SPEEDAPP_SITES.iter() {
        let prefix = prefix_for(spec.id);
        let Some(email) = env(&prefix, "EMAIL") else {
            continue;
        };
        let Some(password) = env(&prefix, "PASSWORD") else {
            continue;
        };
        let config = super::speedapp::SpeedAppConfig {
            email,
            password,
            freeleech_only: env_flag(&prefix, "FREELEECH", false),
        };
        push_result(&mut out, spec.id, spec.build(config));
    }
    out
}

/// Generic CouchPotato/TorrentPotato endpoint (no derived classes in C#).
/// Enabled by `JACKETT_COUCHPOTATO_URL`, `..._ENDPOINT`, `..._USERNAME`,
/// `..._PASSKEY`.
fn build_couchpotato_from_env() -> Vec<Arc<dyn IIndexer>> {
    let prefix = "JACKETT_COUCHPOTATO";
    let Some(site_link) = env(prefix, "URL") else {
        return Vec::new();
    };
    let Some(username) = env(prefix, "USERNAME") else {
        return Vec::new();
    };
    let Some(passkey) = env(prefix, "PASSKEY") else {
        return Vec::new();
    };
    let endpoint = env(prefix, "ENDPOINT").unwrap_or_else(|| "api".to_string());
    let config = super::couchpotato::CouchPotatoConfig { username, passkey };
    let mut out = Vec::new();
    push_result(
        &mut out,
        "couchpotato",
        super::couchpotato::build_generic(
            "couchpotato".into(),
            "CouchPotato".into(),
            site_link,
            endpoint,
            config,
        ),
    );
    out
}

/// Public, credential-free indexers (always available, like the C# build).
fn build_public_sites() -> Vec<Arc<dyn IIndexer>> {
    let mut out = Vec::new();
    for spec in super::public_brazilian::PUBLIC_BRAZILIAN_SITES.iter() {
        // Allow opting out via `JACKETT_<ID>_DISABLED=1`.
        if env_flag(&prefix_for(spec.id), "DISABLED", false) {
            continue;
        }
        push_result(&mut out, spec.id, spec.build());
    }
    out
}

/// Data-driven JSON/RSS/API indexers (SubsPlease, Knaben, HDBits, ...).
///
/// Public ones are always enabled unless `JACKETT_<ID>_DISABLED=1`; private ones
/// need the config fields listed in their spec:
/// `JACKETT_<ID>_USERNAME`, `..._PASSWORD`, `..._APIKEY`, `..._APIUSER`,
/// `..._RSSKEY`, `..._PASSKEY`.
fn build_simple_from_env() -> Vec<Arc<dyn IIndexer>> {
    use super::simple::{SimpleConfig, SIMPLE_SPECS};

    let mut out = Vec::new();
    for spec in SIMPLE_SPECS.iter() {
        let prefix = prefix_for(spec.id);
        if env_flag(&prefix, "DISABLED", false) {
            continue;
        }
        let config = SimpleConfig {
            username: env(&prefix, "USERNAME").unwrap_or_default(),
            password: env(&prefix, "PASSWORD").unwrap_or_default(),
            api_key: env(&prefix, "APIKEY").unwrap_or_default(),
            api_user: env(&prefix, "APIUSER").unwrap_or_default(),
            rss_key: env(&prefix, "RSSKEY").unwrap_or_default(),
            passkey: env(&prefix, "PASSKEY").unwrap_or_default(),
        };
        if !config.is_complete(spec.required) {
            if spec.requires_credentials() {
                tracing::debug!(
                    "skipping {}: set {} for JACKETT_{}_*",
                    spec.id,
                    spec.required.join(", "),
                    prefix.trim_start_matches("JACKETT_")
                );
            }
            continue;
        }
        push_result(&mut out, spec.id, spec.build(config));
    }
    out
}

/// Looks up a Gazelle spec by id (used by the API layer for introspection).
pub fn gazelle_spec(id: &str) -> Option<&'static super::gazelle_sites::GazelleSpec> {
    find_spec(id)
}

/// Looks up a simple spec by id (used by the API layer for introspection).
pub fn simple_spec(id: &str) -> Option<&'static super::simple::SimpleSpec> {
    super::simple::find_simple_spec(id)
}

/// Metadata describing an indexer that can be added via the UI/API.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AvailableIndexer {
    pub id: String,
    pub name: String,
    /// `"gazelle" | "avistaz" | "speedapp" | "simple" | "public" | "cardigann"`
    pub kind: String,
    /// Config field names the caller must supply (empty for public trackers).
    pub required_fields: Vec<String>,
    /// Tracker access type as the C# dashboard shows it
    /// (`public` / `private` / `semi-private`).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Top-level category names advertised by the definition.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub site_link: String,
    /// True when a persisted user config exists for this id (C# semantics:
    /// configured indexers disappear from the Add Indexer list).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub configured: bool,
}

/// Lists every indexer definition the port can build on demand. Private trackers
/// that need credentials advertise their required fields; public trackers need
/// none. Mirrors the engine side of `IndexerApiController.AddIndexer`.
pub fn available_indexers() -> Vec<AvailableIndexer> {
    let mut out = Vec::new();
    for spec in PUBLIC_BRAZILIAN_SITES.iter() {
        out.push(AvailableIndexer {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            kind: "public".into(),
            site_link: spec.site_link.to_string(),
            type_: Some("public".into()),
            ..Default::default()
        });
    }
    for spec in SIMPLE_SPECS.iter() {
        out.push(AvailableIndexer {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            kind: "simple".into(),
            required_fields: spec.required.iter().map(|s| s.to_string()).collect(),
            description: Some(spec.description.to_string()),
            type_: Some(spec.indexer_type.to_string()),
            language: Some(spec.language.to_string()),
            site_link: spec.site_link.to_string(),
            ..Default::default()
        });
    }
    for spec in GAZELLE_SITES.iter() {
        out.push(AvailableIndexer {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            kind: "gazelle".into(),
            required_fields: vec!["api_key".into(), "cookie".into()],
            description: Some(spec.description.to_string()),
            type_: Some("private".into()),
            language: Some(spec.language.to_string()),
            site_link: spec.site_link.to_string(),
            ..Default::default()
        });
    }
    for spec in AVISTAZ_SITES.iter() {
        out.push(AvailableIndexer {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            kind: "avistaz".into(),
            required_fields: vec!["username".into(), "password".into(), "pid".into()],
            description: Some(spec.description.to_string()),
            type_: Some("private".into()),
            language: Some(spec.language.to_string()),
            site_link: spec.site_link.to_string(),
            ..Default::default()
        });
    }
    for spec in SPEEDAPP_SITES.iter() {
        out.push(AvailableIndexer {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            kind: "speedapp".into(),
            required_fields: vec!["email".into(), "password".into()],
            description: Some(spec.description.to_string()),
            type_: Some("private".into()),
            language: Some(spec.language.to_string()),
            site_link: spec.site_link.to_string(),
            ..Default::default()
        });
    }
    for (id, name) in [
        ("alpharatio", "AlphaRatio"),
        ("redacted", "Redacted"),
        ("gazellegamesapi", "GazelleGames"),
    ] {
        out.push(AvailableIndexer {
            id: id.into(),
            name: name.into(),
            kind: "gazelle".into(),
            required_fields: vec!["api_key".into(), "cookie".into()],
            type_: Some("private".into()),
            ..Default::default()
        });
    }
    out
}

/// Builds a single indexer from an explicit field map (mirror of the engine
/// side of `AddIndexer`). Returns `None` when the id is unknown.
pub fn build_indexer(id: &str, fields: &HashMap<String, String>) -> Option<Arc<dyn IIndexer>> {
    let get = |k: &str| fields.get(k).cloned().filter(|s| !s.is_empty());

    for spec in PUBLIC_BRAZILIAN_SITES.iter() {
        if spec.id == id {
            return spec.build().ok();
        }
    }
    for spec in SIMPLE_SPECS.iter() {
        if spec.id == id {
            let cfg = SimpleConfig {
                username: get("username").unwrap_or_default(),
                password: get("password").unwrap_or_default(),
                api_key: get("api_key").unwrap_or_default(),
                api_user: get("api_user").unwrap_or_default(),
                rss_key: get("rss_key").unwrap_or_default(),
                passkey: get("passkey").unwrap_or_default(),
            };
            return spec.build(cfg).ok();
        }
    }
    for spec in GAZELLE_SITES.iter() {
        if spec.id == id {
            let cfg = GazelleConfig {
                api_key: get("api_key"),
                cookie: get("cookie"),
                ..Default::default()
            };
            return spec.build(cfg).ok();
        }
    }
    for spec in AVISTAZ_SITES.iter() {
        if spec.id == id {
            let cfg = AvistazConfig {
                username: get("username")?,
                password: get("password")?,
                pid: get("pid")?,
                freeleech: false,
            };
            return spec.build(cfg).ok();
        }
    }
    for spec in SPEEDAPP_SITES.iter() {
        if spec.id == id {
            let cfg = SpeedAppConfig {
                email: get("email")?,
                password: get("password")?,
                freeleech_only: false,
            };
            return spec.build(cfg).ok();
        }
    }
    match id {
        "alpharatio" => {
            build_alpharatio(GazelleConfig { api_key: get("api_key"), ..Default::default() }).ok()
        }
        "redacted" => {
            build_redacted(GazelleConfig { api_key: get("api_key"), ..Default::default() }).ok()
        }
        "gazellegamesapi" => {
            build_gazelle_games(GazelleConfig { api_key: get("api_key"), ..Default::default() }).ok()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_are_normalised() {
        assert_eq!(prefix_for("secret-cinema"), "JACKETT_SECRET_CINEMA");
        assert_eq!(prefix_for("gazellegamesapi"), "JACKETT_GAZELLEGAMESAPI");
        assert_eq!(prefix_for("privatehd"), "JACKETT_PRIVATEHD");
    }

    #[test]
    fn all_gazelle_specs_are_buildable() {
        for spec in GAZELLE_SITES.iter() {
            let cfg = GazelleConfig {
                api_key: Some("test".into()),
                ..Default::default()
            };
            assert!(spec.build(cfg).is_ok(), "failed to build {}", spec.id);
        }
    }

    #[test]
    fn public_indexers_are_registered_once() {
        let ids: Vec<String> = build_all_from_env()
            .iter()
            .map(|i| i.id().to_string())
            .collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(ids.len(), unique.len(), "duplicate indexer ids: {ids:?}");
        assert!(ids.iter().any(|i| i == "subsplease"));
        assert!(ids.iter().any(|i| i == "hdrtorrent"));
    }

    #[test]
    fn available_indexers_lists_known_ids() {
        let ids: Vec<String> = available_indexers().iter().map(|a| a.id.clone()).collect();
        assert!(ids.iter().any(|i| i == "subsplease"));
        assert!(ids.iter().any(|i| i == "alpharatio"));
        assert!(ids.iter().any(|i| i == "avistaz"));
    }

    #[test]
    fn build_indexer_public_needs_no_fields() {
        assert!(build_indexer("subsplease", &HashMap::new()).is_some());
    }
}
