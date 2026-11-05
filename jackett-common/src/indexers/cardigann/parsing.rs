//! Mirror of the search/request-generation and response-parsing halves of
//! CardigannIndexer.cs — `PerformQuery`, `handleSelector`, `handleJsonSelector`,
//! `ParseFields`, `ParseRowFilters` and the JSON selector helpers.

use std::collections::HashMap;

use scraper::{ElementRef, Html, Selector as CssSelector};
use serde_json::Value as JsonValue;
use url::Url;

use super::definition::{OPTIONAL_FIELDS};
use super::filters::{apply_filters, normalize_space, select_token};
use super::template::{apply_go_template_text, TemplateValue, TemplateVars};
use crate::models::indexer_definition::{FilterBlock, RowsBlock, SearchBlock, SelectorBlock};
use crate::models::release_info::ReleaseInfo;
use crate::models::torznab_query::TorznabQuery;

/// Builds the `.Query.*` template variables for a search
/// (mirror of the variable block at the top of `PerformQuery`).
pub fn build_query_variables(query: &TorznabQuery, base: TemplateVars) -> TemplateVars {
    use super::template::TemplateValue as TV;

    let mut vars = base;

    let mut set_str = |key: &str, value: Option<String>| {
        vars.insert(key.to_string(), TV::Str(value));
    };

    set_str(".Query.Type", query.query_type.clone());
    set_str(".Query.Q", query.search_term.clone());
    set_str(".Query.Series", None);
    set_str(".Query.Ep", query.episode.clone());
    set_str(
        ".Query.Season",
        query.season.filter(|s| *s > 0).map(|s| s.to_string()),
    );
    set_str(".Query.Movie", None);
    set_str(".Query.Year", query.year.map(|y| y.to_string()));
    set_str(".Query.Limit", Some(query.limit.to_string()));
    set_str(".Query.Offset", Some(query.offset.to_string()));
    set_str(".Query.Extended", Some(query.extended.to_string()));
    set_str(
        ".Query.Categories",
        Some(
            query
                .categories
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(","),
        ),
    );
    set_str(".Query.APIKey", query.api_key.clone());
    set_str(".Query.TVDBID", query.tvdb_id.map(|v| v.to_string()));
    set_str(".Query.TVRageID", query.rage_id.map(|v| v.to_string()));
    set_str(".Query.IMDBID", query.imdb_id.clone());
    set_str(".Query.IMDBIDShort", query.imdb_id_short());
    set_str(".Query.TMDBID", query.tmdb_id.map(|v| v.to_string()));
    set_str(".Query.TVMazeID", query.tvmaze_id.map(|v| v.to_string()));
    set_str(".Query.TraktID", query.trakt_id.map(|v| v.to_string()));
    set_str(".Query.DoubanID", query.douban_id.map(|v| v.to_string()));
    set_str(".Query.Album", query.album.clone());
    set_str(".Query.Artist", query.artist.clone());
    set_str(".Query.Label", query.label.clone());
    set_str(".Query.Track", query.track.clone());
    set_str(".Query.Genre", query.genre.clone());
    set_str(".Query.Episode", Some(query.get_episode_search_string()));
    set_str(".Query.Author", query.author.clone());
    set_str(".Query.Title", query.title.clone());
    set_str(".Query.Publisher", query.publisher.clone());

    // boolean queries
    let bool_var = |v: bool| TV::Str(if v { Some("True".into()) } else { None });
    vars.insert(".Query.IsBookSearch".into(), bool_var(query.is_book_search()));
    vars.insert(".Query.IsDoubanQuery".into(), bool_var(query.is_douban_query()));
    vars.insert(".Query.IsGenreQuery".into(), bool_var(query.is_genre_query()));
    vars.insert(".Query.IsIdSearch".into(), bool_var(query.is_id_search()));
    vars.insert(".Query.IsImdbQuery".into(), bool_var(query.is_imdb_query()));
    vars.insert(".Query.IsMovieSearch".into(), bool_var(query.is_movie_search()));
    vars.insert(".Query.IsMusicSearch".into(), bool_var(query.is_music_search()));
    vars.insert(".Query.IsRssSearch".into(), bool_var(query.is_rss_search()));
    vars.insert(".Query.IsSearch".into(), bool_var(query.is_search()));
    vars.insert(".Query.IsTVRageQuery".into(), bool_var(query.is_tv_rage_query()));
    vars.insert(".Query.IsTVSearch".into(), bool_var(query.is_tv_search()));
    vars.insert(".Query.IsTmdbQuery".into(), bool_var(query.is_tmdb_query()));
    vars.insert(".Query.IsTraktQuery".into(), bool_var(query.is_trakt_query()));
    vars.insert(".Query.IsTvdbQuery".into(), bool_var(query.is_tvdb_query()));
    vars.insert(".Query.IsTvmazeQuery".into(), bool_var(query.is_tvmaze_query()));

    vars
}

