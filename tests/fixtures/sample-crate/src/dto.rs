use crate::{User, UserId};

// locus: ot boundary identity.user api.v1
#[derive(Serialize, Deserialize)]
pub struct UserDto {
    pub id: String,
    pub email: String,
    pub display_name: String,
}

impl TryFrom<UserDto> for User {
    type Error = ConversionError;

    fn try_from(value: UserDto) -> Result<Self, Self::Error> {
        Ok(User::create(UserId(value.id), value.email, value.display_name))
    }
}

pub struct ConversionError;

pub fn map_user(dto: UserDto) -> User {
    User {
        id: UserId(dto.id),
        email: dto.email,
        display_name: dto.display_name,
    }
}

pub fn is_active_status(status: &str) -> bool {
    status == "active"
}
