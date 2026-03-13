use crate::prelude::*;
use core::fmt;

/// Errors produced during SCALE encoding, decoding, or type resolution.
#[derive(Debug)]
pub enum Error {
    /// End of input reached unexpectedly
    Eof,
    /// Type ID not found in registry
    TypeNotFound(u32),
    /// Variant index not found
    InvalidVariant(u8),
    /// Invalid UTF-8 in string data
    InvalidUtf8,
    /// Invalid unicode codepoint
    InvalidChar(u32),
    /// Bad input data
    BadInput(String),
    /// Unsupported type for operation
    BadType(String),
    /// Serialization/deserialization error
    Ser(String),
    /// Serializing from type to target not supported
    NotSupported(&'static str, String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Eof => write!(f, "unexpected end of input"),
            Error::TypeNotFound(id) => write!(f, "type {id} not found in registry"),
            Error::InvalidVariant(idx) => write!(f, "invalid variant index {idx}"),
            Error::InvalidUtf8 => write!(f, "invalid UTF-8"),
            Error::InvalidChar(code) => write!(f, "invalid char codepoint {code:#x}"),
            Error::BadInput(msg) => write!(f, "bad input: {msg}"),
            Error::BadType(msg) => write!(f, "unexpected type: {msg}"),
            Error::Ser(msg) => write!(f, "{msg}"),
            Error::NotSupported(from, to) => {
                write!(f, "serializing {from} as {to} is not supported")
            }
        }
    }
}

impl core::error::Error for Error {}

impl From<fmt::Error> for Error {
    fn from(_: fmt::Error) -> Self {
        Error::Ser("format error".into())
    }
}

impl serde::ser::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error::Ser(msg.to_string())
    }
}
