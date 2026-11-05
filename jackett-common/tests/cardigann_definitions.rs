//! Integration check: how many of the C# Cardigann YAML definitions the Rust
//! engine can parse and instantiate today. Run from the repo root:
//!
//! ```sh
//! cargo test -p jackett-common --test cardigann_definitions -- --nocapture
//! ```
//!
//! The C# definitions live in `../../src/Jackett.Common/Definitions/*.yml`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use jackett_common::indexers::cardigann::definition::parse_definition;
use jackett_common::indexers::cardigann::engine::{CardigannConfig, CardigannIndexer};

#[test]
fn parse_all_csharp_definitions() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../Definitions");
    let entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("Definitions folder not found; expected at the repository root")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("yml"))
        .collect();

    let mut ok = 0usize;
    let mut parse_fail: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut build_fail: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for path in &entries {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let yaml = match std::fs::read_to_string(path) {
            Ok(y) => y,
            Err(e) => {
                parse_fail.entry("read error".into()).or_default().push(format!("{name}: {e}"));
                continue;
            }
        };
        match parse_definition(&yaml) {
            Err(e) => {
                let reason = format!("{e:#}");
                let key = reason.split('\n').next().unwrap_or("?").to_string();
                parse_fail.entry(key).or_default().push(name);
            }
            Ok(def) => match CardigannIndexer::new(def, CardigannConfig::default()) {
                Ok(_) => ok += 1,
                Err(e) => {
                    let reason = format!("{e:#}");
                    let key = reason.split('\n').next().unwrap_or("?").to_string();
                    build_fail.entry(key).or_default().push(name);
                }
            },
        }
    }

    println!("\n===== Cardigann definition coverage =====");
    println!("total .yml: {}", entries.len());
    println!("loaded OK : {ok}");

    let report = |title: &str, map: &BTreeMap<String, Vec<String>>| {
        let total: usize = map.values().map(|v| v.len()).sum();
        println!("\n--- {title}: {total} ---");
        for (reason, names) in map {
            println!("[{}] {} -> {}", names.len(), reason, names.join(", "));
        }
    };
    report("parse failures", &parse_fail);
    report("build failures", &build_fail);

    // Informational only — coverage is the point, not a hard gate.
    assert!(ok > 0, "engine failed to load any definition");
}
