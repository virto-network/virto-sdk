use core::fmt::{Debug, Display};

use crate::{
    Public, Signer,
};

pub trait Account: Signer + Display {
    fn public(&self) -> impl Public;
}
