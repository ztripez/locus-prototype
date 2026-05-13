//! Fixture exercising the markers loader's `// locus: fact <kind>` source
//! hints. Used by `locus query` integration tests to prove fact-derived
//! queries surface marker-produced facts.

// locus: fact hot-path
pub fn frame_step() {}

// locus: fact runtime-state-owner
pub fn supervisor_loop() {}
