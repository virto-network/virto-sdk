//! Metadata-discovered pallet-pass configuration.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use sube::metadata::TypeId;
use sube::scales::{Fields, TypeDef};
use sube::{DynValue, Error, Metadata, Result, Value};

use crate::{Account, AuthorityId, Challenge, HashedUserId, blake2b_256, block_challenge};

pub type AccountDerivation = fn([u8; 8], HashedUserId) -> Account;
pub type Challenger = fn(&[u8; 32], &[u8]) -> Challenge;

/// Names that may be overridden for runtimes that do not use Kreivo's
/// standard pallet-pass naming.
#[derive(Clone, Debug)]
pub struct RuntimeOverrides {
    pub pallet: String,
    pub extension: String,
    pub register: String,
    pub add_device: String,
    pub remove_device: String,
    pub add_session_key: String,
    pub remove_session_key: String,
    pub pallet_id_constant: String,
    pub max_session_duration_constant: String,
    pub authority_id: Option<AuthorityId>,
    pub account_derivation: AccountDerivation,
    pub challenger: Challenger,
}

impl Default for RuntimeOverrides {
    fn default() -> Self {
        Self {
            pallet: "Pass".into(),
            extension: "PassAuthenticate".into(),
            register: "register".into(),
            add_device: "add_device".into(),
            remove_device: "remove_device".into(),
            add_session_key: "add_session_key".into(),
            remove_session_key: "remove_session_key".into(),
            pallet_id_constant: "PalletId".into(),
            max_session_duration_constant: "MaxSessionDuration".into(),
            authority_id: None,
            account_derivation: derive_account,
            challenger: block_challenge,
        }
    }
}

/// A metadata-resolved call and its named fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCall {
    pub name: String,
    pub index: u8,
    pub fields: Vec<String>,
}

impl RuntimeCall {
    pub fn has_field(&self, field: &str) -> bool {
        self.fields.iter().any(|candidate| candidate == field)
    }
}

/// Runtime capabilities needed by pass enrollment, devices, and sessions.
///
/// This contains names and indices only; workflow APIs continue to encode all
/// values through the runtime metadata registry.
#[derive(Clone, Debug)]
pub struct PassRuntimeConfig {
    pub pallet: String,
    pub pallet_index: u8,
    pub extension: String,
    pub pallet_id: [u8; 8],
    pub authority_id: AuthorityId,
    pub max_session_duration: u32,
    pub credential_variants: Vec<String>,
    pub attestation_variants: Vec<String>,
    pub register: RuntimeCall,
    pub add_device: RuntimeCall,
    pub remove_device: RuntimeCall,
    pub add_session_key: RuntimeCall,
    pub remove_session_key: Option<RuntimeCall>,
    pub account_derivation: AccountDerivation,
    pub challenger: Challenger,
}

impl PassRuntimeConfig {
    pub fn discover(metadata: &Metadata) -> Result<Self> {
        Self::discover_with(metadata, &RuntimeOverrides::default())
    }

