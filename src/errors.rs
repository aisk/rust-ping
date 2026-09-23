use thiserror::Error;

/// Errors that can occur while sending a ping or decoding its reply.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// The target address used an unsupported protocol.
    #[doc(hidden)]
    #[deprecated(since = "0.10.0", note = "no longer produced")]
    #[error("invalid procotol")]
    InvalidProtocol,
    /// An internal error, such as failing to encode the request packet.
    #[error("internal error")]
    InternalError,
    /// The ICMP echo reply could not be decoded.
    #[doc(hidden)]
    #[deprecated(since = "0.10.0", note = "malformed packets are silently dropped")]
    #[error("Decode echo reply error occurred while processing the ICMP echo reply.")]
    DecodeEchoReplyError,
    /// An underlying I/O error, such as missing privileges to open the socket.
    #[error("io error: {error}")]
    IoError {
        #[from]
        #[source]
        error: ::std::io::Error,
    },
    /// No matching reply arrived before the timeout elapsed.
    #[error("ping request timed out")]
    Timeout,
    /// The request payload does not fit in a single ICMP packet.
    #[error("payload too large, the maximum is {max} bytes")]
    PayloadTooLarge {
        /// The largest payload accepted for the target's address family.
        max: usize,
    },
}

/// Converts an I/O error from a send or receive, reporting timeouts as
/// [`Error::Timeout`].
pub(crate) fn io_error(error: ::std::io::Error) -> Error {
    match error.kind() {
        ::std::io::ErrorKind::TimedOut | ::std::io::ErrorKind::WouldBlock => Error::Timeout,
        _ => Error::IoError { error },
    }
}
