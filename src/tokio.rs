//! Asynchronous pinger for Tokio, available with the `tokio` feature.
//!
//! [`ping()`] and [`Pinger`] mirror the blocking [`crate::ping()`] and
//! [`crate::Pinger`], and use the same [`Request`], [`Reply`] and [`Error`]
//! types.
//!
//! ```no_run
//! use std::net::IpAddr;
//! use std::time::Duration;
//!
//! # #[::tokio::main]
//! # async fn main() -> Result<(), ping::Error> {
//! let target: IpAddr = "8.8.8.8".parse().unwrap();
//! let mut pinger = ping::tokio::Pinger::new();
//! let reply = pinger.ping(target, Duration::from_secs(1)).await?;
//! println!("rtt {:?} from {}", reply.rtt, reply.source);
//! # Ok(())
//! # }
//! ```
//!
//! On Unix the sockets are registered with Tokio's reactor. On Windows each
//! ping runs the blocking implementation on Tokio's blocking thread pool.

use std::time::Duration;

#[cfg(unix)]
use std::time::Instant;

#[cfg(unix)]
use ::tokio::io::unix::AsyncFd;
#[cfg(unix)]
use socket2::Socket;

use crate::errors::Error;
#[cfg(unix)]
use crate::errors::io_error;
use crate::pinger::{Reply, Request};
use crate::socket::Config;
#[cfg(unix)]
use crate::socket::{FamilyState, check_payload, recv_from};

/// Sends ICMP echo requests asynchronously, reusing its sockets across pings.
///
/// Behaves like the blocking [`crate::Pinger`]. `ping` takes `&mut self`, so
/// to ping concurrently, create one pinger per task. To set socket level
/// options, use [`crate::Pinger::builder`] and finish with
/// [`build_tokio`](crate::PingerBuilder::build_tokio).
#[derive(Debug)]
pub struct Pinger {
    config: Config,
    #[cfg(unix)]
    v4: Option<(AsyncFd<Socket>, FamilyState)>,
    #[cfg(unix)]
    v6: Option<(AsyncFd<Socket>, FamilyState)>,
    #[cfg(not(unix))]
    inner: Option<crate::Pinger>,
}

impl Default for Pinger {
    fn default() -> Self {
        Self::new()
    }
}

impl Pinger {
    /// Creates a pinger with default options.
    pub fn new() -> Self {
        Self::from_config(Config::default())
    }

    pub(crate) fn from_config(config: Config) -> Self {
        Pinger {
            config,
            #[cfg(unix)]
            v4: None,
            #[cfg(unix)]
            v6: None,
            #[cfg(not(unix))]
            inner: None,
        }
    }

    /// Returns the ICMP identifier of an address family once its socket is
    /// open.
    pub(crate) fn ident(&self, v6: bool) -> Option<u16> {
        #[cfg(unix)]
        {
            let slot = if v6 { &self.v6 } else { &self.v4 };
            slot.as_ref().map(|(_, state)| state.ident)
        }
        #[cfg(not(unix))]
        self.inner.as_ref()?.ident(v6)
    }

    /// Sends an echo request and waits until the matching reply arrives or
    /// `timeout` elapses.
    ///
    /// Returns [`Error::Timeout`] if no reply arrives in time.
    #[cfg(unix)]
    pub async fn ping(
        &mut self,
        request: impl Into<Request>,
        timeout: Duration,
    ) -> Result<Reply, Error> {
        let request = request.into();
        check_payload(&request)?;

        let v6 = request.target.is_ipv6();
        let slot = if v6 { &mut self.v6 } else { &mut self.v4 };
        let (socket, state) = match slot {
            Some(family) => family,
            None => {
                let (socket, state) = self.config.open(v6)?;
                socket.set_nonblocking(true)?;
                slot.insert((AsyncFd::new(socket)?, state))
            }
        };

        let outgoing = state.prepare(socket.get_ref(), &request)?;
        let started_at = Instant::now();

        let exchange = async {
            loop {
                let mut ready = socket.writable().await.map_err(io_error)?;
                match ready
                    .try_io(|inner| inner.get_ref().send_to(&outgoing.packet, &outgoing.dest))
                {
                    Ok(result) => {
                        result.map_err(io_error)?;
                        break;
                    }
                    Err(_would_block) => continue,
                }
            }

            loop {
                let mut ready = socket.readable().await.map_err(io_error)?;
                let received = ready.try_io(|inner| recv_from(inner.get_ref(), &mut state.buffer));
                let (n, source) = match received {
                    Ok(result) => result.map_err(io_error)?,
                    Err(_would_block) => continue,
                };
                let rtt = started_at.elapsed();
                if let Some(reply) = state.match_reply(&outgoing, &state.buffer[..n], &source, rtt)
                {
                    return Ok(reply);
                }
            }
        };

        ::tokio::time::timeout(timeout, exchange)
            .await
            .map_err(|_| Error::Timeout)?
    }

    /// Sends an echo request and waits until the matching reply arrives or
    /// `timeout` elapses.
    ///
    /// Returns [`Error::Timeout`] if no reply arrives in time.
    #[cfg(not(unix))]
    pub async fn ping(
        &mut self,
        request: impl Into<Request>,
        timeout: Duration,
    ) -> Result<Reply, Error> {
        let request = request.into();
        // The blocking pinger is moved into the task and put back afterwards.
        // If this future is dropped midway, a new one is created next time.
        let mut inner = self
            .inner
            .take()
            .unwrap_or_else(|| crate::Pinger::from_config(self.config.clone()));
        let (inner, result) = ::tokio::task::spawn_blocking(move || {
            let result = inner.ping(request, timeout);
            (inner, result)
        })
        .await
        .map_err(|_| Error::InternalError)?;
        self.inner = Some(inner);
        result
    }
}

/// Sends a single echo request and waits until the matching reply arrives or
/// `timeout` elapses.
///
/// Takes an [`IpAddr`](std::net::IpAddr) or a [`Request`], like
/// [`Pinger::ping`]. A new socket is opened for every call. To ping repeatedly or to ping
/// several targets, reuse a [`Pinger`] instead.
///
/// ```no_run
/// use std::net::IpAddr;
/// use std::time::Duration;
///
/// # #[::tokio::main]
/// # async fn main() -> Result<(), ping::Error> {
/// let target: IpAddr = "8.8.8.8".parse().unwrap();
/// let reply = ping::tokio::ping(target, Duration::from_secs(1)).await?;
/// println!("rtt {:?} from {}", reply.rtt, reply.source);
/// # Ok(())
/// # }
/// ```
pub async fn ping(request: impl Into<Request>, timeout: Duration) -> Result<Reply, Error> {
    Pinger::new().ping(request, timeout).await
}
