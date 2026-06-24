//! An ICMP echo ("ping") implementation for IPv4 and IPv6.
//!
//! Send an ICMP echo request to a target [`IpAddr`] and wait for the reply or
//! a timeout.
//!
//! # Quick start
//!
//! The main entry point is the [`Ping`] builder, created with [`new`].
//! Configure the options you need and call [`Ping::send`], which returns a
//! [`PingResult`] describing the reply.
//!
//! ```no_run
//! use std::time::Duration;
//!
//! let target = "8.8.8.8".parse().unwrap();
//! let result = ping::new(target)
//!     .timeout(Duration::from_secs(2))
//!     .ttl(64)
//!     .send()
//!     .expect("ping failed");
//!
//! println!("round-trip time: {:?}", result.rtt);
//! ```
//!
//! # Pinging a host name
//!
//! Only an [`IpAddr`] is accepted. To ping a host name, resolve it first with
//! [`ToSocketAddrs`](std::net::ToSocketAddrs).
//!
//! ```no_run
//! use std::net::ToSocketAddrs;
//!
//! // The port is irrelevant, we only need the resolved IP.
//! let addr = "www.google.com:0"
//!     .to_socket_addrs()
//!     .unwrap()
//!     .next()
//!     .unwrap()
//!     .ip();
//!
//! ping::new(addr).send().expect("ping failed");
//! ```
//!
//! # Socket types
//!
//! Sending ICMP traffic over a [`RAW`] socket needs elevated privileges, while
//! a [`DGRAM`] socket works unprivileged on most systems. See [`SocketType`]
//! for the per-platform default and how to override it.
//!
//! [`IpAddr`]: std::net::IpAddr

mod errors;
mod packet;
mod ping;

pub use crate::errors::Error;
pub use crate::ping::{
    Ping, PingResult, SocketType, SocketType::DGRAM, SocketType::RAW, dgramsock, new, ping, rawsock,
};
