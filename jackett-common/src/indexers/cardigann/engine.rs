//! Mirror of `CardigannIndexer` itself: the `IIndexer` implementation with
//! DoLogin/TestLogin/PerformQuery/Download. HTTP uses reqwest with a shared
//! cookie jar; HTML parsing uses scraper; JSON uses serde_json.

use std::collections::HashMap;
use std::sync::{Mutex, RwLock};

use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{ElementRef, Html};
use serde_json::Value as JsonValue;
use url::Url;

use super::definition::{apply_definition_defaults, OPTIONAL_FIELDS};
use super::filters::apply_filters;
use super::parsing::{
    build_keywords, build_query_variables, build_search_request, handle_json_selector,
    handle_selector, json_parse_rows_selector, merge_rows_after, parse_fields, parse_html_row,
    parse_row_filters, resolve_path, FieldContext,
};
use super::template::{apply_go_template_text, get_base_template_variables, TemplateValue, TemplateVars};
use crate::indexers::i_indexer::IIndexer;
use crate::models::indexer_definition::{
    IndexerDefinition, LoginBlock, RequestBlock, SelectorField, SelectorBlock,
};
use crate::models::release_info::ReleaseInfo;
use crate::models::torznab_capabilities::TorznabCapabilities;
use crate::models::torznab_query::TorznabQuery;
use crate::utils::browser_util::BrowserUtil;

/// Runtime user configuration for an indexer (settings values + cookies).
#[derive(Debug, Clone, Default)]
pub struct CardigannConfig {
    pub settings: HashMap<String, String>,
    /// Cookie header captured from a previous login.
    pub cookie_header: Option<String>,
    pub site_link_override: Option<String>,
}

pub struct CardigannIndexer {
    pub definition: IndexerDefinition,
    pub site_link: RwLock<Url>,
    pub config: RwLock<CardigannConfig>,
    pub torznab_caps: TorznabCaps,
    pub default_categories: Vec<String>,
    /// Cache of the landing page HTML (used by form login). Stored as raw text
    /// because `scraper::Html` is not Sync.
    landing_document: Mutex<Option<String>>,
    http: reqwest::Client,
}

pub struct TorznabCaps {
    pub inner: TorznabCapabilities,
    /// category mapping state from Caps blocks
    pub category_mappings: Vec<(String, Option<String>, i32)>,
}

impl CardigannIndexer {
    /// Mirror of `AddTemplateVariablesFromUri` — exposes the download target
    /// URI to templates as `.DownloadUri.*`.
    fn add_download_uri_variables(&self, variables: &mut TemplateVars, uri: &Url) {
        let path_and_query = match uri.query() {
            Some(q) => format!("{}?{}", uri.path(), q),
            None => uri.path().to_string(),
        };
        let str_val = |s: String| TemplateValue::Str(Some(s));
        variables.insert(".DownloadUri.AbsolutePath".into(), str_val(uri.path().to_string()));
        variables.insert(
            ".DownloadUri.Host".into(),
            str_val(uri.host_str().unwrap_or_default().to_string()),
        );
        variables.insert(".DownloadUri.PathAndQuery".into(), str_val(path_and_query));
        variables.insert(".DownloadUri.AbsoluteUri".into(), str_val(uri.to_string()));
        variables.insert(".DownloadUri.Scheme".into(), str_val(uri.scheme().to_string()));
    }

    /// Runs a `download.before` request block: evaluates its path and inputs
    /// templates (e.g. `hashes: {{ re_replace .DownloadUri.AbsolutePath ... }}`),
    /// issues the request and returns the response text for later blocks.
    /// `path_override` supplies the path resolved from a `pathselector`.
    async fn run_before_request(
        &self,
        before: &crate::models::indexer_definition::BeforeBlock,
        path_override: Option<String>,
        variables: &TemplateVars,
        headers: &HashMap<String, String>,
    ) -> Result<String, anyhow::Error> {
        let path = match path_override {
            Some(p) => p,
            None => apply_go_template_text(
                before.path.as_deref().unwrap_or(""),
                Some(variables),
                None,
            ),
        };
        let url = if before.inputs.is_empty() {
            resolve_path(&path, &self.site_link())
                .ok_or_else(|| anyhow::anyhow!("Invalid before path: {}", path))?
        } else {
            let query: Vec<(String, String)> = before
                .inputs
                .iter()
                .map(|(k, v)| (k.clone(), apply_go_template_text(v, Some(variables), None)))
                .collect();
            let qs = super::parsing::encode_query_string(&query);
            resolve_path(&format!("{}?{}", path, qs), &self.site_link())
                .ok_or_else(|| anyhow::anyhow!("Invalid before path: {}?{}", path, qs))?
        };
        let method = if before.method.as_deref() == Some("post") {
            reqwest::Method::POST
        } else {
            reqwest::Method::GET
        };
        let response = self.request_with_cookies(&url, method, headers, None).await?;
        Ok(response.text().await?)
    }

