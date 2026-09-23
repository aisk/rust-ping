// The legacy API is kept until it is removed.
#![allow(deprecated)]

use std::net::IpAddr;
use std::time::Duration;

use socket2::Type;

use crate::errors::Error;
use crate::pinger::{PingerBuilder, Reply, Request};

#[cfg(feature = "tokio")]
mod async_ping;

const TOKEN_SIZE: usize = 24;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(4);
type Token = [u8; TOKEN_SIZE];

/// A legacy ping translated into a pinger configuration and a request.
struct Legacy {
    builder: PingerBuilder,
    request: Request,
    timeout: Duration,
}

impl Legacy {
    fn send(self) -> Result<PingResult, Error> {
        let target = self.request.target;
        let mut pinger = self.builder.build()?;
        let result = pinger.ping(self.request, self.timeout);
        finish(result, pinger.ident(target.is_ipv6()), target)
    }
}

/// Converts the outcome of a ping to what the legacy API returned.
fn finish(
    result: Result<Reply, Error>,
    ident: Option<u16>,
    target: IpAddr,
) -> Result<PingResult, Error> {
    let reply = result.map_err(|error| match error {
        Error::Timeout => Error::from(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "ping request timed out",
        )),
        error => error,
    })?;
    Ok(PingResult {
        rtt: reply.rtt,
        ident: ident.ok_or(Error::InternalError)?,
        seq_cnt: reply.seq,
        payload: reply.payload,
        source: reply.source,
        target,
        ttl: reply.ttl,
    })
}

fn ping_with_socktype(
    socket_type: SocketType,
    addr: IpAddr,
    timeout: Option<Duration>,
    ttl: Option<u32>,
    ident: Option<u16>,
    seq_cnt: Option<u16>,
    payload: Option<&Token>,
) -> Result<(), Error> {
    Ping {
        socket_type,
        addr,
        timeout,
        ttl,
        ident,
        seq_cnt,
        payload,
        #[cfg(any(target_os = "linux", target_os = "android"))]
        bind_device: None,
    }
    .send()?;
    Ok(())
}

/// The kind of socket used to send the ICMP request.
///
/// By default [`Pinger`](crate::Pinger) tries [`DGRAM`](SocketType::DGRAM)
/// first on Linux, Android and macOS, and [`RAW`](SocketType::RAW) first
/// elsewhere, falling back to the other one. Choose one explicitly with
/// [`PingerBuilder::socket_type`](crate::PingerBuilder::socket_type).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SocketType {
    /// Raw socket. Needs elevated privileges (root, or `CAP_NET_RAW` on Linux).
    RAW,
    /// Datagram socket. Works without elevated privileges on most systems, but
    /// some Linux distributions disable it by default.
    DGRAM,
}

impl From<SocketType> for Type {
    fn from(socket_type: SocketType) -> Self {
        match socket_type {
            SocketType::RAW => Type::RAW,
            SocketType::DGRAM => Type::DGRAM,
        }
    }
}

/// The outcome of a successful ping, returned by [`Ping::send`].
#[doc(hidden)]
#[deprecated(since = "0.10.0", note = "use `Pinger` and `Reply` instead")]
#[derive(Debug)]
#[non_exhaustive]
pub struct PingResult {
    /// The measured round-trip time between sending the request and receiving
    /// the matching reply.
    pub rtt: Duration,
    /// The ICMP identifier observed in the reply.
    ///
    /// This is not guaranteed to equal the value passed to [`Ping::ident`].
    /// On unprivileged datagram sockets (the default on Linux and macOS) the
    /// kernel overwrites the identifier with the socket's local port, so the
    /// reply, and therefore this field, carries the kernel-chosen value rather
    /// than the requested one.
    pub ident: u16,
    /// The sequence number echoed back in the reply.
    pub seq_cnt: u16,
    /// The payload token echoed back in the reply, used to match it to the
    /// request.
    pub payload: Vec<u8>,
    /// The actual source IP address from the reply packet.
    pub source: IpAddr,
    /// The target address passed to the ping.
    #[deprecated(since = "0.7.1", note = "use `source` instead")]
    pub target: IpAddr,
    /// The TTL from the reply IP header. Only available for IPv4 RAW sockets;
    /// `None` for IPv4 DGRAM (Linux, no IP header) and all IPv6 responses.
    pub ttl: Option<u8>,
}

