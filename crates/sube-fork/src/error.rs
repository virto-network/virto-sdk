use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// A snapshot couldn't be downloaded, read or verified.
    Snapshot(String),
    /// The runtime failed to compile, or a runtime call trapped.
    Runtime(String),
    /// The chain doesn't match what the relay chain mock expects (see `para`).
    Layout(String),
    /// The runtime refused a transaction.
    Invalid(String),
    /// The block is unknown, or its state was pruned.
    UnknownBlock,
    Io(std::io::Error),
}

pub type Result<T> = core::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Snapshot(e) => write!(f, "snapshot: {e}"),
            Error::Runtime(e) => write!(f, "runtime: {e}"),
            Error::Layout(e) => write!(f, "unexpected parachain layout: {e}"),
            Error::Invalid(e) => write!(f, "invalid transaction: {e}"),
            Error::UnknownBlock => f.write_str("unknown block or pruned state"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
