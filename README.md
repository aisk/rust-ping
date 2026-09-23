# rust ping

[![Crates.io](https://img.shields.io/crates/v/ping.svg)](https://crates.io/crates/ping)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Docs](https://docs.rs/ping/badge.svg)](https://docs.rs/ping/)

Ping function implemented in rust.

## Usage

Create a `Pinger` and call `ping` with a target address and a timeout. The pinger keeps its sockets open, so reuse it when pinging repeatedly or pinging several hosts.

```rust
use std::net::IpAddr;
use std::time::Duration;

fn main() {
    let target: IpAddr = "8.8.8.8".parse().unwrap();
    let mut pinger = ping::Pinger::new();
    match pinger.ping(target, Duration::from_secs(1)) {
        Ok(reply) => println!("rtt {:?} from {}", reply.rtt, reply.source),
        Err(e) => eprintln!("Ping failed: {}", e),
    }
}
```

The same pinger works for both IPv4 and IPv6 targets. A timeout is reported as `Error::Timeout`:

```rust
use std::net::IpAddr;
use std::time::Duration;
use ping::{Error, Pinger};

fn main() -> Result<(), Error> {
    let target: IpAddr = "8.8.8.8".parse().unwrap();
    let mut pinger = Pinger::new();
    for _ in 0..10 {
        match pinger.ping(target, Duration::from_secs(1)) {
            Ok(r) => println!("seq={} rtt={:?} ttl={:?}", r.seq, r.rtt, r.ttl),
            Err(Error::Timeout) => println!("timeout"),
            Err(e) => return Err(e),
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}
```

Options that apply to a single request, like TTL, sequence number and payload, are set on a `Request`. Options that apply to the socket, like the socket type, ICMP identifier, network interface and fwmark, are set with `Pinger::builder`:

```rust
use std::net::IpAddr;
use std::time::Duration;
use ping::{Pinger, Request, SocketType};

fn main() -> Result<(), ping::Error> {
    let target: IpAddr = "8.8.8.8".parse().unwrap();
    let mut pinger = Pinger::builder()
        .socket_type(SocketType::RAW)
        .bind_device("eth0") // Linux and Android only
        .build()?;
    let request = Request::new(target).ttl(5).payload(b"hello".to_vec());
    pinger.ping(request, Duration::from_secs(1))?;
    Ok(())
}
```

`ping` takes `&mut self`, so a pinger handles one request at a time. To ping from several threads, give each thread its own `Pinger`.

To perform a ping using a domain name instead of an IP address, you can use any 3rd-party DNS resolver or [`ToSocketAddrs`](https://doc.rust-lang.org/std/net/trait.ToSocketAddrs.html) from the standard library:

```rust
use std::net::ToSocketAddrs;
use std::time::Duration;

fn main() {
    let address = "www.google.com:0"  // use any port, we only need the IP
        .to_socket_addrs() // convert domain name to socket address iterator
        .unwrap()
        .next() // take the first socket address
        .unwrap()
        .ip(); // convert to IP

    match ping::Pinger::new().ping(address, Duration::from_secs(1)) {
        Ok(_) => println!("Ping successful!"),
        Err(e) => eprintln!("Ping failed: {}", e),
    }
}
```

## Optional Tokio support

Tokio-based asynchronous pinging is available behind the optional `tokio` feature. The feature is disabled by default, so synchronous users do not pull Tokio into their dependency graph.

```toml
[dependencies]
ping = { version = "0.9", features = ["tokio"] }
```

`ping::tokio::Pinger` is built the same way as `ping::Pinger` and uses the same `Request`, `Reply` and `Error` types, but `ping` is an `async fn`:

```rust
use std::net::IpAddr;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let target: IpAddr = "8.8.8.8".parse().unwrap();
    let mut pinger = ping::tokio::Pinger::new();
    let reply = pinger
        .ping(target, Duration::from_secs(1))
        .await
        .expect("ping failed");

    println!("round-trip time: {:?}", reply.rtt);
}
```

On Unix, the async pinger uses nonblocking sockets registered with Tokio's reactor, so waiting for a reply does not occupy a thread. Windows currently uses Tokio's blocking task pool as a compatibility implementation; native asynchronous Windows socket support may be added in a future release. To ping concurrently, create one pinger per task.

## Socket Types: DGRAM vs. RAW

Sending an ICMP package typically requires creating a `raw` socket, which often demands special privileges (e.g., running with `sudo` on Linux). This can introduce security risks.

Modern operating systems support `unprivileged ping` using `dgram` sockets, which do not require elevated privileges.

By default, `Pinger` tries a `DGRAM` socket first on Linux, Android and macOS and falls back to `RAW`, and does the opposite on other platforms. You can pick one explicitly with the `socket_type` method of the builder:

```rust
use std::net::IpAddr;
use std::time::Duration;
use ping::{Pinger, SocketType};

fn main() -> Result<(), ping::Error> {
    let target: IpAddr = "8.8.8.8".parse().unwrap();
    let timeout = Duration::from_secs(1);

    // Using a DGRAM socket (unprivileged)
    let mut pinger = Pinger::builder().socket_type(SocketType::DGRAM).build()?;
    match pinger.ping(target, timeout) {
        Ok(_) => println!("Ping successful with DGRAM socket!"),
        Err(e) => eprintln!("Ping failed with DGRAM socket: {}", e),
    }

    // Using a RAW socket (may require privileges)
    let mut pinger = Pinger::builder().socket_type(SocketType::RAW).build()?;
    match pinger.ping(target, timeout) {
        Ok(_) => println!("Ping successful with RAW socket!"),
        Err(e) => eprintln!("Ping failed with RAW socket: {}", e),
    }
    Ok(())
}
```

The TTL of the reply (`Reply::ttl`) is only available when the socket receives the IP header, which is the case for IPv4 `RAW` sockets, and IPv4 `DGRAM` sockets outside Linux and Android.

For Linux users, even if the kernel supports `dgram` ping, some distributions (like Arch) might disable it by default. More details: https://wiki.archlinux.org/title/sysctl#Allow_unprivileged_users_to_create_IPPROTO_ICMP_sockets

## License

This library is licensed under the MIT license ([LICENSE](./LICENSE)).
Parts of the packet parsing code are derived from
https://github.com/knsd/tokio-ping, used under its MIT license option.
