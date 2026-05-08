use crate::domain::User;

pub struct UserResponse {
    pub id: u64,
    pub email: String,
}

impl From<User> for UserResponse {
    fn from(u: User) -> Self {
        UserResponse {
            id: u.id,
            email: u.email,
        }
    }
}
