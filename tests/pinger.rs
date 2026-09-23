use std::net::IpAddr;
use std::time::Duration;

use ping::{Error, Pinger, Request, SocketType};
use socket2::{Domain, Protocol, Socket, Type};

const TIMEOUT: Duration = Duration::from_secs(1);

fn v4() -> IpAddr {
    "127.0.0.1".parse().unwrap()
}

fn v6() -> IpAddr {
    "::1".parse().unwrap()
}

fn available(socket_type: SocketType, target: IpAddr) -> bool {
    let socket_type = match socket_type {
        SocketType::RAW => Type::RAW,
        _ => Type::DGRAM,
    };
    let (domain, protocol) = match target {
        IpAddr::V4(_) => (Domain::IPV4, Protocol::ICMPV4),
        IpAddr::V6(_) => (Domain::IPV6, Protocol::ICMPV6),
    };
    Socket::new(domain, socket_type, Some(protocol)).is_ok()
}

fn any_available(target: IpAddr) -> bool {
    available(SocketType::RAW, target) || available(SocketType::DGRAM, target)
}

macro_rules! skip_unless {
    ($cond:expr) => {
        if !$cond {
            eprintln!("Skipping test: ICMP socket not available");
            return;
        }
    };
}

#[test]
fn ping_v4() {
    skip_unless!(any_available(v4()));
    let reply = Pinger::new().ping(v4(), TIMEOUT).unwrap();
    assert_eq!(reply.source, v4());
    assert!(reply.rtt <= TIMEOUT);
    assert_eq!(reply.payload, vec![0; 48]);
}

#[test]
fn ping_v6() {
    skip_unless!(any_available(v6()));
    let reply = Pinger::new().ping(v6(), TIMEOUT).unwrap();
    assert_eq!(reply.source, v6());
    assert_eq!(reply.ttl, None);
}

#[test]
fn ping_function() {
    for target in [v4(), v6()] {
        if !any_available(target) {
            continue;
        }
        assert_eq!(ping::ping(target, TIMEOUT).unwrap().source, target);
    }
}

#[test]
fn seq_increments() {
    skip_unless!(any_available(v4()));
    let mut pinger = Pinger::new();
    let seqs: Vec<u16> = (0..3)
        .map(|_| pinger.ping(v4(), TIMEOUT).unwrap().seq)
        .collect();
    assert_eq!(seqs, vec![seqs[0], seqs[0] + 1, seqs[0] + 2]);
}

#[test]
fn explicit_seq() {
    skip_unless!(any_available(v4()));
    let reply = Pinger::new()
        .ping(Request::new(v4()).seq(4242), TIMEOUT)
        .unwrap();
    assert_eq!(reply.seq, 4242);
}

#[test]
fn alternate_families() {
    skip_unless!(any_available(v4()) && any_available(v6()));
    let mut pinger = Pinger::new();
    for _ in 0..2 {
        assert_eq!(pinger.ping(v4(), TIMEOUT).unwrap().source, v4());
        assert_eq!(pinger.ping(v6(), TIMEOUT).unwrap().source, v6());
    }
}

#[test]
fn threads() {
    skip_unless!(any_available(v4()) && any_available(v6()));
    let handles: Vec<_> = [v4(), v6(), v4(), v6()]
        .into_iter()
        .map(|target| {
            std::thread::spawn(move || {
                let mut pinger = Pinger::new();
                for _ in 0..5 {
                    assert_eq!(pinger.ping(target, TIMEOUT).unwrap().source, target);
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn payload_round_trip() {
    skip_unless!(any_available(v4()));
    let mut pinger = Pinger::new();
    for payload in [vec![], b"hello".to_vec(), vec![0xab; 4000]] {
        let reply = pinger
            .ping(Request::new(v4()).payload(payload.clone()), TIMEOUT)
            .unwrap();
        assert_eq!(reply.payload, payload);
    }
}

#[test]
fn payload_too_large() {
    let mut pinger = Pinger::new();
    let request = Request::new(v4()).payload(vec![0; 65500]);
    assert!(matches!(
        pinger.ping(request, TIMEOUT),
        Err(Error::PayloadTooLarge { max: 65499 })
    ));
    let request = Request::new(v6()).payload(vec![0; 65520]);
    assert!(matches!(
        pinger.ping(request, TIMEOUT),
        Err(Error::PayloadTooLarge { max: 65519 })
    ));
}

#[test]
fn request_ttl() {
    skip_unless!(any_available(v4()));
    let mut pinger = Pinger::new();
    pinger.ping(Request::new(v4()).ttl(5), TIMEOUT).unwrap();
    pinger.ping(v4(), TIMEOUT).unwrap();
}

#[test]
fn zero_timeout() {
    skip_unless!(any_available(v4()));
    let result = Pinger::new().ping(v4(), Duration::ZERO);
    assert!(matches!(result, Err(Error::Timeout)), "{result:?}");
}

#[test]
fn each_socket_type_with_ident() {
    for socket_type in [SocketType::RAW, SocketType::DGRAM] {
        for target in [v4(), v6()] {
            if !available(socket_type, target) {
                continue;
            }
            let mut pinger = Pinger::builder()
                .socket_type(socket_type)
                .ident(4321)
                .build()
                .unwrap();
            // A reply is only accepted when its ident matches.
            let reply = pinger.ping(target, TIMEOUT).unwrap();
            assert_eq!(reply.source, target);

            let has_ttl = target.is_ipv4()
                && !(cfg!(target_os = "linux") && socket_type == SocketType::DGRAM);
            assert_eq!(reply.ttl.is_some(), has_ttl, "{socket_type:?} {target}");
        }
    }
}

#[test]
#[cfg(target_os = "linux")]
fn bind_device() {
    skip_unless!(any_available(v4()));
    let mut pinger = Pinger::builder().bind_device("lo").build().unwrap();
    pinger.ping(v4(), TIMEOUT).unwrap();
}

#[cfg(feature = "tokio")]
#[tokio::test]
async fn async_ping() {
    skip_unless!(any_available(v4()) && any_available(v6()));
    let mut pinger = ping::tokio::Pinger::new();
    let first = pinger.ping(v4(), TIMEOUT).await.unwrap();
    let second = pinger.ping(v4(), TIMEOUT).await.unwrap();
    assert_eq!(second.seq, first.seq + 1);
    assert_eq!(pinger.ping(v6(), TIMEOUT).await.unwrap().source, v6());

    let request = Request::new(v4()).ttl(5).payload(b"hello".to_vec());
    assert_eq!(
        pinger.ping(request, TIMEOUT).await.unwrap().payload,
        b"hello"
    );
}

#[cfg(feature = "tokio")]
#[tokio::test]
async fn async_ping_function() {
    for target in [v4(), v6()] {
        if !any_available(target) {
            continue;
        }
        let reply = ping::tokio::ping(target, TIMEOUT).await.unwrap();
        assert_eq!(reply.source, target);
    }
}

#[cfg(feature = "tokio")]
#[tokio::test]
async fn async_builder() {
    skip_unless!(available(SocketType::RAW, v4()));
    let mut pinger = ping::tokio::Pinger::builder()
        .socket_type(SocketType::RAW)
        .ident(4323)
        .build()
        .unwrap();
    assert!(pinger.ping(v4(), TIMEOUT).await.unwrap().ttl.is_some());
}
