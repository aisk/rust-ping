//! An ICMP echo ("ping") implementation for IPv4 and IPv6.
//!
//! Send ICMP echo requests to a target [`IpAddr`] and wait for the reply or a
//! timeout.
//!
//! # Quick start
//!
//! For a one off ping, call [`ping()`] with a target and a timeout.
//!
//! ```no_run
//! use std::net::IpAddr;
//! use std::time::Duration;
//!
//! let target: IpAddr = "8.8.8.8".parse().unwrap();
//! let reply = ping::ping(target, Duration::from_secs(1))?;
//! println!("rtt {:?} from {}", reply.rtt, reply.source);
//! # Ok::<(), ping::Error>(())
//! ```
//!
//! [`ping()`] opens a new socket every time. To ping repeatedly, create a
//! [`Pinger`] and call [`Pinger::ping`] instead. The pinger keeps its sockets
//! open between pings.
//!
//! ```no_run
//! use std::net::IpAddr;
//! use std::time::Duration;
//! use ping::Pinger;
//!
//! let target: IpAddr = "8.8.8.8".parse().unwrap();
//! let mut pinger = Pinger::new();
//! let reply = pinger.ping(target, Duration::from_secs(1))?;
//! println!("rtt {:?} from {}", reply.rtt, reply.source);
//! # Ok::<(), ping::Error>(())
//! ```
//!
//! Per request options such as the TTL and the payload are set on a
//! [`Request`], and socket options such as the socket type are set with
//! [`Pinger::builder`].
//!
//! ```no_run
//! use std::net::IpAddr;
//! use std::time::Duration;
//! use ping::{Pinger, Request, SocketType};
//!
//! let target: IpAddr = "8.8.8.8".parse().unwrap();
//! let mut pinger = Pinger::builder()
//!     .socket_type(SocketType::RAW)
//!     .build()?;
//! let request = Request::new(target).ttl(5).payload(b"hello".to_vec());
//! pinger.ping(request, Duration::from_secs(1))?;
//! # Ok::<(), ping::Error>(())
//! ```
//!
//! # Pinging a host name
//!
//! Only an [`IpAddr`] is accepted. To ping a host name, resolve it first with
//! [`ToSocketAddrs`](std::net::ToSocketAddrs).
//!
//! ```no_run
//! use std::net::ToSocketAddrs;
//! use std::time::Duration;
//!
//! // The port is irrelevant, we only need the resolved IP.
//! let addr = "www.google.com:0"
//!     .to_socket_addrs()
//!     .unwrap()
//!     .next()
//!     .unwrap()
//!     .ip();
//!
//! ping::ping(addr, Duration::from_secs(1))?;
//! # Ok::<(), ping::Error>(())
//! ```
//!
//! # Socket types
//!
//! Sending ICMP traffic over a [`RAW`](SocketType::RAW) socket needs elevated
//! privileges, while a [`DGRAM`](SocketType::DGRAM) socket works unprivileged
//! on most systems. See [`SocketType`] for the default order.
//!
//! # Tokio
//!
//! With the `tokio` feature, `ping::tokio::ping` and `ping::tokio::Pinger`
//! provide the same API with an `async` ping.
//!
//! [`IpAddr`]: std::net::IpAddr

mod errors;
// Copied from tokio-ping, so unused parts are kept.
#[allow(dead_code, unused_imports)]
mod packet;
mod ping;
mod pinger;
#[cfg(feature = "tokio")]
pub mod tokio;

pub use crate::errors::Error;
pub use crate::ping::SocketType;
#[allow(deprecated)]
pub use crate::ping::{Ping, PingResult, dgramsock, new, rawsock};
pub use crate::pinger::{Pinger, PingerBuilder, Reply, Request, ping};

#[doc(hidden)]
#[deprecated(since = "0.10.0", note = "use `SocketType::RAW` instead")]
pub const RAW: SocketType = SocketType::RAW;

#[doc(hidden)]
#[deprecated(since = "0.10.0", note = "use `SocketType::DGRAM` instead")]
pub const DGRAM: SocketType = SocketType::DGRAM;
