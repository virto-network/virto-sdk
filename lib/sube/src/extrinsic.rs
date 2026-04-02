use alloc::rc::Rc;

use crate::hasher::hash;
use crate::metadata::{self as meta, Hasher, SignedExtensionMeta, TypeId};
use crate::prelude::*;
use crate::{Backend, Error, Metadata, Response, Result};

use crate::value::DynValue;
use codec::{Compact, Encode};
use scales::Value;
use serde::Serialize;

/// Encode a call body into SCALE bytes given the call variant name and type.
///
/// Implemented for:
/// - Any `serde::Serialize` type (JSON values, structs, etc.) — wraps as `{"variant": body}`
/// - [`Text`] wrapper — parses from scales text format
pub trait EncodeCall {
    fn encode_call(
        &self,
        variant: &str,
        registry: &scales::Registry,
        calls_ty: TypeId,
    ) -> Result<Vec<u8>>;
}

/// Blanket impl for serde types — wraps body as `{"variant": body}` for scales serialization.
impl<T: Serialize> EncodeCall for T {
    fn encode_call(
        &self,
        variant: &str,
        registry: &scales::Registry,
        calls_ty: TypeId,
    ) -> Result<Vec<u8>> {
        // Wraps body as {"VariantName": body} for scales SCALE encoding.
        use alloc::collections::BTreeMap;
        let mut map = BTreeMap::new();
        map.insert(variant, self);
        scales::to_vec_with_info(&map, Some((registry, calls_ty)))
            .map_err(|e| Error::Encode(e.to_string()))
    }
}

/// A call body in scales text format.
///
/// The text represents just the fields of the call variant, e.g.:
/// ```text
/// (dest:MultiAddress::Id(0xd435...);value:1000000000000)
/// ```
///
/// The variant name is prepended automatically from the URL path.
#[derive(Debug)]
pub struct Text<'a>(pub &'a str);

// Override the blanket impl — Text uses from_text instead of serde.
// Since the blanket impl covers `&T where T: Serialize` and `str` is Serialize,
// we can't directly override. Instead Text is a newtype that is NOT Serialize.
impl EncodeCall for Text<'_> {
    fn encode_call(
        &self,
        variant: &str,
        registry: &scales::Registry,
        calls_ty: TypeId,
    ) -> Result<Vec<u8>> {
        // Look up the variant name in the call enum to build the full text
        let vdef = match registry.resolve(calls_ty) {
            Some(scales::TypeDef::Variant(vdef)) => vdef,
            _ => return Err(Error::Encode("calls type is not a variant".into())),
        };
        let full_text = alloc::format!("{}::{}{}", vdef.name(), variant, self.0);
        scales::from_text(&full_text, registry, calls_ty).map_err(|e| Error::Encode(e.to_string()))
    }
}

/// The body of an extrinsic to be submitted.
pub struct ExtrinsicBody<Body> {
    pub nonce: Option<u64>,
    pub body: Body,
    pub extensions: Vec<(String, DynValue)>,
}

/// Chain context fetched once for extension defaults.
pub struct ChainContext {
    pub spec_version: u32,
    pub tx_version: u32,
    pub genesis_hash: [u8; 32],
    pub account_nonce: u64,
}

/// Look up a caller-provided extension value by identifier.
fn find_override(extensions: &[(String, DynValue)], id: &str) -> Option<DynValue> {
    extensions
        .iter()
        .find(|(k, _)| k == id)
        .map(|(_, v)| v.clone())
}

/// Default JSON value for a well-known extension's "extra" data.
fn default_extra(identifier: &str, ctx: &ChainContext) -> Option<DynValue> {
    match identifier {
        "CheckMortality" => Some(DynValue::obj(&[("Immortal", DynValue::Null)])),
        "CheckNonce" => Some(DynValue::from(ctx.account_nonce)),
        "ChargeTransactionPayment" => Some(DynValue::from(0u32)),
        "ChargeAssetTxPayment" => Some(DynValue::obj(&[
            ("tip", DynValue::from(0u32)),
            ("asset_id", DynValue::Null),
        ])),
        _ => None,
    }
}

