use thiserror::Error;

/// Errors that can occur while sending a ping or decoding its reply.
#[derive(Debug, Error)]
pub enum Error {
    /// The target address used an unsupported protocol.
    #[error("invalid procotol")]
    InvalidProtocol,
    /// An internal error, such as failing to encode the request packet.
    #[error("internal error")]
    InternalError,
    /// The ICMP echo reply could not be decoded.
    #[error("Decode echo reply error occurred while processing the ICMP echo reply.")]
    DecodeEchoReplyError,
    /// An underlying I/O error. A timeout is reported here with kind
    /// [`ErrorKind::TimedOut`](std::io::ErrorKind::TimedOut).
    #[error("io error: {error}")]
    IoError {
        #[from]
        #[source]
        error: ::std::io::Error,
    },
}
