extern crate alloc;
use crate::{any::AnySignature, Account, Signer, Vault};
use alloc::vec::Vec;
use pjs::{Account as PjsAccount, Error, PjsExtension};
use log;
use hex;
use sp_core::crypto::{AccountId32, Ss58Codec};

#[derive(Clone)]
pub struct Pjs {
    inner: PjsExtension,
}

impl Pjs {
    pub async fn connect(name: &str) -> Result<Self, Error> {
        Ok(Pjs {
            inner: PjsExtension::connect(name).await?,
        })
    }

    pub async fn list_accounts(&mut self) -> Result<Vec<PjsAccount>, Error> {
        self.inner.fetch_accounts().await?;
        Ok(self.inner.accounts())
    }

    pub fn select_account(&mut self, idx: u8) {
        self.inner.select_account(idx)
    }
}

impl Signer for Pjs {
    type Signature = AnySignature;

    async fn sign_msg(&self, msg: impl AsRef<[u8]>) -> Result<Self::Signature, ()> {
        log::info!("signing: {}", hex::encode(&msg.as_ref()));
        let sig = self.inner.sign(msg.as_ref()).await.map_err(|_| ())?;
        log::info!("signature {:?}", hex::encode(&sig));
        Ok(AnySignature::from(sig))
    }

    async fn verify(&self, _: impl AsRef<[u8]>, _: impl AsRef<[u8]>) -> bool {
        unimplemented!()
    }
}

impl Account for Pjs {
    fn public(&self) -> impl crate::Public {
        let mut key = [0u8; 32];

        let address = self
            .inner
            .get_selected()
            .expect("an account must be defined")
            .address();

        let address = <AccountId32 as Ss58Codec>::from_string(&address)
            .expect("it must be a valid ss58 address");
        key.copy_from_slice(address.as_ref());
        key
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
    type Credentials = ();
    type Account = Pjs;
    type Error = Error;

    async fn unlock(
        &mut self,
        account: Self::Id,
        _: impl Into<Self::Credentials>,
    ) -> Result<Self::Account, Self::Error> {
        let mut pjs_signer = self.clone();
        pjs_signer.select_account(account);
        Ok(pjs_signer)
    }
}
