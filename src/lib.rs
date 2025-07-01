mod errors;
mod packet;
mod ping;

pub use crate::errors::Error;
pub use crate::ping::{
    dgramsock, new, ping, rawsock, Ping, SocketType, SocketType::DGRAM, SocketType::RAW,
};
