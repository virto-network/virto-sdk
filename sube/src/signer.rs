use crate::Result;
use core::{future::Future, marker::PhantomData};

type Bytes<const N: usize> = [u8; N];

/// Signed extrinsics need to be signed by a `Signer` before submission  
pub trait Signer {
    type Account: AsRef<[u8]>;
    type Signature: AsRef<[u8]>;

    fn sign<'a>(&'a self, data: &'a [u8]) -> impl Future<Output = Result<Self::Signature>> + 'a;

    fn account(&self) -> Self::Account;
}

/// Wrapper to create a standard signer from an account and closure
pub struct SignerFn<'a, S, SF> {
    account: Bytes<32>,
    signer: S,
    _fut: PhantomData<&'a SF>,
}

impl<'a, S, SF> Signer for SignerFn<'a, S, SF>
where
    S: for<'b> Fn(&'b [u8]) -> SF + 'a,
    SF: Future<Output = Result<Bytes<64>>> + 'a,
{
    type Account = Bytes<32>;
    type Signature = Bytes<64>;

    fn sign<'b>(&'b self, data: &'b [u8]) -> impl Future<Output = Result<Self::Signature>> + 'b {
        (self.signer)(data)
    }

    fn account(&self) -> Self::Account {
        self.account
    }
}

impl<'a, A: AsRef<[u8]>, S, SF> From<(A, S)> for SignerFn<'a, S, SF> 
where
    for<'b> S:  Fn(&'b [u8]) -> SF + 'b,
    SF: Future<Output = Result<Bytes<64>>> + 'a,
{
    fn from((account, signer): (A, S)) -> Self {
        SignerFn {
            account: account.as_ref().try_into().expect("32bit account"),
            signer,
            _fut: PhantomData,
        }
    }
}
