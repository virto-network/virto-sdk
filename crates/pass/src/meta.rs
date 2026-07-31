//! Fields shared by every credential provider.

use codec::Encode;
use sube::DynValue;

use crate::{AuthorityId, HashedUserId};

/// Common credential metadata: who is authenticating (user + authority),
/// what context they are authenticating against (typically a block number),
/// and which variant of the runtime's composite credential enum to emit.
///
/// This is held inside every credential provider (`WalletCredential`,
/// `WebAuthnCredential`, ...) to avoid duplicated fields and constructors.
pub struct CredentialMeta<Cx> {
    pub user_id: HashedUserId,
    pub authority_id: AuthorityId,
    pub context: Cx,
    /// Hash of `context` fetched from the chain. The runtime challenger uses
    /// this hash directly; it is not the hash of the encoded block number.
    pub block_hash: [u8; 32],
    /// Variant name in the runtime's composite credential enum.
    pub variant: &'static str,
}

impl<Cx> CredentialMeta<Cx>
where
    Cx: Encode + Into<DynValue> + Clone,
{
    /// Construct with the given variant name.
    pub fn new(
        user_id: HashedUserId,
        authority_id: AuthorityId,
        context: Cx,
        block_hash: [u8; 32],
        variant: &'static str,
    ) -> Self {
        Self {
            user_id,
            authority_id,
            context,
            block_hash,
            variant,
        }
    }

    /// Override the composite-enum variant name.
    pub fn with_variant(mut self, variant: &'static str) -> Self {
        self.variant = variant;
        self
    }

    /// Build the `AssertionMeta`/`AssertionMeta`-shaped inner DynValue.
    /// Layout matches pallet-pass webauthn::AssertionMeta:
    /// `{ authority_id, user_id, context }`.
    pub fn to_assertion_meta(&self) -> DynValue {
        DynValue::obj(&[
            ("authority_id", DynValue::from(self.authority_id.0)),
            ("user_id", DynValue::from(self.user_id.0)),
            ("context", self.context.clone().into()),
        ])
    }

    /// Build the `SignedMessage<Cx>`-shaped inner DynValue used by the
    /// key-based authenticators (substrate-keys, ssh, nostr, ...).
    /// Layout: `{ context, challenge, authority_id }`.
    pub fn to_signed_message(&self, challenge: &[u8; 32]) -> DynValue {
        DynValue::obj(&[
            ("context", self.context.clone().into()),
            ("challenge", DynValue::from(*challenge)),
            ("authority_id", DynValue::from(self.authority_id.0)),
        ])
    }
}
