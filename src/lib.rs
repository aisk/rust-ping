#[macro_use]
extern crate failure;
extern crate rand;
extern crate socket2;

mod errors;
mod packet;
mod ping;

pub use ping::ping;
