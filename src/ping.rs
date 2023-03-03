use std::net::{SocketAddr, IpAddr};
use std::io::Read;
use std::time::{Duration,SystemTime};

use rand::random;
use socket2::{Domain, Protocol, Socket, Type};

use crate::errors::{Error};
use crate::packet::{EchoReply, EchoRequest, IpV4Packet, IcmpV4, IcmpV6, ICMP_HEADER_SIZE};

const TOKEN_SIZE: usize = 24;
const ECHO_REQUEST_BUFFER_SIZE: usize = ICMP_HEADER_SIZE + TOKEN_SIZE;
type Token = [u8; TOKEN_SIZE];

pub fn ping(addr: IpAddr, timeout: Option<Duration>, ttl: Option<u32>, ident: Option<u16>, seq_cnt: Option<u16>, payload: Option<&Token>) -> Result<(), Error> {
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
        Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4))?
    } else {
        if request.encode::<IcmpV6>(&mut buffer[..]).is_err() {
            return Err(Error::InternalError.into());
        }
        Socket::new(Domain::IPV6, Type::RAW, Some(Protocol::ICMPV6))?
    };

    socket.set_ttl(ttl.unwrap_or(64))?;

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
                Err(_) => return Err(Error::InternalError.into()),
            };
            match EchoReply::decode::<IcmpV4>(ipv4_packet.data) {
                Ok(reply) => reply,
                Err(_) => return Err(Error::InternalError.into()),
            }
        } else {
            match EchoReply::decode::<IcmpV6>(&buffer) {
                Ok(reply) => reply,
                Err(_) => return Err(Error::InternalError.into()),
            }
        };

        if reply.ident == request.ident {
            // received correct ident
            return Ok(());
        }

        // if ident is not correct check if timeout is over
        time_elapsed = SystemTime::now().duration_since(time_start).expect("Clock may have gone backwards");
        if time_elapsed >= timeout {
            let error = std::io::Error::new(std::io::ErrorKind::TimedOut, "Timeout occured");
            return Err(Error::IoError { error: (error) });
        }
    }
}
