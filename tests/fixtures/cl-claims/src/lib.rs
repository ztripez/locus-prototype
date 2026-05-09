//! Fixture crate exercising CL001 (orphan external reference).
//!
//! Each public item below carries a doc comment that's either an orphan
//! reference (CL001 should fire when `paradigms.CL.require_local_rationale = true`)
//! or a properly-rationalised reference (CL001 should stay quiet).
//! The fixture is generic — issue numbers and URLs are illustrative,
//! not project-specific.

/// See #123.
pub struct OrphanIssueRef;

/// See https://example.org/spec/v2.
pub struct OrphanUrlRef;

/// Use the compatibility path because mobile clients still send v1
/// payloads. See #123 for the migration plan.
pub struct ReferenceWithRationale;

/// Plain explanation of what this type represents in the domain. No
/// external references at all — should not trip CL001.
pub struct PlainDoc;

/// Tracks the validation cursor while parsing inbound payloads. We keep
/// this counter in a separate field so the parser can emit partial
/// progress to the UI. See #1 for context on the streaming protocol.
pub struct ReferenceWithLongRationale;

/// See #1 and https://example.org/issue/1.
pub fn orphan_function_ref() {}

/// Drive the migration step described locally: read the legacy row, map
/// every column to the canonical shape, then write through the new
/// adapter. See #2 if you need the original spec discussion.
pub fn function_with_rationale() {}
