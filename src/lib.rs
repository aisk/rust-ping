mod errors;
mod packet;
mod ping;

pub use crate::errors::Error;
pub use crate::ping::{
    Ping, PingResult, SocketType, SocketType::DGRAM, SocketType::RAW, SocketType::SYSTEM,
    dgramsock, new, ping, rawsock,
};
