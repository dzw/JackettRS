// Mirror of Jackett.Common/Models/DTO
pub mod api_search;
pub mod config;
pub mod indexer;
pub mod manual_search_result;
pub mod server_config;
pub mod torrent_potato_request;
pub mod torrent_potato_response;
pub mod torrent_potato_response_item;
pub mod torznab_request;

pub use api_search::ApiSearch;
pub use config::Config;
pub use indexer::Indexer;
pub use manual_search_result::ManualSearchResult;
pub use server_config::ServerConfig;
pub use torrent_potato_request::TorrentPotatoRequest;
pub use torrent_potato_response::TorrentPotatoResponse;
pub use torrent_potato_response_item::TorrentPotatoResponseItem;
pub use torznab_request::TorznabRequest;
