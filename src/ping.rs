use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, SystemTime};

use rand::random;
use socket2::{Domain, Protocol, Socket, Type};

use crate::errors::Error;
use crate::packet::{EchoReply, EchoRequest, ICMP_HEADER_SIZE, IcmpV4, IcmpV6, IpV4Packet};

const TOKEN_SIZE: usize = 24;
const ECHO_REQUEST_BUFFER_SIZE: usize = ICMP_HEADER_SIZE + TOKEN_SIZE;
type Token = [u8; TOKEN_SIZE];

/// The kind of socket used to send the ICMP request.
///
/// The default depends on the platform. On Windows [`Ping::new`] uses
/// [`RAW`](SocketType::RAW), elsewhere it uses [`DGRAM`](SocketType::DGRAM).
/// Override it with [`Ping::socket_type`].
#[derive(Clone, Copy, Debug)]
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

#[allow(deprecated)]
fn ping_with_socktype(
    socket_type: Type,
    addr: IpAddr,
    timeout: Option<Duration>,
    ttl: Option<u32>,
    ident: Option<u16>,
    seq_cnt: Option<u16>,
    payload: Option<&Token>,
    bind_device: Option<&str>,
) -> Result<PingResult, Error> {
    let time_start = SystemTime::now();

    let timeout = match timeout {
        Some(timeout) => timeout,
        None => Duration::from_secs(4),
    };

    let dest = SocketAddr::new(addr, 0);
    let mut buffer = [0; ECHO_REQUEST_BUFFER_SIZE];

    let default_payload: &Token = &random();

    let request = EchoRequest {
        ident: ident.unwrap_or(random()),
        seq_cnt: seq_cnt.unwrap_or(1),
        payload: payload.unwrap_or(default_payload),
    };

    let socket = if dest.is_ipv4() {
        if request.encode::<IcmpV4>(&mut buffer[..]).is_err() {
            return Err(Error::InternalError.into());
        }
        Socket::new(Domain::IPV4, socket_type, Some(Protocol::ICMPV4))?
    } else {
        if request.encode::<IcmpV6>(&mut buffer[..]).is_err() {
            return Err(Error::InternalError.into());
        }
        Socket::new(Domain::IPV6, socket_type, Some(Protocol::ICMPV6))?
    };

    if dest.is_ipv4() {
        socket.set_ttl_v4(ttl.unwrap_or(64))?;
    } else {
        socket.set_unicast_hops_v6(ttl.unwrap_or(64))?;
    }

    #[allow(unused)]
    if let Some(device) = bind_device {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            socket.bind_device(Some(device.as_bytes()))?;
        }
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            eprintln!("Warning: bind_device is only supported on Linux and Android platforms");
        }
    }

    socket.set_write_timeout(Some(timeout))?;

    socket.send_to(&mut buffer, &dest.into())?;

    // loop until either an echo whose payload token matches was received or timeout is over
    let mut elapsed_time = Duration::from_secs(0);
    loop {
        socket.set_read_timeout(Some(timeout - elapsed_time))?;

        let mut buffer: [u8; 2048] = [0; 2048];
        // socket2 0.6 recv_from requires &mut [MaybeUninit<u8>]; cast is sound
        // because MaybeUninit<u8> has the same layout as u8.
        let (n, src_addr) = socket.recv_from(unsafe {
            std::slice::from_raw_parts_mut(
                buffer.as_mut_ptr() as *mut std::mem::MaybeUninit<u8>,
                buffer.len(),
            )
        })?;
        let source_ip = src_addr.as_socket().map(|s| s.ip()).unwrap_or(addr);

        let mut recv_ttl: Option<u8> = None;
        let reply = if dest.is_ipv4() {
            // DGRAM socket on Linux may return pure ICMP packet without IP header.
            if n == ECHO_REQUEST_BUFFER_SIZE {
                match EchoReply::decode::<IcmpV4>(&buffer[..n]) {
                    Ok(reply) => reply,
                    Err(_) => continue,
                }
            } else {
                // Skip undecodable IP packets (malformed, truncated, or
                // unrelated ICMP traffic from other hosts on a RAW socket)
                // instead of failing the whole ping; keep waiting for our reply.
                let ipv4_packet = match IpV4Packet::decode(&buffer[..n]) {
                    Ok(packet) => packet,
                    Err(_) => continue,
                };
                recv_ttl = Some(ipv4_packet.ttl);
                match EchoReply::decode::<IcmpV4>(ipv4_packet.data) {
                    Ok(reply) => reply,
                    Err(_) => continue,
                }
            }
        } else {
            match EchoReply::decode::<IcmpV6>(&buffer[..n]) {
                Ok(reply) => reply,
                Err(_) => continue,
            }
        };

        // update elapsed time before deciding whether the payload token matches
        elapsed_time = match SystemTime::now().duration_since(time_start) {
            Ok(reply) => reply,
            Err(_) => return Err(Error::InternalError.into()),
        };

        if reply.payload == request.payload {
            // payload token matched: this reply belongs to our request
            return Ok(PingResult {
                rtt: elapsed_time,
                ident: reply.ident,
                seq_cnt: reply.seq_cnt,
                payload: reply.payload.to_vec(),
                source: source_ip,
                target: addr,
                ttl: recv_ttl,
            });
        }

        if elapsed_time >= timeout {
            let error = std::io::Error::new(std::io::ErrorKind::TimedOut, "Timeout occured");
            return Err(Error::IoError { error: (error) });
        }
    }
}

pub mod rawsock {
    use super::*;
    pub fn ping(
        addr: IpAddr,
        timeout: Option<Duration>,
        ttl: Option<u32>,
        ident: Option<u16>,
        seq_cnt: Option<u16>,
        payload: Option<&Token>,
    ) -> Result<(), Error> {
        ping_with_socktype(Type::RAW, addr, timeout, ttl, ident, seq_cnt, payload, None)?;
        Ok(())
    }
}

pub mod dgramsock {
    use super::*;
    pub fn ping(
        addr: IpAddr,
        timeout: Option<Duration>,
        ttl: Option<u32>,
        ident: Option<u16>,
        seq_cnt: Option<u16>,
        payload: Option<&Token>,
    ) -> Result<(), Error> {
        ping_with_socktype(
            Type::DGRAM,
            addr,
            timeout,
            ttl,
            ident,
            seq_cnt,
            payload,
            None,
        )?;
        Ok(())
    }
}

pub fn ping(
    addr: IpAddr,
    timeout: Option<Duration>,
    ttl: Option<u32>,
    ident: Option<u16>,
    seq_cnt: Option<u16>,
    payload: Option<&Token>,
) -> Result<(), Error> {
    rawsock::ping(addr, timeout, ttl, ident, seq_cnt, payload)?;
    Ok(())
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

    fn ping_with_socket(&self, sock_type: Type) -> Result<PingResult, Error> {
        ping_with_socktype(
            sock_type,
            self.addr,
            self.timeout,
            self.ttl,
            self.ident,
            self.seq_cnt,
            self.payload,
            #[cfg(any(target_os = "linux", target_os = "android"))]
            self.bind_device,
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            None,
        )
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
        self.ping_with_socket(self.socket_type.into())
    }
}

/// Creates a [`Ping`] builder targeting `addr`.
///
/// Shorthand for [`Ping::new`].
pub fn new<'a>(addr: IpAddr) -> Ping<'a> {
    return Ping::new(addr);
}
