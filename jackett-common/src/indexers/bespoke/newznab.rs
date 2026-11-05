//! Mirror of Jackett.Common/Indexers/Feeds/BaseNewznabIndexer.cs — a generic
//! Torznab/Newznab API proxy. A single coded indexer *type* whose instances are
//! configured with an external site URL + apikey (here supplied via config/env).
//! It forwards the Torznab query to the remote `api` endpoint and parses the
//! returned Torznab RSS feed into `ReleaseInfo`.

use async_trait::async_trait;
use std::sync::Arc;
use url::Url;

use crate::indexers::i_indexer::IIndexer;
use crate::models::release_info::ReleaseInfo;
use crate::models::torznab_capabilities::{
    BookSearchParam, MovieSearchParam, MusicSearchParam, TorznabCapabilities, TvSearchParam,
};
use crate::models::torznab_cat_type::TorznabCatType;
use crate::models::torznab_category::TorznabCategory;
use crate::models::torznab_query::TorznabQuery;
use crate::utils::browser_util::BrowserUtil;
use crate::utils::date_time_util::DateTimeUtil;

/// A configured Newznab/Torznab API proxy indexer.
pub struct NewznabIndexer {
    id: String,
    name: String,
    description: String,
    site_link: String,
    apikey: String,
    language: String,
    caps: TorznabCapabilities,
    client: reqwest::Client,
}

impl NewznabIndexer {
    /// Builds a configured Newznab indexer. `categories` are the standard
    /// Torznab category ids the remote supports (e.g. 2000 Movies, 5000 TV).
    pub fn new(
        id: &str,
        name: &str,
        site_link: &str,
        apikey: &str,
        categories: &[i32],
        language: &str,
    ) -> anyhow::Result<Arc<dyn IIndexer>> {
        let client = reqwest::Client::builder()
            .user_agent(BrowserUtil::default_user_agent())
            .build()?;
        let caps = build_caps(categories);
        Ok(Arc::new(Self {
            id: id.to_string(),
            name: name.to_string(),
            description: format!("Newznab indexer for {}", site_link),
            site_link: site_link.to_string(),
            apikey: apikey.to_string(),
            language: language.to_string(),
            caps,
            client,
        }))
    }

    /// Forwards the query to the remote Torznab API and parses the response.
    async fn run_query(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let url = self.build_url(query)?;
        let response = self.client.get(url).send().await?;
        let body = response.text().await?;
        let mut releases = parse_torznab_rss(&body)?;
        for r in releases.iter_mut() {
            r.origin = Some(self.id.clone());
        }
        Ok(releases)
    }

    /// Builds the remote Torznab API request URL.
    fn build_url(&self, query: &TorznabQuery) -> anyhow::Result<Url> {
        let t = match query.query_type.as_deref() {
            Some("tvsearch") => "tvsearch",
            Some("movie") => "movie",
            Some("music") => "music",
            Some("book") => "book",
            _ => "search",
        };
        let base = format!("{}/api", self.site_link.trim_end_matches('/'));
        let mut url = Url::parse(&base)?;

        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("t", t);
            if let Some(q) = &query.search_term {
                if !q.trim().is_empty() {
                    pairs.append_pair("q", q);
                }
            }
            if !query.categories.is_empty() {
                let cat = query
                    .categories
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                pairs.append_pair("cat", &cat);
            }
            if query.limit > 0 {
                pairs.append_pair("limit", &query.limit.to_string());
            }
            if query.offset > 0 {
                pairs.append_pair("offset", &query.offset.to_string());
            }
            if let Some(imdb) = &query.imdb_id {
                pairs.append_pair("imdbid", imdb);
            }
            if let Some(tvdb) = query.tvdb_id {
                pairs.append_pair("tvdbid", &tvdb.to_string());
            }
            if let Some(tmdb) = query.tmdb_id {
                pairs.append_pair("tmdbid", &tmdb.to_string());
            }
            if let Some(rid) = query.rage_id {
                pairs.append_pair("rid", &rid.to_string());
            }
            if let Some(season) = query.season {
                pairs.append_pair("season", &season.to_string());
            }
            if let Some(ep) = &query.episode {
                pairs.append_pair("ep", ep);
            }
            if let Some(year) = query.year {
                pairs.append_pair("year", &year.to_string());
            }
            if let Some(genre) = &query.genre {
                pairs.append_pair("genre", genre);
            }
            pairs.append_pair("extended", "1");
            pairs.append_pair("apikey", &self.apikey);
        }

        Ok(url)
    }
}

