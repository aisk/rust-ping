use std::net::IpAddr;
use std::time::{Duration, Instant};

use socket2::{Socket, Type};

use crate::errors::{Error, io_error};
use crate::socket::{Config, FamilyState, MIN_SOCKET_TIMEOUT, check_payload, recv_from};

const DEFAULT_PAYLOAD_SIZE: usize = 48;

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

/// A single echo request sent by [`Pinger::ping`].
///
/// Anything that converts into a `Request` can be passed to `ping`, so a
/// plain [`IpAddr`] works when the defaults are fine.
///
/// ```no_run
/// # use std::net::IpAddr;
/// # use std::time::Duration;
/// # use ping::{Pinger, Request};
/// let target: IpAddr = "8.8.8.8".parse().unwrap();
/// let request = Request::new(target).ttl(5).payload(b"hello".to_vec());
/// Pinger::new().ping(request, Duration::from_secs(1))?;
/// # Ok::<(), ping::Error>(())
/// ```
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct Request {
    /// The address to ping.
    pub target: IpAddr,
    /// The IP time-to-live (hop limit). `None` uses 64.
    pub ttl: Option<u8>,
    /// The ICMP sequence number. `None` picks the next number in a counter
    /// kept per address family by the [`Pinger`].
    pub seq: Option<u16>,
    /// User data carried by the request, 48 zero bytes by default. An 8-byte
    /// random nonce is prepended on the wire to match the reply.
    pub payload: Vec<u8>,
}

impl Request {
    /// Creates a request for `target` with default options.
    pub fn new(target: IpAddr) -> Self {
        Request {
            target,
            ttl: None,
            seq: None,
            payload: vec![0; DEFAULT_PAYLOAD_SIZE],
        }
    }

    /// Sets the IP time-to-live (hop limit).
    pub fn ttl(mut self, ttl: u8) -> Self {
        self.ttl = Some(ttl);
        self
    }

    /// Sets the ICMP sequence number instead of using the automatic counter.
    pub fn seq(mut self, seq: u16) -> Self {
        self.seq = Some(seq);
        self
    }

    /// Sets the user data carried by the request.
    pub fn payload(mut self, payload: Vec<u8>) -> Self {
        self.payload = payload;
        self
    }
}

impl From<IpAddr> for Request {
    fn from(target: IpAddr) -> Self {
        Request::new(target)
    }
}

/// A matching echo reply, returned by [`Pinger::ping`].
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct Reply {
    /// The address the reply came from.
    pub source: IpAddr,
    /// The sequence number of the request and its reply.
    pub seq: u16,
    /// The time between sending the request and receiving the reply.
    pub rtt: Duration,
    /// The TTL from the reply's IP header. Only available when the socket
    /// receives the IP header: IPv4 raw sockets, and IPv4 datagram sockets on
    /// platforms other than Linux and Android. Always `None` for IPv6.
    pub ttl: Option<u8>,
    /// The user data echoed back, with the nonce removed.
    pub payload: Vec<u8>,
}

/// Builder for a [`Pinger`] with socket level options.
///
/// ```no_run
/// # use ping::{Pinger, SocketType};
/// let mut pinger = Pinger::builder()
///     .socket_type(SocketType::RAW)
///     .ident(1234)
///     .build()?;
/// # Ok::<(), ping::Error>(())
/// ```
#[derive(Clone, Debug, Default)]
pub struct PingerBuilder {
    pub(crate) config: Config,
}

impl PingerBuilder {
    /// Creates a builder with default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Uses only the given socket type.
    ///
    /// By default a datagram socket is tried first on Linux, Android and
    /// macOS, falling back to a raw socket, and the other way around on other
    /// platforms.
    pub fn socket_type(mut self, socket_type: SocketType) -> Self {
        self.config.socket_type = Some(socket_type);
        self
    }

    /// Sets the ICMP identifier. A random one is used by default.
    ///
    /// On Linux datagram sockets the identifier is the port the socket is
    /// bound to. Do not share an identifier between pingers there: depending
    /// on the kernel, the first ping either fails with an [`Error::IoError`],
    /// or succeeds and replies are then delivered to only one of the pingers.
    pub fn ident(mut self, ident: u16) -> Self {
        self.config.ident = Some(ident);
        self
    }

    /// Binds the sockets to a network interface by name, such as `"eth0"`.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub fn bind_device(mut self, device: impl Into<String>) -> Self {
        self.config.bind_device = Some(device.into());
        self
    }

    /// Sets the `SO_MARK` (fwmark) of the sockets, used by policy routing.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub fn mark(mut self, mark: u32) -> Self {
        self.config.mark = Some(mark);
        self
    }

    /// Creates the pinger. Sockets are opened lazily by the first ping to
    /// each address family.
    pub fn build(self) -> Result<Pinger, Error> {
        Ok(Pinger::from_config(self.config))
    }
}

