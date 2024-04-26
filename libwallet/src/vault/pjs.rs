
use pjs::{Error, PjsExtension, Account as PjsAccount};
use crate::{any::AnySignature, Account, Signature, Signer, Vault};
use core::{fmt::Display, marker::PhantomData};

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

    pub async fn list_accounts(&mut self) -> Result<Vec<PjsAccount>, Error> {
        self.inner.fetch_accounts().await?;
        self.inner.accounts()
    }

    pub fn select_account(&mut self, idx: u8) {
        self.inner.select_account(idx)
    }
}

impl Signer for Pjs {
    type Signature = AnySignature;

    async fn sign_msg(&self, msg: impl AsRef<[u8]>) -> Result<Self::Signature, ()> {
        self.inner.sign(msg).await.map_err(|_| Err(()))
    }

    async fn verify(&self, _: impl AsRef<[u8]>, _: impl AsRef<[u8]>) -> bool {
        unimplemented!()
    }
}

impl Account for Pjs {
    fn public(&self) -> impl crate::Public {
        self.inner
            .get_selected()
            .expect("an account must be defined")
            .address()
    }
}

impl core::fmt::Display for Pjs {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        for byte in self.public().as_ref() {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

impl Vault for Pjs {
    type Id = u8;
    type Credentials = String;
    type Account = Pjs;
    type Error = Error;

    async fn unlock(
        &mut self,
        account: Self::Id,
        cred: impl Into<Self::Credentials>,
    ) -> Result<Self::Account, Self::Error> {
        let mut pjs_signer = self.clone();
        pjs_signer.select_account(account);
        Ok(pjs_signer)
    }
}
