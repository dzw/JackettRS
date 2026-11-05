//! Mirror of Jackett.Common/Indexers/Definitions/Abstract/PublicBrazilianIndexerBase.cs
//! and its only concrete implementation `HDRTorrent.cs`.
//!
//! Public, no-auth Brazilian-Portuguese trackers. Search is a simple GET with a
//! configurable query key; results are scraped from the listing page and each
//! detail page (magnet links + Portuguese info fields).
//!
//! Note: `scraper 0.19` has no `:contains()` pseudo-class, so the C#/
//! AngleSharp `span:contains("Tamanho:")` lookups are reproduced by iterating
//! elements and matching their text with a regex.

use std::sync::Arc;

use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{Html, Selector};
use url::Url;

use crate::indexers::i_indexer::IIndexer;
use crate::models::release_info::ReleaseInfo;
use crate::models::torznab_capabilities::{TorznabCapabilities, TvSearchParam};
use crate::models::torznab_cat_type::TorznabCatType;
use crate::models::torznab_query::TorznabQuery;
use crate::utils::browser_util::BrowserUtil;
use crate::utils::date_time_util::DateTimeUtil;

pub struct PublicBrazilianSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub site_link: &'static str,
    /// e.g. `"index.php?busca="` (HDRTorrent) or `"?s="` (base default).
    pub search_query_params_key: &'static str,
}

impl PublicBrazilianSpec {
    fn caps() -> TorznabCapabilities {
        let mut caps = TorznabCapabilities::new();
        caps.tv_search_params = vec![TvSearchParam::Q, TvSearchParam::Season, TvSearchParam::Ep];
        for (key, name) in [("filmes", "Movies"), ("series", "TV")] {
            if let Some(cat) = TorznabCatType::get_cat_by_name(name) {
                caps.categories
                    .add_category_mapping(key, cat.clone(), None);
            }
        }
        caps
    }

    pub fn build(&self) -> anyhow::Result<Arc<dyn IIndexer>> {
        let client = reqwest::Client::builder()
            .user_agent(BrowserUtil::default_user_agent())
            .build()?;
        Ok(Arc::new(PublicBrazilianIndexer {
            id: self.id.to_string(),
            name: self.name.to_string(),
            description: format!(
                "{} is a Public Torrent Tracker for Movies and TV Shows dubbed in Brazilian Portuguese",
                self.name
            ),
            site_link: self.site_link.trim_end_matches('/').to_string(),
            search_query_params_key: self.search_query_params_key.to_string(),
            caps: Self::caps(),
            client,
        }))
    }
}

pub static PUBLIC_BRAZILIAN_SITES: Lazy<Vec<PublicBrazilianSpec>> = Lazy::new(|| {
    vec![PublicBrazilianSpec {
        id: "hdrtorrent",
        name: "HDRTorrent",
        site_link: "https://hdrtorrents.net/",
        search_query_params_key: "index.php?busca=",
    }]
});

pub struct PublicBrazilianIndexer {
    id: String,
    name: String,
    description: String,
    site_link: String,
    search_query_params_key: String,
    caps: TorznabCapabilities,
    client: reqwest::Client,
}

impl PublicBrazilianIndexer {
    fn search_url(&self, query: &TorznabQuery) -> String {
        let term = query.search_term.clone().unwrap_or_default();
        if term.trim().is_empty() {
            return self.site_link.clone();
        }
        let mut encoded = url::form_urlencoded::Serializer::new(String::new());
        let mut q = term;
        if let Some(season) = query.season {
            if season > 0 {
                q.push_str(&format!(" {season}"));
            }
        }
        encoded.append_pair("", &q);
        // `append_pair("", q)` yields `=q`; we only want the encoded value.
        let encoded_value = encoded
            .finish()
            .trim_start_matches('=')
            .to_string();
        format!("{}/{}{}", self.site_link, self.search_query_params_key, encoded_value)
    }