/// Default JSON value for a well-known extension's "additional_signed" data.
fn default_additional(identifier: &str, ctx: &ChainContext) -> Option<DynValue> {
    match identifier {
        "CheckSpecVersion" => Some(DynValue::from(ctx.spec_version)),
        "CheckTxVersion" => Some(DynValue::from(ctx.tx_version)),
        "CheckGenesis" | "CheckMortality" => Some(DynValue::from(format!(
            "0x{}",
            hex::encode(ctx.genesis_hash)
        ))),
        _ => None,
    }
}

/// Encode extra and additional_signed bytes from extension metadata.
///
/// Pure and sync — no I/O needed, so it's testable in isolation.
pub fn encode_extensions(
    extensions: &[SignedExtensionMeta],
    registry: &scales::Registry,
    ctx: &ChainContext,
    overrides: &[(String, DynValue)],
) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut extra = Vec::new();
    let mut additional = Vec::new();

    for ext in extensions {
        // "extra" bytes — included in extrinsic body
        if meta::is_zero_size_type(ext.ty, registry) {
            // zero bytes, nothing to encode
        } else {
            let value = find_override(overrides, &ext.identifier)
                .or_else(|| default_extra(&ext.identifier, ctx))
                .ok_or_else(|| Error::MissingExtensionValue(ext.identifier.clone()))?;
            scales::to_bytes_with_info(&mut extra, &value, Some((registry, ext.ty)))
                .map_err(|e| Error::Encode(e.to_string()))?;
        }

        // "additional_signed" bytes — signing payload only
        if meta::is_zero_size_type(ext.additional_signed, registry) {
            // zero bytes
        } else {
            let value = default_additional(&ext.identifier, ctx).ok_or_else(|| {
                Error::MissingExtensionValue(format!("{} (additional_signed)", ext.identifier))
            })?;
            scales::to_bytes_with_info(
                &mut additional,
                &value,
                Some((registry, ext.additional_signed)),
            )
            .map_err(|e| Error::Encode(e.to_string()))?;
        }
    }

    Ok((extra, additional))
}

/// Build and submit an extrinsic using metadata-driven extensions.
///
/// Delegates extrinsic assembly to the [`ExtrinsicAssembler`](crate::ExtrinsicAssembler):
/// - [`Signer`](crate::Signer) impls get V4 signed assembly via blanket impl
/// - Custom assemblers (e.g. pallet-pass `PassAuthenticator`) produce V5 General extrinsics
pub async fn submit<V>(
    chain: &mut (impl Backend + ?Sized),
    meta: &Rc<Metadata>,
    path: &str,
    tx_data: &ExtrinsicBody<V>,
    assembler: &(impl crate::ExtrinsicAssembler + ?Sized),
    wait_for_finalization: bool,
) -> Result<Response>
where
    V: EncodeCall + core::fmt::Debug,
{
    let (pallet, item_or_call, _keys) = crate::parse_uri(path).ok_or(Error::BadInput)?;
    let pallet = meta
        .pallet_by_name(&pallet)
        .ok_or(Error::PalletNotFound(pallet))?;
    let calls_ty = pallet.calls_ty.ok_or(Error::CallNotFound)?;

    // Encode call data
    let mut encoded_call = vec![pallet.index];
    let call_data =
        tx_data
            .body
            .encode_call(&item_or_call.to_lowercase(), &meta.registry, calls_ty)?;
    encoded_call.extend(&call_data);

    let from_account = assembler.account();

    // Build chain context
    let ctx = build_context(
        chain,
        meta,
        tx_data.nonce,
        &tx_data.extensions,
        from_account.as_ref(),
    )
    .await?;

    // Delegate assembly to the assembler
    let encoded_inner = assembler
        .assemble(
            &encoded_call,
            &meta.extrinsic,
            &meta.registry,
            &ctx,
            &tx_data.extensions,
        )
        .await?;

    let len = Compact(
        u32::try_from(encoded_inner.len())
            .map_err(|_| Error::Encode("extrinsic too large".into()))?,
    )
    .encode();

    chain
        .submit(&[len, encoded_inner].concat(), wait_for_finalization)
        .await?;

    Ok(Response::Void)
}

