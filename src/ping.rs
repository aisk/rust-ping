use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration,SystemTime};

use rand::random;
use socket2::{Domain, Protocol, Socket, Type};

use crate::errors::Error;
use crate::packet::{EchoReply, EchoRequest, IcmpV4, IcmpV6, IpV4Packet, ICMP_HEADER_SIZE};

const TOKEN_SIZE: usize = 24;
const ECHO_REQUEST_BUFFER_SIZE: usize = ICMP_HEADER_SIZE + TOKEN_SIZE;
type Token = [u8; TOKEN_SIZE];

fn ping_with_socktype(
    socket_type: Type,
    addr: IpAddr,
    timeout: Option<Duration>,
    ttl: Option<u32>,
    ident: Option<u16>,
    seq_cnt: Option<u16>,
    payload: Option<&Token>,
) -> Result<(), Error> {
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

    let mut socket = if dest.is_ipv4() {
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
        socket.set_ttl(ttl.unwrap_or(64))?;
    } else {
        socket.set_unicast_hops_v6(ttl.unwrap_or(64))?;
    }

    socket.set_write_timeout(Some(timeout))?;

    socket.send_to(&mut buffer, &dest.into())?;

    // loop until either an echo with correct ident was received or timeout is over
    let mut time_elapsed = Duration::from_secs(0);
    loop {
        socket.set_read_timeout(Some(timeout - time_elapsed))?;

        let mut buffer: [u8; 2048] = [0; 2048];
        socket.read(&mut buffer)?;

        let reply = if dest.is_ipv4() {
            let ipv4_packet = match IpV4Packet::decode(&buffer) {
                Ok(packet) => packet,
                Err(_) => return Err(Error::DecodeV4Error.into()),
            };
            match EchoReply::decode::<IcmpV4>(ipv4_packet.data) {
                Ok(reply) => reply,
                Err(_) => continue,
            }
        } else {
            match EchoReply::decode::<IcmpV6>(&buffer) {
                Ok(reply) => reply,
                Err(_) => continue,
            }
        };

        if reply.ident == request.ident {
            // received correct ident
            return Ok(());
        }

        // if ident is not correct check if timeout is over
        time_elapsed = match SystemTime::now().duration_since(time_start) {
            Ok(reply) => reply,
            Err(_) => return Err(Error::InternalError.into()),
        };
        if time_elapsed >= timeout {
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
        return ping_with_socktype(Type::RAW, addr, timeout, ttl, ident, seq_cnt, payload);
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
        return ping_with_socktype(Type::DGRAM, addr, timeout, ttl, ident, seq_cnt, payload);
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
    return rawsock::ping(addr, timeout, ttl, ident, seq_cnt, payload);
}
