#[macro_use]
extern crate failure;
extern crate rand;
extern crate socket2;

use packet::{EchoReply, EchoRequest, IcmpV4, IcmpV6, ICMP_HEADER_SIZE};
use packet::{IpV4Packet, IpV4Protocol};
use rand::random;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{SocketAddr, IpAddr};

mod packet;

const TOKEN_SIZE: usize = 24;
const ECHO_REQUEST_BUFFER_SIZE: usize = ICMP_HEADER_SIZE + TOKEN_SIZE;
type Token = [u8; TOKEN_SIZE];

pub fn ping(addr: IpAddr) {
    let dest = SocketAddr::new(addr, 0);
    let mut buffer = [0; ECHO_REQUEST_BUFFER_SIZE];

    let token: Token = random();
    let ident = random();

    let request = EchoRequest {
        ident: ident,
        seq_cnt: 1,
        payload: &token,
    };

    if dest.is_ipv4() {
        request.encode::<IcmpV4>(&mut buffer[..]).unwrap();
    } else {
        request.encode::<IcmpV6>(&mut buffer[..]).unwrap();
    }

    let socket = if dest.is_ipv4() {
        Socket::new(Domain::ipv4(), Type::raw(), Some(Protocol::icmpv4())).unwrap()
    } else {
        Socket::new(Domain::ipv6(), Type::raw(), Some(Protocol::icmpv6())).unwrap()
    };

    socket.send_to(&mut buffer, &dest.into()).unwrap();

    let mut buffer: [u8; 2048] = [0; 2048];
    socket.recv_from(&mut buffer).unwrap();

    let reply = if dest.is_ipv4() {
        let ipv4_packet = IpV4Packet::decode(&buffer).unwrap();
        assert!(ipv4_packet.protocol == IpV4Protocol::Icmp);
        EchoReply::decode::<IcmpV4>(ipv4_packet.data).unwrap()
    } else {
        EchoReply::decode::<IcmpV6>(&buffer).unwrap()
    };
    assert!(reply.ident == request.ident);
    assert!(reply.seq_cnt == request.seq_cnt);
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        super::ping("127.0.0.1".parse().unwrap());
    }
}