    /// Mirror of the CardigannIndexer constructor.
    pub fn new(definition: IndexerDefinition, config: CardigannConfig) -> Result<Self, anyhow::Error> {
        let definition = apply_definition_defaults(definition);

        // init TorznabCaps from caps block
        let caps_block = definition
            .caps
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Cardigann definition is missing the caps"))?;

        let mut caps = TorznabCapabilities::new();
        caps.parse_cardigann_search_modes(&caps_block.modes)?;
        caps.supports_raw_search = caps_block.allow_raw_search;
        caps.tv_search_imdb_available = caps_block.allow_tv_search_imdb;

        let mut category_mappings = Vec::new();
        let mut default_categories = Vec::new();

        for (cat_id, cat_name) in &caps_block.categories {
            if let Some(cat) = crate::models::torznab_cat_type::TorznabCatType::get_cat_by_name(cat_name) {
                caps.categories
                    .add_category_mapping(cat_id, cat.clone(), None);
            } else {
                tracing::error!(
                    "CardigannIndexer ({}): invalid Torznab category for id {}: {}",
                    definition.id.as_deref().unwrap_or("?"),
                    cat_id,
                    cat_name
                );
            }
        }

        for mapping in &caps_block.category_mappings {
            let Some(mapping_id) = &mapping.id else { continue };
            let torznab_cat = mapping
                .cat
                .as_deref()
                .and_then(crate::models::torznab_cat_type::TorznabCatType::get_cat_by_name);
            if mapping.cat.is_some() && torznab_cat.is_none() {
                tracing::error!(
                    "CardigannIndexer ({}): invalid Torznab category for id {}: {}",
                    definition.id.as_deref().unwrap_or("?"),
                    mapping_id,
                    mapping.cat.as_deref().unwrap_or("")
                );
                continue;
            }
            if mapping.default {
                default_categories.push(mapping_id.clone());
            }
            caps.categories.add_category_mapping(
                mapping_id,
                match torznab_cat {
                    Some(cat) => cat.clone(),
                    None => crate::models::torznab_category::TorznabCategory::new(0, "unknown"),
                },
                mapping.desc.as_deref(),
            );
            category_mappings.push((
                mapping_id.clone(),
                mapping.desc.clone(),
                torznab_cat.map(|c| c.id).unwrap_or(0),
            ));
        }

        let site_link_str = config
            .site_link_override
            .clone()
            .or_else(|| definition.links.first().cloned())
            .unwrap_or_default();
        let site_link = Url::parse(&site_link_str)?;

        let http = reqwest::Client::builder()
            .user_agent(BrowserUtil::default_user_agent())
            .danger_accept_invalid_certs(!definition.certificates.is_empty())
            .build()?;

        Ok(Self {
            definition,
            site_link: RwLock::new(site_link),
            config: RwLock::new(config),
            torznab_caps: TorznabCaps {
                inner: caps,
                category_mappings,
            },
            default_categories,
            landing_document: Mutex::new(None),
            http,
        })
    }

    fn site_link(&self) -> Url {
        self.site_link.read().unwrap().clone()
    }

    /// Mirror of `GetBaseTemplateVariables`.
    fn get_base_variables(&self) -> TemplateVars {
        let config = self.config.read().unwrap();
        let settings_values: HashMap<String, TemplateValue> = config
            .settings
            .iter()
            .map(|(k, v)| (k.clone(), TemplateValue::from(v.as_str())))
            .collect();
        get_base_template_variables(
            self.site_link().as_str(),
            &self.definition.settings,
            &settings_values,
        )
    }

