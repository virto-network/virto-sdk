//! Strongly-typed identifiers.
//!
//! Each is a transparent wrapper over `[u8; 32]`, so the SCALE encoding is
//! identical to the underlying array. The purpose is to prevent argument-swap
//! bugs at call sites — a `DeviceId` can't be accidentally used where an
//! `Account` is expected.

/// A pass-derived account address (used for on-chain nonce lookup).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Account(pub [u8; 32]);

/// A registered device identifier — 32 bytes.
/// Matches `fc_traits_authn::DeviceId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct DeviceId(pub [u8; 32]);

/// A hashed user identifier — 32 bytes.
/// Typically `sha256(user_identifier)` where the identifier is an email or
/// account name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct HashedUserId(pub [u8; 32]);

/// An authority identifier — 32 bytes.
/// Matches the runtime's `fc_traits_authn::AuthorityId`, typically derived
/// from a `PalletId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct AuthorityId(pub [u8; 32]);

macro_rules! as_ref_32 {
    ($($t:ident),*) => {
        $(
            impl AsRef<[u8; 32]> for $t {
                fn as_ref(&self) -> &[u8; 32] { &self.0 }
            }
            impl AsRef<[u8]> for $t {
                fn as_ref(&self) -> &[u8] { &self.0 }
            }
            impl From<[u8; 32]> for $t {
                fn from(bytes: [u8; 32]) -> Self { Self(bytes) }
            }
        )*
    };
}

as_ref_32!(Account, DeviceId, HashedUserId, AuthorityId);
