use serde::{Deserialize, Serialize};

use super::torznab_category::TorznabCategory;
use super::torznab_cat_type::TorznabCatType;
use super::torznab_query::TorznabQuery;
use super::category_mapping::CategoryMapping;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TvSearchParam {
    Q,
    Season,
    Ep,
    ImdbId,
    TvdbId,
    RId,
    TmdbId,
    TvmazeId,
    TraktId,
    DoubanId,
    Year,
    Genre,
}

impl TvSearchParam {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "q" => Self::Q,
            "season" => Self::Season,
            "ep" => Self::Ep,
            "imdbid" => Self::ImdbId,
            "tvdbid" => Self::TvdbId,
            "rid" => Self::RId,
            "tmdbid" => Self::TmdbId,
            "tvmazeid" => Self::TvmazeId,
            "traktid" => Self::TraktId,
            "doubanid" => Self::DoubanId,
            "year" => Self::Year,
            "genre" => Self::Genre,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MovieSearchParam {
    Q,
    ImdbId,
    TmdbId,
    TraktId,
    DoubanId,
    Year,
    Genre,
}

impl MovieSearchParam {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "q" => Self::Q,
            "imdbid" => Self::ImdbId,
            "tmdbid" => Self::TmdbId,
            "traktid" => Self::TraktId,
            "doubanid" => Self::DoubanId,
            "year" => Self::Year,
            "genre" => Self::Genre,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MusicSearchParam {
    Q,
    Album,
    Artist,
    Label,
    Track,
    Year,
    Genre,
}

impl MusicSearchParam {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "q" => Self::Q,
            "album" => Self::Album,
            "artist" => Self::Artist,
            "label" => Self::Label,
            "track" => Self::Track,
            "year" => Self::Year,
            "genre" => Self::Genre,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BookSearchParam {
    Q,
    Title,
    Author,
    Publisher,
    Year,
    Genre,
}

impl BookSearchParam {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "q" => Self::Q,
            "title" => Self::Title,
            "author" => Self::Author,
            "publisher" => Self::Publisher,
            "year" => Self::Year,
            "genre" => Self::Genre,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct TorznabCapabilities {
    pub limits_max: Option<i32>,
    pub limits_default: Option<i32>,
    pub search_available: bool,
    pub supports_raw_search: bool,
    pub tv_search_params: Vec<TvSearchParam>,
    /// Introduce setter so individual trackers can override (temporarily disabled
    /// in C# due to #8107).
    pub tv_search_imdb_available: bool,
    pub movie_search_params: Vec<MovieSearchParam>,
    pub music_search_params: Vec<MusicSearchParam>,
    pub book_search_params: Vec<BookSearchParam>,
    pub categories: TorznabCapabilitiesCategories,
}

impl Default for TvSearchDefaults {
    fn default() -> Self {
        Self
    }
}

struct TvSearchDefaults;

impl TorznabCapabilities {
    pub fn new() -> Self {
        Self {
            supports_raw_search: false,
            search_available: true,
            limits_default: Some(100),
            limits_max: Some(100),
            ..Default::default()
        }
    }

    pub fn tv_search_available(&self) -> bool {
        !self.tv_search_params.is_empty()
    }
    pub fn tv_search_season_available(&self) -> bool {
        self.tv_search_params.contains(&TvSearchParam::Season)
    }
    pub fn tv_search_ep_available(&self) -> bool {
        self.tv_search_params.contains(&TvSearchParam::Ep)
    }
    pub fn tv_search_tvdb_available(&self) -> bool {
        self.tv_search_params.contains(&TvSearchParam::TvdbId)
    }
    pub fn tv_search_tv_rage_available(&self) -> bool {
        self.tv_search_params.contains(&TvSearchParam::RId)
    }
    pub fn tv_search_tmdb_available(&self) -> bool {
        self.tv_search_params.contains(&TvSearchParam::TmdbId)
    }
    pub fn tv_search_tvmaze_available(&self) -> bool {
        self.tv_search_params.contains(&TvSearchParam::TvmazeId)
    }
    pub fn tv_search_trakt_available(&self) -> bool {
        self.tv_search_params.contains(&TvSearchParam::TraktId)
    }
    pub fn tv_search_douban_available(&self) -> bool {
        self.tv_search_params.contains(&TvSearchParam::DoubanId)
    }
    pub fn tv_search_year_available(&self) -> bool {
        self.tv_search_params.contains(&TvSearchParam::Year)
    }
    pub fn tv_search_genre_available(&self) -> bool {
        self.tv_search_params.contains(&TvSearchParam::Genre)
    }

