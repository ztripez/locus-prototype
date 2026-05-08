//! Rust-language loaders. Each loader inspects the visitor-emitted AIR
//! (specifically `AirItem::CallSite` and `AirItem::Import` items) and
//! produces normalized [`locus_air::AirFact`] entries that paradigms in
//! `locus-core` consume in place of framework-specific reasoning.

pub mod markers;
pub mod std_rt;

pub use markers::MarkersLoader;
pub use std_rt::StdRtLoader;
