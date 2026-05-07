// Deliberate OT003 + OT004 target.
//
// This file is *not* a boundary file (it doesn't define any accepted boundary
// type) and `create_user` is *not* an accepted converter (its name doesn't
// match the converter heuristic, and `locus init` won't promote it).
//
// Therefore:
// - param `UserDto` (boundary) leaking into a non-boundary signature → OT003
// - direct `User { ... }` literal in a non-converter, non-owner location → OT004
use crate::dto::UserDto;
use crate::identity::{User, UserId};

pub fn create_user(req: UserDto) -> User {
    User {
        id: UserId(req.id),
        email: req.email,
        display_name: req.display_name,
    }
}