    /// Mirror of `ParseCustomHeaders`.
    fn parse_custom_headers(
        &self,
        custom_headers: Option<&HashMap<String, Vec<String>>>,
        vars: &TemplateVars,
    ) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        if let Some(custom) = custom_headers {
            for (key, values) in custom {
                if let Some(first) = values.first() {
                    headers.insert(
                        key.clone(),
                        apply_go_template_text(first, Some(vars), None),
                    );
                }
            }
        }
        headers
    }

    async fn request_with_cookies(
        &self,
        url: &Url,
        method: reqwest::Method,
        headers: &HashMap<String, String>,
        post_data: Option<&[(String, String)]>,
    ) -> Result<reqwest::Response, anyhow::Error> {
        let mut req = self.http.request(method, url.clone());
        for (k, v) in headers {
            req = req.header(k, v);
        }
        if let Some(data) = post_data {
            req = req.form(&data.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<Vec<_>>());
        }
        Ok(req.send().await?)
    }

    /// Mirror of `PerformQuery` main flow (search over all paths).
    pub async fn perform_query(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let search = self
            .definition
            .search
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("definition has no search block"))?;

        let mut variables = build_query_variables(query, self.get_base_variables());
        build_keywords(&mut variables, Some(&search.keywords_filters));

        let mut mapped_categories = self
            .torznab_caps
            .inner
            .categories
            .map_torznab_caps_to_trackers(query, false);
        if mapped_categories.is_empty() {
            mapped_categories = self.default_categories.clone();
        }

        let mut releases: Vec<ReleaseInfo> = Vec::new();
        let mut previous_urls: Vec<String> = Vec::new();

        for search_path in &search.paths {
            let request = match build_search_request(
                search,
                search_path,
                &self.site_link(),
                &variables,
                &mapped_categories,
                &mut previous_urls,
            )? {
                Some(r) => r,
                None => continue,
            };

            let mut headers = self.parse_custom_headers(Some(&search.headers), &variables);

            let response = self
                .request_with_cookies(&request.url, request.method.clone(), &headers, request.post_data.as_deref())
                .await?;
            let results = response.text().await?;

            self.run_search_path(search, search_path, &request, &results, &mut variables, query, &mut releases)?;
        }

        if query.limit > 0 {
            releases.truncate(query.limit as usize);
        }

        Ok(releases)
    }

    /// Executes a single search path against an already-fetched response body.
    /// Split out from `perform_query` so tests can drive the engine offline by
    /// injecting a captured HTML/JSON string (mirrors C# `ParseResponse`).
    fn run_search_path(
        &self,
        search: &crate::models::indexer_definition::SearchBlock,
        search_path: &crate::models::indexer_definition::SearchPathBlock,
        request: &super::parsing::GeneratedSearchRequest,
        results: &str,
        variables: &mut TemplateVars,
        query: &TorznabQuery,
        releases: &mut Vec<ReleaseInfo>,
    ) -> Result<(), anyhow::Error> {
        let is_json = search_path
            .response
            .as_ref()
            .map(|r| r.type_.as_deref() == Some("json"))
            .unwrap_or(false);

        if is_json {
            self.parse_json_response(search, search_path, request, results, variables, releases)?;
        } else {
            self.parse_html_response(search, search_path, request, results, variables, query, releases)?;
        }
        Ok(())
    }

    /// Offline variant of `perform_query`: builds the real requests (so the
    /// template/generated URL is exercised) but uses `responses` as the body
    /// for each path in order. Used by tests with captured fixtures.
    #[cfg(test)]
    pub(crate) fn perform_query_offline(
        &self,
        query: &TorznabQuery,
        responses: &[&str],
    ) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        let search = self
            .definition
            .search
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("definition has no search block"))?;

        let mut variables = build_query_variables(query, self.get_base_variables());
        build_keywords(&mut variables, Some(&search.keywords_filters));

        let mut mapped_categories = self
            .torznab_caps
            .inner
            .categories
            .map_torznab_caps_to_trackers(query, false);
        if mapped_categories.is_empty() {
            mapped_categories = self.default_categories.clone();
        }

        let mut releases: Vec<ReleaseInfo> = Vec::new();
        let mut previous_urls: Vec<String> = Vec::new();

        for (idx, search_path) in search.paths.iter().enumerate() {
            let request = match build_search_request(
                search,
                search_path,
                &self.site_link(),
                &variables,
                &mapped_categories,
                &mut previous_urls,
            )? {
                Some(r) => r,
                None => continue,
            };
            let body = responses.get(idx).copied().unwrap_or("");
            self.run_search_path(search, search_path, &request, body, &mut variables, query, &mut releases)?;
        }

        if query.limit > 0 {
            releases.truncate(query.limit as usize);
        }

        Ok(releases)
    }

    /// JSON branch of PerformQuery response handling.
    fn parse_json_response(
        &self,
        search: &crate::models::indexer_definition::SearchBlock,
        _search_path: &crate::models::indexer_definition::SearchPathBlock,
        request: &super::parsing::GeneratedSearchRequest,
        results: &str,
        variables: &mut TemplateVars,
        releases: &mut Vec<ReleaseInfo>,
    ) -> Result<(), anyhow::Error> {
        let parsed: JsonValue = serde_json::from_str(results)
            .map_err(|e| anyhow::anyhow!("Error Parsing Json Response: {}", e))?;

        let rows = search
            .rows
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("search.rows missing"))?;

        if let Some(count_block) = &rows.count {
            if let Ok(Some(count_val)) = handle_json_selector(count_block, &parsed, Some(variables), false) {
                if let Ok(count) = count_val.parse::<i64>() {
                    if count < 1 {
                        return Ok(());
                    }
                }
            }
        }

        let rows_selector = rows.selector.as_deref().unwrap_or("");
        let Some(rows_array) = json_parse_rows_selector(&parsed, rows_selector) else {
            if rows.missing_attribute_equals_no_results {
                return Ok(());
            }
            anyhow::bail!("Error Parsing Rows Selector. There are 0 rows.");
        };

        if rows_array.is_empty() {
            return Ok(());
        }

        for row in &rows_array {
            let sel_obj = match &rows.attribute {
                Some(attr) => select_json_token(row, attr),
                None => Some(row),
            };

            let mul_rows: Vec<&JsonValue> = if rows.multiple {
                sel_obj
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().collect())
                    .unwrap_or_default()
            } else {
                sel_obj.map(|v| vec![v]).unwrap_or_default()
            };

            for mul_row in mul_rows {
                let mut release = ReleaseInfo::new();
                self.parse_json_fields(mul_row, row, search, variables, &request.url, &mut release)?;
                if parse_row_filters(rows, &release, &mut {
                    TorznabQuery::new()
                }, variables) {
                    continue;
                }
                releases.push(release);
            }
        }

        Ok(())
    }

    /// Fields loop for JSON rows.
    fn parse_json_fields(
        &self,
        mul_row: &JsonValue,
        row: &JsonValue,
        search: &crate::models::indexer_definition::SearchBlock,
        variables: &mut TemplateVars,
        base_url: &Url,
        release: &mut ReleaseInfo,
    ) -> Result<(), anyhow::Error> {
        let fields = &search.fields;
        if fields.is_empty() {
            return Ok(());
        }
        for (field_key, field_value) in fields {
            let (field_name, field_modifiers) = super::parsing::split_field_key(field_key);
            let variables_key = format!(".Result.{}", field_name);
            let is_optional = OPTIONAL_FIELDS.iter().any(|f| f == field_key)
                || field_modifiers.iter().any(|m| m == "optional")
                || field_value.optional;

            let parent_obj = if field_value
                .selector
                .as_deref()
                .map(|s| s.starts_with(".."))
                .unwrap_or(false)
            {
                row
            } else {
                mul_row
            };

            match handle_json_selector(field_value, parent_obj, Some(variables), !is_optional) {
                Ok(Some(value)) => {
                    let mut value = value;
                    if is_optional && value.trim().is_empty() {
                        let default_value = field_value
                            .default
                            .as_deref()
                            .map(|d| apply_go_template_text(d, Some(variables), None))
                            .unwrap_or_default();
                        if default_value.trim().is_empty() {
                            variables.insert(variables_key, TemplateValue::Str(None));
                            continue;
                        }
                        value = default_value;
                    }
                    let ctx = FieldContext { site_link: base_url, base_url };
                    let parsed = parse_fields(&value, &field_name, &field_modifiers, release, &ctx);
                    variables.insert(variables_key, TemplateValue::Str(Some(parsed)));
                }
                Ok(None) => {
                    variables.insert(variables_key, TemplateValue::Str(None));
                }
                Err(e) => {
                    variables.insert(variables_key, TemplateValue::Str(None));
                    if is_optional {
                        continue;
                    }
                    return Err(anyhow::anyhow!("Error while parsing field={}: {}", field_key, e));
                }
            }
        }
        Ok(())
    }

    /// HTML/XML branch of PerformQuery response handling.
    fn parse_html_response(
        &self,
        search: &crate::models::indexer_definition::SearchBlock,
        search_path: &crate::models::indexer_definition::SearchPathBlock,
        request: &super::parsing::GeneratedSearchRequest,
        results: &str,
        variables: &mut TemplateVars,
        query: &TorznabQuery,
        releases: &mut Vec<ReleaseInfo>,
    ) -> Result<(), anyhow::Error> {
        let _ = search_path;
        let rows = search
            .rows
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("search.rows missing"))?;

        // checkForError (search.error blocks)
        self.check_for_error_html(results, Some(&search.error))?;

        // preprocessingfilters may rewrite the HTML
        let mut results = results.to_string();
        if !search.preprocessing_filters.is_empty() {
            results = apply_filters(&results, Some(&search.preprocessing_filters), Some(variables));
        }

        let document = crate::indexers::cardigann::parsing::parse_html_safe(&results);
        let rows_selector = apply_go_template_text(
            rows.selector.as_deref().unwrap_or(""),
            Some(variables),
            None,
        );

        // collect row elements matching the selector (template-expanded)
        let mut row_elements: Vec<ElementRef<'_>> = Vec::new();
        if let Ok(css) = scraper::Selector::parse(&rows_selector) {
            row_elements.extend(document.select(&css));
        }

        let after = rows.after;
        let row_groups = merge_rows_after(row_elements, after);

        for group in row_groups {
            let row = group[0];
            let mut release = ReleaseInfo::new();

            if !search.fields.is_empty() {
                if let Err(e) = parse_html_row(row, &search.fields, variables, &request.url, &mut release) {
                    tracing::error!("CardigannIndexer: error while parsing row: {}", e);
                    continue;
                }
            }

            if parse_row_filters(rows, &release, &mut query.clone(), variables) {
                continue;
            }

            // DateHeaders: look back through previous sibling rows
            if release.publish_date.is_none() {
                if let Some(date_headers) = &rows.date_headers {
                    if let Some(value) = self.find_date_header(document.root_element(), date_headers) {
                        release.publish_date = crate::utils::date_time_util::DateTimeUtil::from_unknown(&value);
                    }
                }
            }

            release.origin = self.definition.id.clone();
            releases.push(release);
        }

        Ok(())
    }

    /// Searches previous siblings for the date header (simplified traversal).
    fn find_date_header(&self, root: ElementRef<'_>, date_headers: &SelectorBlock) -> Option<String> {
        // walk all elements and try the header selector on each; the first match wins
        // (C# walked backwards from the data row)
        let mut result = None;
        'outer: for el in root.descendants() {
            let el_ref = ElementRef::wrap(el)?;
            if let Ok(Some(v)) = handle_selector(date_headers, el_ref, None, false) {
                result = Some(v);
                break 'outer;
            }
        }
        result
    }

    /// Mirror of `checkForError` (HTML branch).
    fn check_for_error_html(
        &self,
        content: &str,
        error_blocks: Option<&Vec<crate::models::indexer_definition::ErrorBlock>>,
    ) -> Result<(), anyhow::Error> {
        let Some(error_blocks) = error_blocks else {
            return Ok(());
        };
        let document = crate::indexers::cardigann::parsing::parse_html_safe(content);
        for error in error_blocks {
            let Some(selector_str) = &error.selector else { continue };
            let Ok(css) = scraper::Selector::parse(selector_str) else { continue };
            if let Some(selection) = document.select(&css).next() {
                let message = match &error.message {
                    Some(message_block) => {
                        handle_selector(message_block, selection, None, false)?
                            .unwrap_or_else(|| selection.text().collect())
                    }
                    None => selection.text().collect(),
                };
                anyhow::bail!("Error: {}", message.trim());
            }
        }
        Ok(())
    }

    /// Mirror of `DoLogin`.
    pub async fn do_login(&self) -> Result<bool, anyhow::Error> {
        let Some(login) = &self.definition.login else {
            return Ok(true);
        };
        let variables = self.get_base_variables();
        let headers = self.parse_custom_headers(
            if login.headers.is_empty() {
                self.definition.search.as_ref().map(|s| &s.headers)
            } else {
                Some(&login.headers)
            },
            &variables,
        );

        let cookie_header_value = if login.cookies.is_empty() {
            None
        } else {
            Some(login.cookies.join("; "))
        };

        match login.method.as_deref() {
            Some("post") => {
                let mut pairs = Vec::new();
                for (key, value) in &login.inputs {
                    pairs.push((key.clone(), apply_go_template_text(value, Some(&variables), None)));
                }
                let path = apply_go_template_text(login.path.as_deref().unwrap_or(""), Some(&variables), None);
                let login_url = resolve_path(&path, &self.site_link())
                    .ok_or_else(|| anyhow::anyhow!("Invalid login path"))?;
                let response = self
                    .request_with_cookies(&login_url, reqwest::Method::POST, &headers, Some(&pairs))
                    .await?;
                self.capture_cookies(&response).await?;
                self.check_for_error_response(response, Some(&login.error)).await?;
            }
            Some("form") => {
                self.do_form_login(login, &variables, &headers).await?;
            }
            Some("cookie") => {
                // config value "cookie" supplies the header directly
            }
            Some("get") => {
                let mut query: Vec<(String, String)> = Vec::new();
                for (key, value) in &login.inputs {
                    query.push((key.clone(), apply_go_template_text(value, Some(&variables), None)));
                }
                let path = apply_go_template_text(login.path.as_deref().unwrap_or(""), Some(&variables), None);
                let qs = super::parsing::encode_query_string(&query);
                let login_url = resolve_path(&format!("{}?{}", path, qs), &self.site_link())
                    .ok_or_else(|| anyhow::anyhow!("Invalid login path"))?;
                let response = self
                    .request_with_cookies(&login_url, reqwest::Method::GET, &headers, None)
                    .await?;
                self.capture_cookies(&response).await?;
            }
            Some("oneurl") => {
                let one_url = apply_go_template_text(
                    login.inputs.get("oneurl").map(|s| s.as_str()).unwrap_or(""),
                    Some(&variables),
                    None,
                );
                let path = apply_go_template_text(login.path.as_deref().unwrap_or(""), Some(&variables), None);
                let login_url = resolve_path(&format!("{}{}", path, one_url), &self.site_link())
                    .ok_or_else(|| anyhow::anyhow!("Invalid login path"))?;
                let response = self
                    .request_with_cookies(&login_url, reqwest::Method::GET, &headers, None)
                    .await?;
                self.capture_cookies(&response).await?;
            }
            other => anyhow::bail!("Login method {:?} not implemented", other),
        }

        let _ = cookie_header_value;
        Ok(true)
    }

    /// The "form" login flow: fetch landing page, fill the form, submit.
    async fn do_form_login(
        &self,
        login: &LoginBlock,
        variables: &TemplateVars,
        headers: &HashMap<String, String>,
    ) -> Result<(), anyhow::Error> {
        let path = apply_go_template_text(login.path.as_deref().unwrap_or(""), Some(variables), None);
        let login_url = resolve_path(&path, &self.site_link())
            .ok_or_else(|| anyhow::anyhow!("Invalid login path"))?;

        // fetch landing page
        let response = self
            .request_with_cookies(&login_url, reqwest::Method::GET, headers, None)
            .await?;
        let content = response.text().await?;
        *self.landing_document.lock().unwrap() = Some(content.clone());
        let document = crate::indexers::cardigann::parsing::parse_html_safe(&content);

        let form_selector_str = login.form.as_deref().unwrap_or("form");
        let root = document.root_element();
        let form = {
            let css = scraper::Selector::parse(form_selector_str)
                .map_err(|e| anyhow::anyhow!("Invalid form selector: {}", e))?;
            root.select(&css).next()
        };

        let Some(form) = form else {
            anyhow::bail!(
                "Login failed: No form found on {} using form selector {}",
                login_url,
                form_selector_str
            );
        };

        let mut pairs: Vec<(String, String)> = Vec::new();
        let input_css = scraper::Selector::parse("input").unwrap();
        for input in form.select(&input_css) {
            let Some(name) = input.value().attr("name") else { continue };
            if input.value().attr("disabled").is_some() {
                continue;
            }
            let value = input.value().attr("value").unwrap_or("").to_string();
            pairs.push((name.to_string(), value));
        }

        // template inputs
        for (key, value) in &login.inputs {
            let mut name = key.clone();
            if login.selectors {
                let css = scraper::Selector::parse(key)
                    .map_err(|e| anyhow::anyhow!("Invalid login input selector: {}", e))?;
                let el = form.select(&css).next().ok_or_else(|| {
                    anyhow::anyhow!("Login failed: No input found using selector {}", key)
                })?;
                name = el.value().attr("name").unwrap_or(key).to_string();
            }
            pairs.push((name, apply_go_template_text(value, Some(variables), None)));
        }

        // selector inputs
        for (key, sel_block) in &login.selector_inputs {
            let value = handle_selector(sel_block, root, Some(variables), !sel_block.optional)?;
            if sel_block.optional && value.is_none() {
                continue;
            }
            pairs.push((key.clone(), value.unwrap_or_default()));
        }

        // get selector inputs -> query string
        let mut query_pairs: Vec<(String, String)> = Vec::new();
        for (key, sel_block) in &login.get_selector_inputs {
            let value = handle_selector(sel_block, root, Some(variables), !sel_block.optional)?;
            if sel_block.optional && value.is_none() {
                continue;
            }
            query_pairs.push((key.clone(), value.unwrap_or_default()));
        }

        let mut submit_url_str = form
            .value()
            .attr("action")
            .unwrap_or("")
            .to_string();
        if let Some(submitpath) = &login.submit_path {
            submit_url_str = submitpath.clone();
        }
        if !query_pairs.is_empty() {
            submit_url_str = format!(
                "{}?{}",
                submit_url_str,
                super::parsing::encode_query_string(&query_pairs)
            );
        }

        let submit_url = resolve_path(&submit_url_str, &login_url)
            .ok_or_else(|| anyhow::anyhow!("Invalid submit path: {}", submit_url_str))?;

        *self.landing_document.lock().unwrap() = None;

        let response = self
            .request_with_cookies(&submit_url, reqwest::Method::POST, headers, Some(&pairs))
            .await?;
        self.capture_cookies(&response).await?;
        self.check_for_error_response(response, Some(&login.error)).await?;
        Ok(())
    }

    /// Mirror of `checkForError` (status code branch + HTML error blocks).
    async fn check_for_error_response(
        &self,
        response: reqwest::Response,
        error_blocks: Option<&Vec<crate::models::indexer_definition::ErrorBlock>>,
    ) -> Result<(), anyhow::Error> {
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            anyhow::bail!("401 Unauthorized, check your credentials");
        }
        let content = response.text().await?;
        self.check_for_error_html(&content, error_blocks)
    }

    /// Captures the Set-Cookie headers into the stored cookie header.
    async fn capture_cookies(&self, response: &reqwest::Response) -> Result<(), anyhow::Error> {
        let mut cookies: Vec<String> = Vec::new();
        for value in response.headers().get_all(reqwest::header::SET_COOKIE) {
            if let Ok(c) = value.to_str() {
                let pair = c.split(';').next().unwrap_or("").trim();
                if !pair.is_empty() {
                    cookies.push(pair.to_string());
                }
            }
        }
        let mut config = self.config.write().unwrap();
        if !cookies.is_empty() {
            config.cookie_header = Some(cookies.join("; "));
        }
        Ok(())
    }

    /// Mirror of `TestLogin`.
    pub async fn test_login(&self) -> Result<bool, anyhow::Error> {
        let Some(login) = &self.definition.login else {
            return Ok(false);
        };
        let Some(test) = &login.test else {
            return Ok(false);
        };

        let test_url = resolve_path(test.path.as_deref().unwrap_or(""), &self.site_link())
            .ok_or_else(|| anyhow::anyhow!("Invalid login test path"))?;
        let headers = self.parse_custom_headers(Some(&login.headers), &self.get_base_variables());
        let response = self
            .request_with_cookies(&test_url, reqwest::Method::GET, &headers, None)
            .await?;

        if response.status().is_redirection() {
            anyhow::bail!(
                "Login Failed, got redirected to: {}",
                response.url()
            );
        }

        if let Some(selector_str) = &test.selector {
            let content = response.text().await?;
            let document = crate::indexers::cardigann::parsing::parse_html_safe(&content);
            let css = scraper::Selector::parse(selector_str)
                .map_err(|e| anyhow::anyhow!("Invalid login test selector: {}", e))?;
            if document.select(&css).next().is_none() {
                anyhow::bail!("Login failed: Selector \"{}\" didn't match", selector_str);
            }
        }

        Ok(true)
    }

    /// Mirror of `MatchSelector` for download selectors.
    fn match_selector(
        &self,
        content: &str,
        selector: &SelectorField,
        variables: &TemplateVars,
    ) -> Result<Option<String>, anyhow::Error> {
        let selector_text = apply_go_template_text(selector.selector.as_deref().unwrap_or(""), Some(variables), None);
        let document = crate::indexers::cardigann::parsing::parse_html_safe(content);
        let css = scraper::Selector::parse(&selector_text)
            .map_err(|e| anyhow::anyhow!("Invalid download selector: {}", e))?;
        let Some(element) = document.select(&css).next() else {
            return Ok(None);
        };
        let raw = match &selector.attribute {
            Some(attr) => element
                .value()
                .attr(attr)
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("Attribute \"{}\" is not set", attr))?,
            None => element.text().collect::<String>(),
        };
        let selector_block = SelectorBlock {
            selector: None,
            optional: false,
            default: None,
            text: None,
            attribute: None,
            remove: None,
            filters: selector.filters.clone(),
            case: HashMap::new(),
        };
        Ok(Some(apply_filters(
            &raw,
            Some(&selector_block.filters),
            Some(variables),
        )))
    }
}

