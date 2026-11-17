//! Scoped session registration and authorization.

use alloc::string::String;
use alloc::{format, vec};

use sube::extrinsic::{ChainContext, assemble_signed_v4};
use sube::metadata::ExtrinsicMeta;
use sube::{DynValue, Error, ExtrinsicAssembler, Metadata, PreparedCall, Registry, Result, Signer};

use crate::config::{PassRuntimeConfig, body};
use crate::{Account, DeviceFilter};

/// V4 session authorization: the pass account supplies the nonce while the
/// session account is encoded and signs the extrinsic.
///
/// Calls outside the session's filter are refused before signing, and an
/// `Admin` filter, which would allow every call, is refused outright.
pub struct SessionAuthorizer<S> {
    pass_account: Account,
    signer: S,
    filter: DeviceFilter,
}

impl<S> SessionAuthorizer<S> {
    pub fn new(pass_account: Account, signer: S, filter: DeviceFilter) -> Result<Self> {
        if matches!(filter, DeviceFilter::Admin) {
            return Err(Error::OperationFailed(
                "Admin sessions are not permitted".into(),
            ));
        }
        Ok(Self {
            pass_account,
            signer,
            filter,
        })
    }
}

impl<S: Signer> ExtrinsicAssembler for SessionAuthorizer<S> {
    type Account = [u8; 32];

    fn nonce_account(&self) -> Self::Account {
        self.pass_account.0
    }

    async fn assemble(
        &self,
        encoded_call: &[u8],
        meta: &ExtrinsicMeta,
        registry: &Registry,
        ctx: &ChainContext,
        overrides: &[(String, DynValue)],
    ) -> Result<sube::extrinsic::AssembledExtrinsic> {
        if !self.filter.allows(encoded_call) {
            return Err(Error::OperationFailed(
                "call is outside the local session policy".into(),
            ));
        }
        assemble_signed_v4(&self.signer, encoded_call, meta, registry, ctx, overrides).await
    }
}

pub fn session_duration(config: &PassRuntimeConfig, requested: Option<u32>) -> Result<u32> {
    match requested {
        Some(0) => Err(Error::BadInput),
        Some(duration) if duration > config.max_session_duration => {
            Err(Error::OperationFailed(format!(
                "requested session duration {duration} exceeds runtime maximum {}",
                config.max_session_duration
            )))
        }
        Some(duration) => Ok(duration),
        None => Ok(config.max_session_duration),
    }
}

pub fn prepare_add_session_key(
    metadata: &Metadata,
    config: &PassRuntimeConfig,
    session_account: [u8; 32],
    filter: &DeviceFilter,
    requested_duration: Option<u32>,
) -> Result<PreparedCall> {
    if matches!(filter, DeviceFilter::Admin) {
        return Err(Error::OperationFailed(
            "Admin sessions are not permitted".into(),
        ));
    }
    // Without a filter argument pallet-pass grants an Admin session key, so
    // never fall back to a filterless call.
    let Some(filter_field) = ["filter", "policy"]
        .into_iter()
        .find(|field| config.add_session_key.has_field(field))
    else {
        return Err(Error::Mapping(format!(
            "{}::{} takes no session filter; refusing to create an unrestricted session",
            config.pallet, config.add_session_key.name
        )));
    };
    let duration = session_duration(config, requested_duration)?;
    let fields = vec![
        (
            "session",
            DynValue::obj(&[("Id", DynValue::from(session_account))]),
        ),
        // A bare value for an `Option` field is encoded as `Some(value)`.
        ("duration", DynValue::from(duration)),
        (filter_field, filter.runtime_value()),
    ];
    sube::extrinsic::prepare_call(
        metadata,
        &format!("{}/{}", config.pallet, config.add_session_key.name),
        &body(fields),
    )
}

