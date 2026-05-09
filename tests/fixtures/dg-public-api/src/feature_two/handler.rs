//! feature_two consumes feature_one. The first import goes through the
//! declared public surface (allowed). The second reaches into internals
//! that aren't in feature_one's `public_api` (must trip DG003).

use crate::feature_one::api::{PublicThing, make_thing};
use crate::feature_one::internals::secret;

pub fn handle() -> u32 {
    let thing: PublicThing = make_thing(1);
    thing.value + secret()
}
