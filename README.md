# rust ping

[![Crates.io](https://img.shields.io/crates/v/ping.svg)](https://crates.io/crates/ping)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Docs](https://docs.rs/ping/badge.svg)](https://docs.rs/ping/)

Ping function implemented in rust.

## Usage

To perform a basic ping, you can use the `ping::new` function to create a `Ping` instance and then call the `send` method. By default, on non-Windows systems, it attempts to use a `DGRAM` socket, falling back to `RAW` on Windows.

```rust
fn main() {
    let target_ip = "8.8.8.8".parse().unwrap();
    match ping::new(target_ip).send() {
        Ok(_) => println!("Ping successful!"),
        Err(e) => eprintln!("Ping failed: {}", e),
    }
}
```

You can also configure various options like timeout, TTL, and socket type using the builder pattern:

```rust
use std::time::Duration;

fn main() {
    let target_ip = "8.8.8.8".parse().unwrap();
    match ping::new(target_ip)
        .timeout(Duration::from_secs(2))
        .ttl(128)
        .send()
    {
        Ok(_) => println!("Ping successful with custom options!"),
        Err(e) => eprintln!("Ping failed: {}", e),
    }
}
```

## Optional Tokio support

Tokio-based asynchronous sending is available behind the optional `tokio` feature. The feature is disabled by default, so synchronous users do not pull Tokio into their dependency graph.

```toml
[dependencies]
ping = { version = "0.9", features = ["tokio"] }
```

```rust
use std::time::Duration;

#[tokio::main]
async fn main() {
    let target_ip = "8.8.8.8".parse().unwrap();
    let result = ping::new(target_ip)
        .timeout(Duration::from_secs(2))
        .send_async()
        .await
        .expect("ping failed");

    println!("round-trip time: {:?}", result.rtt);
}
```

On Unix, `send_async` uses a nonblocking socket registered with Tokio's reactor, so each in-flight ping does not occupy a thread. Windows currently uses Tokio's blocking task pool as a compatibility implementation; native asynchronous Windows socket support may be added in a future release.

To perform a ping using a domain name instead of an IP address, you can use any 3rd-party DNS resolver or [`ToSocketAddrs`](https://doc.rust-lang.org/std/net/trait.ToSocketAddrs.html) from the standard library:

```rust
use std::net::ToSocketAddrs;

fn main() {
    let address = "www.google.com:0"  // use any port, we only need the IP
        .to_socket_addrs() // convert domain name to socket address iterator
        .unwrap()
        .next() // take the first socket address
        .unwrap()
        .ip(); // convert to IP

    match ping::new(address).send() {
        Ok(_) => println!("Ping successful!"),
        Err(e) => eprintln!("Ping failed: {}", e),
    }
}
```

## Socket Types: DGRAM vs. RAW

Sending an ICMP package typically requires creating a `raw` socket, which often demands special privileges (e.g., running with `sudo` on Linux). This can introduce security risks.

Modern operating systems support `unprivileged ping` using `dgram` sockets, which do not require elevated privileges.

You can specify the socket type using the `socket_type` method of the `Ping` builder.

```rust
fn main() {
    let target_ip = "8.8.8.8".parse().unwrap();

    // Using a DGRAM socket (unprivileged)
    match ping::new(target_ip).socket_type(ping::DGRAM).send() {
        Ok(_) => println!("Ping successful with DGRAM socket!"),
        Err(e) => eprintln!("Ping failed with DGRAM socket: {}", e),
    }

    // Using a RAW socket (may require privileges)
    match ping::new(target_ip).socket_type(ping::RAW).send() {
        Ok(_) => println!("Ping successful with RAW socket!"),
        Err(e) => eprintln!("Ping failed with RAW socket: {}", e),
    }
}
```

For Linux users, even if the kernel supports `dgram` ping, some distributions (like Arch) might disable it by default. More details: https://wiki.archlinux.org/title/sysctl#Allow_unprivileged_users_to_create_IPPROTO_ICMP_sockets

## License

This library contains codes from https://github.com/knsd/tokio-ping, which is licensed under either of

- Apache License, Version 2.0, (LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license (LICENSE-MIT or http://opensource.org/licenses/MIT)

And other codes is licensed under

- MIT license (LICENSE-MIT or http://opensource.org/licenses/MIT)
