use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use rand::random;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};

use crate::errors::Error;
use crate::packet::{EchoReply, ICMP_HEADER_SIZE, IcmpV4, IcmpV6, IpV4Packet, build_echo_request};
use crate::pinger::{Reply, Request, SocketType};

const NONCE_SIZE: usize = 8;
const DEFAULT_TTL: u8 = 64;
const IPV4_HEADER_SIZE: usize = 20;
const MAX_IP_PAYLOAD: usize = 65535;
const RECV_BUFFER_SIZE: usize = 65536;
/// `set_read_timeout` treats zero as "no timeout" and socket2 truncates to
/// microseconds (milliseconds on Windows), so never go below this.
pub(crate) const MIN_SOCKET_TIMEOUT: Duration = Duration::from_millis(1);

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
const DEFAULT_SOCKET_TYPES: &[SocketType] = &[SocketType::DGRAM, SocketType::RAW];
#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
const DEFAULT_SOCKET_TYPES: &[SocketType] = &[SocketType::RAW, SocketType::DGRAM];

fn raw_type(socket_type: SocketType) -> Type {
    match socket_type {
        SocketType::RAW => Type::RAW,
        SocketType::DGRAM => Type::DGRAM,
    }
}

/// Socket level options shared by every address family of a pinger.
#[derive(Clone, Debug, Default)]
pub(crate) struct Config {
    pub(crate) socket_type: Option<SocketType>,
    pub(crate) ident: Option<u16>,
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(crate) bind_device: Option<String>,
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(crate) mark: Option<u32>,
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
            match Socket::new(domain, raw_type(socket_type), Some(protocol)) {
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

fn max_payload(target: IpAddr) -> usize {
    let overhead = ICMP_HEADER_SIZE + NONCE_SIZE;
    match target {
        IpAddr::V4(_) => MAX_IP_PAYLOAD - IPV4_HEADER_SIZE - overhead,
        // The IPv6 payload length does not include the fixed header.
        IpAddr::V6(_) => MAX_IP_PAYLOAD - overhead,
    }
}

pub(crate) fn check_payload(request: &Request) -> Result<(), Error> {
    let max = max_payload(request.target);
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
