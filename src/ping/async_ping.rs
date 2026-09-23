use super::{Ping, PingResult, finish};
use crate::errors::Error;
use crate::tokio::Pinger;

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
        let legacy = self.legacy()?;
        let target = legacy.request.target;
        let mut pinger = Pinger::from_config(legacy.builder.config);
        let result = pinger.ping(legacy.request, legacy.timeout).await;
        finish(result, pinger.ident(target.is_ipv6()), target)
    }
}