pub const FORGET_SESSION_WARNING: &str = "The local session secret was deleted. pallet-pass currently leaves the on-chain session present until its scheduled expiry.";

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    struct MockSigner;

    impl sube::Signer for MockSigner {
        type Account = [u8; 32];
        type Signature = [u8; 64];

        async fn sign(&self, _: impl AsRef<[u8]>) -> Result<Self::Signature> {
            Ok([9; 64])
        }

        fn account(&self) -> Self::Account {
            [3; 32]
        }
    }

    fn call_filter() -> DeviceFilter {
        DeviceFilter::Calls(vec![crate::CallIndex { pallet: 5, call: 2 }])
    }

    fn context() -> ChainContext {
        ChainContext {
            spec_version: 1,
            tx_version: 1,
            genesis_hash: [1; 32],
            account_nonce: 0,
            checkpoint_number: 0,
            checkpoint_hash: [1; 32],
            mortality: sube::Mortality::Immortal,
            tip: 0,
        }
    }

    fn kreivo() -> (Metadata, PassRuntimeConfig) {
        let metadata =
            Metadata::from_bytes(include_bytes!("../../sube/tests/fixtures/kreivo.scale")).unwrap();
        let config = PassRuntimeConfig::discover(&metadata).unwrap();
        (metadata, config)
    }

    /// The fixture's runtime has a filter-taking session call.
    fn kreivo_with_filtered_sessions() -> (Metadata, PassRuntimeConfig) {
        let (metadata, mut config) = kreivo();
        config.add_session_key.fields.push("filter".into());
        (metadata, config)
    }

    #[test]
    fn admin_sessions_are_rejected() {
        let (metadata, config) = kreivo_with_filtered_sessions();
        let error =
            prepare_add_session_key(&metadata, &config, [1; 32], &DeviceFilter::Admin, None)
                .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Admin sessions are not permitted"),
            "{error}"
        );
    }

    #[test]
    fn admin_session_authorizers_are_refused() {
        // A hand-edited stored policy must not yield an allow-all signer.
        let error = SessionAuthorizer::new(Account([2; 32]), MockSigner, DeviceFilter::Admin)
            .err()
            .expect("an Admin session authorizer");
        assert!(
            error
                .to_string()
                .contains("Admin sessions are not permitted"),
            "{error}"
        );
    }

    #[test]
    fn session_authorizer_refuses_calls_outside_its_filter() {
        let (metadata, _) = kreivo();
        let authorizer =
            SessionAuthorizer::new(Account([2; 32]), MockSigner, call_filter()).unwrap();
        let error = futures_lite::future::block_on(authorizer.assemble(
            &[5, 3, 9],
            &metadata.extrinsic,
            &metadata.registry,
            &context(),
            &[],
        ))
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("outside the local session policy"),
            "{error}"
        );
    }

    #[test]
    fn session_signer_and_nonce_identities_are_distinct() {
        let (metadata, _) = kreivo();
        let authorizer =
            SessionAuthorizer::new(Account([2; 32]), MockSigner, call_filter()).unwrap();
        assert_eq!(authorizer.nonce_account(), [2; 32]);
        let assembled = futures_lite::future::block_on(authorizer.assemble(
            &[5, 2, 9],
            &metadata.extrinsic,
            &metadata.registry,
            &context(),
            &[],
        ))
        .unwrap();
        assert_eq!(assembled.signing_account, vec![3; 32]);
        assert_eq!(assembled.scheme, "Sr25519");
    }

    #[test]
    fn session_call_without_a_filter_argument_is_refused() {
        // This fixture predates `add_session_key_with_filter`: discovery falls
        // back to the filterless call, which would grant an Admin session.
        let (metadata, config) = kreivo();
        assert!(!config.add_session_key.has_field("filter"));
        let filter = DeviceFilter::Pallets(vec![10]);
        let error =
            prepare_add_session_key(&metadata, &config, [1; 32], &filter, None).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("refusing to create an unrestricted session"),
            "{error}"
        );
    }
}