#[doc(hidden)]
pub mod rawsock {
    use super::*;
    #[deprecated(since = "0.10.0", note = "use `Pinger` instead")]
    pub fn ping(
        addr: IpAddr,
        timeout: Option<Duration>,
        ttl: Option<u32>,
        ident: Option<u16>,
        seq_cnt: Option<u16>,
        payload: Option<&Token>,
    ) -> Result<(), Error> {
        ping_with_socktype(SocketType::RAW, addr, timeout, ttl, ident, seq_cnt, payload)
    }
}

#[doc(hidden)]
pub mod dgramsock {
    use super::*;
    #[deprecated(since = "0.10.0", note = "use `Pinger` instead")]
    pub fn ping(
        addr: IpAddr,
        timeout: Option<Duration>,
        ttl: Option<u32>,
        ident: Option<u16>,
        seq_cnt: Option<u16>,
        payload: Option<&Token>,
    ) -> Result<(), Error> {
        ping_with_socktype(
            SocketType::DGRAM,
            addr,
            timeout,
            ttl,
            ident,
            seq_cnt,
            payload,
        )
    }
}

#[doc(hidden)]
#[deprecated(since = "0.8.0", note = "use `Pinger` instead")]
pub fn ping(
    addr: IpAddr,
    timeout: Option<Duration>,
    ttl: Option<u32>,
    ident: Option<u16>,
    seq_cnt: Option<u16>,
    payload: Option<&Token>,
) -> Result<(), Error> {
    rawsock::ping(addr, timeout, ttl, ident, seq_cnt, payload)
}

/// Builder for a single ping.
///
/// Create one with [`Ping::new`] or [`new`], set any options, then call
/// [`send`](Ping::send). All options are optional and have sensible defaults.
///
/// ```no_run
/// let target = "8.8.8.8".parse().unwrap();
/// let result = ping::new(target).send().expect("ping failed");
/// println!("{:?}", result.rtt);
/// ```
#[doc(hidden)]
#[deprecated(since = "0.10.0", note = "use `Pinger` instead")]
#[derive(Debug, Clone)]
pub struct Ping<'a> {
    socket_type: SocketType,
    addr: IpAddr,
    timeout: Option<Duration>,
    ttl: Option<u32>,
    ident: Option<u16>,
    seq_cnt: Option<u16>,
    payload: Option<&'a Token>,
    #[cfg(any(target_os = "linux", target_os = "android"))]
    bind_device: Option<&'a str>,
}

impl<'a> Ping<'a> {
    /// Creates a builder targeting `addr`, with the default socket type for
    /// the current platform ([`RAW`](SocketType::RAW) on Windows,
    /// [`DGRAM`](SocketType::DGRAM) elsewhere).
    pub fn new(addr: IpAddr) -> Self {
        let socket_type = if std::env::consts::OS == "windows" {
            SocketType::RAW
        } else {
            SocketType::DGRAM
        };
        return Ping {
            socket_type,
            addr,
            timeout: None,
            ttl: None,
            ident: None,
            seq_cnt: None,
            payload: None,
            #[cfg(any(target_os = "linux", target_os = "android"))]
            bind_device: None,
        };
    }

    /// Overrides the [`SocketType`] used to send the request, replacing the
    /// platform default chosen by [`Ping::new`].
    pub fn socket_type(&mut self, socket_type: SocketType) -> &mut Self {
        self.socket_type = socket_type;
        return self;
    }

