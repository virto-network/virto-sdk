//! Scoped session resolution, authorization, and persistence.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::{format, vec};

use sube::extrinsic::{ChainContext, assemble_signed_v4};
use sube::metadata::ExtrinsicMeta;
use sube::{
    AuthorizationSummary, DispatchOutcome, DynValue, Error, ExtrinsicAssembler, Metadata,
    PreparedCall, Registry, Result, Signer, TransactionReceipt,
};

use crate::Account;
use crate::config::{PassRuntimeConfig, body};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CallIndex {
    pub pallet: u8,
    pub call: u8,
}

/// An explicit request which is resolved to metadata indices before signing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionPolicyRequest {
    Calls(Vec<String>),
    Pallets(Vec<String>),
    Spend {
        pallet: String,
        calls: Vec<String>,
        limit: u128,
    },
    Admin,
}

/// A metadata-resolved, locally enforceable session filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionPolicy {
    Calls(Vec<CallIndex>),
    Pallets(Vec<u8>),
    Spend { calls: Vec<CallIndex>, limit: u128 },
}

impl SessionPolicy {
    pub fn resolve(metadata: &Metadata, request: SessionPolicyRequest) -> Result<Self> {
        match request {
            SessionPolicyRequest::Admin => Err(Error::OperationFailed(
                "Admin sessions are not permitted".into(),
            )),
            SessionPolicyRequest::Calls(paths) => {
                if paths.is_empty() {
                    return Err(Error::BadInput);
                }
                let mut calls = Vec::with_capacity(paths.len());
                for path in paths {
                    calls.push(resolve_call(metadata, &path)?);
                }
                calls.sort();
                calls.dedup();
                Ok(Self::Calls(calls))
            }
            SessionPolicyRequest::Pallets(names) => {
                if names.is_empty() {
                    return Err(Error::BadInput);
                }
                let mut pallets = Vec::with_capacity(names.len());
                for name in names {
                    pallets.push(
                        metadata
                            .pallet_by_name(&name)
                            .ok_or_else(|| Error::PalletNotFound(name.clone()))?
                            .index,
                    );
                }
                pallets.sort_unstable();
                pallets.dedup();
                Ok(Self::Pallets(pallets))
            }
            SessionPolicyRequest::Spend {
                pallet,
                calls,
                limit,
            } => {
                if calls.is_empty() || limit == 0 {
                    return Err(Error::BadInput);
                }
                let mut resolved = Vec::with_capacity(calls.len());
                for call in calls {
                    resolved.push(resolve_call(metadata, &format!("{pallet}/{call}"))?);
                }
                resolved.sort();
                resolved.dedup();
                Ok(Self::Spend {
                    calls: resolved,
                    limit,
                })
            }
        }
    }

    pub fn this_call(call: &PreparedCall, metadata: &Metadata) -> Result<Self> {
        Self::resolve(
            metadata,
            SessionPolicyRequest::Calls(vec![format!("{}/{}", call.pallet, call.call)]),
        )
    }

    pub fn this_pallet(call: &PreparedCall, metadata: &Metadata) -> Result<Self> {
        Self::resolve(
            metadata,
            SessionPolicyRequest::Pallets(vec![call.pallet.clone()]),
        )
    }

    pub fn allows_encoded_call(&self, encoded_call: &[u8]) -> bool {
        let Some((&pallet, rest)) = encoded_call.split_first() else {
            return false;
        };
        let Some(&call) = rest.first() else {
            return false;
        };
        match self {
            Self::Calls(calls) | Self::Spend { calls, .. } => {
                calls.contains(&CallIndex { pallet, call })
            }
            Self::Pallets(pallets) => pallets.contains(&pallet),
        }
    }