    async fn run_query(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let list_url = self.search_url(query);
        let html = self
            .client
            .get(&list_url)
            .send()
            .await?
            .text()
            .await?;
        let detail_urls = parse_listing(&html, &self.site_link);
        let mut releases = Vec::new();
        for (title, detail_url) in detail_urls {
            let Ok(resp) = self.client.get(detail_url.clone()).send().await else {
                continue;
            };
            let Ok(detail_html) = resp.text().await else {
                continue;
            };
            releases.extend(parse_detail(
                &detail_html,
                &title,
                &detail_url,
                &self.caps,
            ));
        }
        Ok(releases)
    }
}

#[async_trait]
impl IIndexer for PublicBrazilianIndexer {
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
        "public"
    }
    fn site_link(&self) -> &str {
        &self.site_link
    }
    fn alternative_site_links(&self) -> Vec<String> {
        Vec::new()
    }
    fn language(&self) -> &str {
        "pt-BR"
    }
    fn tags(&self) -> Vec<String> {
        Vec::new()
    }
    fn is_configured(&self) -> bool {
        true // public, no credentials
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
        let resp = self.client.get(link.clone()).send().await?;
        Ok(resp.bytes().await?.to_vec())
    }
}

/// Extracts `(title, absolute detail URL)` pairs from a listing page.
pub fn parse_listing(html: &str, site_link: &str) -> Vec<(String, Url)> {
    let doc = crate::indexers::cardigann::parsing::parse_html_safe(html);
    let row_sel = Selector::parse("div.col").unwrap();
    let link_sel = Selector::parse("a.media-card-link").unwrap();
    let mut out = Vec::new();
    for row in doc.select(&row_sel) {
        let Some(a) = row.select(&link_sel).next() else {
            continue;
        };
        let title = a.text().collect::<String>().trim().to_string();
        let Some(href) = a.value().attr("href") else {
            continue;
        };
        let url = if href.starts_with("http") {
            Url::parse(href).ok()
        } else {
            Url::parse(&format!(
                "{}/{}",
                site_link.trim_end_matches('/'),
                href.trim_start_matches('/')
            ))
            .ok()
        };
        if let Some(u) = url {
            if !title.is_empty() {
                out.push((title, u));
            }
        }
    }
    out
}

/// Parses a detail page into releases (one per magnet link).
pub fn parse_detail(
    html: &str,
    listing_title: &str,
    detail_url: &Url,
    caps: &TorznabCapabilities,
) -> Vec<ReleaseInfo> {
    let doc = crate::indexers::cardigann::parsing::parse_html_safe(html);
    let info = parse_info_fields(&doc);
    let publish_date = {
        let sel = Selector::parse("meta[property='article:published_time']").unwrap();
        doc.select(&sel)
            .next()
            .and_then(|m| m.value().attr("content"))
            .and_then(DateTimeUtil::from_unknown)
    };

    let magnet_sel = Selector::parse("a[href^='magnet:?']").unwrap();
    let mut out = Vec::new();
    for a in doc.select(&magnet_sel) {
        let Some(magnet) = a.value().attr("href") else {
            continue;
        };
        let Ok(magnet_uri) = Url::parse(magnet) else {
            continue;
        };

        let mut title = clean_title(listing_title);
        // The magnet's parent text carries extra release tags.
        if let Some(parent) = a.parent() {
            if let Some(el) = scraper::ElementRef::wrap(parent) {
                let mut extra = el.text().collect::<String>().replace("DOWNLOAD TORRENT", "");
                for (from, to) in [
                    ("DUAL ÁUDIO", "Dual"), ("DUAL AUDIO", "Dual"),
                    ("ÁUDIO", "Audio"), ("AUDIO", "Audio"),
                    ("DUBLADO", "Dubbed"), ("LEGENDADO", "Subbed"),
                    ("MKV", ""), ("MP4", ""), ("MAGNET", ""), ("TORRENT", ""), ("LINK", ""),
                ] {
                    extra = extra.replace(from, to);
                }
                let extra = extra.trim().to_string();
                if !extra.is_empty() {
                    title.push_str(&format!(" {extra}"));
                }
            }
        }
        if let Some(q) = info.get("Qualidade").or_else(|| info.get("Qualidade de Vídeo")) {
            if !q.is_empty() {
                title.push_str(&format!(" {q}"));
            }
        }
        if title.trim().is_empty() {
            continue;
        }

        let mut rel = ReleaseInfo::new();
        rel.title = Some(title.clone());
        rel.details = Some(detail_url.clone());
        rel.guid = Some(magnet_uri.clone());
        rel.magnet_uri = Some(magnet_uri);
        rel.seeders = Some(1);
        rel.publish_date = publish_date;
        rel.category = extract_category(&title);
        rel.size = info
            .get("Tamanho")
            .and_then(|s| parse_size(s))
            .or_else(|| Some(size_by_resolution(&title)));
        rel.download_volume_factor = Some(0.0);
        rel.upload_volume_factor = Some(1.0);
        rel.languages = info
            .get("Áudio")
            .or_else(|| info.get("Idioma"))
            .map(|s| split_list(s))
            .unwrap_or_default();
        rel.genres = info
            .get("Gênero")
            .map(|s| split_list(s))
            .unwrap_or_default();
        if let Some(sub) = info.get("Legenda") {
            rel.subs = vec![sub.clone()];
        }
        if let Some(imdb) = info.get("IMDb") {
            let digits: String = imdb.chars().filter(|c| c.is_ascii_digit()).collect();
            rel.imdb = digits.parse().ok();
        }
        let _ = caps; // categories derived from title, matching the C# helper
        out.push(rel);
    }
    out
}

