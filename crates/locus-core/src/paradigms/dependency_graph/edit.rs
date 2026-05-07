//! `locus dg ...` — symbol-by-symbol mutators for the DG lockfile section.
//!
//! Mirror of OT's `accept` module. All operations validate the inputs and
//! refuse silent overwrites unless `force` is set.

use thiserror::Error;

use super::lockfile_schema::{DgSection, ForbiddenEdge};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DgEditError {
    #[error("`from` pattern must not be empty")]
    EmptyFrom,
    #[error("`to` pattern must not be empty")]
    EmptyTo,
    #[error(
        "edge `{from}` -> `{to}` already exists in forbidden_edges; pass --force to update its reason"
    )]
    DuplicateEdge { from: String, to: String },
}

/// Add a forbidden edge to the DG section. If an edge with the same
/// `(from, to)` pair already exists, refuse unless `force` is set; with
/// `force`, the edge's reason is overwritten with the new one.
pub fn forbid_edge(
    section: &mut DgSection,
    from: &str,
    to: &str,
    reason: Option<&str>,
    force: bool,
) -> Result<(), DgEditError> {
    if from.is_empty() {
        return Err(DgEditError::EmptyFrom);
    }
    if to.is_empty() {
        return Err(DgEditError::EmptyTo);
    }
    if let Some(existing) = section
        .forbidden_edges
        .iter_mut()
        .find(|e| e.from == from && e.to == to)
    {
        if !force {
            return Err(DgEditError::DuplicateEdge {
                from: from.into(),
                to: to.into(),
            });
        }
        existing.reason = reason.map(str::to_string);
        return Ok(());
    }
    section.forbidden_edges.push(ForbiddenEdge {
        from: from.into(),
        to: to.into(),
        reason: reason.map(str::to_string),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbid_edge_appends_a_new_entry() {
        let mut section = DgSection::default();
        forbid_edge(&mut section, "lore::domain::*", "lore::api::*", None, false).unwrap();
        assert_eq!(section.forbidden_edges.len(), 1);
        assert_eq!(section.forbidden_edges[0].from, "lore::domain::*");
        assert_eq!(section.forbidden_edges[0].to, "lore::api::*");
        assert_eq!(section.forbidden_edges[0].reason, None);
    }

    #[test]
    fn forbid_edge_records_reason() {
        let mut section = DgSection::default();
        forbid_edge(
            &mut section,
            "lore::domain::*",
            "lore::api::*",
            Some("layered architecture"),
            false,
        )
        .unwrap();
        assert_eq!(
            section.forbidden_edges[0].reason.as_deref(),
            Some("layered architecture")
        );
    }

    #[test]
    fn forbid_edge_rejects_duplicates_without_force() {
        let mut section = DgSection::default();
        forbid_edge(&mut section, "a::*", "b::*", None, false).unwrap();
        let err = forbid_edge(&mut section, "a::*", "b::*", None, false).unwrap_err();
        assert!(matches!(err, DgEditError::DuplicateEdge { .. }));
        assert_eq!(section.forbidden_edges.len(), 1);
    }

    #[test]
    fn forbid_edge_with_force_updates_reason() {
        let mut section = DgSection::default();
        forbid_edge(&mut section, "a::*", "b::*", Some("first reason"), false).unwrap();
        forbid_edge(&mut section, "a::*", "b::*", Some("better reason"), true).unwrap();
        assert_eq!(section.forbidden_edges.len(), 1);
        assert_eq!(
            section.forbidden_edges[0].reason.as_deref(),
            Some("better reason")
        );
    }

    #[test]
    fn forbid_edge_rejects_empty_patterns() {
        let mut section = DgSection::default();
        assert_eq!(
            forbid_edge(&mut section, "", "b::*", None, false).unwrap_err(),
            DgEditError::EmptyFrom
        );
        assert_eq!(
            forbid_edge(&mut section, "a::*", "", None, false).unwrap_err(),
            DgEditError::EmptyTo
        );
    }
}
