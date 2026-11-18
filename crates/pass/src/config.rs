//! Metadata-discovered pallet-pass configuration.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use sube::metadata::TypeId;
use sube::scales::{Fields, TypeDef};
use sube::{DynValue, Error, Metadata, Result, Value};

use crate::{Account, AuthorityId, HashedUserId, PASS_AUTHENTICATE, blake2b_256};

const PALLET: &str = "Pass";
/// Plain `add_session_key` grants an Admin (unrestricted) session; scoped
/// sessions must use the filtered variant.
const ADD_SESSION_KEY: &str = "add_session_key_with_filter";
/// Plain `add_device` grants an Admin device; scoped devices must use the
/// filtered variant.
const ADD_DEVICE: &str = "add_device_with_filter";

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
    pub pallet_id: [u8; 8],
    pub authority_id: AuthorityId,
    pub max_session_duration: u32,
    pub credential_variants: Vec<String>,
    pub attestation_variants: Vec<String>,
    pub register: RuntimeCall,
    pub add_device: RuntimeCall,
    /// Name of the device removal call.
    pub remove_device: String,
    pub add_session_key: RuntimeCall,
}

impl PassRuntimeConfig {
    pub fn discover(metadata: &Metadata) -> Result<Self> {
        let pallet = metadata
            .pallet_by_name(PALLET)
            .ok_or_else(|| Error::PalletNotFound(PALLET.into()))?;
        let calls_ty = pallet.calls_ty.ok_or(Error::CallNotFound)?;

        let extension = metadata
            .extrinsic
            .extensions
            .iter()
            .find(|extension| extension.identifier == PASS_AUTHENTICATE)
            .ok_or_else(|| Error::MissingExtensionValue(PASS_AUTHENTICATE.into()))?;

        let pallet_id_constant = pallet
            .constants
            .iter()
            .find(|constant| constant.name == "PalletId")
            .ok_or_else(|| Error::ConstantNotFound(format!("{}_PalletId", pallet.name)))?;
        let pallet_id: [u8; 8] = pallet_id_constant
            .value
            .as_slice()
            .try_into()
            .map_err(|_| Error::Mapping("pass PalletId must encode as exactly 8 bytes".into()))?;

        let max_duration = pallet
            .constants
            .iter()
            .find(|constant| constant.name == "MaxSessionDuration")
            .ok_or_else(|| {
                Error::ConstantNotFound(format!("{}_MaxSessionDuration", pallet.name))
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

        let register = resolve_call("register")?;
        // Older runtimes only have the filterless call, which
        // `prepare_add_device` accepts for Admin devices only.
        let add_device = resolve_call(ADD_DEVICE).or_else(|_| resolve_call("add_device"))?;
        let remove_device = resolve_call("remove_device")?.name;
        // Older runtimes only have the filterless call; it is still discovered
        // so the rest of the pass workflow works, and `prepare_add_session_key`
        // refuses it because it would grant an Admin session.
        let add_session_key =
            resolve_call(ADD_SESSION_KEY).or_else(|_| resolve_call("add_session_key"))?;

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
            pallet_id,
            authority_id: pallet_authority_id(pallet_id),
            max_session_duration,
            credential_variants,
            attestation_variants,
            register,
            add_device,
            remove_device,
            add_session_key,
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

    /// The pass account of `user_id`, assuming the runtime uses pallet-pass's
    /// default `AddressGenerator` (`()`). The generator is a runtime
    /// configuration type and is not described by the metadata.
    pub fn derive_account(&self, user_id: HashedUserId) -> Account {
        derive_account(user_id)
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

/// The authority id of a pallet, as frame-contrib's `AuthorityFromPalletId`
/// derives it: the pallet id, zero-padded to 32 bytes.
pub fn pallet_authority_id(pallet_id: [u8; 8]) -> AuthorityId {
    let mut authority = [0u8; 32];
    authority[..8].copy_from_slice(&pallet_id);
    AuthorityId(authority)
}

/// The pass account of `user_id` under pallet-pass's default
/// `AddressGenerator`: `blake2_256([0; 32] ++ user_id)`.
pub fn derive_account(user_id: HashedUserId) -> Account {
    let mut input = [0u8; 64];
    input[32..].copy_from_slice(&user_id.0);
    Account(blake2b_256(&input))
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
        assert_eq!(config.add_device.name, "add_device_with_filter");
        assert!(config.add_device.has_field("filter"));
        assert_eq!(config.add_session_key.name, "add_session_key_with_filter");
        assert!(config.add_session_key.has_field("duration"));
        assert!(config.add_session_key.has_field("filter"));
    }

    #[test]
    fn accounts_are_derived_like_the_default_address_generator() {
        // blake2_256([0; 32] ++ [0x11; 32]), computed independently.
        let account = derive_account(HashedUserId([0x11; 32]));
        assert_eq!(
            hex::encode(account.0),
            "04422c9487f442300bf68985ccf6aa95843dffa3c16f4b0003ee06a54bcb9c12"
        );
    }

    #[test]
    fn the_authority_is_the_zero_padded_pallet_id() {
        let metadata = Metadata::from_bytes(KREIVO_META).unwrap();
        let config = PassRuntimeConfig::discover(&metadata).unwrap();
        let mut expected = [0u8; 32];
        expected[..8].copy_from_slice(b"kreivo_p");
        assert_eq!(config.authority_id, AuthorityId(expected));
    }
}