/// Category mapping helpers bridging to the caps categories
/// (mirror of MapTrackerCatToNewznab/MapTrackerCatDescToNewznab on BaseWebIndexer).
/// The mapping table is populated when an indexer is registered.
static TRACKER_CAT_MAPPING: Lazy<Mutex<HashMap<String, Vec<i32>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn register_tracker_cat_mapping(tracker_category: &str, newznab: Vec<i32>) {
    TRACKER_CAT_MAPPING
        .lock()
        .unwrap()
        .insert(tracker_category.to_string(), newznab);
}

pub fn map_tracker_cat_to_newznab(value: &str) -> Vec<i32> {
    TRACKER_CAT_MAPPING
        .lock()
        .unwrap()
        .get(value)
        .cloned()
        .unwrap_or_default()
}

pub fn map_tracker_cat_desc_to_newznab(value: &str) -> Vec<i32> {
    map_tracker_cat_to_newznab(value)
}

fn select_json_token<'a>(json: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    let mut current = json;
    for segment in path.split('.').filter(|s| !s.is_empty()) {
        current = current.get(segment)?;
    }
    Some(current)
}

fn _unused(_: &dyn std::any::Any) {}

impl SelectorFieldLike for SelectorField {
    fn filters(&self) -> &[crate::models::indexer_definition::FilterBlock] {
        &self.filters
    }
}

