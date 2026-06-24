use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid procotol")]
    InvalidProtocol,
    #[error("internal error")]
    InternalError,
    #[error("Decode echo reply error occurred while processing the ICMP echo reply.")]
    DecodeEchoReplyError,
    #[error("io error: {error}")]
    IoError {
        #[from]
        #[source]
        error: ::std::io::Error,
    },
}