#[async_trait]
impl IIndexer for NewznabIndexer {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn type_(&self) -> &str {
        "newznab"
    }
    fn site_link(&self) -> &str {
        &self.site_link
    }
    fn alternative_site_links(&self) -> Vec<String> {
        Vec::new()
    }
    fn language(&self) -> &str {
        &self.language
    }
    fn tags(&self) -> Vec<String> {
        Vec::new()
    }
    fn is_configured(&self) -> bool {
        !self.apikey.is_empty()
    }
    fn last_error(&self) -> Option<String> {
        None
    }
    fn torznab_caps(&self) -> &TorznabCapabilities {
        &self.caps
    }
    async fn query(
        &self,
        query: &TorznabQuery,
    ) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        self.run_query(query).await
    }
    async fn download(&self, link: &Url) -> Result<Vec<u8>, anyhow::Error> {
        let response = self.client.get(link.clone()).send().await?;
        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }
}

/// Builds a `TorznabCapabilities` from the supported category ids. Newznab maps
/// tracker categories 1:1 onto standard Torznab categories.
fn build_caps(categories: &[i32]) -> TorznabCapabilities {
    let mut caps = TorznabCapabilities::new();
    caps.search_available = true;
    caps.supports_raw_search = true;
    caps.tv_search_params = vec![
        TvSearchParam::Q,
        TvSearchParam::Season,
        TvSearchParam::Ep,
        TvSearchParam::ImdbId,
        TvSearchParam::TvdbId,
        TvSearchParam::Year,
        TvSearchParam::Genre,
    ];
    caps.movie_search_params = vec![
        MovieSearchParam::Q,
        MovieSearchParam::ImdbId,
        MovieSearchParam::TmdbId,
        MovieSearchParam::TraktId,
        MovieSearchParam::DoubanId,
    ];
    caps.music_search_params = vec![
        MusicSearchParam::Q,
        MusicSearchParam::Album,
        MusicSearchParam::Artist,
        MusicSearchParam::Label,
    ];
    caps.book_search_params = vec![BookSearchParam::Q, BookSearchParam::Author, BookSearchParam::Title];
    for &id in categories {
        let name = TorznabCatType::get_cat_desc(id);
        let cat = TorznabCategory::new(id, name);
        caps.categories.add_category_mapping_int(id, cat, None);
    }
    caps
}

/// Parses a Torznab (Newznab) RSS response into `ReleaseInfo`s. The structured
/// fields are carried in `<torznab:attr name="..." value="..."/>` elements; the
/// free-text fields live in the usual `<item>` child elements.
pub fn parse_torznab_rss(xml: &str) -> anyhow::Result<Vec<ReleaseInfo>> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);

    let mut releases: Vec<ReleaseInfo> = Vec::new();
    let mut current: Option<ReleaseInfo> = None;
    let mut buf = Vec::new();
    let mut text = String::new();

    loop {
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                if name == "item" {
                    current = Some(ReleaseInfo::new());
                }
                text.clear();

                if name.ends_with("attr") {
                    let mut attr_name: Option<String> = None;
                    let mut attr_value: Option<String> = None;
                    for a in e.attributes().flatten() {
                        let an = String::from_utf8_lossy(a.key.as_ref()).to_lowercase();
                        let av = a
                            .unescape_value()
                            .map(|v| v.to_string())
                            .unwrap_or_default();
                        if an == "name" {
                            attr_name = Some(av);
                        } else if an == "value" {
                            attr_value = Some(av);
                        }
                    }
                    if let (Some(an), Some(av), Some(r)) =
                        (attr_name, attr_value, current.as_mut())
                    {
                        apply_attr(r, &an, &av);
                    }
                } else if name == "enclosure" {
                    for a in e.attributes().flatten() {
                        let an = String::from_utf8_lossy(a.key.as_ref()).to_lowercase();
                        if an == "url" {
                            if let Some(r) = current.as_mut() {
                                if r.link.is_none() {
                                    r.link = a
                                        .unescape_value()
                                        .ok()
                                        .and_then(|v| Url::parse(&v).ok());
                                }
                            }
                        }
                    }
                }
            }
            Ok(Event::Text(e)) => {
                if let Ok(t) = e.unescape() {
                    text.push_str(&t);
                }
            }
            Ok(Event::CData(e)) => {
                text.push_str(&String::from_utf8_lossy(e.as_ref()));
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                if name == "item" {
                    if let Some(r) = current.take() {
                        releases.push(r);
                    }
                } else if let Some(r) = current.as_mut() {
                    apply_text(r, &name, &text);
                }
                text.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => anyhow::bail!("Torznab RSS parse error: {e}"),
            _ => {}
        }
    }

    Ok(releases)
}

fn apply_text(r: &mut ReleaseInfo, name: &str, text: &str) {
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    match name {
        "title" => r.title = Some(text),
        "guid" => r.guid = Url::parse(&text).ok(),
        "link" => {
            if r.link.is_none() {
                r.link = Url::parse(&text).ok();
            }
        }
        "comments" => r.details = Url::parse(&text).ok(),
        "pubdate" => r.publish_date = DateTimeUtil::from_unknown(&text),
        "description" => r.description = Some(text),
        "category" => {
            if let Ok(c) = text.parse::<i32>() {
                if !r.category.contains(&c) {
                    r.category.push(c);
                }
            }
        }
        _ => {}
    }
}

