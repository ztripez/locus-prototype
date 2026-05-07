//! Locus core: paradigm host.
//!
//! Locus is multi-paradigm (see `Paradigms.md`). Each paradigm consumes
//! paradigm-neutral AIR and emits paradigm-specific diagnostics tagged with
//! its own rule prefix (`OT###`, `DG###`, `CF###`, …). This crate hosts the
//! paradigm registry, the shared lockfile / diagnostic types, and the
//! per-paradigm modules under [`paradigms`].

pub mod diagnostics;
pub mod exceptions;
pub mod loaders;
pub mod lockfile;
pub mod paradigms;

pub use diagnostics::{CheckMode, Diagnostic, Severity};
pub use exceptions::{EXPIRED_EXCEPTION_RULE, apply_exceptions, today_utc};
pub use loaders::{Loader, apply_loaders};
pub use lockfile::{Lockfile, LockfileError};
pub use paradigms::{Paradigm, registry};
