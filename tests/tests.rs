use std::time::{Duration, SystemTime};
use rand::random;

#[test]
fn basic() {
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
    let addr = "127.0.0.1".parse().unwrap();
    let timeout = Duration::from_secs(1);
    let mut pinger = ping::new(addr);
    pinger.timeout(timeout).ttl(42);
    pinger.send().unwrap();
}

#[test]
fn builder_api2() {
    let addr = "127.0.0.1".parse().unwrap();
    let timeout = Duration::from_secs(1);
    ping::new(addr).timeout(timeout).ttl(42).send().unwrap();
}

#[test]
fn duration() {
    // Ensure that the duration returned is less than the time elapsed 
    let addr = "127.0.0.1".parse().unwrap();
    let timeout = Duration::from_secs(1);
    let time_start = SystemTime::now();
    let time_reply = ping::new(addr).timeout(timeout).ttl(42).send().unwrap();
    assert!(time_reply < SystemTime::now().duration_since(time_start).unwrap());
}
