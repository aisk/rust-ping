use crate::errors::Error;

#[cfg(unix)]
use std::net::{IpAddr, SocketAddr};
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
use socket2::Type;
#[cfg(unix)]
use tokio::io::unix::AsyncFd;

#[cfg(not(unix))]
use super::ping_with_socktype;
use super::{Ping, PingResult};
#[cfg(unix)]
use super::{Token, create_socket, decode_reply, prepare_request};

#[cfg(unix)]
#[allow(deprecated)]
async fn ping_with_socktype_async(
    socket_type: Type,
    addr: IpAddr,
    timeout: Option<Duration>,
    ttl: Option<u32>,
    ident: Option<u16>,
    seq_cnt: Option<u16>,
    payload: Option<&Token>,
    bind_device: Option<&str>,
) -> Result<PingResult, Error> {
    let timeout = timeout.unwrap_or(Duration::from_secs(4));
    let dest = SocketAddr::new(addr, 0);
    let (request_bytes, request_payload) = prepare_request(addr, ident, seq_cnt, payload)?;
    let socket = create_socket(socket_type, addr, ttl, bind_device)?;
    socket.set_nonblocking(true)?;
    let socket = AsyncFd::new(socket)?;
    let started_at = Instant::now();

    let operation = async {
        loop {
            let mut ready = socket.writable().await?;
            match ready.try_io(|inner| inner.get_ref().send_to(&request_bytes, &dest.into())) {
                Ok(result) => {
                    result?;
                    break;
                }
                Err(_would_block) => continue,
            }
        }

        loop {
            let mut ready = socket.readable().await?;
            let mut buffer = [0u8; 2048];
            let received = ready.try_io(|inner| {
                // socket2 0.6 receives into MaybeUninit because the kernel may
                // initialize only the returned prefix of the buffer.
                inner.get_ref().recv_from(unsafe {
                    std::slice::from_raw_parts_mut(
                        buffer.as_mut_ptr() as *mut std::mem::MaybeUninit<u8>,
                        buffer.len(),
                    )
                })
            });
            let (n, src_addr) = match received {
                Ok(result) => result?,
                Err(_would_block) => continue,
            };
            let source_ip = src_addr.as_socket().map(|s| s.ip()).unwrap_or(addr);

            let Some((reply, recv_ttl)) = decode_reply(addr, &buffer[..n]) else {
                continue;
            };
            if reply.payload == request_payload {
                return Ok(PingResult {
                    rtt: started_at.elapsed(),
                    ident: reply.ident,
                    seq_cnt: reply.seq_cnt,
                    payload: reply.payload.to_vec(),
                    source: source_ip,
                    target: addr,
                    ttl: recv_ttl,
                });
            }
        }
    };

    tokio::time::timeout(timeout, operation)
        .await
        .map_err(|_| {
            Error::from(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "ping request timed out",
            ))
        })?
}

impl Ping<'_> {
    /// Sends the echo request asynchronously using Tokio.
    ///
    /// This method is available when the crate's `tokio` feature is enabled.
    /// On Unix this uses a nonblocking socket registered with Tokio's reactor.
    /// On Windows it currently falls back to Tokio's blocking thread pool.
    /// All builder options and the return value are identical to
    /// [`send`](Ping::send).
    ///
    /// ```no_run
    /// # #[cfg(feature = "tokio")]
    /// # #[tokio::main]
    /// # async fn main() {
    /// let target = "8.8.8.8".parse().unwrap();
    /// let result = ping::new(target).send_async().await.expect("ping failed");
    /// println!("{:?}", result.rtt);
    /// # }
    /// ```
    pub async fn send_async(&self) -> Result<PingResult, Error> {
        #[cfg(unix)]
        return ping_with_socktype_async(
            self.socket_type.into(),
            self.addr,
            self.timeout,
            self.ttl,
            self.ident,
            self.seq_cnt,
            self.payload,
            #[cfg(any(target_os = "linux", target_os = "android"))]
            self.bind_device,
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            None,
        )
        .await;

        // Windows support currently keeps the portable blocking implementation
        // off Tokio's runtime worker threads.
        #[cfg(not(unix))]
        {
            let socket_type = self.socket_type;
            let addr = self.addr;
            let timeout = self.timeout;
            let ttl = self.ttl;
            let ident = self.ident;
            let seq_cnt = self.seq_cnt;
            let payload = self.payload.copied();

            tokio::task::spawn_blocking(move || {
                ping_with_socktype(
                    socket_type.into(),
                    addr,
                    timeout,
                    ttl,
                    ident,
                    seq_cnt,
                    payload.as_ref(),
                    None,
                )
            })
            .await
            .map_err(|_| Error::InternalError)?
        }
    }
}
