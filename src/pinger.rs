use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};

use rand::random;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};

use crate::errors::{Error, io_error};
use crate::packet::{EchoReply, ICMP_HEADER_SIZE, IcmpV4, IcmpV6, IpV4Packet, build_echo_request};
use crate::ping::SocketType;

const NONCE_SIZE: usize = 8;
const DEFAULT_TTL: u8 = 64;
const DEFAULT_PAYLOAD_SIZE: usize = 48;
const IPV4_HEADER_SIZE: usize = 20;
const MAX_IP_PAYLOAD: usize = 65535;
const RECV_BUFFER_SIZE: usize = 65536;
/// `set_read_timeout` treats zero as "no timeout" and socket2 truncates to
/// microseconds (milliseconds on Windows), so never go below this.
const MIN_SOCKET_TIMEOUT: Duration = Duration::from_millis(1);

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
const DEFAULT_SOCKET_TYPES: &[SocketType] = &[SocketType::DGRAM, SocketType::RAW];
#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
const DEFAULT_SOCKET_TYPES: &[SocketType] = &[SocketType::RAW, SocketType::DGRAM];

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

    fn max_payload(&self) -> usize {
        let overhead = ICMP_HEADER_SIZE + NONCE_SIZE;
        match self.target {
            IpAddr::V4(_) => MAX_IP_PAYLOAD - IPV4_HEADER_SIZE - overhead,
            // The IPv6 payload length does not include the fixed header.
            IpAddr::V6(_) => MAX_IP_PAYLOAD - overhead,
        }
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

/// Socket level options shared by every address family of a pinger.
#[derive(Clone, Debug, Default)]
pub(crate) struct Config {
    socket_type: Option<SocketType>,
    ident: Option<u16>,
    #[cfg(any(target_os = "linux", target_os = "android"))]
    bind_device: Option<String>,
    #[cfg(any(target_os = "linux", target_os = "android"))]
    mark: Option<u32>,
}

impl Config {
    /// Opens a socket for one address family, trying each candidate socket
    /// type in order.
    pub(crate) fn open(&self, v6: bool) -> Result<(Socket, FamilyState), Error> {
        let socket_types = match &self.socket_type {
            Some(socket_type) => std::slice::from_ref(socket_type),
            None => DEFAULT_SOCKET_TYPES,
        };
        let (domain, protocol) = if v6 {
            (Domain::IPV6, Protocol::ICMPV6)
        } else {
            (Domain::IPV4, Protocol::ICMPV4)
        };

        let mut first_error = None;
        for &socket_type in socket_types {
            match Socket::new(domain, Type::from(socket_type), Some(protocol)) {
                Ok(socket) => return self.setup(socket, socket_type, v6),
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        Err(first_error.map(Error::from).unwrap_or(Error::InternalError))
    }

    fn setup(
        &self,
        socket: Socket,
        socket_type: SocketType,
        v6: bool,
    ) -> Result<(Socket, FamilyState), Error> {
        if v6 {
            socket.set_unicast_hops_v6(DEFAULT_TTL.into())?;
        } else {
            socket.set_ttl_v4(DEFAULT_TTL.into())?;
        }

        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            if let Some(device) = &self.bind_device {
                socket.bind_device(Some(device.as_bytes()))?;
            }
            if let Some(mark) = self.mark {
                socket.set_mark(mark)?;
            }
        }

        let is_linux = cfg!(any(target_os = "linux", target_os = "android"));
        let linux_dgram = is_linux && matches!(socket_type, SocketType::DGRAM);

        let ident = if linux_dgram {
            // Linux ping sockets use the bound port as the ICMP identifier and
            // overwrite whatever is written in the header.
            let unspecified = if v6 {
                IpAddr::V6(Ipv6Addr::UNSPECIFIED)
            } else {
                IpAddr::V4(Ipv4Addr::UNSPECIFIED)
            };
            socket.bind(&SocketAddr::new(unspecified, self.ident.unwrap_or(0)).into())?;
            socket
                .local_addr()?
                .as_socket()
                .map(|addr| addr.port())
                .ok_or(Error::InternalError)?
        } else {
            self.ident.unwrap_or_else(random)
        };

        let state = FamilyState {
            v6,
            ident,
            has_ip_header: !v6 && !linux_dgram,
            ttl: DEFAULT_TTL,
            next_seq: 1,
            buffer: vec![0; RECV_BUFFER_SIZE],
        };
        Ok((socket, state))
    }
}

/// Per address family state of a pinger, apart from the socket itself.
pub(crate) struct FamilyState {
    v6: bool,
    pub(crate) ident: u16,
    has_ip_header: bool,
    /// The TTL currently set on the socket.
    ttl: u8,
    next_seq: u16,
    pub(crate) buffer: Vec<u8>,
}

// Written by hand to leave out the 64 KiB receive buffer.
impl std::fmt::Debug for FamilyState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FamilyState")
            .field("v6", &self.v6)
            .field("ident", &self.ident)
            .field("has_ip_header", &self.has_ip_header)
            .field("ttl", &self.ttl)
            .field("next_seq", &self.next_seq)
            .finish_non_exhaustive()
    }
}