    pub fn movie_search_available(&self) -> bool {
        !self.movie_search_params.is_empty()
    }
    pub fn movie_search_imdb_available(&self) -> bool {
        self.movie_search_params.contains(&MovieSearchParam::ImdbId)
    }
    pub fn movie_search_tmdb_available(&self) -> bool {
        self.movie_search_params.contains(&MovieSearchParam::TmdbId)
    }
    pub fn movie_search_trakt_available(&self) -> bool {
        self.movie_search_params.contains(&MovieSearchParam::TraktId)
    }
    pub fn movie_search_douban_available(&self) -> bool {
        self.movie_search_params.contains(&MovieSearchParam::DoubanId)
    }
    pub fn movie_search_year_available(&self) -> bool {
        self.movie_search_params.contains(&MovieSearchParam::Year)
    }
    pub fn movie_search_genre_available(&self) -> bool {
        self.movie_search_params.contains(&MovieSearchParam::Genre)
    }

    pub fn music_search_available(&self) -> bool {
        !self.music_search_params.is_empty()
    }
    pub fn music_search_album_available(&self) -> bool {
        self.music_search_params.contains(&MusicSearchParam::Album)
    }
    pub fn music_search_artist_available(&self) -> bool {
        self.music_search_params.contains(&MusicSearchParam::Artist)
    }
    pub fn music_search_label_available(&self) -> bool {
        self.music_search_params.contains(&MusicSearchParam::Label)
    }
    pub fn music_search_track_available(&self) -> bool {
        self.music_search_params.contains(&MusicSearchParam::Track)
    }
    pub fn music_search_year_available(&self) -> bool {
        self.music_search_params.contains(&MusicSearchParam::Year)
    }
    pub fn music_search_genre_available(&self) -> bool {
        self.music_search_params.contains(&MusicSearchParam::Genre)
    }

    pub fn book_search_available(&self) -> bool {
        !self.book_search_params.is_empty()
    }
    pub fn book_search_title_available(&self) -> bool {
        self.book_search_params.contains(&BookSearchParam::Title)
    }
    pub fn book_search_author_available(&self) -> bool {
        self.book_search_params.contains(&BookSearchParam::Author)
    }
    pub fn book_search_publisher_available(&self) -> bool {
        self.book_search_params.contains(&BookSearchParam::Publisher)
    }
    pub fn book_search_year_available(&self) -> bool {
        self.book_search_params.contains(&BookSearchParam::Year)
    }
    pub fn book_search_genre_available(&self) -> bool {
        self.book_search_params.contains(&BookSearchParam::Genre)
    }

    pub fn parse_cardigann_search_modes(
        &mut self,
        modes: &std::collections::HashMap<String, Vec<String>>,
    ) -> anyhow::Result<()> {
        if modes.is_empty() {
            anyhow::bail!("At least one search mode is required");
        }
        if !modes.contains_key("search") {
            anyhow::bail!("The search mode 'search' is mandatory");
        }
        for (key, value) in modes {
            match key.as_str() {
                "search" => {
                    if value.len() != 1 || value[0] != "q" {
                        anyhow::bail!(
                            "In search mode 'search' only 'q' parameter is supported and it's mandatory"
                        );
                    }
                }
                "tv-search" => self.parse_tv_search_params(Some(value))?,
                "movie-search" => self.parse_movie_search_params(Some(value))?,
                "music-search" => self.parse_music_search_params(Some(value))?,
                "book-search" => self.parse_book_search_params(Some(value))?,
                other => anyhow::bail!("Unsupported search mode: {}", other),
            }
        }
        Ok(())
    }

