use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid procotol")]
    InvalidProtocol,
    #[error("internal error")]
    InternalError,
    #[error("decode v4 error")]
    DecodeV4Error,
    #[error("decode echo reply error")]
    DecodeEchoReplyError,
    #[error("io error: {error}")]
    IoError {
        #[from]
        #[source]
        error: ::std::io::Error,
    },
}
