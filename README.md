# rust ping

[![Crates.io](https://img.shields.io/crates/v/ping.svg)](https://crates.io/crates/ping)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Docs](https://docs.rs/ping/badge.svg)](https://docs.rs/ping/)

Ping function implemented in rust.

## dgram sock and raw sock

Sending an ICMP package should create a socket of type `raw` on most platforms. And most of these platforms require special privileges. Basically, it needs to run with sudo on Linux to create a `raw` socket.

These requirements introduce security risks, so on modern platforms, `unprivileged ping` has been introduced, with socket type `dgram`. So there are two mods in this crate, rawsock and dgramsock, which have the same function `ping`. And the global ping function is just an alias for the `rawsock::ping`. You can pick the one which is suitable for your use case.

For Linux users, although modern kernels support ping with `dgram`, in some distributions (like Arch), it's disabled by default. More details: https://wiki.archlinux.org/title/sysctl#Allow_unprivileged_users_to_create_IPPROTO_ICMP_sockets

## License

This library contains codes from https://github.com/knsd/tokio-ping, which is licensed under either of

- Apache License, Version 2.0, (LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license (LICENSE-MIT or http://opensource.org/licenses/MIT)

And other codes is licensed under

- MIT license (LICENSE-MIT or http://opensource.org/licenses/MIT)