    pub fn discover_with(metadata: &Metadata, names: &RuntimeOverrides) -> Result<Self> {
        let pallet = metadata
            .pallet_by_name(&names.pallet)
            .ok_or_else(|| Error::PalletNotFound(names.pallet.clone()))?;
        let calls_ty = pallet.calls_ty.ok_or(Error::CallNotFound)?;

        let extension = metadata
            .extrinsic
            .extensions
            .iter()
            .find(|extension| extension.identifier == names.extension)
            .ok_or_else(|| Error::MissingExtensionValue(names.extension.clone()))?;

        let pallet_id_constant = pallet
            .constants
            .iter()
            .find(|constant| constant.name == names.pallet_id_constant)
            .ok_or_else(|| {
                Error::ConstantNotFound(format!("{}_{}", pallet.name, names.pallet_id_constant))
            })?;
        let pallet_id: [u8; 8] = pallet_id_constant
            .value
            .as_slice()
            .try_into()
            .map_err(|_| Error::Mapping("pass PalletId must encode as exactly 8 bytes".into()))?;

        let max_duration = pallet
            .constants
            .iter()
            .find(|constant| constant.name == names.max_session_duration_constant)
            .ok_or_else(|| {
                Error::ConstantNotFound(format!(
                    "{}_{}",
                    pallet.name, names.max_session_duration_constant
                ))
            })?;
        let max_session_duration =
            Value::new(&max_duration.value, max_duration.ty, &metadata.registry)
                .as_u32()
                .ok_or_else(|| Error::Mapping("Pass::MaxSessionDuration is not a u32".into()))?;

        let calls = match metadata.registry.resolve(calls_ty) {
            Some(TypeDef::Variant(calls)) => calls,
            _ => return Err(Error::Mapping("pass call type is not an enum".into())),
        };
        let resolve_call = |wanted: &str| -> Result<RuntimeCall> {
            let variant = calls
                .variants()
                .find(|variant| variant.name().eq_ignore_ascii_case(wanted))
                .ok_or(Error::CallNotFound)?;
            Ok(RuntimeCall {
                name: variant.name().to_string(),
                index: variant.index(),
                fields: named_fields(variant.fields())?,
            })
        };

        let register = resolve_call(&names.register)?;
        let add_device = resolve_call(&names.add_device)?;
        let remove_device = resolve_call(&names.remove_device)?;
        let add_session_key = resolve_call(&names.add_session_key)?;
        let remove_session_key = resolve_call(&names.remove_session_key).ok();

        let attestation_ty =
            call_field_type(&metadata.registry, calls_ty, &register.name, "attestation")?;
        let mut attestation_variants = Vec::new();
        collect_named_enum_variants(
            &metadata.registry,
            attestation_ty,
            "Attestation",
            0,
            &mut attestation_variants,
        );

        let mut credential_variants = Vec::new();
        collect_named_enum_variants(
            &metadata.registry,
            extension.ty,
            "Credential",
            0,
            &mut credential_variants,
        );

        Ok(Self {
            pallet: pallet.name.clone(),
            pallet_index: pallet.index,
            extension: extension.identifier.clone(),
            pallet_id,
            authority_id: names
                .authority_id
                .unwrap_or_else(|| pallet_authority_id(pallet_id)),
            max_session_duration,
            credential_variants,
            attestation_variants,
            register,
            add_device,
            remove_device,
            add_session_key,
            remove_session_key,
            account_derivation: names.account_derivation,
            challenger: names.challenger,
        })
    }

    pub fn supports_credential(&self, variant: &str) -> bool {
        self.credential_variants
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(variant))
    }

    pub fn supports_attestation(&self, variant: &str) -> bool {
        self.attestation_variants
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(variant))
    }

    /// Default AccountId32 derivation used by `PalletId::into_sub_account_truncating`.
    pub fn derive_account(&self, user_id: HashedUserId) -> Account {
        (self.account_derivation)(self.pallet_id, user_id)
    }
}

fn named_fields(fields: Fields<'_>) -> Result<Vec<String>> {
    match fields {
        Fields::Struct(fields) => Ok(fields.iter().map(|field| field.name.to_string()).collect()),
        Fields::Unit => Ok(Vec::new()),
        _ => Err(Error::Mapping(
            "pass workflow requires named call fields".into(),
        )),
    }
}

fn call_field_type(
    registry: &sube::Registry,
    calls_ty: TypeId,
    call_name: &str,
    field_name: &str,
) -> Result<TypeId> {
    let calls = match registry.resolve(calls_ty) {
        Some(TypeDef::Variant(calls)) => calls,
        _ => return Err(Error::Mapping("call type is not an enum".into())),
    };
    let call = calls
        .variants()
        .find(|variant| variant.name().eq_ignore_ascii_case(call_name))
        .ok_or(Error::CallNotFound)?;
    match call.fields() {
        Fields::Struct(fields) => fields
            .iter()
            .find(|field| field.name == field_name)
            .map(|field| field.ty)
            .ok_or_else(|| Error::Mapping(format!("{call_name} has no {field_name} field"))),
        _ => Err(Error::Mapping(format!(
            "{call_name} does not have named fields"
        ))),
    }
}