    /// Sets how long [`send`](Ping::send) waits for a reply before failing.
    ///
    /// When unset, the timeout defaults to 4 seconds. On timeout, `send`
    /// returns an [`Error::IoError`] whose kind is
    /// [`ErrorKind::TimedOut`](std::io::ErrorKind::TimedOut).
    pub fn timeout(&mut self, timeout: Duration) -> &mut Self {
        self.timeout = Some(timeout);
        return self;
    }

    /// Sets the IP time-to-live (hop limit) of the request.
    ///
    /// Defaults to 64 when unset.
    pub fn ttl(&mut self, ttl: u32) -> &mut Self {
        self.ttl = Some(ttl);
        return self;
    }

    /// Sets the ICMP identifier to send.
    ///
    /// When unset, a random identifier is generated for each ping.
    ///
    /// Note that on unprivileged datagram sockets (the default on Linux and
    /// macOS) the kernel overwrites this field with the socket's local port,
    /// so the value set here never reaches the wire and is not reflected in
    /// [`PingResult::ident`]. It takes effect only on raw sockets (the default
    /// on Windows, or when selected via [`Ping::socket_type`]).
    pub fn ident(&mut self, ident: u16) -> &mut Self {
        self.ident = Some(ident);
        return self;
    }

    /// Sets the ICMP sequence number of the request.
    ///
    /// Defaults to 1 when unset.
    pub fn seq_cnt(&mut self, seq_cnt: u16) -> &mut Self {
        self.seq_cnt = Some(seq_cnt);
        return self;
    }

    /// Sets the 24-byte payload token carried by the request.
    ///
    /// The reply is matched to the request by this token, so it acts as the
    /// correlation id. When unset, a random token is generated for each ping.
    pub fn payload(&mut self, payload: &'a Token) -> &mut Self {
        self.payload = Some(payload);
        return self;
    }

    /// Binds the socket to a network interface by name (e.g. `"eth0"`), so the
    /// request is sent from that interface.
    ///
    /// Only available on Linux and Android.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub fn bind_device(&mut self, device: &'a str) -> &mut Self {
        self.bind_device = Some(device);
        return self;
    }

    /// Sends the echo request and blocks until a matching reply arrives or the
    /// timeout elapses.
    ///
    /// On success returns a [`PingResult`]. A timeout is reported as an
    /// [`Error::IoError`] with kind
    /// [`ErrorKind::TimedOut`](std::io::ErrorKind::TimedOut).
    pub fn send(&self) -> Result<PingResult, Error> {
        self.legacy()?.send()
    }

    fn legacy(&self) -> Result<Legacy, Error> {
        let mut builder = PingerBuilder::new().socket_type(self.socket_type);
        // Linux ping sockets always ignored the identifier here. Binding to it
        // instead could clash with other pings using the same one.
        let ignores_ident = cfg!(any(target_os = "linux", target_os = "android"))
            && self.socket_type == SocketType::DGRAM;
        if let Some(ident) = self.ident.filter(|_| !ignores_ident) {
            builder = builder.ident(ident);
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        if let Some(device) = self.bind_device {
            builder = builder.bind_device(device);
        }

        let token = self.payload.copied().unwrap_or_else(rand::random);
        let mut request = Request::new(self.addr)
            .seq(self.seq_cnt.unwrap_or(1))
            .payload(token.to_vec());
        if let Some(ttl) = self.ttl {
            let ttl = u8::try_from(ttl).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "ttl out of range")
            })?;
            request = request.ttl(ttl);
        }

        Ok(Legacy {
            builder,
            request,
            timeout: self.timeout.unwrap_or(DEFAULT_TIMEOUT),
        })
    }
}

/// Creates a [`Ping`] builder targeting `addr`.
///
/// Shorthand for [`Ping::new`].
#[doc(hidden)]
#[deprecated(since = "0.10.0", note = "use `Pinger` instead")]
pub fn new<'a>(addr: IpAddr) -> Ping<'a> {
    return Ping::new(addr);
}