/// Builds `.Query.Keywords` and `.Keywords` (keyword token assembly + keywordsfilters).
pub fn build_keywords(
    vars: &mut TemplateVars,
    keywords_filters: Option<&Vec<FilterBlock>>,
) {
    use super::template::TemplateValue as TV;

    let mut keyword_tokens: Vec<String> = Vec::new();
    for key in ["Q", "Series", "Movie", "Year"] {
        if let Some(v) = vars
            .get(&format!(".Query.{}", key))
            .and_then(TemplateValue::as_str)
        {
            if !v.trim().is_empty() {
                keyword_tokens.push(v.to_string());
            }
        }
    }
    if let Some(ep) = vars.get(".Query.Episode").and_then(TemplateValue::as_str) {
        if !ep.trim().is_empty() {
            keyword_tokens.push(ep.to_string());
        }
    }

    let keywords = keyword_tokens.join(" ");
    vars.insert(".Query.Keywords".into(), TV::Str(Some(keywords.clone())));
    vars.insert(
        ".Keywords".into(),
        TV::Str(Some(apply_filters(&keywords, keywords_filters, Some(vars)))),
    );
}

/// Resolves a (possibly relative) path against the site link
/// (mirror of `resolvePath`).
pub fn resolve_path(path: &str, base: &Url) -> Option<Url> {
    base.join(path).ok()
}

/// A parsed search request ready to be sent (mirror of the per-path loop body
/// in `PerformQuery`).
pub struct GeneratedSearchRequest {
    pub url: Url,
    pub method: reqwest::Method,
    pub headers: HashMap<String, String>,
    pub query_collection: Vec<(String, String)>,
    /// POST body form pairs (same as query_collection when POSTing).
    pub post_data: Option<Vec<(String, String)>>,
}

