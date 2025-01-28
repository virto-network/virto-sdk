use crate::{Public, Signer};

pub trait Account: Signer + core::fmt::Display {
    fn public(&self) -> impl Public;
}