/// Assemble a V4 signed extrinsic from a [`Signer`](crate::Signer).
///
/// Called by the blanket `ExtrinsicAssembler` impl for `Signer` types.
/// Also available for custom assemblers that need to fall back to V4.
pub async fn assemble_signed_v4(
    signer: &(impl crate::Signer + ?Sized),
    encoded_call: &[u8],
    meta: &meta::ExtrinsicMeta,
    registry: &scales::Registry,
    ctx: &ChainContext,
    overrides: &[(String, DynValue)],
) -> Result<Vec<u8>> {
    // Encode extensions
    let (extra_bytes, additional_signed) =
        encode_extensions(&meta.extensions, registry, ctx, overrides)?;

    // Sign
    let signature_payload = [encoded_call, &extra_bytes, &additional_signed].concat();

    let payload = if signature_payload.len() > 256 {
        hash(&Hasher::Blake2_256, &signature_payload)
    } else {
        signature_payload
    };
    let signature = signer.sign(payload).await?;

    // Assemble extrinsic
    let version = meta.version;
    let from_account = signer.account();

    // MultiAddress::Id → variant 0 + 32-byte account
    let address_bytes = [vec![0x00], from_account.as_ref().to_vec()].concat();

    // Find Sr25519 variant index from signature type
    let sig_prefix = meta
        .signature_ty
        .and_then(|ty| match registry.resolve(ty) {
            Some(scales::TypeDef::Variant(vdef)) => vdef
                .variants()
                .find(|v| v.name().contains("Sr25519"))
                .map(|v| v.index()),
            _ => None,
        })
        .unwrap_or(0x01);

    Ok([
        vec![0b10000000 | version],
        address_bytes,
        [vec![sig_prefix], signature.as_ref().to_vec()].concat(),
        extra_bytes,
        encoded_call.to_vec(),
    ]
    .concat())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> ChainContext {
        ChainContext {
            spec_version: 100,
            tx_version: 2,
            genesis_hash: [0xab; 32],
            account_nonce: 42,
        }
    }

    #[test]
    fn default_extra_check_mortality() {
        let ctx = test_ctx();
        assert_eq!(
            default_extra("CheckMortality", &ctx),
            Some(DynValue::obj(&[("Immortal", DynValue::Null)]))
        );
    }

    #[test]
    fn default_extra_check_nonce() {
        let ctx = test_ctx();
        assert_eq!(
            default_extra("CheckNonce", &ctx),
            Some(DynValue::from(42u64))
        );
    }

    #[test]
    fn default_extra_charge_transaction_payment() {
        let ctx = test_ctx();
        assert_eq!(
            default_extra("ChargeTransactionPayment", &ctx),
            Some(DynValue::from(0u32))
        );
    }

    #[test]
    fn default_extra_unknown() {
        let ctx = test_ctx();
        assert_eq!(default_extra("UnknownExtension", &ctx), None);
    }

    #[test]
    fn default_additional_check_spec_version() {
        let ctx = test_ctx();
        assert_eq!(
            default_additional("CheckSpecVersion", &ctx),
            Some(DynValue::from(100u32))
        );
    }

    #[test]
    fn default_additional_check_tx_version() {
        let ctx = test_ctx();
        assert_eq!(
            default_additional("CheckTxVersion", &ctx),
            Some(DynValue::from(2u32))
        );
    }

    #[test]
    fn default_additional_check_genesis() {
        let ctx = test_ctx();
        let expected = format!("0x{}", hex::encode([0xab; 32]));
        assert_eq!(
            default_additional("CheckGenesis", &ctx),
            Some(DynValue::from(expected))
        );
    }

    #[test]
    fn default_additional_unknown() {
        let ctx = test_ctx();
        assert_eq!(default_additional("UnknownExtension", &ctx), None);
    }

    #[test]
    fn find_override_present() {
        let overrides = vec![("CheckNonce".to_string(), DynValue::from(99u32))];
        assert_eq!(
            find_override(&overrides, "CheckNonce"),
            Some(DynValue::from(99u32))
        );
    }

    #[test]
    fn find_override_absent() {
        let overrides: Vec<(String, DynValue)> = vec![];
        assert_eq!(find_override(&overrides, "CheckNonce"), None);
    }
}