fn apply_attr(r: &mut ReleaseInfo, name: &str, value: &str) {
    match name {
        "size" => r.size = value.parse::<i64>().ok(),
        "seeders" => r.seeders = value.parse().ok(),
        "peers" => r.peers = value.parse().ok(),
        "imdbid" => r.imdb = parse_imdb(value),
        "tvdbid" => r.tvdb_id = value.parse().ok(),
        "rageid" => r.rage_id = value.parse().ok(),
        "tmdbid" => r.tmdb = value.parse().ok(),
        "traktid" => r.trakt_id = value.parse().ok(),
        "tvmazeid" => r.tvmaze_id = value.parse().ok(),
        "doubanid" => r.douban_id = value.parse().ok(),
        "genre" => {
            r.genres = value
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        "category" => {
            if let Ok(c) = value.parse::<i32>() {
                if !r.category.contains(&c) {
                    r.category.push(c);
                }
            }
        }
        "files" => r.files = value.parse().ok(),
        "grabs" => r.grabs = value.parse().ok(),
        "infohash" => r.info_hash = Some(value.to_string()),
        "poster" => r.poster = Url::parse(value).ok(),
        "magneturl" => r.magnet_uri = Url::parse(value).ok(),
        "downloadvolumefactor" => r.download_volume_factor = value.parse().ok(),
        "uploadvolumefactor" => r.upload_volume_factor = value.parse().ok(),
        "minimumratio" => r.minimum_ratio = value.parse().ok(),
        "minimumseedtime" => r.minimum_seed_time = value.parse().ok(),
        "author" => r.author = Some(value.to_string()),
        "booktitle" => r.book_title = Some(value.to_string()),
        "publisher" => r.publisher = Some(value.to_string()),
        "artist" => r.artist = Some(value.to_string()),
        "album" => r.album = Some(value.to_string()),
        "label" => r.label = Some(value.to_string()),
        "track" => r.track = Some(value.to_string()),
        "year" => r.year = value.parse().ok(),
        _ => {}
    }
}

fn parse_imdb(v: &str) -> Option<i64> {
    let digits: String = v.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:torznab="http://torznab.com/schemas/2015/feed" xmlns:atom="http://www.w3.org/2005/Atom">
  <channel>
    <title>Test Newznab</title>
    <item>
      <title>Big Buck Bunny 1080p</title>
      <guid>https://example.com/details/123</guid>
      <link>https://example.com/download/123</link>
      <comments>https://example.com/details/123</comments>
      <pubDate>Wed, 04 Nov 2026 10:00:00 +0000</pubDate>
      <category>Movies</category>
      <description>An animated short</description>
      <enclosure url="https://example.com/dl/123.torrent" length="12345" type="application/x-bittorrent"/>
      <torznab:attr name="size" value="1610612736"/>
      <torznab:attr name="seeders" value="42"/>
      <torznab:attr name="peers" value="45"/>
      <torznab:attr name="imdbid" value="tt0121176"/>
      <torznab:attr name="category" value="2000"/>
      <torznab:attr name="category" value="2045"/>
      <torznab:attr name="magneturl" value="magnet:?xt=urn:btih:abc"/>
      <torznab:attr name="downloadvolumefactor" value="0"/>
      <torznab:attr name="genre" value="Animation, Comedy"/>
    </item>
  </channel>
</rss>"#;

    #[test]
    fn parses_torznab_rss_offline() {
        let rels = parse_torznab_rss(SAMPLE).expect("parse");
        assert_eq!(rels.len(), 1);
        let r = &rels[0];
        assert_eq!(r.title.as_deref(), Some("Big Buck Bunny 1080p"));
        assert_eq!(r.size, Some(1610612736));
        assert_eq!(r.seeders, Some(42));
        assert_eq!(r.peers, Some(45));
        assert_eq!(r.imdb, Some(121176));
        assert_eq!(r.category, vec![2000, 2045]);
        assert_eq!(
            r.magnet_uri.as_ref().map(|u| u.to_string()),
            Some("magnet:?xt=urn:btih:abc".to_string())
        );
        assert_eq!(r.download_volume_factor, Some(0.0));
        assert_eq!(r.genres, vec!["Animation".to_string(), "Comedy".to_string()]);
        assert!(r.publish_date.is_some());
        // link may come from <link> or enclosure url; both should resolve to a URL
        assert!(r.link.is_some());
    }

    #[test]
    fn caps_builds_categories() {
        let idx = NewznabIndexer::new(
            "nz",
            "NZ",
            "https://example.com/",
            "key",
            &[2000, 5000],
            "en",
        )
        .expect("build");
        let caps = idx.torznab_caps();
        assert!(caps.search_available);
        assert!(caps.supports_raw_search);
        assert_eq!(caps.categories.get_tracker_categories().len(), 2);
    }
}