/// Reproduces the Portuguese `div.infos p` key/value parsing.
fn parse_info_fields(doc: &Html) -> std::collections::HashMap<String, String> {
    let sel = Selector::parse("div.infos p").unwrap();
    let mut map = std::collections::HashMap::new();
    for p in doc.select(&sel) {
        let inner = p.inner_html();
        for line in inner.split("<br>").flat_map(|s| s.split("<br/>")) {
            let plain = strip_tags(line);
            let Some((key, value)) = plain.split_once(':') else {
                continue;
            };
            let key = key.trim().to_string();
            let value = normalise_value(value.trim());
            if !key.is_empty() && !value.is_empty() {
                map.insert(key, value);
            }
        }
    }
    map
}

fn strip_tags(s: &str) -> String {
    static TAG: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]*>").unwrap());
    TAG.replace_all(s, "").to_string()
}

fn normalise_value(v: &str) -> String {
    let mut s = v.to_string();
    for (from, to) in [
        ("Dual Áudio", "Dual"), ("Dual Audio", "Dual"),
        ("Full HD", "1080p"), ("4K", "2160p"), ("SD", "480p"),
        ("WEB", "WEB-DL"),
    ] {
        s = s.replace(from, to);
    }
    s
}

fn split_list(s: &str) -> Vec<String> {
    static SPLIT: Lazy<Regex> = Lazy::new(|| Regex::new(r"[,|]").unwrap());
    SPLIT
        .split(s)
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

fn clean_title(s: &str) -> String {
    static PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
        [
            r"(?i)\(\d+(\.\d+)?\s*(GB|MB)\)",
            r"(?i)720p|1080p|2160p|4K",
            r"(?i)WEB-DL|BRRip|HDRip|WEBRip|BluRay|Torrent|Download",
            r"(?i)Legendado|Leg|Dublado|Dub|[AÁ]udio",
            r"(?i)HIDRATORRENTS\.ORG|HDRTORRENTS",
            r"\[.*?\]|\(.*?\)",
            r"[\\/,|~_-]+\s*",
            r"(?i)MKV|MP4",
        ]
        .iter()
        .map(|p| Regex::new(p).unwrap())
        .collect()
    });
    let mut t = s.to_string();
    for re in PATTERNS.iter() {
        t = re.replace_all(&t, " ").to_string();
    }
    static WS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
    t = WS.replace_all(&t, " ").to_string();
    t.trim_matches(|c| " .,~_-/\\|".contains(c)).to_string()
}