    pub fn runtime_value(&self) -> DynValue {
        match self {
            Self::Calls(calls) => DynValue::obj(&[(
                "Calls",
                DynValue::Seq(
                    calls
                        .iter()
                        .map(|call| {
                            DynValue::Seq(vec![
                                DynValue::from(call.pallet),
                                DynValue::from(call.call),
                            ])
                        })
                        .collect(),
                ),
            )]),
            Self::Pallets(pallets) => DynValue::obj(&[(
                "Pallets",
                DynValue::Seq(
                    pallets
                        .iter()
                        .map(|pallet| DynValue::from(*pallet))
                        .collect(),
                ),
            )]),
            Self::Spend { calls, limit } => DynValue::obj(&[(
                "Spend",
                DynValue::obj(&[
                    (
                        "calls",
                        DynValue::Seq(
                            calls
                                .iter()
                                .map(|call| {
                                    DynValue::Seq(vec![
                                        DynValue::from(call.pallet),
                                        DynValue::from(call.call),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                    ("limit", u128_value(*limit)),
                ]),
            )]),
        }
    }
}

fn u128_value(value: u128) -> DynValue {
    DynValue::from(value)
}

fn resolve_call(metadata: &Metadata, path: &str) -> Result<CallIndex> {
    let (pallet_name, call_name) = path.split_once('/').ok_or(Error::BadInput)?;
    let pallet = metadata
        .pallet_by_name(pallet_name)
        .ok_or_else(|| Error::PalletNotFound(pallet_name.into()))?;
    let calls_ty = pallet.calls_ty.ok_or(Error::CallNotFound)?;
    let calls = match metadata.registry.resolve(calls_ty) {
        Some(sube::scales::TypeDef::Variant(calls)) => calls,
        _ => return Err(Error::CallNotFound),
    };
    let call = calls
        .variants()
        .find(|variant| variant.name().eq_ignore_ascii_case(call_name))
        .ok_or(Error::CallNotFound)?;
    Ok(CallIndex {
        pallet: pallet.index,
        call: call.index(),
    })
}

/// V4 session authorization: the pass account supplies the nonce while the
/// session account is encoded and signs the extrinsic.
pub struct SessionAuthorizer<S> {
    pass_account: Account,
    signer: S,
    policy: SessionPolicy,
}

impl<S> SessionAuthorizer<S> {
    pub fn new(pass_account: Account, signer: S, policy: SessionPolicy) -> Self {
        Self {
            pass_account,
            signer,
            policy,
        }
    }

    pub fn policy(&self) -> &SessionPolicy {
        &self.policy
    }

    pub fn signer(&self) -> &S {
        &self.signer
    }
}

impl<S: Signer> ExtrinsicAssembler for SessionAuthorizer<S> {
    type Account = [u8; 32];

    fn nonce_account(&self) -> Self::Account {
        self.pass_account.0
    }

    fn authorization(&self) -> AuthorizationSummary {
        AuthorizationSummary {
            signing_account: self.signer.account().as_ref().to_vec(),
            nonce_account: self.pass_account.0.to_vec(),
            scheme: self.signer.signature_variant().map(ToString::to_string),
        }
    }

    async fn assemble(
        &self,
        encoded_call: &[u8],
        meta: &ExtrinsicMeta,
        registry: &Registry,
        ctx: &ChainContext,
        overrides: &[(String, DynValue)],
    ) -> Result<Vec<u8>> {
        if !self.policy.allows_encoded_call(encoded_call) {
            return Err(Error::OperationFailed(
                "call is outside the local session policy".into(),
            ));
        }
        assemble_signed_v4(&self.signer, encoded_call, meta, registry, ctx, overrides).await
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalSession {
    pub genesis_hash: [u8; 32],
    pub pass_account: Account,
    pub session_account: [u8; 32],
    pub policy: SessionPolicy,
    pub expires_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnChainSession {
    pub pass_account: Account,
    pub session_account: [u8; 32],
    pub policy: SessionPolicy,
    pub expires_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionPlan {
    Reuse(LocalSession),
    RegisterExisting(LocalSession),
    GenerateAndRegister,
}

/// Stateless coordinator for session lifecycle decisions.
pub struct SessionManager;

impl SessionManager {
    pub fn plan(
        local: Option<LocalSession>,
        on_chain: Option<&OnChainSession>,
        desired_pass: Account,
        desired_policy: &SessionPolicy,
        now: u64,
    ) -> SessionPlan {
        plan_session(local, on_chain, desired_pass, desired_policy, now)
    }

    pub fn duration(config: &PassRuntimeConfig, requested: Option<u32>) -> Result<u32> {
        session_duration(config, requested)
    }
}

/// Pure lifecycle decision used after reading both local secure state and
/// on-chain storage.
pub fn plan_session(
    local: Option<LocalSession>,
    on_chain: Option<&OnChainSession>,
    desired_pass: Account,
    desired_policy: &SessionPolicy,
    now: u64,
) -> SessionPlan {
    let Some(mut local) = local else {
        return SessionPlan::GenerateAndRegister;
    };
    if local.pass_account != desired_pass || local.session_account == [0; 32] {
        return SessionPlan::GenerateAndRegister;
    }

    let exact_on_chain = on_chain.is_some_and(|registered| {
        registered.pass_account == desired_pass
            && registered.session_account == local.session_account
            && registered.policy == *desired_policy
            && registered.expires_at > now
    });
    if exact_on_chain && local.policy == *desired_policy {
        SessionPlan::Reuse(local)
    } else {
        // Keep the same local key and update it in place.
        local.policy = desired_policy.clone();
        SessionPlan::RegisterExisting(local)
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
    policy: &SessionPolicy,
    requested_duration: Option<u32>,
) -> Result<PreparedCall> {
    let duration = session_duration(config, requested_duration)?;
    let mut fields = vec![
        (
            "session",
            DynValue::obj(&[("Id", DynValue::from(session_account))]),
        ),
        (
            "duration",
            DynValue::obj(&[("Some", DynValue::from(duration))]),
        ),
    ];
    if config.add_session_key.has_field("filter") {
        fields.push(("filter", policy.runtime_value()));
    } else if config.add_session_key.has_field("policy") {
        fields.push(("policy", policy.runtime_value()));
    }
    sube::extrinsic::prepare_call(
        metadata,
        &format!("{}/{}", config.pallet, config.add_session_key.name),
        &body(fields),
    )
}

pub trait SessionStore {
    type Error;

    fn upsert(&mut self, session: LocalSession) -> core::result::Result<(), Self::Error>;
    fn delete(
        &mut self,
        genesis_hash: [u8; 32],
        pass_account: Account,
    ) -> core::result::Result<(), Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum SessionCommitError<E> {
    NotFinalized,
    DispatchFailed,
    Store(E),
}

pub fn persist_finalized_session<S: SessionStore>(
    pending: LocalSession,
    receipt: &TransactionReceipt,
    store: &mut S,
) -> core::result::Result<(), SessionCommitError<S::Error>> {
    if receipt.finalized_block_hash.is_none() {
        return Err(SessionCommitError::NotFinalized);
    }
    if !matches!(receipt.dispatch_outcome, DispatchOutcome::Success) {
        return Err(SessionCommitError::DispatchFailed);
    }
    store.upsert(pending).map_err(SessionCommitError::Store)
}

pub const FORGET_SESSION_WARNING: &str = "The local session secret was deleted. pallet-pass currently leaves the on-chain session present until its scheduled expiry.";

pub fn forget_session<S: SessionStore>(
    store: &mut S,
    genesis_hash: [u8; 32],
    pass_account: Account,
) -> core::result::Result<&'static str, S::Error> {
    store.delete(genesis_hash, pass_account)?;
    Ok(FORGET_SESSION_WARNING)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[derive(Default)]
    struct MemoryStore {
        value: Option<LocalSession>,
        deletes: usize,
    }

    impl SessionStore for MemoryStore {
        type Error = ();

        fn upsert(&mut self, session: LocalSession) -> core::result::Result<(), Self::Error> {
            self.value = Some(session);
            Ok(())
        }

        fn delete(&mut self, _: [u8; 32], _: Account) -> core::result::Result<(), Self::Error> {
            self.value = None;
            self.deletes += 1;
            Ok(())
        }
    }

    fn call_policy() -> SessionPolicy {
        SessionPolicy::Calls(vec![CallIndex { pallet: 5, call: 2 }])
    }

    #[test]
    fn admin_sessions_are_rejected() {
        let metadata =
            Metadata::from_bytes(include_bytes!("../../sube/tests/fixtures/kreivo.scale")).unwrap();
        assert!(SessionPolicy::resolve(&metadata, SessionPolicyRequest::Admin).is_err());
    }

    #[test]
    fn exact_policy_reuses_and_changed_policy_updates_same_key() {
        let local = LocalSession {
            genesis_hash: [1; 32],
            pass_account: Account([2; 32]),
            session_account: [3; 32],
            policy: call_policy(),
            expires_at: 100,
        };
        let registered = OnChainSession {
            pass_account: local.pass_account,
            session_account: local.session_account,
            policy: local.policy.clone(),
            expires_at: local.expires_at,
        };
        assert!(matches!(
            plan_session(
                Some(local.clone()),
                Some(&registered),
                local.pass_account,
                &local.policy,
                50
            ),
            SessionPlan::Reuse(_)
        ));

        let changed = SessionPolicy::Pallets(vec![5]);
        match plan_session(
            Some(local.clone()),
            Some(&registered),
            local.pass_account,
            &changed,
            50,
        ) {
            SessionPlan::RegisterExisting(updated) => {
                assert_eq!(updated.session_account, local.session_account);
                assert_eq!(updated.policy, changed);
            }
            other => panic!("unexpected plan: {other:?}"),
        }

        let expired = LocalSession {
            expires_at: 40,
            ..local.clone()
        };
        let expired_on_chain = OnChainSession {
            expires_at: 40,
            ..registered.clone()
        };
        match plan_session(
            Some(expired),
            Some(&expired_on_chain),
            local.pass_account,
            &local.policy,
            50,
        ) {
            SessionPlan::RegisterExisting(updated) => {
                assert_eq!(updated.session_account, local.session_account);
            }
            other => panic!("expired registration should reuse its local key: {other:?}"),
        }
    }

    #[test]
    fn local_filter_rejects_disallowed_call() {
        assert!(call_policy().allows_encoded_call(&[5, 2, 9]));
        assert!(!call_policy().allows_encoded_call(&[5, 3, 9]));
    }

    #[test]
    fn session_signer_and_nonce_identities_are_distinct() {
        let authorizer = SessionAuthorizer::new(Account([2; 32]), MockSigner, call_policy());
        assert_eq!(authorizer.nonce_account(), [2; 32]);
        let summary = authorizer.authorization();
        assert_eq!(summary.nonce_account, vec![2; 32]);
        assert_eq!(summary.signing_account, vec![3; 32]);
    }

    #[test]
    fn session_is_persisted_only_after_finalized_success_and_delete_is_explicit() {
        let pending = LocalSession {
            genesis_hash: [1; 32],
            pass_account: Account([2; 32]),
            session_account: [3; 32],
            policy: call_policy(),
            expires_at: 100,
        };
        let mut store = MemoryStore::default();
        let best_only = TransactionReceipt {
            best_block_hash: Some("0x01".into()),
            dispatch_outcome: DispatchOutcome::Success,
            ..TransactionReceipt::default()
        };
        assert!(persist_finalized_session(pending.clone(), &best_only, &mut store).is_err());
        assert!(store.value.is_none());

        let finalized = TransactionReceipt {
            finalized_block_hash: Some("0x02".into()),
            dispatch_outcome: DispatchOutcome::Success,
            ..TransactionReceipt::default()
        };
        persist_finalized_session(pending.clone(), &finalized, &mut store).unwrap();
        assert_eq!(store.value, Some(pending.clone()));
        assert_eq!(
            forget_session(&mut store, pending.genesis_hash, pending.pass_account).unwrap(),
            FORGET_SESSION_WARNING
        );
        assert!(store.value.is_none());
        assert_eq!(store.deletes, 1);
    }
}
