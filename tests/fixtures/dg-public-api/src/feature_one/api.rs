//! Public surface of feature_one. Other features may import these items.

pub struct PublicThing {
    pub value: u32,
}

pub fn make_thing(value: u32) -> PublicThing {
    PublicThing { value }
}
