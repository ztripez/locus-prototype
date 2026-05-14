//! Fixture for the rustdoc-JSON semantic backend integration test.
//!
//! Carries exactly two converter impls — one infallible `From` and
//! one fallible `TryFrom` — plus the participating types. The
//! integration test against `RustdocJsonBackend` asserts that both
//! get resolved into `ResolvedConversion` records with canonical
//! paths and the right `ConversionMechanism`.

pub struct UserDto {
    pub id: String,
    pub email: String,
}

pub struct User {
    pub id: UserId,
    pub email: String,
}

pub struct UserId(pub String);

pub struct ConversionError;

impl From<UserDto> for User {
    fn from(value: UserDto) -> Self {
        User {
            id: UserId(value.id),
            email: value.email,
        }
    }
}

impl TryFrom<&str> for UserId {
    type Error = ConversionError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.is_empty() {
            Err(ConversionError)
        } else {
            Ok(UserId(value.to_string()))
        }
    }
}
