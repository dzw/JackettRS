//! Mirror of Jackett.Common/Indexers/Definitions — the hand-coded ("bespoke")
//! indexers that cannot be expressed purely as Cardigann YAML. Each is a plain
//! `IIndexer` implementation; the `cardigann` engine covers the YAML trackers.
//!
//! The first port is `Newznab` (the universal Torznab/Newznab API proxy), which
//! is also the most-used generic indexer type in Jackett. `GazelleTracker` is the
//! reusable base for the large Gazelle-API tracker family.

pub mod avistaz;
pub mod couchpotato;
pub mod gazelle;
pub mod gazelle_sites;
pub mod newznab;
pub mod public_brazilian;
pub mod registry;
pub mod simple;
pub mod speedapp;
