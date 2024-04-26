use crate::{Signer, Vault};
use core::marker::PhantomData;
use pjs::{Account, Error, PjsExtension};


#[derive(Clone)]
struct Pjs {
    inner: PjsExtension,
}

impl Pjs {
    pub async fn connect(name: &str) -> Result<Self, Error> {
        Pjs {
            inner: PjsExtension::connect(name).await?,
        }
    }

    pub async fn list_accounts(&mut self) -> Result<Vec<Account>, Error> {
        self.inner.fetch_accounts().await?;
        self.inner.accounts()
    }

    pub fn select_account(&mut self, idx: u8) {
        self.inner.select_account(idx)
    }
}

impl Signer for Pjs {
    type Signature = AsRef<[u8]>;

    async fn sign_msg<M: AsRef<[u8]>>(&self, msg: M) -> Result<Self::Signature, ()> {
        self.inner.sign(msg).await.map_err(|_| Err(()))
    }

    async fn verify<M: AsRef<[u8]>>(&self, msg: M, sig: &[u8]) -> bool {
        unimplemented!()
    }
}

impl Account for Pjs {
    fn public(&self) -> impl crate::Public {
        self.inner
            .current_account()
            .expect("an account must be defined")
            .address()
    }
}

impl Vault for Pjs {
    type Id = u8;
    type Credentials = String;
    type Account = Account;
    type Error = Error;

    async fn unlock(
        &mut self,
        account: Self::Id,
        cred: impl Into<Self::Credentials>,
    ) -> Result<Self::Account, Self::Error> {
       let pjs_signer = self.clone();
       pjs_signer.select_account(account);
       return Ok(pjs_signer)
    }
}
