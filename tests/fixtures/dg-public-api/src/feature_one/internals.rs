//! Private to feature_one. Not in `public_api`; cross-feature reaches into
//! these symbols must trip DG003.

pub fn secret() -> u32 {
    42
}

pub struct InternalState {
    pub counter: u32,
}
