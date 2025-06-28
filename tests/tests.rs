extern crate ping;
extern crate rand;

use std::time::Duration;

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
fn builder_api() {
    let addr = "127.0.0.1".parse().unwrap();
    let timeout = Duration::from_secs(1);
    ping::new(addr).timeout(timeout).send().unwrap();
}