trait SelectorFieldLike {
    fn filters(&self) -> &[crate::models::indexer_definition::FilterBlock];
}

#[async_trait]
impl IIndexer for CardigannIndexer {
    fn id(&self) -> &str {
        self.definition.id.as_deref().unwrap_or("")
    }

    fn name(&self) -> &str {
        self.definition.name.as_deref().unwrap_or("")
    }

    fn description(&self) -> &str {
        self.definition.description.as_deref().unwrap_or("")
    }

    fn type_(&self) -> &str {
        self.definition.type_.as_deref().unwrap_or("")
    }

    fn site_link(&self) -> &str {
        // return the static definition link; runtime link is a RwLock<Url>
        self.definition.links.first().map(|s| s.as_str()).unwrap_or("")
    }

    fn alternative_site_links(&self) -> Vec<String> {
        self.definition.links.clone()
    }

    fn language(&self) -> &str {
        self.definition.language.as_deref().unwrap_or("")
    }

    fn tags(&self) -> Vec<String> {
        Vec::new()
    }

    fn is_configured(&self) -> bool {
        self.config.read().unwrap().cookie_header.is_some()
            || self.definition.login.is_none()
    }

    fn last_error(&self) -> Option<String> {
        None
    }

    fn torznab_caps(&self) -> &TorznabCapabilities {
        &self.torznab_caps.inner
    }