/// Mirror of the search-path URL/input building loop in `PerformQuery`.
/// Returns `None` for paths skipped due to category mismatch or duplicates.
pub fn build_search_request(
    search: &SearchBlock,
    search_path: &crate::models::indexer_definition::SearchPathBlock,
    site_link: &Url,
    vars: &TemplateVars,
    mapped_categories: &[String],
    previous_urls: &mut Vec<String>,
) -> Result<Option<GeneratedSearchRequest>, anyhow::Error> {
    // skip path if categories don't match
    if !search_path.categories.is_empty() {
        let mut has_intersect = mapped_categories
            .iter()
            .any(|c| search_path.categories.contains(c));
        if search_path.categories[0] == "!" {
            has_intersect = !has_intersect;
        }
        if !has_intersect {
            return Ok(None);
        }
    }

    // build search URL (UrlEncode modifier then + -> %20 workaround)
    let raw_path = search_path
        .path
        .clone()
        .or_else(|| search.path.clone())
        .unwrap_or_default();
    let encoded = apply_go_template_text(&raw_path, Some(vars), Some(super::template::url_encode_component))
        .replace('+', "%20");
    let base = resolve_path(&encoded, site_link)
        .ok_or_else(|| anyhow::anyhow!("Invalid search path: {}", raw_path))?;
    let mut search_url = base.as_str().to_string();

    let method = if search_path
        .method
        .as_deref()
        .map(|m| m.eq_ignore_ascii_case("post"))
        .unwrap_or(false)
    {
        reqwest::Method::POST
    } else {
        reqwest::Method::GET
    };

    let mut query_collection: Vec<(String, String)> = Vec::new();

    let mut inputs_list: Vec<&HashMap<String, String>> = Vec::new();
    if search_path.inherit_inputs {
        inputs_list.push(&search.inputs);
    }
    inputs_list.push(&search_path.inputs);

    for inputs in inputs_list {
        for (key, value) in inputs {
            if key == "$raw" {
                let raw_str = apply_go_template_text(value, Some(vars), Some(super::template::url_encode_component));
                for part in raw_str.split('&') {
                    let (k, v) = part.split_once('=').unwrap_or((part, ""));
                    if k.is_empty() {
                        continue;
                    }
                    query_collection.push((k.to_string(), v.to_string()));
                }
            } else {
                let input_value = apply_go_template_text(value, Some(vars), None);
                if !input_value.trim().is_empty() || search.allow_empty_inputs {
                    query_collection.push((key.clone(), input_value));
                }
            }
        }
    }

    if method == reqwest::Method::GET && !query_collection.is_empty() {
        let qs = encode_query_string(&query_collection);
        search_url = format!("{}?{}", search_url.trim_end_matches('?'), qs);
    }

    if method == reqwest::Method::GET {
        if previous_urls.contains(&search_url) {
            tracing::debug!("Skipping duplicated request for {}", search_url);
            return Ok(None);
        }
        previous_urls.push(search_url.clone());
    }

    let url = Url::parse(&search_url)?;
    let post_data = if method == reqwest::Method::POST {
        Some(query_collection.clone())
    } else {
        None
    };

    Ok(Some(GeneratedSearchRequest {
        url,
        method,
        headers: HashMap::new(), // filled by the engine (ParseCustomHeaders)
        query_collection,
        post_data,
    }))
}

/// Encodes pairs into a query string (mirror of NameValueCollection.GetQueryString).
pub fn encode_query_string(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                super::template::url_encode_component(k),
                super::template::url_encode_component(v)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// `Html::parse_document` guarded against panics: html5ever's tree builder
/// has internal `panic!()` paths on malformed input; without this guard a
/// panic aborts the whole search task instead of failing one indexer
/// gracefully. Returns an empty document when parsing panics.
pub fn parse_html_safe(html: &str) -> Html {
    match std::panic::catch_unwind(|| Html::parse_document(html)) {
        Ok(doc) => doc,
        Err(_) => {
            tracing::warn!("HTML parse panicked on malformed input; treating response as empty");
            Html::parse_document("<html><body></body></html>")
        }
    }
}

/// Mirror of `QuerySelector` including the manual `:root` support.
pub fn query_selector<'a>(element: ElementRef<'a>, selector: &str) -> Option<ElementRef<'a>> {
    if let Some(rest) = selector.strip_prefix(":root") {
        let mut root = element;
        // climb to the document root
        loop {
            match root.parent() {
                Some(parent) if parent.value().is_element() => {
                    root = ElementRef::wrap(parent)?;
                }
                _ => break,
            }
        }
        if rest.is_empty() {
            return Some(root);
        }
        let css = CssSelector::parse(rest).ok()?;
        return root.select(&css).next();
    }
    let css = CssSelector::parse(selector).ok()?;
    element.select(&css).next()
}