fn extract_category(title: &str) -> Vec<i32> {
    static TV: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)temporada|season|\bS\d{1,2}(?:E\d{1,2})?\b").unwrap());
    if TV.is_match(title) {
        vec![5000]
    } else {
        vec![2000]
    }
}

fn size_by_resolution(title: &str) -> i64 {
    const GIB: i64 = 1024 * 1024 * 1024;
    if title.contains("2160p") {
        5 * GIB
    } else if title.contains("1080p") {
        (2.5 * GIB as f64) as i64
    } else if title.contains("720p") {
        GIB
    } else {
        512 * 1024 * 1024
    }
}

fn parse_size(s: &str) -> Option<i64> {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)([\d.,]+)\s*([KMGTP]?B)").unwrap());
    let c = RE.captures(s)?;
    let num: f64 = c.get(1)?.as_str().replace(',', ".").parse().ok()?;
    let unit = c.get(2)?.as_str().to_ascii_uppercase();
    let mult = match unit.as_str() {
        "KB" => 1024.0,
        "MB" => 1024.0f64.powi(2),
        "GB" => 1024.0f64.powi(3),
        "TB" => 1024.0f64.powi(4),
        "PB" => 1024.0f64.powi(5),
        _ => 1.0,
    };
    Some((num * mult) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdrtorrent_builds_public_caps() {
        assert_eq!(PUBLIC_BRAZILIAN_SITES.len(), 1);
        let idx = PUBLIC_BRAZILIAN_SITES[0].build().unwrap();
        assert_eq!(idx.id(), "hdrtorrent");
        assert_eq!(idx.type_(), "public");
        assert_eq!(idx.language(), "pt-BR");
        assert!(idx.torznab_caps().categories.get_tracker_categories().len() == 2);
    }

    #[test]
    fn listing_extracts_detail_links() {
        let html = r#"<div class="col"><a class="media-card-link" href="/filme/123">Big Buck Bunny 1080p</a></div>
                      <div class="col"><a class="media-card-link" href="/serie/9">Show S01E01</a></div>"#;
        let links = parse_listing(html, "https://hdrtorrents.net");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].1.as_str(), "https://hdrtorrents.net/filme/123");
        assert_eq!(links[0].0, "Big Buck Bunny 1080p");
    }

    #[test]
    fn detail_extracts_magnets_and_portuguese_fields() {
        let html = r#"<html><head>
          <meta property="article:published_time" content="2024-01-02T10:00:00Z"/>
        </head><body>
          <div class="infos">
            <p><b>Gênero:</b> Ação, Aventura<br><b>Áudio:</b> Português, Inglês<br>
               <b>Legenda:</b> Português<br><b>Qualidade:</b> Full HD<br>
               <b>Tamanho:</b> 2.5 GB<br><b>IMDb:</b> tt1254207</p>
          </div>
          <div><a href="magnet:?xt=urn:btih:abc&amp;dn=release">DOWNLOAD TORRENT Dual Áudio MKV</a></div>
        </body></html>"#;
        let caps = PublicBrazilianSpec::caps();
        let url = Url::parse("https://hdrtorrents.net/filme/123").unwrap();
        let rels = parse_detail(html, "Big Buck Bunny 1080p", &url, &caps);
        assert_eq!(rels.len(), 1);
        let r = &rels[0];
        assert!(r.magnet_uri.is_some());
        assert!(r.title.as_deref().unwrap().contains("Dual"));
        assert_eq!(r.imdb, Some(1254207));
        assert_eq!(r.genres.len(), 2);
        assert_eq!(r.languages.len(), 2);
        assert_eq!(r.subs, vec!["Português".to_string()]);
        assert!(r.size.unwrap() > 2_000_000_000);
        assert!(r.publish_date.is_some());
        assert_eq!(r.download_volume_factor, Some(0.0));
    }

    #[test]
    fn category_and_size_helpers() {
        assert_eq!(extract_category("Show S01E01 720p"), vec![5000]);
        assert_eq!(extract_category("Movie 1080p"), vec![2000]);
        assert_eq!(size_by_resolution("x 720p"), 1024 * 1024 * 1024);
    }
}
