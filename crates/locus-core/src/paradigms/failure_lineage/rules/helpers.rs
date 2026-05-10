//! Shared helpers for FL rule implementations.
//!
//! These are internal utilities used by two or more rule modules. They are
//! not part of the public API — only `pub(super)` visibility so the rule
//! modules (siblings under `rules/`) can import them.

use locus_air::ArmBodyShape;

use super::super::lockfile_schema::{containing_module_of, matches_pattern};

/// Shared helper: is the (file, function) considered an invariant-owner
/// context for FL002–FL005 suppression?
///
/// File-level match: `module_path` matches any pattern.
/// Function-level match: the symbol's containing module (everything
/// before the last `::`) matches any pattern. This catches inline
/// `mod tests { ... }` blocks whose enclosing file's `module_path`
/// doesn't include `::tests::` but whose function symbols do.
pub(super) fn callsite_in_invariant_owner(
    file_module: &str,
    function_symbol: Option<&str>,
    patterns: &[String],
) -> bool {
    if patterns.iter().any(|p| matches_pattern(p, file_module)) {
        return true;
    }
    if let Some(sym) = function_symbol {
        let containing = containing_module_of(sym);
        if patterns.iter().any(|p| matches_pattern(p, containing)) {
            return true;
        }
    }
    false
}

/// Extract the `E` from a top-level `Result<T, E>` rendered as a string.
///
/// Returns `None` if the string isn't a top-level `Result<...>`, the angle
/// brackets don't balance, or the `<...>` body doesn't have a top-level
/// comma (e.g. `Result<T>` from a custom `Result` alias with one type
/// parameter — not what FL001 reasons about).
///
/// The renderer in `locus-rust::type_render` strips superfluous spaces but
/// we still trim once to be defensive against future renderer changes. We
/// also accept a leading `::` (`::std::result::Result<T, E>` style) by
/// peeling it off once before the prefix check.
pub(crate) fn extract_result_error_type(rendered: &str) -> Option<&str> {
    let s = rendered.trim();
    let s = s.strip_prefix("::").unwrap_or(s);
    // Accept the bare `Result<...>` shape. We deliberately don't try to
    // resolve `std::result::Result` / `core::result::Result` here — the
    // adapter renders the path the user wrote, so a fully-qualified
    // `std::result::Result<T, E>` simply won't be matched. That's fine: the
    // overwhelmingly common form in domain code is bare `Result<...>`, and
    // false positives on a hand-qualified `Result` alias would be worse
    // than missing the diagnostic.
    let inner = s.strip_prefix("Result<")?.strip_suffix('>')?;
    // Find the top-level comma — angle-bracket-aware so `Result<HashMap<K,
    // V>, E>` correctly returns `E`, not `V>, E`.
    let mut depth: i32 = 0;
    let mut split_at: Option<usize> = None;
    for (idx, ch) in inner.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            ',' if depth == 0 => {
                split_at = Some(idx);
                break;
            }
            _ => {}
        }
    }
    let split_at = split_at?;
    let err_ty = inner[split_at + 1..].trim();
    if err_ty.is_empty() {
        None
    } else {
        Some(err_ty)
    }
}

/// Render an [`ArmBodyShape`] for diagnostic messages.
pub(super) fn body_shape_label(shape: ArmBodyShape) -> &'static str {
    match shape {
        ArmBodyShape::Empty => "empty body",
        ArmBodyShape::Literal => "literal default",
        ArmBodyShape::Call => "call expression",
        ArmBodyShape::Return => "return",
        ArmBodyShape::ErrorPropagation => "?-propagation",
        ArmBodyShape::Block => "block",
        ArmBodyShape::Other => "other",
    }
}

/// True when an arm body is one of the silent / default-producing shapes
/// FL007 and FL011 fire on.
pub(super) fn is_silent_body_shape(shape: ArmBodyShape) -> bool {
    matches!(
        shape,
        ArmBodyShape::Empty | ArmBodyShape::Literal | ArmBodyShape::Call
    )
}
