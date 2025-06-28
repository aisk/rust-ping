mod errors;
mod packet;
mod ping;

pub use crate::errors::Error;
pub use crate::ping::new;
pub use crate::ping::{dgramsock, ping, rawsock};