/// An echo request ready to be sent.
pub(crate) struct Outgoing {
    pub(crate) packet: Vec<u8>,
    pub(crate) dest: SockAddr,
    target: IpAddr,
    seq: u16,
    nonce: [u8; NONCE_SIZE],
}

impl FamilyState {
    pub(crate) fn prepare(
        &mut self,
        socket: &Socket,
        request: &Request,
    ) -> Result<Outgoing, Error> {
        let ttl = request.ttl.unwrap_or(DEFAULT_TTL);
        if ttl != self.ttl {
            if self.v6 {
                socket.set_unicast_hops_v6(ttl.into())?;
            } else {
                socket.set_ttl_v4(ttl.into())?;
            }
            self.ttl = ttl;
        }

        let seq = request.seq.unwrap_or_else(|| {
            let seq = self.next_seq;
            self.next_seq = seq.wrapping_add(1);
            seq
        });
        let nonce: [u8; NONCE_SIZE] = random();
        let packet = if self.v6 {
            build_echo_request::<IcmpV6>(self.ident, seq, &nonce, &request.payload)
        } else {
            build_echo_request::<IcmpV4>(self.ident, seq, &nonce, &request.payload)
        };

        Ok(Outgoing {
            packet,
            dest: SocketAddr::new(request.target, 0).into(),
            target: request.target,
            seq,
            nonce,
        })
    }

    /// Returns the reply if `packet` answers `outgoing`. Anything else,
    /// including malformed packets, is ignored.
    pub(crate) fn match_reply(
        &self,
        outgoing: &Outgoing,
        packet: &[u8],
        source: &SockAddr,
        rtt: Duration,
    ) -> Option<Reply> {
        let (icmp, ttl) = if self.has_ip_header {
            let ip = IpV4Packet::decode(packet).ok()?;
            (ip.data, Some(ip.ttl))
        } else {
            (packet, None)
        };
        let reply = if self.v6 {
            EchoReply::decode_body::<IcmpV6>(icmp)
        } else {
            EchoReply::decode_body::<IcmpV4>(icmp)
        }
        .ok()?;
        if reply.ident != self.ident || reply.seq_cnt != outgoing.seq {
            return None;
        }
        let payload = reply.payload.strip_prefix(&outgoing.nonce[..])?;

        Some(Reply {
            source: source
                .as_socket()
                .map(|addr| addr.ip())
                .unwrap_or(outgoing.target),
            seq: outgoing.seq,
            rtt,
            ttl,
            payload: payload.to_vec(),
        })
    }
}

pub(crate) fn check_payload(request: &Request) -> Result<(), Error> {
    let max = request.max_payload();
    if request.payload.len() > max {
        return Err(Error::PayloadTooLarge { max });
    }
    Ok(())
}

pub(crate) fn recv_from(socket: &Socket, buffer: &mut [u8]) -> std::io::Result<(usize, SockAddr)> {
    // socket2 0.6 recv_from requires &mut [MaybeUninit<u8>]; cast is sound
    // because MaybeUninit<u8> has the same layout as u8.
    socket.recv_from(unsafe {
        std::slice::from_raw_parts_mut(
            buffer.as_mut_ptr() as *mut std::mem::MaybeUninit<u8>,
            buffer.len(),
        )
    })
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
