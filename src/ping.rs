use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, SystemTime};

use rand::random;
use socket2::{Domain, Protocol, Socket, Type};

use crate::errors::Error;
use crate::packet::{EchoReply, EchoRequest, ICMP_HEADER_SIZE, IcmpV4, IcmpV6, IpV4Packet};

const TOKEN_SIZE: usize = 24;
const ECHO_REQUEST_BUFFER_SIZE: usize = ICMP_HEADER_SIZE + TOKEN_SIZE;
type Token = [u8; TOKEN_SIZE];

#[derive(Clone, Copy, Debug)]
pub enum SocketType {
    RAW,
    DGRAM,
    /// Call the system `ping`/`ping6` command instead of a raw socket.
    SYSTEM,
}

#[derive(Debug)]
#[non_exhaustive]
pub struct PingResult {
    pub rtt: Duration,
    pub ident: u16,
    pub seq_cnt: u16,
    pub payload: Vec<u8>,
    /// The actual source IP address from the reply packet.
    pub source: IpAddr,
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

    // loop until either an echo with correct ident was received or timeout is over
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
                match EchoReply::decode::<IcmpV4>(&buffer) {
                    Ok(reply) => reply,
                    Err(_) => continue,
                }
            } else {
                let ipv4_packet = match IpV4Packet::decode(&buffer) {
                    Ok(packet) => packet,
                    Err(_) => return Err(Error::DecodeV4Error.into()),
                };
                recv_ttl = Some(ipv4_packet.ttl);
                match EchoReply::decode::<IcmpV4>(ipv4_packet.data) {
                    Ok(reply) => reply,
                    Err(_) => continue,
                }
            }
        } else {
            match EchoReply::decode::<IcmpV6>(&buffer) {
                Ok(reply) => reply,
                Err(_) => continue,
            }
        };

        // if ident is not correct check if timeout is over
        elapsed_time = match SystemTime::now().duration_since(time_start) {
            Ok(reply) => reply,
            Err(_) => return Err(Error::InternalError.into()),
        };

        if reply.payload == request.payload {
            // received correct ident
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

    pub fn timeout(&mut self, timeout: Duration) -> &mut Self {
        self.timeout = Some(timeout);
        return self;
    }

    pub fn ttl(&mut self, ttl: u32) -> &mut Self {
        self.ttl = Some(ttl);
        return self;
    }

    pub fn ident(&mut self, ident: u16) -> &mut Self {
        self.ident = Some(ident);
        return self;
    }

    pub fn seq_cnt(&mut self, seq_cnt: u16) -> &mut Self {
        self.seq_cnt = Some(seq_cnt);
        return self;
    }

    pub fn payload(&mut self, payload: &'a Token) -> &mut Self {
        self.payload = Some(payload);
        return self;
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub fn bind_device(&mut self, device: &'a str) -> &mut Self {
        self.bind_device = Some(device);
        return self;
    }

    pub fn send(&self) -> Result<PingResult, Error> {
        match self.socket_type {
            SocketType::SYSTEM => ping_with_system_cmd(self.addr, self.timeout, self.ttl),
            SocketType::RAW => self.ping_with_socket(Type::RAW),
            SocketType::DGRAM => self.ping_with_socket(Type::DGRAM),
        }
    }
}

pub fn new<'a>(addr: IpAddr) -> Ping<'a> {
    return Ping::new(addr);
}

#[cfg(target_os = "macos")]
fn extract_float_field(line: &str, prefix: &str) -> Option<f64> {
    let start = line.find(prefix)? + prefix.len();
    let rest = &line[start..];
    let end = rest.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[cfg(target_os = "macos")]
fn extract_u8_field(line: &str, prefix: &str) -> Option<u8> {
    let start = line.find(prefix)? + prefix.len();
    let rest = &line[start..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[cfg(target_os = "macos")]
fn extract_u16_field(line: &str, prefix: &str) -> Option<u16> {
    let start = line.find(prefix)? + prefix.len();
    let rest = &line[start..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[cfg(target_os = "macos")]
fn extract_source_ip(line: &str) -> Option<IpAddr> {
    // Format variants:
    //   "64 bytes from 8.8.8.8: icmp_seq=..."
    //   "64 bytes from dns.google (8.8.8.8): icmp_seq=..."
    //   "16 bytes from ::1%lo0: icmp_seq=..."
    let after_from = line.find("bytes from ")? + "bytes from ".len();
    let rest = &line[after_from..];

    // If there's a parenthesised IP, prefer that.
    let candidate = if let Some(open) = rest.find('(') {
        let close = rest.find(')')?;
        &rest[open + 1..close]
    } else {
        // Strip trailing ':'
        let end = rest.find([':', ' ']).unwrap_or(rest.len());
        &rest[..end]
    };

    // Strip IPv6 zone ID (e.g. "::1%lo0")
    let candidate = match candidate.find('%') {
        Some(pct) => &candidate[..pct],
        None => candidate,
    };

    candidate.parse().ok()
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn ping_with_system_cmd(
    addr: IpAddr,
    timeout: Option<Duration>,
    ttl: Option<u32>,
) -> Result<PingResult, Error> {
    use std::process::Command;

    let time_start = SystemTime::now();

    let output = if addr.is_ipv4() {
        let mut cmd = Command::new("ping");
        cmd.arg("-c").arg("1");
        if let Some(t) = timeout {
            cmd.arg("-W").arg(t.as_millis().to_string());
        }
        if let Some(t) = ttl {
            cmd.arg("-m").arg(t.to_string());
        }
        cmd.arg(addr.to_string()).output()
    } else {
        let mut cmd = Command::new("ping6");
        cmd.arg("-c").arg("1");
        if let Some(t) = ttl {
            cmd.arg("-h").arg(t.to_string());
        }
        cmd.arg(addr.to_string()).output()
    }
    .map_err(|e| Error::IoError { error: e })?;

    if !output.status.success() {
        let error = std::io::Error::new(std::io::ErrorKind::TimedOut, "ping command failed");
        return Err(Error::IoError { error });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    let reply_line = stdout
        .lines()
        .find(|line| line.contains("bytes from") && line.contains("time="))
        .ok_or_else(|| {
            let error =
                std::io::Error::new(std::io::ErrorKind::InvalidData, "failed to parse ping output");
            Error::IoError { error }
        })?;

    let rtt_ms = extract_float_field(reply_line, "time=").ok_or_else(|| {
        let error = std::io::Error::new(std::io::ErrorKind::InvalidData, "failed to parse RTT");
        Error::IoError { error }
    })?;

    let seq_cnt = extract_u16_field(reply_line, "icmp_seq=").unwrap_or(0);
    let reply_ttl = extract_u8_field(reply_line, "ttl=")
        .or_else(|| extract_u8_field(reply_line, "hlim="));
    let source = extract_source_ip(reply_line).unwrap_or(addr);

    let elapsed = SystemTime::now()
        .duration_since(time_start)
        .unwrap_or(Duration::from_micros((rtt_ms * 1000.0) as u64));

    Ok(PingResult {
        rtt: elapsed,
        ident: 0,
        seq_cnt,
        payload: vec![],
        source,
        target: addr,
        ttl: reply_ttl,
    })
}

#[cfg(not(target_os = "macos"))]
fn ping_with_system_cmd(
    _addr: IpAddr,
    _timeout: Option<Duration>,
    _ttl: Option<u32>,
) -> Result<PingResult, Error> {
    Err(Error::InvalidProtocol)
}
