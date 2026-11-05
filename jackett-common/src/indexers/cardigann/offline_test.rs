//! Offline end-to-end test for the Cardigann engine.
//!
//! Drives a real definition through the full flow (template/URL building ->
//! response parsing) using an injected sample HTML page, proving that a
//! `CardigannIndexer` turns a definition + HTML into `ReleaseInfo` results
//! without any network access.

use super::engine::{CardigannConfig, CardigannIndexer};
use crate::indexers::cardigann::parse_definition;
use crate::models::torznab_query::TorznabQuery;

const DEF: &str = r#"
id: test-offline
name: Test Offline
description: offline end-to-end test definition
language: en-US
type: public
encoding: UTF-8
links:
  - https://example.org/
caps:
  modes:
    search: [q]
  categorymappings:
    - id: "1"
      cat: "5000"
      desc: "Movies"
search:
  paths:
    - path: /search?q={{ .Query.Keywords }}
  rows:
    selector: tr.info
  fields:
    title:
      selector: a.name
    details:
      selector: a.name
      attribute: href
    download:
      selector: a.dl
      attribute: href
    size:
      selector: td.size
    seeders:
      selector: td.seeders
    leechers:
      selector: td.leechers
    date:
      selector: td.date
"#;

const HTML: &str = r#"
<table>
  <tr class="info">
    <td><a class="name" href="/torrent/123/x.html">Big Buck Bunny 2008</a></td>
    <td class="size">1.5 GB</td>
    <td class="seeders">42</td>
    <td class="leechers">3</td>
    <td class="date">Wed, 15 Jan 2020 12:00:00 +0000</td>
    <td><a class="dl" href="/download/123.torrent">DL</a></td>
  </tr>
  <tr class="info">
    <td><a class="name" href="/torrent/456/y.html">Sample Movie 2010</a></td>
    <td class="size">700 MB</td>
    <td class="seeders">7</td>
    <td class="leechers">1</td>
    <td class="date">Wed, 16 Jan 2020 12:00:00 +0000</td>
    <td><a class="dl" href="/download/456.torrent">DL</a></td>
  </tr>
</table>"#;

#[test]
fn offline_search_end_to_end() {
    let def = parse_definition(DEF).expect("definition should parse");
    let indexer =
        CardigannIndexer::new(def, CardigannConfig::default()).expect("indexer should build");

    let mut query = TorznabQuery::new();
    query.search_term = Some("big buck bunny".into());

    let releases = indexer
        .perform_query_offline(&query, &[HTML])
        .expect("offline search should succeed");

    assert_eq!(releases.len(), 2, "expected two result rows");

    let first = &releases[0];
    assert_eq!(first.title.as_deref(), Some("Big Buck Bunny 2008"));
    assert_eq!(first.size, Some(1_500_000_000), "1.5 GB -> bytes");
    assert_eq!(first.seeders, Some(42));
    assert_eq!(first.peers, Some(45), "peers = seeders + leechers");
    assert!(
        first
            .details
            .as_ref()
            .unwrap()
            .as_str()
            .ends_with("/torrent/123/x.html"),
        "details URL resolved against site link"
    );
    assert!(
        first
            .link
            .as_ref()
            .unwrap()
            .as_str()
            .ends_with("/download/123.torrent"),
        "download URL resolved against site link"
    );
    assert!(first.publish_date.is_some(), "date parsed via from_unknown");
    assert_eq!(first.origin.as_deref(), Some("test-offline"));

    let second = &releases[1];
    assert_eq!(second.title.as_deref(), Some("Sample Movie 2010"));
    assert_eq!(second.seeders, Some(7));
    assert_eq!(second.size, Some(700_000_000));
}