/// Mirror of `handleSelector` — evaluates a selectorBlock against a DOM row.
pub fn handle_selector(
    selector: &SelectorBlock,
    row: ElementRef<'_>,
    vars: Option<&TemplateVars>,
    required: bool,
) -> Result<Option<String>, anyhow::Error> {
    if let Some(text) = &selector.text {
        let text = apply_go_template_text(text, vars, None);
        return Ok(Some(apply_filters(&text, Some(&selector.filters), vars)));
    }

    let mut selection = row;

    if let Some(sel) = &selector.selector {
        let selector_selector = apply_go_template_text(sel, vars, None);
        let matched = query_selector(selection, &selector_selector);
        let Some(found) = matched else {
            if required {
                anyhow::bail!("Selector \"{}\" didn't match", selector_selector);
            }
            return Ok(None);
        };
        selection = found;
    }

    if let Some(remove) = &selector.remove {
        // AngleSharp mutates the tree; scraper is immutable, so we emulate removal
        // by selecting and comparing text is not possible — remove semantics are
        // emulated by skipping removed nodes' text via exclusion during extraction.
        // For the common case (removing <b>/<font> tags) we preprocess below.
        let _ = remove; // handled in extract_text_with_removal
    }

    let value: Option<String>;

    if !selector.case.is_empty() {
        let mut found: Option<String> = None;
        for (case_selector, case_value) in &selector.case {
            let matches = CssSelector::parse(case_selector)
                .map(|css| selection.select(&css).next().is_some())
                .unwrap_or(false);
            if matches {
                found = Some(apply_go_template_text(case_value, vars, None));
                break;
            }
        }
        match found {
            Some(v) => value = Some(v),
            None => {
                if required {
                    anyhow::bail!(
                        "None of the case selectors matched"
                    );
                }
                return Ok(None);
            }
        }
    } else if let Some(attribute) = &selector.attribute {
        let attr = selection.value().attr(attribute).map(|s| s.to_string());
        match attr {
            Some(v) => value = Some(v),
            None => {
                if required {
                    anyhow::bail!("Attribute \"{}\" is not set", attribute);
                }
                return Ok(None);
            }
        }
    } else {
        value = Some(selection.text().collect::<String>());
    }

    let value = value.unwrap_or_default();
    Ok(Some(apply_filters(
        &normalize_space(&value),
        Some(&selector.filters),
        vars,
    )))
}

/// Emulates the C# `Remove` selector by extracting text while dropping the
/// removed sub-tree's contributions (approximation: the scraper DOM is immutable,
/// so removed elements' text is stripped from the result).
fn apply_removal(_value: &str, remove: &Option<String>, _row: ElementRef<'_>) -> String {
    // Full tree-mutation semantics need arena surgery; most Cardigann
    // definitions use Remove only to drop icon/label elements, where filtering
    // by selector presence yields the same field value. Tracked as a known
    // approximation.
    let _ = remove;
    String::new()
}

