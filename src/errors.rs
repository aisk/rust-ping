use std::fmt;

/// Errors that can occur while sending a ping or decoding its reply.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The target address used an unsupported protocol.
    #[doc(hidden)]
    #[deprecated(since = "0.10.0", note = "no longer produced")]
    InvalidProtocol,
    /// An internal error, such as failing to encode the request packet.
    InternalError,
    /// The ICMP echo reply could not be decoded.
    #[doc(hidden)]
    #[deprecated(since = "0.10.0", note = "malformed packets are silently dropped")]
    DecodeEchoReplyError,
    /// An underlying I/O error, such as missing privileges to open the socket.
    IoError { error: ::std::io::Error },
    /// No matching reply arrived before the timeout elapsed.
    Timeout,
    /// The request payload does not fit in a single ICMP packet.
    PayloadTooLarge {
        /// The largest payload accepted for the target's address family.
        max: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[allow(deprecated)]
            Error::InvalidProtocol => f.write_str("invalid procotol"),
            Error::InternalError => f.write_str("internal error"),
            #[allow(deprecated)]
            Error::DecodeEchoReplyError => f.write_str(
                "Decode echo reply error occurred while processing the ICMP echo reply.",
            ),
            Error::IoError { error } => write!(f, "io error: {error}"),
            Error::Timeout => f.write_str("ping request timed out"),
            Error::PayloadTooLarge { max } => {
                write!(f, "payload too large, the maximum is {max} bytes")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::IoError { error } => Some(error),
            _ => None,
        }
    }
}

impl From<::std::io::Error> for Error {
    fn from(error: ::std::io::Error) -> Self {
        Error::IoError { error }
    }
}

/// Converts an I/O error from a send or receive, reporting timeouts as
/// [`Error::Timeout`].
pub(crate) fn io_error(error: ::std::io::Error) -> Error {
    match error.kind() {
        ::std::io::ErrorKind::TimedOut | ::std::io::ErrorKind::WouldBlock => Error::Timeout,
        _ => Error::IoError { error },
    }
}
