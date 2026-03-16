use crate::hasher::hash;
use crate::metadata::{self as meta, Hasher, SignedExtensionMeta};
use crate::prelude::*;
use crate::{Backend, Error, Response, Result};

use codec::{Compact, Encode};
use scales::Value;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

/// The body of an extrinsic to be submitted.
#[derive(Serialize, Deserialize, Debug)]
pub struct ExtrinsicBody<Body> {
    pub nonce: Option<u64>,
    pub body: Body,
    #[serde(default)]
    pub extensions: Vec<(String, JsonValue)>,
}

/// Chain context fetched once for extension defaults.
pub struct ChainContext {
    pub spec_version: u32,
    pub tx_version: u32,
    pub genesis_hash: [u8; 32],
    pub account_nonce: u64,
}

/// Look up a caller-provided extension value by identifier.
fn find_override(extensions: &[(String, JsonValue)], id: &str) -> Option<JsonValue> {
    extensions
        .iter()
        .find(|(k, _)| k == id)
        .map(|(_, v)| v.clone())
}

/// Default JSON value for a well-known extension's "extra" data.
fn default_extra(identifier: &str, ctx: &ChainContext) -> Option<JsonValue> {
    match identifier {
        "CheckMortality" => Some(json!({"Immortal": null})),
        "CheckNonce" => Some(json!(ctx.account_nonce)),
        "ChargeTransactionPayment" => Some(json!(0)),
        "ChargeAssetTxPayment" => Some(json!({"tip": 0, "asset_id": null})),
        _ => None,
    }
}

/// Default JSON value for a well-known extension's "additional_signed" data.
fn default_additional(identifier: &str, ctx: &ChainContext) -> Option<JsonValue> {
    match identifier {
        "CheckSpecVersion" => Some(json!(ctx.spec_version)),
        "CheckTxVersion" => Some(json!(ctx.tx_version)),
        "CheckGenesis" | "CheckMortality" => {
            Some(json!(format!("0x{}", hex::encode(ctx.genesis_hash))))
        }
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
    overrides: &[(String, JsonValue)],
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

/// Build and submit a signed extrinsic using metadata-driven extensions.
pub(crate) async fn submit<'m, V>(
    chain: impl Backend,
    meta: &'m crate::Metadata,
    path: &str,
    tx_data: ExtrinsicBody<V>,
    signer: impl crate::Signer,
) -> Result<Response<'m>>
where
    V: serde::Serialize + core::fmt::Debug,
{
    let (pallet, item_or_call, _keys) =
        crate::parse_uri(path).ok_or(Error::BadInput)?;
    let pallet = meta
        .pallet_by_name(&pallet)
        .ok_or(Error::PalletNotFound(pallet))?;
    let calls_ty = pallet.calls_ty.ok_or(Error::CallNotFound)?;

    // Encode call data
    let mut encoded_call = vec![pallet.index];
    let call_json = &json!({ &item_or_call.to_lowercase(): &tx_data.body });
    let call_data = scales::to_vec_with_info(call_json, Some((&meta.registry, calls_ty)))
        .map_err(|e| Error::Encode(e.to_string()))?;
    encoded_call.extend(&call_data);

    let from_account = signer.account();

    // Build chain context
    let ctx = build_context(&chain, meta, &tx_data, from_account.as_ref()).await?;

    // Encode extensions
    let (extra_bytes, additional_signed) = encode_extensions(
        &meta.extrinsic.extensions,
        &meta.registry,
        &ctx,
        &tx_data.extensions,
    )?;

    // Sign
    let signature_payload = [
        encoded_call.clone(),
        extra_bytes.clone(),
        additional_signed,
    ]
    .concat();

    let payload = if signature_payload.len() > 256 {
        hash(&Hasher::Blake2_256, &signature_payload)
    } else {
        signature_payload
    };
    let signature = signer.sign(payload).await?;

    // Assemble extrinsic
    let version = meta.extrinsic.version;

    // MultiAddress::Id → variant 0 + 32-byte account
    let address_bytes = [vec![0x00], from_account.as_ref().to_vec()].concat();

    // Find Sr25519 variant index from signature type
    let sig_prefix = meta
        .extrinsic
        .signature_ty
        .and_then(|ty| match meta.registry.resolve(ty) {
            Some(scales::TypeDef::Variant(vdef)) => vdef
                .variants
                .iter()
                .find(|v| v.name.contains("Sr25519"))
                .map(|v| v.index),
            _ => None,
        })
        .unwrap_or(0x01);

    let encoded_inner = [
        vec![0b10000000 | version],
        address_bytes,
        [vec![sig_prefix], signature.as_ref().to_vec()].concat(),
        extra_bytes,
        encoded_call,
    ]
    .concat();

    let len =
        Compact(u32::try_from(encoded_inner.len()).expect("extrinsic size expected to be <4GB"))
            .encode();

    chain.submit(&[len, encoded_inner].concat()).await?;
    Ok(Response::Void)
}

/// Fetch spec/tx version, genesis hash, and account nonce.
async fn build_context<V>(
    chain: &impl Backend,
    meta: &crate::Metadata,
    tx_data: &ExtrinsicBody<V>,
    account: &[u8],
) -> Result<ChainContext>
where
    V: serde::Serialize + core::fmt::Debug,
{
    // System::Version constant
    let system = meta
        .pallet_by_name("System")
        .ok_or(Error::PalletNotFound("System".into()))?;
    let version_const = system
        .constants
        .iter()
        .find(|c| c.name == "Version")
        .ok_or(Error::ConstantNotFound("System_Version".into()))?;
    let version_json: JsonValue =
        Value::new(&version_const.value, version_const.ty, &meta.registry)
            .try_into()
            .map_err(|_| Error::Mapping("failed to decode System::Version".into()))?;
    let obj = version_json
        .as_object()
        .ok_or(Error::ConstantNotFound("System_Version".into()))?;

    let spec_version = obj
        .get("spec_version")
        .and_then(|v| v.as_u64())
        .ok_or(Error::Mapping("spec_version not found".into()))? as u32;
    let tx_version = obj
        .get("transaction_version")
        .and_then(|v| v.as_u64())
        .ok_or(Error::Mapping("transaction_version not found".into()))? as u32;

    // Genesis hash
    let genesis_block: Vec<u8> = chain.block_info(Some(0u32)).await?.into();
    let mut genesis_hash = [0u8; 32];
    genesis_hash.copy_from_slice(&genesis_block[..32]);

    // Nonce
    let account_nonce = resolve_nonce(chain, meta, tx_data, account).await?;

    Ok(ChainContext {
        spec_version,
        tx_version,
        genesis_hash,
        account_nonce,
    })
}

/// Resolve nonce from: explicit field, extension override, or on-chain query.
async fn resolve_nonce<V>(
    chain: &impl Backend,
    meta: &crate::Metadata,
    tx_data: &ExtrinsicBody<V>,
    account: &[u8],
) -> Result<u64>
where
    V: serde::Serialize + core::fmt::Debug,
{
    if let Some(nonce) = tx_data.nonce {
        return Ok(nonce);
    }
    if let Some(val) = find_override(&tx_data.extensions, "CheckNonce") {
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
        Response::Value(entry, reg) => {
            let value = entry.as_value(reg);
            if let Some(n) = value
                .field("nonce")
                .and_then(|v| v.as_u32().map(|n| n as u64).or_else(|| v.as_u64()))
            {
                return Ok(n);
            }
            let json_val: JsonValue = value
                .try_into()
                .map_err(|_| Error::Mapping("failed to decode account info".into()))?;
            json_val
                .as_object()
                .and_then(|o| o.get("nonce"))
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