    fn parse_tv_search_params(&mut self, params_list: Option<&Vec<String>>) -> anyhow::Result<()> {
        let Some(params) = params_list else { return Ok(()) };
        for param_str in params {
            match TvSearchParam::parse(param_str) {
                Some(param) => {
                    if self.tv_search_params.contains(&param) {
                        anyhow::bail!("Duplicate tv-search param: {}", param_str);
                    }
                    self.tv_search_params.push(param);
                }
                None => anyhow::bail!("Not supported tv-search param: {}", param_str),
            }
        }
        Ok(())
    }

    fn parse_movie_search_params(
        &mut self,
        params_list: Option<&Vec<String>>,
    ) -> anyhow::Result<()> {
        let Some(params) = params_list else { return Ok(()) };
        for param_str in params {
            match MovieSearchParam::parse(param_str) {
                Some(param) => {
                    if self.movie_search_params.contains(&param) {
                        anyhow::bail!("Duplicate movie-search param: {}", param_str);
                    }
                    self.movie_search_params.push(param);
                }
                None => anyhow::bail!("Not supported movie-search param: {}", param_str),
            }
        }
        Ok(())
    }

    fn parse_music_search_params(
        &mut self,
        params_list: Option<&Vec<String>>,
    ) -> anyhow::Result<()> {
        let Some(params) = params_list else { return Ok(()) };
        for param_str in params {
            match MusicSearchParam::parse(param_str) {
                Some(param) => {
                    if self.music_search_params.contains(&param) {
                        anyhow::bail!("Duplicate music-search param: {}", param_str);
                    }
                    self.music_search_params.push(param);
                }
                None => anyhow::bail!("Not supported music-search param: {}", param_str),
            }
        }
        Ok(())
    }

    fn parse_book_search_params(&mut self, params_list: Option<&Vec<String>>) -> anyhow::Result<()> {
        let Some(params) = params_list else { return Ok(()) };
        for param_str in params {
            match BookSearchParam::parse(param_str) {
                Some(param) => {
                    if self.book_search_params.contains(&param) {
                        anyhow::bail!("Duplicate book-search param: {}", param_str);
                    }
                    self.book_search_params.push(param);
                }
                None => anyhow::bail!("Not supported book-search param: {}", param_str),
            }
        }
        Ok(())
    }

    fn supported_tv_search_params(&self) -> String {
        let mut parameters = vec!["q"]; // q is always enabled
        if self.tv_search_season_available() {
            parameters.push("season");
        }
        if self.tv_search_ep_available() {
            parameters.push("ep");
        }
        if self.tv_search_imdb_available {
            parameters.push("imdbid");
        }
        if self.tv_search_tvdb_available() {
            parameters.push("tvdbid");
        }
        if self.tv_search_tv_rage_available() {
            parameters.push("rid");
        }
        if self.tv_search_tmdb_available() {
            parameters.push("tmdbid");
        }
        if self.tv_search_tvmaze_available() {
            parameters.push("tvmazeid");
        }
        if self.tv_search_trakt_available() {
            parameters.push("traktid");
        }
        if self.tv_search_douban_available() {
            parameters.push("doubanid");
        }
        if self.tv_search_year_available() {
            parameters.push("year");
        }
        if self.tv_search_genre_available() {
            parameters.push("genre");
        }
        parameters.join(",")
    }

    fn supported_movie_search_params(&self) -> String {
        let mut parameters = vec!["q"];
        if self.movie_search_imdb_available() {
            parameters.push("imdbid");
        }
        if self.movie_search_tmdb_available() {
            parameters.push("tmdbid");
        }
        if self.movie_search_trakt_available() {
            parameters.push("traktid");
        }
        if self.movie_search_douban_available() {
            parameters.push("doubanid");
        }
        if self.movie_search_year_available() {
            parameters.push("year");
        }
        if self.movie_search_genre_available() {
            parameters.push("genre");
        }
        parameters.join(",")
    }

