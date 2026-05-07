// Deliberate violation: UserModel overlaps with the canonical `User` concept
// but is annotated as neither canonical nor boundary. `locus check` should
// emit OT002 for this type.
pub struct UserModel {
    pub id: String,
    pub email: String,
    pub display_name: String,
}