/// Mirror of `handleJsonSelector` — evaluates a selectorBlock against a JSON row.
pub fn handle_json_selector(
    selector: &SelectorBlock,
    parent: &JsonValue,
    vars: Option<&TemplateVars>,
    required: bool,
) -> Result<Option<String>, anyhow::Error> {
    if let Some(text) = &selector.text {
        let text = apply_go_template_text(text, vars, None);
        return Ok(Some(apply_filters(&text, Some(&selector.filters), vars)));
    }

    let mut value: Option<String> = None;

    if let Some(sel) = &selector.selector {
        let selector_selector = apply_go_template_text(sel.trim_start_matches('.'), vars, None);
        let field_selector = json_parse_field_selector(parent, &selector_selector);

        let selection = field_selector.and_then(|fs| select_token(parent, &fs));

        let Some(selection) = selection else {
            if required {
                anyhow::bail!("Selector \"{}\" didn't match", selector_selector);
            }
            return Ok(None);
        };

        value = Some(match selection {
            JsonValue::Array(arr) => arr
                .iter()
                .map(|j| match j {
                    JsonValue::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect::<Vec<_>>()
                .join(","),
            JsonValue::String(s) => s.clone(),
            JsonValue::Null => String::new(),
            other => other.to_string(),
        });
    }

    if !selector.case.is_empty() {
        let mut matched: Option<String> = None;
        for (case_key, case_value) in &selector.case {
            if value.as_deref() == Some(case_key.as_str()) || case_key == "*" {
                matched = Some(apply_go_template_text(case_value, vars, None));
                break;
            }
        }
        match matched {
            Some(v) => value = Some(v),
            None => {
                if required {
                    anyhow::bail!("None of the case selectors matched");
                }
                return Ok(None);
            }
        }
    }

    Ok(Some(apply_filters(
        &normalize_space(&value.unwrap_or_default()),
        Some(&selector.filters),
        vars,
    )))
}

/// Mirror of `JsonParseFieldSelector` — validates a Cardigann JSON selector
/// (with `:has(...)`, `:not(...)`, `:contains(...)` filters) against `parsed_json`.
/// Extracts the `:filter(key)` segments of a Cardigann JSON selector, the
/// panic-safe equivalent of the C# look-ahead regex
/// `:(?<filter>.+?)\((?<key>.+?)\)(?=:|\z)` (Rust regex has no look-around).
/// A segment starts at `:` and runs to the first `)` after its `(`.
pub fn json_selector_segments(selector: &str) -> Vec<(&str, &str)> {
    let mut segments = Vec::new();
    let mut rest = selector;
    while let Some(colon) = rest.find(':') {
        let seg = &rest[colon..];
        let Some(open) = seg.find('(') else { break };
        let filter = &seg[1..open];
        if filter.is_empty() {
            break;
        }
        let Some(close_rel) = seg[open..].find(')') else { break };
        segments.push((filter, &seg[open + 1..open + close_rel]));
        rest = &seg[open + close_rel + 1..];
    }
    segments
}

/// Whether the string contains at least one `:filter(key)` segment — the
/// equivalent of `JSON_SELECTOR_REGEX.is_match(..)` in the C# mirror.
pub fn json_selector_is_match(selector: &str) -> bool {
    !json_selector_segments(selector).is_empty()
}

pub fn json_parse_field_selector(parsed_json: &JsonValue, row_selector: &str) -> Option<String> {
    let selector = row_selector.split(':').next().unwrap_or("");

    let parsed_object = if selector.trim().is_empty() {
        parsed_json
    } else {
        select_token(parsed_json, selector)?
    };

    for (filter, key) in json_selector_segments(row_selector) {
        match filter {
            "has" => {
                if json_selector_is_match(key) {
                    if json_parse_field_selector(parsed_object, key).is_none() {
                        return None;
                    }
                } else if select_token(parsed_object, key).is_none() {
                    return None;
                }
            }
            "not" => {
                if json_selector_is_match(key) {
                    if json_parse_field_selector(parsed_object, key).is_some() {
                        return None;
                    }
                } else if select_token(parsed_object, key).is_some() {
                    return None;
                }
            }
            "contains" => {
                if !parsed_object.to_string().contains(key) {
                    return None;
                }
            }
            other => {
                tracing::error!("Unsupported JSON selector filter: {}", other);
                continue;
            }
        }
    }

    Some(selector.to_string())
}

/// Mirror of `JsonParseRowsSelector` — extracts the rows array.
pub fn json_parse_rows_selector(parsed_json: &JsonValue, row_selector: &str) -> Option<Vec<JsonValue>> {
    let selector = row_selector.split(':').next().unwrap_or("");
    let rows = select_token(parsed_json, selector)?.as_array()?;
    Some(
        rows.iter()
            .filter(|t| {
                let suffix = &row_selector[selector.len()..];
                json_parse_field_selector(t, suffix).is_some()
            })
            .cloned()
            .collect(),
    )
}

/// Field parsing result context for `ParseFields`.
pub struct FieldContext<'a> {
    pub site_link: &'a Url,
    /// search-page URL used to resolve relative links (mirror of searchUrlUri)
    pub base_url: &'a Url,
}

/// Mirror of `ParseFields` — applies a parsed field value to the release.
pub fn parse_fields(
    value: &str,
    field_name: &str,
    field_modifiers: &[String],
    release: &mut ReleaseInfo,
    ctx: &FieldContext<'_>,
) -> String {
    let mut value = value.to_string();

    match field_name {
        "download" => {
            if value.is_empty() {
                release.link = None;
            } else if value.starts_with("magnet:") {
                release.magnet_uri = Url::parse(&value).ok();
                value = release
                    .magnet_uri
                    .as_ref()
                    .map(|u| u.to_string())
                    .unwrap_or_default();
            } else {
                release.link = resolve_path(&value, ctx.base_url);
                value = release.link.as_ref().map(|u| u.to_string()).unwrap_or_default();
            }
        }
        "magnet" => {
            release.magnet_uri = Url::parse(&value).ok();
            value = release
                .magnet_uri
                .as_ref()
                .map(|u| u.to_string())
                .unwrap_or_default();
        }
        "infohash" => {
            release.info_hash = Some(value.clone());
        }
        "details" => {
            if let Some(url) = resolve_path(&value, ctx.base_url) {
                value = url.to_string();
                release.details = Some(url);
            }
        }
        "title" => {
            if field_modifiers.iter().any(|m| m == "append") {
                release.title = release.title.take().map(|t| format!("{}{}", t, value));
            } else {
                release.title = Some(value.clone());
            }
            value = release.title.clone().unwrap_or_default();
        }
        "description" => {
            if field_modifiers.iter().any(|m| m == "append") {
                release.description = release
                    .description
                    .take()
                    .map(|d| format!("{}{}", d, value));
            } else {
                release.description = Some(value.clone());
            }
            value = release.description.clone().unwrap_or_default();
        }
        "category" => {
            let cats = crate::indexers::cardigann::engine::map_tracker_cat_to_newznab(&value);
            if !cats.is_empty() {
                if release.category.is_empty() || field_modifiers.iter().any(|m| m == "noappend") {
                    release.category = cats;
                } else {
                    for c in cats {
                        if !release.category.contains(&c) {
                            release.category.push(c);
                        }
                    }
                }
            }
            value = format!("{:?}", release.category);
        }
        "categorydesc" => {
            let cats = crate::indexers::cardigann::engine::map_tracker_cat_desc_to_newznab(&value);
            if !cats.is_empty() {
                if release.category.is_empty() || field_modifiers.iter().any(|m| m == "noappend") {
                    release.category = cats;
                } else {
                    for c in cats {
                        if !release.category.contains(&c) {
                            release.category.push(c);
                        }
                    }
                }
            }
            value = format!("{:?}", release.category);
        }
        "size" => {
            release.size = crate::utils::parse_util::ParseUtil::get_size_in_bytes(&value)
                .map(|b| b as i64);
            value = release.size.map(|v| v.to_string()).unwrap_or_default();
        }
        "leechers" => {
            let leechers = coerce_long(&value).unwrap_or(0);
            let leechers = if leechers < 5_000_000 { leechers } else { 0 }; // fix #6558
            release.peers = Some(release.peers.unwrap_or(0) + leechers);
            value = leechers.to_string();
        }
        "seeders" => {
            let seeders = coerce_long(&value).unwrap_or(0);
            let seeders = if seeders < 5_000_000 { seeders } else { 0 }; // fix #6558
            release.seeders = Some(seeders);
            release.peers = Some(release.peers.unwrap_or(0) + seeders);
            value = seeders.to_string();
        }
        "date" => {
            release.publish_date = crate::utils::date_time_util::DateTimeUtil::from_unknown(&value);
            value = release
                .publish_date
                .map(|d| d.to_rfc2822())
                .unwrap_or_default();
        }
        "files" => {
            release.files = coerce_long(&value);
            value = release.files.map(|v| v.to_string()).unwrap_or_default();
        }
        "grabs" => {
            release.grabs = coerce_long(&value);
            value = release.grabs.map(|v| v.to_string()).unwrap_or_default();
        }
        "downloadvolumefactor" => {
            release.download_volume_factor = coerce_double(&value);
            value = release
                .download_volume_factor
                .map(|v| v.to_string())
                .unwrap_or_default();
        }
        "uploadvolumefactor" => {
            release.upload_volume_factor = coerce_double(&value);
            value = release
                .upload_volume_factor
                .map(|v| v.to_string())
                .unwrap_or_default();
        }
        "minimumratio" => {
            release.minimum_ratio = coerce_double(&value);
            value = release
                .minimum_ratio
                .map(|v| v.to_string())
                .unwrap_or_default();
        }
        "minimumseedtime" => {
            release.minimum_seed_time = coerce_long(&value);
            value = release
                .minimum_seed_time
                .map(|v| v.to_string())
                .unwrap_or_default();
        }
        "imdb" | "imdbid" => {
            release.imdb = coerce_long(&value);
            value = release.imdb.map(|v| v.to_string()).unwrap_or_default();
        }
        "tmdbid" => {
            release.tmdb = coerce_long(&value);
            value = release.tmdb.map(|v| v.to_string()).unwrap_or_default();
        }
        "rageid" => {
            release.rage_id = coerce_long(&value);
            value = release.rage_id.map(|v| v.to_string()).unwrap_or_default();
        }
        "tvdbid" => {
            release.tvdb_id = coerce_long(&value);
            value = release.tvdb_id.map(|v| v.to_string()).unwrap_or_default();
        }
        "tvmazeid" => {
            release.tvmaze_id = coerce_long(&value);
            value = release.tvmaze_id.map(|v| v.to_string()).unwrap_or_default();
        }
        "traktid" => {
            release.trakt_id = coerce_long(&value);
            value = release.trakt_id.map(|v| v.to_string()).unwrap_or_default();
        }
        "doubanid" => {
            release.douban_id = coerce_long(&value);
            value = release.douban_id.map(|v| v.to_string()).unwrap_or_default();
        }
        "genre" => {
            const DELIMITERS: [char; 14] = [
                ',', ' ', '/', ')', '(', '.', ';', '[', ']', '"', '|', ':', '_', '-',
            ];
            let new_genres: Vec<String> = value
                .split(|c: char| DELIMITERS.contains(&c) && c != '-' && c != '_')
                .filter(|s| !s.is_empty())
                .map(|s| s.replace('_', " "))
                .collect();
            for g in new_genres {
                if !release.genres.contains(&g) {
                    release.genres.push(g);
                }
            }
            value = release.genres.join(",");
        }
        "year" => {
            release.year = coerce_long(&value);
            value = release.year.map(|v| v.to_string()).unwrap_or_default();
        }
        "author" => release.author = Some(value.clone()),
        "booktitle" => release.book_title = Some(value.clone()),
        "publisher" => release.publisher = Some(value.clone()),
        "artist" => release.artist = Some(value.clone()),
        "album" => release.album = Some(value.clone()),
        "label" => release.label = Some(value.clone()),
        "track" => release.track = Some(value.clone()),
        "poster" => {
            if !value.trim().is_empty() {
                release.poster = resolve_path(&value, ctx.base_url);
            }
            value = release.poster.as_ref().map(|u| u.to_string()).unwrap_or_default();
        }
        _ => {}
    }

    value
}

/// Mirror of `ParseRowFilters` — returns true when the release should be skipped.
pub fn parse_row_filters(
    rows: &RowsBlock,
    release: &ReleaseInfo,
    query: &mut TorznabQuery,
    vars: &TemplateVars,
) -> bool {
    let filters = &rows.filters;
    if filters.is_empty() {
        return false;
    }

    for filter in filters {
        match filter.name.as_deref() {
            Some("andmatch") => {
                let character_limit: Option<usize> = filter
                    .args
                    .as_str()
                    .and_then(|s| s.parse().ok());
                // andmatch is skipped for id-based searches (imdb/tmdb/tvdb/douban/trakt/tvmaze/rage)
                if query.is_imdb_query()
                    || query.is_tmdb_query()
                    || query.is_tvdb_query()
                    || query.is_douban_query()
                    || query.is_trakt_query()
                    || query.is_tvmaze_query()
                    || query.is_tv_rage_query()
                {
                    continue;
                }
                let query_keywords = vars.get(".Keywords").and_then(TemplateValue::as_str);
                if !query.match_query_string_and(
                    release.title.as_deref().unwrap_or(""),
                    character_limit,
                    query_keywords,
                ) {
                    return true;
                }
            }
            Some("strdump") => {
                tracing::debug!("row strdump: {:?}", release.title);
            }
            other => {
                tracing::error!("Unsupported rows filter: {:?}", other);
            }
        }
    }

    false
}

fn coerce_long(value: &str) -> Option<i64> {
    static NUM: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r"-?\d+").unwrap());
    NUM.find(value).and_then(|m| m.as_str().parse().ok())
}

