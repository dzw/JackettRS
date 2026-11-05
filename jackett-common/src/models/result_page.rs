use once_cell::sync::Lazy;
use regex::Regex;
use url::Url;

use super::channel_info::ChannelInfo;
use super::release_info::ReleaseInfo;

/// Mirror of Jackett.Common/Models/ResultPage.cs
pub struct ResultPage {
    pub channel_info: ChannelInfo,
    pub releases: Vec<ReleaseInfo>,
}

// filters control characters but allows only properly-formed surrogate sequences
// https://stackoverflow.com/a/961504
// NOTE: Rust strings are always valid UTF-8 so the lone-surrogate alternatives
// cannot occur; we only filter the control-character range present in C#.
static INVALID_XML_CHARS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[\x{00}-\x{08}\x{0B}\x{0C}\x{0E}-\x{1F}\x{7F}-\x{9F}\x{FEFF}\x{FFFE}\x{FFFF}]")
        .unwrap()
});

fn remove_invalid_xml_chars(text: Option<&str>) -> Option<String> {
    text.map(|t| INVALID_XML_CHARS.replace_all(t, "").to_string())
}

fn xml_date_format(dt: &chrono::DateTime<chrono::Local>) -> String {
    // Sat, 14 Mar 2015 17:10:42 -0400
    let base = dt.format("%a, %d %b %Y %H:%M:%S").to_string();
    let tz = dt.format("%:z").to_string().replace(':', "");
    format!("{} {}", base, tz)
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn attr(name: &str, value: &str) -> String {
    format!(" {}=\"{}\"", name, escape_xml(value))
}

impl ResultPage {
    pub fn new(channel_info: ChannelInfo) -> Self {
        Self {
            channel_info,
            releases: Vec::new(),
        }
    }

    pub fn to_xml(&self, self_atom: &Url) -> String {
        let ci = &self.channel_info;
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str("<rss version=\"2.0\"");
        out.push_str(" xmlns:atom=\"http://www.w3.org/2005/Atom\"");
        out.push_str(" xmlns:torznab=\"http://torznab.com/schemas/2015/feed\"");
        out.push_str("><channel>");
        out.push_str(&format!(
            "<atom:link href=\"{}\" rel=\"self\" type=\"application/rss+xml\" />",
            escape_xml(self_atom.as_str())
        ));
        out.push_str(&format!(
            "<title>{}</title>",
            escape_xml(ci.title.as_deref().unwrap_or(""))
        ));
        out.push_str(&format!(
            "<description>{}</description>",
            escape_xml(ci.description.as_deref().unwrap_or(""))
        ));
        out.push_str(&format!(
            "<link>{}</link>",
            escape_xml(ci.link.as_ref().map(|u| u.as_str()).unwrap_or(""))
        ));
        out.push_str(&format!(
            "<language>{}</language>",
            escape_xml(ci.language.as_deref().unwrap_or(""))
        ));
        out.push_str(&format!(
            "<category>{}</category>",
            escape_xml(ci.category.as_deref().unwrap_or(""))
        ));

        for r in &self.releases {
            out.push_str("<item>");
            out.push_str(&format!(
                "<title>{}</title>",
                escape_xml(&remove_invalid_xml_chars(r.title.as_deref()).unwrap_or_default())
            ));
            // GUID and (Link or Magnet) are mandatory
            out.push_str(&format!(
                "<guid>{}</guid>",
                escape_xml(r.guid.as_ref().map(|u| u.as_str()).unwrap_or(""))
            ));
            out.push_str(&format!(
                "<jackettindexer id=\"{}\">{}</jackettindexer>",
                escape_xml(r.origin.as_deref().unwrap_or("")),
                escape_xml(r.origin.as_deref().unwrap_or(""))
            ));
            out.push_str(&format!(
                "<type>{}</type>",
                escape_xml(r.origin.as_deref().unwrap_or(""))
            ));
            if let Some(details) = &r.details {
                out.push_str(&format!(
                    "<comments>{}</comments>",
                    escape_xml(details.as_str())
                ));
            }
            let pub_date = match r.publish_date {
                Some(d) => xml_date_format(&d.with_timezone(&chrono::Local)),
                None => xml_date_format(&chrono::Local::now()),
            };
            out.push_str(&format!("<pubDate>{}</pubDate>", pub_date));
            if let Some(size) = r.size {
                out.push_str(&format!("<size>{}</size>", size));
            }
            if let Some(files) = r.files {
                out.push_str(&format!("<files>{}</files>", files));
            }
            if let Some(grabs) = r.grabs {
                out.push_str(&format!("<grabs>{}</grabs>", grabs));
            }
            out.push_str(&format!(
                "<description>{}</description>",
                escape_xml(&remove_invalid_xml_chars(r.description.as_deref()).unwrap_or_default())
            ));
            let link = r
                .link
                .as_ref()
                .or(r.magnet_uri.as_ref())
                .map(|u| u.as_str())
                .unwrap_or("");
            out.push_str(&format!("<link>{}</link>", escape_xml(link)));
            for c in &r.category {
                out.push_str(&format!("<category>{}</category>", c));
            }
            out.push_str(&format!(
                "<enclosure url=\"{}\"{} type=\"application/x-bittorrent\" />",
                escape_xml(link),
                r.size.map(|s| attr("length", &s.to_string())).unwrap_or_default(),
            ));
            for c in &r.category {
                out.push_str(&format!(
                    "<torznab:attr name=\"category\" value=\"{}\" />",
                    c
                ));
            }
            fn attr_el(name: &str, value: Option<String>) -> String {
                value
                    .map(|v| format!("<torznab:attr name=\"{}\" value=\"{}\" />", name, escape_xml(&v)))
                    .unwrap_or_default()
            }
            out.push_str(&attr_el("rageid", r.rage_id.map(|v| v.to_string())));
            out.push_str(&attr_el("tvdbid", r.tvdb_id.map(|v| v.to_string())));
            out.push_str(&attr_el(
                "imdb",
                r.imdb.map(|v| format!("{:07}", v)),
            ));
            out.push_str(&attr_el(
                "imdbid",
                r.imdb.map(|v| format!("tt{:07}", v)),
            ));
            out.push_str(&attr_el("tmdbid", r.tmdb.map(|v| v.to_string())));
            out.push_str(&attr_el("tvmazeid", r.tvmaze_id.map(|v| v.to_string())));
            out.push_str(&attr_el("traktid", r.trakt_id.map(|v| v.to_string())));
            out.push_str(&attr_el("doubanid", r.douban_id.map(|v| v.to_string())));
            if !r.genres.is_empty() {
                out.push_str(&attr_el("genre", Some(r.genres.join(", "))));
            }
            for lang in &r.languages {
                out.push_str(&attr_el("language", Some(lang.clone())));
            }
            for subs in &r.subs {
                out.push_str(&attr_el("subs", Some(subs.clone())));
            }
            out.push_str(&attr_el("year", r.year.map(|v| v.to_string())));
            out.push_str(&attr_el(
                "author",
                remove_invalid_xml_chars(r.author.as_deref()),
            ));
            out.push_str(&attr_el(
                "booktitle",
                remove_invalid_xml_chars(r.book_title.as_deref()),
            ));
            out.push_str(&attr_el(
                "publisher",
                remove_invalid_xml_chars(r.publisher.as_deref()),
            ));
            out.push_str(&attr_el(
                "artist",
                remove_invalid_xml_chars(r.artist.as_deref()),
            ));
            out.push_str(&attr_el(
                "album",
                remove_invalid_xml_chars(r.album.as_deref()),
            ));
            out.push_str(&attr_el(
                "label",
                remove_invalid_xml_chars(r.label.as_deref()),
            ));
            out.push_str(&attr_el(
                "track",
                remove_invalid_xml_chars(r.track.as_deref()),
            ));
            out.push_str(&attr_el("seeders", r.seeders.map(|v| v.to_string())));
            out.push_str(&attr_el("peers", r.peers.map(|v| v.to_string())));
            out.push_str(&attr_el(
                "coverurl",
                r.poster.as_ref().map(|u| u.to_string()),
            ));
            out.push_str(&attr_el(
                "infohash",
                remove_invalid_xml_chars(r.info_hash.as_deref()),
            ));
            out.push_str(&attr_el(
                "magneturl",
                r.magnet_uri.as_ref().map(|u| u.to_string()),
            ));
            out.push_str(&attr_el(
                "minimumratio",
                r.minimum_ratio.map(|v| v.to_string()),
            ));
            out.push_str(&attr_el(
                "minimumseedtime",
                r.minimum_seed_time.map(|v| v.to_string()),
            ));
            out.push_str(&attr_el(
                "downloadvolumefactor",
                r.download_volume_factor.map(|v| v.to_string()),
            ));
            out.push_str(&attr_el(
                "uploadvolumefactor",
                r.upload_volume_factor.map(|v| v.to_string()),
            ));
            out.push_str("</item>");
        }

        out.push_str("</channel></rss>");
        out
    }
}
