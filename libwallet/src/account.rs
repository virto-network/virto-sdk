use crate::{
    any::{self, AnySignature},
    Derive, Network, Pair, Public,
    Signer,
};


pub trait Account: Signer {
    fn public(&self) -> impl Public;
}