    fn supported_music_search_params(&self) -> String {
        let mut parameters = vec!["q"];
        if self.music_search_album_available() {
            parameters.push("album");
        }
        if self.music_search_artist_available() {
            parameters.push("artist");
        }
        if self.music_search_label_available() {
            parameters.push("label");
        }
        if self.music_search_track_available() {
            parameters.push("track");
        }
        if self.music_search_year_available() {
            parameters.push("year");
        }
        if self.music_search_genre_available() {
            parameters.push("genre");
        }
        parameters.join(",")
    }

    fn supported_book_search_params(&self) -> String {
        let mut parameters = vec!["q"];
        if self.book_search_title_available() {
            parameters.push("title");
        }
        if self.book_search_author_available() {
            parameters.push("author");
        }
        if self.book_search_publisher_available() {
            parameters.push("publisher");
        }
        if self.book_search_year_available() {
            parameters.push("year");
        }
        if self.book_search_genre_available() {
            parameters.push("genre");
        }
        parameters.join(",")
    }

    fn searching_element(
        &self,
        tag: &str,
        available: bool,
        supported_params: &str,
    ) -> xml::Element {
        let mut el = xml::Element::new(tag);
        el.set_attr("available", if available { "yes" } else { "no" });
        el.set_attr("supportedParams", supported_params);
        if self.supports_raw_search {
            el.set_attr("searchEngine", "raw");
        }
        el
    }

    /// Mirror of `GetXDocument` — builds the `<caps>` XML document.
    pub fn get_x_document(&self) -> xml::Document {
        let mut caps = xml::Element::new("caps");

        let mut server = xml::Element::new("server");
        server.set_attr("title", "Jackett");
        caps.push_child(server);

        if self.limits_max.is_some() || self.limits_default.is_some() {
            let mut limits = xml::Element::new("limits");
            if let Some(d) = self.limits_default {
                limits.set_attr("default", &d.to_string());
            }
            if let Some(m) = self.limits_max {
                limits.set_attr("max", &m.to_string());
            }
            caps.push_child(limits);
        }

        let mut searching = xml::Element::new("searching");
        searching.push_child(self.searching_element(
            "search",
            self.search_available,
            "q",
        ));
        searching.push_child(self.searching_element(
            "tv-search",
            self.tv_search_available(),
            &self.supported_tv_search_params(),
        ));
        searching.push_child(self.searching_element(
            "movie-search",
            self.movie_search_available(),
            &self.supported_movie_search_params(),
        ));
        searching.push_child(self.searching_element(
            "music-search",
            self.music_search_available(),
            &self.supported_music_search_params(),
        ));
        // inconsistent but apparently already used by various newznab indexers (see #1896)
        searching.push_child(self.searching_element(
            "audio-search",
            self.music_search_available(),
            &self.supported_music_search_params(),
        ));
        searching.push_child(self.searching_element(
            "book-search",
            self.book_search_available(),
            &self.supported_book_search_params(),
        ));
        caps.push_child(searching);

        let mut categories = xml::Element::new("categories");
        for c in self.categories.get_torznab_category_tree(true) {
            let mut cat_el = xml::Element::new("category");
            cat_el.set_attr("id", &c.id.to_string());
            cat_el.set_attr("name", c.name.as_deref().unwrap_or(""));
            for sc in &c.sub_categories {
                let mut sub_el = xml::Element::new("subcat");
                sub_el.set_attr("id", &sc.id.to_string());
                sub_el.set_attr("name", sc.name.as_deref().unwrap_or(""));
                cat_el.push_child(sub_el);
            }
            categories.push_child(cat_el);
        }
        caps.push_child(categories);

        let mut doc = xml::Document::new();
        doc.set_root(caps);
        doc
    }

    pub fn to_xml(&self) -> String {
        format!("{}\n{}", xml::DECLARATION, self.get_x_document().to_string())
    }

