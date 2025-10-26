use std::time::{Duration, SystemTime};
use rand::random;
use socket2::{Domain, Protocol, Socket, Type};

macro_rules! skip_if_no_capability {
    () => {
        if Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).is_err() {
            eprintln!("Skipping test: raw socket capability not available");
            return;
        }
    };
}

#[cfg(not(target_os = "windows"))]
macro_rules! skip_if_not_root {
    () => {
        if unsafe { libc::getuid() } != 0 {
            eprintln!("Skipping test: requires root privileges on Unix-like systems");
            return;
        }
    };
}

#[cfg(target_os = "windows")]
macro_rules! skip_if_not_root {
    () => {};
}

#[test]
fn basic() {
    skip_if_not_root!();

    let addr = "127.0.0.1".parse().unwrap();
    let timeout = Duration::from_secs(1);
    ping::ping(
        addr,
        Some(timeout),
        Some(166),
        Some(3),
        Some(5),
        Some(&random()),
    )
    .unwrap();
}

#[test]
fn basic_v6() {
    skip_if_not_root!();

    let addr = "::1".parse().unwrap();
    let timeout = Duration::from_secs(1);
    ping::ping(
        addr,
        Some(timeout),
        Some(166),
        Some(3),
        Some(5),
        Some(&random()),
    )
    .unwrap();
}

#[cfg(not(target_os = "windows"))]
#[test]
fn basic_dgram() {
    skip_if_no_capability!();
    let addr = "127.0.0.1".parse().unwrap();
    let timeout = Duration::from_secs(1);
    ping::dgramsock::ping(
        addr,
        Some(timeout),
        Some(166),
        Some(3),
        Some(5),
        Some(&random()),
    )
    .unwrap();
}

#[cfg(not(target_os = "windows"))]
#[test]
fn basic_dgram_v6() {
    skip_if_no_capability!();
    let addr = "::1".parse().unwrap();
    let timeout = Duration::from_secs(1);
    ping::dgramsock::ping(
        addr,
        Some(timeout),
        Some(166),
        Some(3),
        Some(5),
        Some(&random()),
    )
    .unwrap();
}

#[test]
fn builder_api1() {
    skip_if_no_capability!();
    let addr = "127.0.0.1".parse().unwrap();
    let timeout = Duration::from_secs(1);
    let mut pinger = ping::new(addr);
    pinger.timeout(timeout).ttl(42);
    pinger.send().unwrap();
}

#[test]
fn builder_api2() {
    skip_if_no_capability!();
    let addr = "127.0.0.1".parse().unwrap();
    let timeout = Duration::from_secs(1);
    ping::new(addr).timeout(timeout).ttl(42).send().unwrap();
}

#[test]
#[cfg(any(target_os = "linux", target_os = "android"))]
fn bind_device() {
    let addr = "127.0.0.1".parse().unwrap();
    let timeout = Duration::from_secs(1);
    ping::new(addr)
        .timeout(timeout)
        .ttl(42)
        .bind_device("lo")
        .socket_type(ping::SocketType::RAW)
        .send()
        .unwrap();
}

#[test]
fn duration() {
    // Ensure that the duration returned is less than the time elapsed
    skip_if_no_capability!();
    let addr = "127.0.0.1".parse().unwrap();
    let timeout = Duration::from_secs(1);
    let time_start = SystemTime::now();
    let time_reply = ping::new(addr).timeout(timeout).ttl(42).send().unwrap().elapsed_time;
    assert!(time_reply < SystemTime::now().duration_since(time_start).unwrap());
}

#[test]
fn ping_result_fields() {
    skip_if_no_capability!();
    let addr = "127.0.0.1".parse().unwrap();
    let timeout = Duration::from_secs(1);
    let custom_ident = 12345;
    let custom_seq = 42;
    let custom_payload = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24];

    let result = ping::new(addr)
        .timeout(timeout)
        .ident(custom_ident)
        .seq_cnt(custom_seq)
        .payload(&custom_payload)
        .send()
        .unwrap();

    // Test all fields in PingResult
    assert!(result.elapsed_time >= Duration::from_secs(0));
    assert!(result.elapsed_time <= timeout);

    assert_eq!(result.ident, custom_ident);
    assert_eq!(result.seq_cnt, custom_seq);
    // Check that our custom payload starts the response payload
    assert!(result.payload.starts_with(&custom_payload));
    assert_eq!(result.target, addr);

    // Verify payload is not empty and contains our custom data
    assert!(!result.payload.is_empty());
    // The response payload should contain at least our custom data
    assert!(result.payload.len() >= 24);
    // Check that our custom payload is contained in the response
    assert!(result.payload.starts_with(&custom_payload));
}

#[test]
fn ping_result_fields_v6() {
    skip_if_no_capability!();
    let addr = "::1".parse().unwrap();
    let timeout = Duration::from_secs(1);
    let custom_ident = 54321;
    let custom_seq = 99;

    let result = ping::new(addr)
        .timeout(timeout)
        .ident(custom_ident)
        .seq_cnt(custom_seq)
        .send()
        .unwrap();

    // Test all fields in PingResult for IPv6
    assert!(result.elapsed_time >= Duration::from_secs(0));
    assert!(result.elapsed_time <= timeout);

    assert_eq!(result.ident, custom_ident);
    assert_eq!(result.seq_cnt, custom_seq);
    assert_eq!(result.target, addr);

    // Verify payload is not empty (should be random if not specified)
    assert!(!result.payload.is_empty());
    // Payload may be larger due to ICMP response structure
    assert!(result.payload.len() >= 24); // TOKEN_SIZE
}

#[test]
fn ping_result_raw_socket() {
    skip_if_not_root!();
    let addr = "127.0.0.1".parse().unwrap();
    let timeout = Duration::from_secs(1);

    let result = ping::new(addr)
        .timeout(timeout)
        .socket_type(ping::SocketType::RAW)
        .ttl(128)
        .send()
        .unwrap();

    // Verify PingResult contains expected data
    assert!(result.elapsed_time > Duration::from_secs(0));
    assert!(result.elapsed_time < timeout);
    assert_eq!(result.target, addr);

    // Verify ident and seq_cnt are reasonable values
    assert!(result.ident > 0);
    assert!(result.seq_cnt > 0);

    // Verify payload exists
    assert!(result.payload.len() >= 24); // TOKEN_SIZE
}
