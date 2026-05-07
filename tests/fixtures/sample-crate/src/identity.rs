// ot: canonical
pub struct User {
    pub id: UserId,
    pub email: String,
    pub display_name: String,
}

pub struct UserId(pub String);

pub enum UserStatus {
    Active,
    Suspended,
    Deleted,
}

impl User {
    pub fn create(id: UserId, email: String, display_name: String) -> Self {
        User { id, email, display_name }
    }
}