    pub fn concat(mut lhs: TorznabCapabilities, rhs: TorznabCapabilities) -> TorznabCapabilities {
        lhs.search_available = lhs.search_available || rhs.search_available;
        lhs.tv_search_params = union(lhs.tv_search_params, rhs.tv_search_params);
        lhs.movie_search_params = union(lhs.movie_search_params, rhs.movie_search_params);
        lhs.music_search_params = union(lhs.music_search_params, rhs.music_search_params);
        lhs.book_search_params = union(lhs.book_search_params, rhs.book_search_params);
        lhs.categories.concat(&rhs.categories);
        lhs.supports_raw_search = lhs.supports_raw_search || rhs.supports_raw_search;
        lhs
    }
}

fn union<T: PartialEq + Clone>(lhs: Vec<T>, rhs: Vec<T>) -> Vec<T> {
    let mut out = lhs;
    for item in rhs {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

/// Minimal XML builder used for the Torznab caps document.
mod xml {
    pub struct Element {
        tag: String,
        attrs: Vec<(String, String)>,
        children: Vec<Node>,
    }

    enum Node {
        Element(Element),
        Text(String),
    }

    pub struct Document {
        root: Option<Element>,
    }

    pub const DECLARATION: &str = r#"<?xml version="1.0" encoding="UTF-8"?>"#;

    impl Element {
        pub fn new(tag: impl Into<String>) -> Self {
            Self {
                tag: tag.into(),
                attrs: Vec::new(),
                children: Vec::new(),
            }
        }

        pub fn set_attr(&mut self, name: &str, value: &str) {
            self.attrs.push((name.to_string(), value.to_string()));
        }

        pub fn push_child(&mut self, child: Element) {
            self.children.push(Node::Element(child));
        }
    }

    impl Document {
        pub fn new() -> Self {
            Self { root: None }
        }

        pub fn set_root(&mut self, root: Element) {
            self.root = Some(root);
        }
    }

    impl std::fmt::Display for Element {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            if self.attrs.is_empty() && self.children.is_empty() {
                return write!(f, "<{} />", self.tag);
            }
            write!(f, "<{}", self.tag)?;
            for (name, value) in &self.attrs {
                write!(f, " {}=\"{}\"", name, escape(value))?;
            }
            if self.children.is_empty() {
                write!(f, " />")
            } else {
                write!(f, ">")?;
                for child in &self.children {
                    match child {
                        Node::Element(el) => write!(f, "{}", el)?,
                        Node::Text(t) => write!(f, "{}", escape(t))?,
                    }
                }
                write!(f, "</{}>", self.tag)
            }
        }
    }

    fn escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }

    impl std::fmt::Display for Document {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match &self.root {
                Some(root) => write!(f, "{}", root),
                None => Ok(()),
            }
        }
    }
}

/// Mirror of Jackett.Common/Models/TorznabCapabilitiesCategories.cs
#[derive(Debug, Clone, Default)]
pub struct TorznabCapabilitiesCategories {
    category_mapping: Vec<CategoryMapping>,
    torznab_category_tree: Vec<TorznabCategory>,
}

impl TorznabCapabilitiesCategories {
    pub fn get_tracker_categories(&self) -> Vec<String> {
        self.category_mapping
            .iter()
            .filter(|m| m.newz_nab_category < 100000)
            .map(|m| m.tracker_category.clone())
            .collect::<Vec<_>>()
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn get_torznab_category_tree(&self, sorted: bool) -> Vec<&TorznabCategory> {
        if !sorted {
            return self.torznab_category_tree.iter().collect();
        }
        // first torznab categories ordered by id and then custom cats ordered by name
        let mut sorted_tree: Vec<&TorznabCategory> = self.torznab_category_tree.iter().collect();
        sorted_tree.sort_by_key(|c| {
            if c.id >= 100000 {
                format!("zzz{}", c.name.as_deref().unwrap_or(""))
            } else {
                c.id.to_string()
            }
        });
        sorted_tree
    }

    pub fn get_torznab_category_list(&self, sorted: bool) -> Vec<TorznabCategory> {
        let tree = self.get_torznab_category_tree(sorted);
        let mut flat = Vec::new();
        for cat in tree {
            flat.push(cat.copy_without_sub_categories());
            flat.extend(cat.sub_categories.iter().cloned());
        }
        flat
    }

    pub fn add_category_mapping_int(
        &mut self,
        tracker_category: i32,
        torznab_category: TorznabCategory,
        tracker_category_desc: Option<&str>,
    ) {
        self.add_category_mapping(
            &tracker_category.to_string(),
            torznab_category,
            tracker_category_desc,
        );
    }

    pub fn add_category_mapping(
        &mut self,
        tracker_category: &str,
        torznab_category: TorznabCategory,
        tracker_category_desc: Option<&str>,
    ) {
        self.category_mapping.push(CategoryMapping::new(
            tracker_category.to_string(),
            tracker_category_desc.map(|s| s.to_string()),
            torznab_category.id,
        ));
        self.add_torznab_category_tree(torznab_category.clone());

        let Some(desc) = tracker_category_desc else { return };

        // create custom cats (1:1 categories) if trackerCategoryDesc is defined
        let tracker_category_int = match tracker_category.parse::<i32>() {
            Ok(n) => n,
            Err(_) => {
                // hash to generate fixed integer id for the custom category
                use sha1_like::Sha1;
                let hashed = Sha1::digest(tracker_category.as_bytes());
                u16::from_le_bytes([hashed[0], hashed[1]]) as i32 // id between 0 and 65535 < 100000
            }
        };
        let custom_cat = TorznabCategory::new(tracker_category_int + 100000, desc);
        self.category_mapping.push(CategoryMapping::new(
            tracker_category.to_string(),
            Some(desc.to_string()),
            custom_cat.id,
        ));
        self.add_torznab_category_tree(custom_cat);
    }

    pub fn map_torznab_caps_to_trackers(
        &self,
        query: &TorznabQuery,
        map_children_cats_to_parent: bool,
    ) -> Vec<String> {
        let expanded = self.expand_torznab_query_categories(query, map_children_cats_to_parent);
        let mut seen = std::collections::HashSet::new();
        self.category_mapping
            .iter()
            .filter(|c| expanded.contains(&c.newz_nab_category))
            .filter(|m| seen.insert(m.tracker_category.clone()))
            .map(|m| m.tracker_category.clone())
            .collect()
    }

    pub fn map_tracker_cat_to_newznab(&self, tracker_category: &str) -> Vec<i32> {
        if tracker_category.trim().is_empty() {
            return Vec::new();
        }
        self.category_mapping
            .iter()
            .filter(|m| {
                !m.tracker_category.trim().is_empty()
                    && m.tracker_category.eq_ignore_ascii_case(tracker_category)
            })
            .map(|c| c.newz_nab_category)
            .collect()
    }

    pub fn map_tracker_cat_desc_to_newznab(&self, tracker_category_desc: &str) -> Vec<i32> {
        if tracker_category_desc.trim().is_empty() {
            return Vec::new();
        }
        self.category_mapping
            .iter()
            .filter(|m| {
                m.tracker_category_desc
                    .as_ref()
                    .map(|d| !d.trim().is_empty() && d.eq_ignore_ascii_case(tracker_category_desc))
                    .unwrap_or(false)
            })
            .map(|c| c.newz_nab_category)
            .collect()
    }

    pub fn supported_categories(&self, categories: &[i32]) -> Vec<i32> {
        if categories.is_empty() {
            return Vec::new();
        }
        let sub_categories = self
            .torznab_category_tree
            .iter()
            .flat_map(|c| c.sub_categories.iter());
        self.torznab_category_tree
            .iter()
            .chain(sub_categories)
            .filter(|c| categories.contains(&c.id))
            .map(|c| c.id)
            .collect()
    }

    pub fn concat(&mut self, rhs: &TorznabCapabilitiesCategories) {
        // exclude indexer specific categories (>= 100000)
        for cat in rhs.get_torznab_category_list(true) {
            if cat.id < 100000 {
                self.add_torznab_category_tree(cat);
            }
        }
    }

    /// If there are parent categories in TorznabQuery.Categories the children
    /// categories are added.
    pub fn expand_torznab_query_categories(
        &self,
        query: &TorznabQuery,
        map_children_cats_to_parent: bool,
    ) -> Vec<i32> {
        let mut expanded: Vec<i32> = Vec::new();
        for &query_category in &query.categories {
            expanded.push(query_category);
            if query_category >= 100000 {
                continue;
            }
            let parent_cat = self
                .torznab_category_tree
                .iter()
                .find(|c| c.id == query_category);
            if let Some(parent_cat) = parent_cat {
                // if it's parent cat we add all the children
                expanded.extend(parent_cat.sub_categories.iter().map(|c| c.id));
            } else if map_children_cats_to_parent {
                // if it's child cat and mapChildrenCatsToParent is enabled we add the parent
                let query_category_torznab = TorznabCategory::new(query_category, "");
                if let Some(parent_cat) = self
                    .torznab_category_tree
                    .iter()
                    .find(|c| c.contains(&query_category_torznab))
                {
                    expanded.push(parent_cat.id);
                }
            }
        }
        expanded.sort_unstable();
        expanded.dedup();
        expanded
    }

    fn add_torznab_category_tree(&mut self, torznab_category: TorznabCategory) {
        // build the category tree
        if TorznabCatType::parent_cats().iter().any(|c| c.id == torznab_category.id) {
            // parent cat
            if !self.torznab_category_tree.iter().any(|c| c.id == torznab_category.id) {
                self.torznab_category_tree
                    .push(torznab_category.copy_without_sub_categories());
            }
        } else {
            // child or custom cat
            let parent_cat = TorznabCatType::parent_cats()
                .into_iter()
                .find(|c| c.contains(&torznab_category));
            if let Some(parent_cat) = parent_cat {
                // child cat
                let node = self
                    .torznab_category_tree
                    .iter_mut()
                    .find(|c| c.id == parent_cat.id);
                match node {
                    Some(node_cat) => {
                        // parent cat already exists
                        if !node_cat.contains(&torznab_category) {
                            node_cat.sub_categories.push(torznab_category);
                        }
                    }
                    None => {
                        // create parent cat and add child
                        let mut node_cat = parent_cat.copy_without_sub_categories();
                        node_cat.sub_categories.push(torznab_category);
                        self.torznab_category_tree.push(node_cat);
                    }
                }
            } else {
                // custom cat
                self.torznab_category_tree.push(torznab_category);
            }
        }
    }
}

/// Minimal SHA-1 used for the custom-category hash (mirrors C# `SHA1.ComputeHash`
/// taking the first two bytes as UInt16 little-endian).
mod sha1_like {
    // Compact SHA-1 implementation (public-domain algorithm).
    pub struct Sha1;
    impl Sha1 {
        pub fn digest(data: &[u8]) -> [u8; 20] {
            let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
            let mut msg = data.to_vec();
            let bit_len = (data.len() as u64) * 8;
            msg.push(0x80);
            while msg.len() % 64 != 56 {
                msg.push(0);
            }
            msg.extend_from_slice(&bit_len.to_be_bytes());

            for chunk in msg.chunks(64) {
                let mut w = [0u32; 80];
                for (i, word) in chunk.chunks(4).enumerate() {
                    w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
                }
                for i in 16..80 {
                    w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
                }
                let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
                for (i, &wi) in w.iter().enumerate() {
                    let (f, k) = match i {
                        0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                        20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                        40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                        _ => (b ^ c ^ d, 0xCA62C1D6),
                    };
                    let temp = a
                        .rotate_left(5)
                        .wrapping_add(f)
                        .wrapping_add(e)
                        .wrapping_add(k)
                        .wrapping_add(wi);
                    e = d;
                    d = c;
                    c = b.rotate_left(30);
                    b = a;
                    a = temp;
                }
                h[0] = h[0].wrapping_add(a);
                h[1] = h[1].wrapping_add(b);
                h[2] = h[2].wrapping_add(c);
                h[3] = h[3].wrapping_add(d);
                h[4] = h[4].wrapping_add(e);
            }

            let mut out = [0u8; 20];
            for (i, word) in h.iter().enumerate() {
                out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
            }
            out
        }
    }
}
