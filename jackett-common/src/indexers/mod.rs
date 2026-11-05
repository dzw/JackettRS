// Mirror of Jackett.Common/Indexers (trait definitions + Cardigann engine live here)
pub mod bespoke;
pub mod cardigann;
pub mod i_indexer;
pub mod indexer_base;
pub mod indexer_request;
pub mod indexer_response;
pub mod indexer_query_result;

pub use i_indexer::IIndexer;
pub use indexer_request::IndexerRequest;
pub use indexer_response::IndexerResponse;
pub use indexer_query_result::IndexerQueryResult;
