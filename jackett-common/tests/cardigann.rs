//! Integration tests for the Cardigann engine using the real YAML definitions
//! shipped in `Definitions/` at the repository root.

use jackett_common::indexers::cardigann::{parse_definition, template};
use jackett_common::models::torznab_query::TorznabQuery;

fn repo_definitions() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("Definitions")
}

/// Parses every shipped definition and validates structure.
/// This mirrors what the C# IndexerManagerService does at startup when it
/// registers all Cardigann indexers.
#[test]
fn parses_all_shipped_definitions() {
    let dir = repo_definitions();
    let files: Vec<_> = std::fs::read_dir(&dir)
        .expect("Definitions folder present")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "yml").unwrap_or(false))
        .collect();
    assert!(files.len() > 100, "expected many definitions, got {}", files.len());

    let mut parsed = 0;
    for file in &files {
        let path = file.path();
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|yaml| parse_definition(&yaml).map_err(|e| e.to_string()))
        {
            Ok(def) => {
                assert!(def.id.is_some(), "{} missing id", path.display());
                parsed += 1;
            }
            Err(e) => panic!("failed to parse {}: {:#}", path.display(), e),
        }
    }
    assert_eq!(parsed, files.len());
}

/// Checks the Cardigann search modes of 1337x produce expected capabilities.
#[test]
fn parses_caps_and_categories() {
    let yaml = std::fs::read_to_string(repo_definitions().join("1337x.yml")).unwrap();
    let def = parse_definition(&yaml).unwrap();

    assert_eq!(def.id.as_deref(), Some("1337x"));
    assert!(def.links.first().unwrap().ends_with('/'));

    let caps = def.caps.as_ref().unwrap();
    assert!(!caps.modes.is_empty(), "modes required");
    assert!(!caps.category_mappings.is_empty(), "categorymappings required");
}

/// Smoke-tests the Go template engine used by the Cardigann definitions.
#[test]
fn template_variables_and_logic() {
    use std::collections::HashMap;

    let mut vars: template::TemplateVars = HashMap::new();
    vars.insert(".Query.Keywords".into(), "big buck bunny".into());
    vars.insert(".Config.sitelink".into(), "https://example.org/".into());
    vars.insert(".Query.IsTVSearch".into(), template::TemplateValue::Str(Some("True".into())));

    // simple variable
    assert_eq!(
        template::apply_go_template_text("{{ .Query.Keywords }}", Some(&vars), None),
        "big buck bunny"
    );

    // re_replace
    assert_eq!(
        template::apply_go_template_text(
            r#"{{ re_replace .Query.Keywords "[^a-zA-Z0-9]+" "%" }}"#,
            Some(&vars),
            None
        ),
        "big%buck%bunny"
    );

    // if/else
    assert_eq!(
        template::apply_go_template_text(
            "{{ if .Query.IsTVSearch }}tv{{ else }}all{{ end }}",
            Some(&vars),
            None
        ),
        "tv"
    );

    // range
    vars.insert(".Categories".into(), template::TemplateValue::List(vec!["1".into(), "2".into()]));
    assert_eq!(
        template::apply_go_template_text("{{ range .Categories }}&cat={{.}}{{ end }}", Some(&vars), None),
        "&cat=1&cat=2"
    );
}

/// A full query run against an embedded sample HTML result page, without
/// network access, exercising handleSelector + ParseFields end-to-end.
#[test]
fn parses_search_results_from_sample_html() {
    use jackett_common::indexers::cardigann::parsing::{build_query_variables, handle_selector, parse_fields, FieldContext};
    use jackett_common::models::indexer_definition::SearchBlock;
    use std::collections::HashMap;

    let html = r#"
    <html><body>
      <table>
        <tr class="info">
          <td><a class="name" href="/torrent/123/x.html">Big Buck Bunny 2008</a></td>
          <td class="size"><span>1.5 GB</span></td>
          <td class="seeders">42</td>
          <td class="leechers">3</td>
          <td><a href="magnet:?xt=urn:btih:ABCDEF">magnet</a></td>
        </tr>
      </table>
    </body></html>"#;

    let doc = scraper::Html::parse_document(html);
    let row_css = scraper::Selector::parse("tr.info").unwrap();
    let row = doc.select(&row_css).next().unwrap();

    // title selector
    let title_sel = serde_yaml::from_str::<jackett_common::models::indexer_definition::SelectorBlock>(
        "selector: 'a.name'\nattribute: href",
    )
    .unwrap();
    let title_value = handle_selector(&title_sel, row, None, true).unwrap().unwrap();
    assert_eq!(title_value, "/torrent/123/x.html");

    // seeders selector (text)
    let seeders_sel = serde_yaml::from_str::<jackett_common::models::indexer_definition::SelectorBlock>(
        "selector: '.seeders'",
    )
    .unwrap();
    let seeders = handle_selector(&seeders_sel, row, None, true).unwrap().unwrap();
    assert_eq!(seeders, "42");

    // size filter chain: parse "1.5 GB" into bytes via the size field
    let mut release = jackett_common::models::release_info::ReleaseInfo::new();
    let base_url = url::Url::parse("https://example.org/search/1/").unwrap();
    let ctx = FieldContext { site_link: &base_url, base_url: &base_url };
    parse_fields("1.5 GB", "size", &[], &mut release, &ctx);
    assert_eq!(release.size, Some(1_500_000_000));

    // seeders field updates peers too
    parse_fields(&seeders, "seeders", &[], &mut release, &ctx);
    assert_eq!(release.seeders, Some(42));
    assert_eq!(release.peers, Some(42));

    let _ = build_query_variables(&TorznabQuery::new(), Default::default());
    let _: HashMap<String, String> = HashMap::new();
    let _ = SearchBlock::default();
}