fn collect_named_enum_variants(
    registry: &sube::Registry,
    ty: TypeId,
    wanted_type_name: &str,
    depth: usize,
    output: &mut Vec<String>,
) {
    if depth > 10 {
        return;
    }
    match registry.resolve(ty) {
        Some(TypeDef::StructNewType(inner) | TypeDef::Compact(inner)) => {
            collect_named_enum_variants(registry, inner, wanted_type_name, depth + 1, output)
        }
        Some(TypeDef::Struct(fields)) => {
            for field in fields {
                collect_named_enum_variants(
                    registry,
                    field.ty,
                    wanted_type_name,
                    depth + 1,
                    output,
                );
            }
        }
        Some(TypeDef::Variant(variants)) => {
            if variants.name().contains(wanted_type_name) {
                output.extend(
                    variants
                        .variants()
                        .map(|variant| variant.name().to_string()),
                );
                return;
            }
            for variant in variants.variants() {
                match variant.fields() {
                    Fields::NewType(inner) => collect_named_enum_variants(
                        registry,
                        inner,
                        wanted_type_name,
                        depth + 1,
                        output,
                    ),
                    Fields::Tuple(fields) => {
                        for inner in fields {
                            collect_named_enum_variants(
                                registry,
                                *inner,
                                wanted_type_name,
                                depth + 1,
                                output,
                            );
                        }
                    }
                    Fields::Struct(fields) => {
                        for field in fields {
                            collect_named_enum_variants(
                                registry,
                                field.ty,
                                wanted_type_name,
                                depth + 1,
                                output,
                            );
                        }
                    }
                    Fields::Unit => {}
                }
            }
        }
        _ => {}
    }
}

/// Convert a pallet id into the authority AccountId32 used by FRAME.
pub fn pallet_authority_id(pallet_id: [u8; 8]) -> AuthorityId {
    let mut authority = [0u8; 32];
    authority[..4].copy_from_slice(b"modl");
    authority[4..12].copy_from_slice(&pallet_id);
    AuthorityId(authority)
}

/// Default pass account derivation for AccountId32 runtimes.
pub fn derive_account(pallet_id: [u8; 8], user_id: HashedUserId) -> Account {
    let mut encoded = Vec::with_capacity(44);
    encoded.extend_from_slice(b"modl");
    encoded.extend_from_slice(&pallet_id);
    encoded.extend_from_slice(&user_id.0);
    Account(blake2b_256(&encoded))
}

/// Build a dynamic call body from metadata-resolved fields.
pub(crate) fn body(entries: Vec<(&str, DynValue)>) -> DynValue {
    DynValue::Map(
        entries
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const KREIVO_META: &[u8] = include_bytes!("../../sube/tests/fixtures/kreivo.scale");

    #[test]
    fn discovers_kreivo_pass_capabilities() {
        let metadata = Metadata::from_bytes(KREIVO_META).unwrap();
        let config = PassRuntimeConfig::discover(&metadata).unwrap();
        assert_eq!(config.pallet_index, 6);
        assert_eq!(config.max_session_duration, 150);
        assert_eq!(config.pallet_id, *b"kreivo_p");
        assert!(config.supports_credential("WebAuthn"));
        assert!(config.supports_attestation("WebAuthn"));
        assert!(config.add_session_key.has_field("duration"));
    }

    #[test]
    fn account_derivation_is_stable() {
        let account = derive_account(*b"kreivo_p", HashedUserId([0x11; 32]));
        assert_eq!(
            hex::encode(account.0),
            "57f6077d10a19211d718c5373d96e3efaa7c9cfa32c149b16ffe41a03ef9995c"
        );
    }

    #[test]
    fn nonstandard_derivation_and_challenger_can_be_overridden() {
        fn account(_: [u8; 8], _: HashedUserId) -> Account {
            Account([7; 32])
        }
        fn challenger(_: &[u8; 32], _: &[u8]) -> Challenge {
            [8; 32]
        }

        let metadata = Metadata::from_bytes(KREIVO_META).unwrap();
        let config = PassRuntimeConfig::discover_with(
            &metadata,
            &RuntimeOverrides {
                account_derivation: account,
                challenger,
                ..RuntimeOverrides::default()
            },
        )
        .unwrap();
        assert_eq!(
            config.derive_account(HashedUserId([1; 32])),
            Account([7; 32])
        );
        assert_eq!((config.challenger)(&[2; 32], &[3; 32]), [8; 32]);
    }
}