/// Sends ICMP echo requests, reusing its sockets across pings.
///
/// A pinger can ping both IPv4 and IPv6 targets. The socket for an address
/// family is opened by the first ping to that family, so errors such as
/// missing privileges are reported by that ping.
///
/// `ping` takes `&mut self`, so one pinger handles one request at a time. To
/// ping from several threads, give each thread its own pinger.
///
/// ```no_run
/// use std::net::IpAddr;
/// use std::time::Duration;
/// use ping::{Error, Pinger};
///
/// let target: IpAddr = "8.8.8.8".parse().unwrap();
/// let mut pinger = Pinger::new();
/// for _ in 0..3 {
///     match pinger.ping(target, Duration::from_secs(1)) {
///         Ok(reply) => println!("seq={} rtt={:?}", reply.seq, reply.rtt),
///         Err(Error::Timeout) => println!("timeout"),
///         Err(e) => return Err(e),
///     }
/// }
/// # Ok::<(), Error>(())
/// ```
#[derive(Debug)]
pub struct Pinger {
    config: Config,
    v4: Option<(Socket, FamilyState)>,
    v6: Option<(Socket, FamilyState)>,
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

    /// Returns a builder to configure socket level options.
    pub fn builder() -> PingerBuilder {
        PingerBuilder::new()
    }

    pub(crate) fn from_config(config: Config) -> Self {
        Pinger {
            config,
            v4: None,
            v6: None,
        }
    }

    /// Returns the ICMP identifier of an address family once its socket is
    /// open.
    pub(crate) fn ident(&self, v6: bool) -> Option<u16> {
        let slot = if v6 { &self.v6 } else { &self.v4 };
        slot.as_ref().map(|(_, state)| state.ident)
    }

    /// Sends an echo request and blocks until the matching reply arrives or
    /// `timeout` elapses.
    ///
    /// Returns [`Error::Timeout`] if no reply arrives in time. ICMP errors
    /// such as Destination Unreachable are ignored, so they also end in a
    /// timeout.
    pub fn ping(&mut self, request: impl Into<Request>, timeout: Duration) -> Result<Reply, Error> {
        let request = request.into();
        check_payload(&request)?;

        let v6 = request.target.is_ipv6();
        let slot = if v6 { &mut self.v6 } else { &mut self.v4 };
        let (socket, state) = match slot {
            Some(family) => family,
            None => slot.insert(self.config.open(v6)?),
        };

        let outgoing = state.prepare(socket, &request)?;
        socket
            .set_write_timeout(Some(timeout.max(MIN_SOCKET_TIMEOUT)))
            .map_err(io_error)?;

        let started_at = Instant::now();
        socket
            .send_to(&outgoing.packet, &outgoing.dest)
            .map_err(io_error)?;

        loop {
            let remaining = timeout
                .checked_sub(started_at.elapsed())
                .filter(|remaining| !remaining.is_zero())
                .ok_or(Error::Timeout)?;
            socket
                .set_read_timeout(Some(remaining.max(MIN_SOCKET_TIMEOUT)))
                .map_err(io_error)?;

            let (n, source) = match recv_from(socket, &mut state.buffer) {
                Ok(received) => received,
                // Windows reports a previous ICMP error on the next receive.
                #[cfg(windows)]
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => continue,
                Err(error) => return Err(io_error(error)),
            };
            let rtt = started_at.elapsed();
            if let Some(reply) = state.match_reply(&outgoing, &state.buffer[..n], &source, rtt) {
                return Ok(reply);
            }
        }
    }
}

/// Sends a single echo request to `target` and blocks until the matching reply
/// arrives or `timeout` elapses.
///
/// A new socket is opened for every call. To ping repeatedly or to ping
/// several targets, reuse a [`Pinger`] instead. Use a [`Pinger`] with a
/// [`Request`] as well to set options such as the TTL or the payload.
///
/// ```no_run
/// use std::net::IpAddr;
/// use std::time::Duration;
///
/// let target: IpAddr = "8.8.8.8".parse().unwrap();
/// let reply = ping::ping(target, Duration::from_secs(1))?;
/// println!("rtt {:?} from {}", reply.rtt, reply.source);
/// # Ok::<(), ping::Error>(())
/// ```
pub fn ping(target: IpAddr, timeout: Duration) -> Result<Reply, Error> {
    Pinger::new().ping(target, timeout)
}