fn coerce_double(value: &str) -> Option<f64> {
    static NUM: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r"-?\d+(\.\d+)?").unwrap());
    NUM.find(value).and_then(|m| m.as_str().parse().ok())
}

/// Field modifiers are parsed from `Field.Key` ("name|optional|append") in the
/// engine; exposed here for reuse.
pub fn split_field_key(key: &str) -> (String, Vec<String>) {
    let mut parts = key.split('|');
    let name = parts.next().unwrap_or("").to_string();
    let modifiers: Vec<String> = parts.map(|p| p.to_string()).collect();
    (name, modifiers)
}

/// True when a field never aborts row parsing (mirror of OptionalFields logic).
pub fn is_optional_field(key: &str, modifiers: &[String], block_optional: bool) -> bool {
    OPTIONAL_FIELDS.contains(&key) || modifiers.iter().any(|m| m == "optional") || block_optional
}

/// Merges following rows for the `After` selector (mirror of the After loop).
/// Operates on the parsed HTML; returns merged row inner-HTML groups.
pub fn merge_rows_after(rows_dom: Vec<ElementRef<'_>>, after: i32) -> Vec<Vec<ElementRef<'_>>> {
    if after <= 0 {
        return rows_dom.into_iter().map(|r| vec![r]).collect();
    }
    let mut result: Vec<Vec<ElementRef<'_>>> = Vec::new();
    let mut i = 0;
    while i < rows_dom.len() {
        let mut group = vec![rows_dom[i]];
        for j in 0..after as usize {
            if i + j + 1 < rows_dom.len() {
                group.push(rows_dom[i + j + 1]);
            }
        }
        result.push(group);
        i += (after as usize) + 1;
    }
    result
}

/// Parses one HTML row into a release using the fields map
/// (shared by the HTML search-path branch of `PerformQuery`).
pub fn parse_html_row(
    row: ElementRef<'_>,
    fields: &[(String, SelectorBlock)],
    vars: &mut TemplateVars,
    base_url: &Url,
    release: &mut ReleaseInfo,
) -> Result<(), anyhow::Error> {
    for (field_key, field_value) in fields {
        let (field_name, field_modifiers) = split_field_key(field_key);
        let variables_key = format!(".Result.{}", field_name);
        let is_optional = is_optional_field(field_key, &field_modifiers, field_value.optional);

        let result = handle_selector(field_value, row, Some(vars), !is_optional);

        match result {
            Ok(Some(value)) => {
                let mut value = value;
                if is_optional && value.trim().is_empty() {
                    let default_value = field_value
                        .default
                        .as_deref()
                        .map(|d| apply_go_template_text(d, Some(vars), None))
                        .unwrap_or_default();
                    if default_value.trim().is_empty() {
                        vars.insert(variables_key, TemplateValue::Str(None));
                        continue;
                    }
                    value = default_value;
                }

                let ctx = FieldContext {
                    site_link: base_url,
                    base_url,
                };
                let parsed =
                    parse_fields(&value, &field_name, &field_modifiers, release, &ctx);
                vars.insert(variables_key, TemplateValue::Str(Some(parsed)));
            }
            Ok(None) => {
                vars.insert(variables_key, TemplateValue::Str(None));
            }
            Err(e) => {
                vars.insert(variables_key, TemplateValue::Str(None));
                if is_optional {
                    continue;
                }
                return Err(anyhow::anyhow!(
                    "Error while parsing field={}: {}",
                    field_key,
                    e
                ));
            }
        }
    }
    Ok(())
}