    async fn query(&self, query: &TorznabQuery) -> Result<Vec<ReleaseInfo>, anyhow::Error> {
        self.perform_query(query).await
    }

    async fn download(&self, link: &Url) -> Result<Vec<u8>, anyhow::Error> {
        // Magnet links cannot be fetched over HTTP — hand the URI itself back
        // (clients and the blackhole folder accept magnet text).
        if link.scheme() == "magnet" {
            return Ok(link.to_string().into_bytes());
        }

        let mut variables = self.get_base_variables();
        self.add_download_uri_variables(&mut variables, link);

        let headers = self.parse_custom_headers(
            self.definition
                .download
                .as_ref()
                .filter(|d| !d.headers.is_empty())
                .map(|d| &d.headers)
                .or_else(|| self.definition.search.as_ref().map(|s| &s.headers)),
            &variables,
        );

        let download = &self.definition.download;

        if let Some(download) = download {
            // Before block: fetch the auxiliary request (api/json_info style)
            // and keep its body for infohash blocks that use it.
            let mut before_response: Option<String> = None;
            if let Some(before) = &download.before {
                if let Some(path_selector) = &before.path_selector {
                    let response = self
                        .request_with_cookies(link, reqwest::Method::GET, &headers, None)
                        .await?;
                    let content = response.text().await?;
                    if let Some(path) = self.match_selector(&content, path_selector, &variables)? {
                        before_response =
                            Some(self.run_before_request(before, Some(path), &variables, &headers).await?);
                    }
                } else if before.path.is_some() {
                    before_response = Some(self.run_before_request(before, None, &variables, &headers).await?);
                }
            }

            // Infohash block: extract hash/title from the before response (or a
            // fresh request to the link) and build the magnet URI, mirroring C#.
            if let Some(infohash) = &download.infohash {
                let content = if infohash.use_before_response && before_response.is_some() {
                    before_response.clone().unwrap()
                } else {
                    let response = self
                        .request_with_cookies(link, reqwest::Method::GET, &headers, None)
                        .await?;
                    response.text().await?
                };
                let hash = self
                    .match_selector(&content, infohash.hash.as_ref().expect("infohash requires hash selector"), &variables)?
                    .ok_or_else(|| anyhow::anyhow!("InfoHash selectors didn't match hash."))?;
                let title = self
                    .match_selector(&content, infohash.title.as_ref().expect("infohash requires title selector"), &variables)?
                    .ok_or_else(|| anyhow::anyhow!("InfoHash selectors didn't match title."))?;
                let magnet = crate::utils::magnet_util::MagnetUtil::magnet_from_info_hash(&hash, Some(&title));
                return Ok(magnet.into_bytes());
            }

            // Selector-based download
            if !download.selectors.is_empty() {
                let response = self
                    .request_with_cookies(link, reqwest::Method::GET, &headers, None)
                    .await?;
                let content = response.text().await?;
                for selector in &download.selectors {
                    if let Ok(Some(href)) = self.match_selector(&content, selector, &variables) {
                        if let Some(torrent_link) = resolve_path(&href, link) {
                            if torrent_link.scheme() == "magnet" {
                                return Ok(href.into_bytes());
                            }
                            let resp = self
                                .request_with_cookies(&torrent_link, reqwest::Method::GET, &headers, None)
                                .await?;
                            let bytes = resp.bytes().await?;
                            if !bytes.is_empty() && bytes[0] == b'd' {
                                return Ok(bytes.to_vec());
                            }
                        }
                    }
                }
                anyhow::bail!("Download selectors didn't match");
            }
        }

        // plain download
        let response = self
            .request_with_cookies(link, reqwest::Method::GET, &headers, None)
            .await?;
        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }
}

fn _unused2() {}