/// Fetch spec/tx version, genesis hash, and account nonce.
pub async fn build_context(
    chain: &mut (impl Backend + ?Sized),
    meta: &Rc<Metadata>,
    nonce: Option<u64>,
    extensions: &[(String, DynValue)],
    account: &[u8],
) -> Result<ChainContext> {
    // System::Version constant
    let system = meta
        .pallet_by_name("System")
        .ok_or(Error::PalletNotFound("System".into()))?;
    let version_const = system
        .constants
        .iter()
        .find(|c| c.name == "Version")
        .ok_or(Error::ConstantNotFound("System_Version".into()))?;
    let version = Value::new(&version_const.value, version_const.ty, &meta.registry);
    // Try direct field access first, fall back to DynValue conversion
    let (spec_version, tx_version) = if let (Some(sv), Some(tv)) = (
        version.field("spec_version").and_then(|v| v.as_u32()),
        version
            .field("transaction_version")
            .and_then(|v| v.as_u32()),
    ) {
        (sv, tv)
    } else {
        let dyn_val: DynValue = version
            .try_into()
            .map_err(|_| Error::Mapping("failed to decode System::Version".into()))?;
        let sv = dyn_val
            .get("spec_version")
            .and_then(|v| v.as_u64())
            .ok_or(Error::Mapping("spec_version not found".into()))? as u32;
        let tv = dyn_val
            .get("transaction_version")
            .and_then(|v| v.as_u64())
            .ok_or(Error::Mapping("transaction_version not found".into()))? as u32;
        (sv, tv)
    };

    // Genesis hash
    let genesis_block: Vec<u8> = chain.block_info(Some(0u32)).await?.into();
    let genesis_hash: [u8; 32] = genesis_block
        .try_into()
        .map_err(|_| Error::Decode("genesis block hash is not 32 bytes".into()))?;

    // Nonce
    let account_nonce = resolve_nonce(chain, meta, nonce, extensions, account).await?;

    Ok(ChainContext {
        spec_version,
        tx_version,
        genesis_hash,
        account_nonce,
    })
}

/// Resolve nonce from: explicit field, extension override, or on-chain query.
async fn resolve_nonce(
    chain: &mut (impl Backend + ?Sized),
    meta: &Rc<Metadata>,
    nonce: Option<u64>,
    extensions: &[(String, DynValue)],
    account: &[u8],
) -> Result<u64> {
    if let Some(nonce) = nonce {
        return Ok(nonce);
    }
    if let Some(val) = find_override(extensions, "CheckNonce") {
        return val
            .as_u64()
            .ok_or(Error::Mapping("CheckNonce override is not a number".into()));
    }

    let response = crate::query(
        chain,
        meta,
        &format!("system/account/0x{}", hex::encode(account)),
        None,
    )
    .await?;

    match response {
        Response::Value(entry, meta) => {
            let value = entry.as_value(&meta.registry);
            if let Some(n) = value
                .field("nonce")
                .and_then(|v| v.as_u32().map(|n| n as u64).or_else(|| v.as_u64()))
            {
                return Ok(n);
            }
            let dyn_val: DynValue = value
                .try_into()
                .map_err(|_| Error::Mapping("failed to decode account info".into()))?;
            dyn_val
                .get("nonce")
                .and_then(|v| v.as_u64())
                .ok_or(Error::Mapping("nonce not found in account info".into()))
        }
        Response::None => {
            log::warn!("account not found, using nonce 0");
            Ok(0)
        }
        _ => Err(Error::AccountNotFound),
    }
}
