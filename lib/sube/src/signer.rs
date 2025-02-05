use crate::Result;
use core::{future::Future, marker::PhantomData};

pub type Bytes<const N: usize> = [u8; N];

/// Signed extrinsics need to be signed by a `Signer` before submission  
pub trait Signer {
    type Account: AsRef<[u8]>;
    type Signature: AsRef<[u8]>;

    fn sign(&self, data: impl AsRef<[u8]>) -> impl Future<Output = Result<Self::Signature>>;

    fn account(&self) -> Self::Account;
}

/// Wrapper to create a standard signer from an account and closure
pub struct SignerFn<S, SF> {
    account: Bytes<32>,
    signer: S,
    _fut: PhantomData<SF>,
}

impl<S, SF> Signer for SignerFn<S, SF>
where
    S: Fn(&[u8]) -> SF,
    SF: Future<Output = Result<Bytes<64>>>,
{
    type Account = Bytes<32>;
    type Signature = Bytes<64>;

    fn sign(&self, data: impl AsRef<[u8]>) -> impl Future<Output = Result<Self::Signature>> {
        (self.signer)(data.as_ref())
    }

    fn account(&self) -> Self::Account {
        self.account
    }
}

impl<A: AsRef<[u8]>, S, SF> From<(A, S)> for SignerFn<S, SF>
where
    A: AsRef<[u8]>,
    S: Fn(&[u8]) -> SF,
{
    fn from((account, signer): (A, S)) -> Self {
        SignerFn {
            account: account.as_ref().try_into().expect("32bit account"),
            signer,
            _fut: PhantomData,
        }
    }
}
