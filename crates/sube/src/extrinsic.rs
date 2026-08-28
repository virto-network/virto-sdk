use alloc::rc::Rc;

use crate::hasher::hash;
use crate::metadata::{self as meta, Hasher, SignedExtensionMeta, TypeId};
use crate::prelude::*;
use crate::signer::SignatureScheme;
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

/// A metadata-validated runtime call. Preparing a call never signs or submits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedCall {
    pub pallet: String,
    pub call: String,
    pub bytes: Vec<u8>,
    pub hex: String,
}

/// Transaction lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mortality {
    Immortal,
    Mortal { period: u64 },
}

/// Canonical values encoded by `sp_runtime::generic::Era::Mortal` for a
/// transaction prepared at `current`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MortalEra {
    pub period: u64,
    pub phase: u64,
    pub birth: u64,
    pub death: u64,
}

/// Options used while signing and encoding a transaction.
#[derive(Clone, Debug)]
pub struct TransactionOptions {
    pub nonce: Option<u64>,
    pub tip: u64,
    pub extensions: Vec<(String, DynValue)>,
    pub mortality: Mortality,
}

impl Default for TransactionOptions {
    fn default() -> Self {
        Self {
            nonce: None,
            tip: 0,
            extensions: Vec::new(),
            mortality: Mortality::Mortal { period: 64 },
        }
    }
}

impl TransactionOptions {
    pub fn nonce(mut self, nonce: u64) -> Self {
        self.nonce = Some(nonce);
        self
    }

    pub fn tip(mut self, tip: u64) -> Self {
        self.tip = tip;
        self
    }

    pub fn immortal(mut self) -> Self {
        self.mortality = Mortality::Immortal;
        self
    }

    pub fn mortal(mut self, period: u64) -> Self {
        self.mortality = Mortality::Mortal { period };
        self
    }

    pub fn with_extension(mut self, identifier: impl Into<String>, value: DynValue) -> Self {
        let identifier = identifier.into();
        self.extensions.retain(|(id, _)| id != &identifier);
        self.extensions.push((identifier, value));
        self
    }
}

/// Identities involved in authorizing an extrinsic.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuthorizationSummary {
    pub signing_account: Vec<u8>,
    pub nonce_account: Vec<u8>,
    pub scheme: Option<String>,
}

/// Exact metadata-ordered bytes contributed by one transaction extension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedExtension {
    pub identifier: String,
    pub extra_hex: String,
    pub additional_signed_hex: String,
}

/// Inner extrinsic bytes and the extension bytes used to assemble them.
#[derive(Clone, Debug)]
pub struct AssembledExtrinsic {
    pub bytes: Vec<u8>,
    pub extensions: Vec<EncodedExtension>,
}

/// Cached identity and denomination properties reported by the chain.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChainProperties {
    pub ss58_format: Option<u16>,
    pub token_symbols: Vec<String>,
    pub token_decimals: Vec<u32>,
}

/// Fully encoded, SCALE length-prefixed extrinsic. Building does not submit it.
#[derive(Clone, Debug)]
pub struct EncodedExtrinsic {
    pub bytes: Vec<u8>,
    pub hex: String,
    pub call: PreparedCall,
    pub checkpoint_hash: [u8; 32],
    pub checkpoint_number: u64,
    pub expires_at: Option<u64>,
    pub genesis_hash: [u8; 32],
    pub spec_version: u32,
    pub transaction_version: u32,
    pub nonce: u64,
    pub authorization: AuthorizationSummary,
    pub extensions: Vec<EncodedExtension>,
}

/// Frozen inputs for an externally-signed V4 transaction.
///
/// `signing_payload` is the exact byte string the external signer must sign.
/// It is already Blake2-256 hashed when the ordinary Substrate payload exceeds
/// 256 bytes. Finishing the request re-encodes and verifies all metadata-driven
/// extension bytes before accepting the signature.
#[derive(Clone, Debug)]
pub struct ExternalSigningRequest {
    pub call: PreparedCall,
    pub signing_payload: Vec<u8>,
    pub signing_payload_hex: String,
    pub context: ChainContext,
    pub signing_account: Vec<u8>,
    pub nonce_account: Vec<u8>,
    pub scheme: SignatureScheme,
    pub extensions: Vec<EncodedExtension>,
    extension_overrides: Vec<(String, DynValue)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransactionWeight {
    pub ref_time: u64,
    pub proof_size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransactionValidity {
    Valid,
    Invalid(String),
    Unknown,
}

/// Non-mutating fee/weight/validity diagnostics.
#[derive(Clone, Debug, Default)]
pub struct TransactionReport {
    pub partial_fee: Option<u128>,
    pub weight: Option<TransactionWeight>,
    pub validity: Option<TransactionValidity>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WaitFor {
    BestBlock,
    #[default]
    Finalized,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DispatchOutcome {
    Success,
    Failed(String),
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionEvent {
    pub pallet: String,
    pub variant: String,
    pub data: Vec<u8>,
    pub decoded: Option<String>,
}

/// Inclusion/finalization information returned by transaction watch.
#[derive(Clone, Debug)]
pub struct TransactionReceipt {
    pub best_block_hash: Option<String>,
    pub finalized_block_hash: Option<String>,
    pub extrinsic_index: Option<u32>,
    pub dispatch_outcome: DispatchOutcome,
    pub events: Vec<TransactionEvent>,
}

impl Default for TransactionReceipt {
    fn default() -> Self {
        Self {
            best_block_hash: None,
            finalized_block_hash: None,
            extrinsic_index: None,
            dispatch_outcome: DispatchOutcome::Unknown,
            events: Vec::new(),
        }
    }
}

/// Chain context fetched once for extension defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainContext {
    pub spec_version: u32,
    pub tx_version: u32,
    pub genesis_hash: [u8; 32],
    pub account_nonce: u64,
    pub checkpoint_number: u64,
    /// Authenticated finalized head used for metadata, nonce lookup, and
    /// runtime diagnostics.
    pub checkpoint_hash: [u8; 32],
    /// Block hash committed by CheckMortality. For long eras the encoded phase
    /// is quantized, so this can precede `checkpoint_hash` by a few blocks.
    pub mortality_checkpoint_hash: [u8; 32],
    pub mortality: Mortality,
    pub tip: u64,
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
        "CheckMortality" => match ctx.mortality {
            Mortality::Immortal => Some(DynValue::obj(&[("Immortal", DynValue::Null)])),
            Mortality::Mortal { period } => {
                let era = mortal_era(period, ctx.checkpoint_number);
                Some(DynValue::obj(&[(
                    "Mortal",
                    DynValue::Seq(vec![DynValue::from(era.period), DynValue::from(era.phase)]),
                )]))
            }
        },
        "CheckNonce" => Some(DynValue::from(ctx.account_nonce)),
        "ChargeTransactionPayment" => Some(DynValue::from(ctx.tip)),
        "ChargeAssetTxPayment" => Some(DynValue::obj(&[
            ("tip", DynValue::from(ctx.tip)),
            ("asset_id", DynValue::Null),
        ])),
        _ => None,
    }
}

/// Default value for a well-known extension's "additional_signed" data.
fn default_additional(identifier: &str, ctx: &ChainContext) -> Option<DynValue> {
    match identifier {
        "CheckSpecVersion" => Some(DynValue::from(ctx.spec_version)),
        "CheckTxVersion" => Some(DynValue::from(ctx.tx_version)),
        // Genesis hash must be encoded as raw bytes — the target is a `[u8; 32]`
        // which scales does not accept as a hex string.
        "CheckGenesis" => Some(DynValue::from(ctx.genesis_hash)),
        "CheckMortality" => Some(DynValue::from(match ctx.mortality {
            Mortality::Immortal => ctx.genesis_hash,
            Mortality::Mortal { .. } => ctx.mortality_checkpoint_hash,
        })),
        _ => None,
    }
}

fn option_none(ty: TypeId, registry: &scales::Registry) -> Option<DynValue> {
    fn is_option(ty: TypeId, registry: &scales::Registry, depth: u8) -> bool {
        if depth == 8 {
            return false;
        }
        match registry.resolve(ty) {
            Some(scales::TypeDef::Variant(def))
                if def.variants().any(|variant| variant.name() == "None")
                    && def.variants().any(|variant| variant.name() == "Some") =>
            {
                true
            }
            Some(scales::TypeDef::StructNewType(inner)) | Some(scales::TypeDef::Compact(inner)) => {
                is_option(inner, registry, depth + 1)
            }
            Some(scales::TypeDef::Tuple(items)) | Some(scales::TypeDef::StructTuple(items))
                if items.len() == 1 =>
            {
                is_option(items[0], registry, depth + 1)
            }
            _ => false,
        }
    }

    is_option(ty, registry, 0).then_some(DynValue::Null)
}

fn normalize_period(period: u64) -> u64 {
    period
        .checked_next_power_of_two()
        .unwrap_or(1 << 16)
        .clamp(4, 1 << 16)
}

/// Reproduce Substrate's mortal-era normalization, phase quantization, and
/// birth/death calculations without depending on `sp_runtime`.
pub fn mortal_era(period: u64, current: u64) -> MortalEra {
    let period = normalize_period(period);
    let quantize_factor = (period >> 12).max(1);
    let phase = (current % period) / quantize_factor * quantize_factor;
    let birth = (current.max(phase) - phase) / period * period + phase;
    MortalEra {
        period,
        phase,
        birth,
        death: birth.saturating_add(period),
    }
}

/// First block at which a transaction prepared at `current` is no longer
/// valid, matching `sp_runtime::generic::Era::death`.
pub fn mortality_expiry(mortality: Mortality, current: u64) -> Option<u64> {
    match mortality {
        Mortality::Immortal => None,
        Mortality::Mortal { period } => Some(mortal_era(period, current).death),
    }
}

/// Encode `sp_runtime::generic::Era::Mortal` using its compact two-byte format.
pub fn encode_mortal_era(period: u64, current: u64) -> [u8; 2] {
    let era = mortal_era(period, current);
    let quantize_factor = (era.period >> 12).max(1);
    let encoded =
        (era.period.trailing_zeros() - 1) as u16 | (((era.phase / quantize_factor) << 4) as u16);
    encoded.to_le_bytes()
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
    let (extra, additional, _) = encode_extensions_detailed(extensions, registry, ctx, overrides)?;
    Ok((extra, additional))
}

/// Encode extensions and retain the exact bytes contributed by each entry.
pub fn encode_extensions_detailed(
    extensions: &[SignedExtensionMeta],
    registry: &scales::Registry,
    ctx: &ChainContext,
    overrides: &[(String, DynValue)],
) -> Result<(Vec<u8>, Vec<u8>, Vec<EncodedExtension>)> {
    let mut extra = Vec::new();
    let mut additional = Vec::new();
    let mut summaries = Vec::with_capacity(extensions.len());

    for ext in extensions {
        let mut extension_extra = Vec::new();
        let mut extension_additional = Vec::new();
        // "extra" bytes — included in extrinsic body
        if meta::is_zero_size_type(ext.ty, registry) {
            // zero bytes, nothing to encode
        } else if ext.identifier == "CheckMortality"
            && find_override(overrides, &ext.identifier).is_none()
            && matches!(ctx.mortality, Mortality::Mortal { .. })
        {
            let Mortality::Mortal { period } = ctx.mortality else {
                unreachable!()
            };
            extension_extra.extend_from_slice(&encode_mortal_era(period, ctx.checkpoint_number));
        } else {
            let value = find_override(overrides, &ext.identifier)
                .or_else(|| default_extra(&ext.identifier, ctx))
                .or_else(|| option_none(ext.ty, registry))
                .ok_or_else(|| Error::MissingExtensionValue(ext.identifier.clone()))?;
            scales::to_bytes_with_info(&mut extension_extra, &value, Some((registry, ext.ty)))
                .map_err(|e| Error::Encode(e.to_string()))?;
        }

        // "additional_signed" bytes — signing payload only
        if meta::is_zero_size_type(ext.additional_signed, registry) {
            // zero bytes
        } else {
            let value = default_additional(&ext.identifier, ctx)
                .or_else(|| option_none(ext.additional_signed, registry))
                .ok_or_else(|| {
                    Error::MissingExtensionValue(format!("{} (additional_signed)", ext.identifier))
                })?;
            scales::to_bytes_with_info(
                &mut extension_additional,
                &value,
                Some((registry, ext.additional_signed)),
            )
            .map_err(|e| Error::Encode(e.to_string()))?;
        }

        summaries.push(EncodedExtension {
            identifier: ext.identifier.clone(),
            extra_hex: format!("0x{}", hex::encode(&extension_extra)),
            additional_signed_hex: format!("0x{}", hex::encode(&extension_additional)),
        });
        extra.extend_from_slice(&extension_extra);
        additional.extend_from_slice(&extension_additional);
    }

    Ok((extra, additional, summaries))
}

/// Metadata-validate and encode a call without signing it.
pub fn prepare_call<V>(meta: &Metadata, path: &str, body: &V) -> Result<PreparedCall>
where
    V: EncodeCall + ?Sized,
{
    let (pallet_name, call_name, keys) = crate::parse_uri(path).ok_or(Error::BadInput)?;
    if !keys.is_empty() {
        return Err(Error::BadInput);
    }
    let pallet = meta
        .pallet_by_name(&pallet_name)
        .ok_or_else(|| Error::PalletNotFound(pallet_name.clone()))?;
    let calls_ty = pallet.calls_ty.ok_or(Error::CallNotFound)?;
    let mut bytes = vec![pallet.index];
    bytes.extend(body.encode_call(&call_name.to_lowercase(), &meta.registry, calls_ty)?);
    Ok(PreparedCall {
        pallet: pallet.name.clone(),
        call: call_name,
        hex: format!("0x{}", hex::encode(&bytes)),
        bytes,
    })
}

/// Sign and encode a prepared call without submitting it.
pub async fn build_transaction(
    chain: &mut (impl Backend + ?Sized),
    meta: &Rc<Metadata>,
    call: &PreparedCall,
    options: &TransactionOptions,
    assembler: &(impl crate::ExtrinsicAssembler + ?Sized),
) -> Result<EncodedExtrinsic> {
    let from_account = assembler.nonce_account();
    // Build chain context
    let ctx = build_context(chain, meta, options, from_account.as_ref()).await?;

    // Delegate assembly to the assembler
    let assembled = assembler
        .assemble(
            &call.bytes,
            &meta.extrinsic,
            &meta.registry,
            &ctx,
            &options.extensions,
        )
        .await?;

    encode_transaction(call, &ctx, assembler.authorization(), assembled)
}

/// Capture chain context and construct the exact bytes an external V4 signer
/// must authorize. This function performs no signing and never submits.
pub async fn prepare_external_signing(
    chain: &mut (impl Backend + ?Sized),
    meta: &Rc<Metadata>,
    call: &PreparedCall,
    signing_account: &[u8],
    nonce_account: &[u8],
    scheme: SignatureScheme,
    options: &TransactionOptions,
) -> Result<ExternalSigningRequest> {
    let checkpoint = chain.block_info(None).await?;
    prepare_external_signing_at(
        chain,
        meta,
        call,
        signing_account,
        nonce_account,
        scheme,
        options,
        checkpoint,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn prepare_external_signing_at(
    chain: &mut (impl Backend + ?Sized),
    meta: &Rc<Metadata>,
    call: &PreparedCall,
    signing_account: &[u8],
    nonce_account: &[u8],
    scheme: SignatureScheme,
    options: &TransactionOptions,
    checkpoint: crate::BlockInfo,
) -> Result<ExternalSigningRequest> {
    ensure_v4(meta)?;
    let context = build_context_at(chain, meta, options, nonce_account, checkpoint).await?;
    let (extra, additional_signed, extensions) = encode_extensions_detailed(
        &meta.extrinsic.extensions,
        &meta.registry,
        &context,
        &options.extensions,
    )?;
    let signing_payload = signing_payload(&call.bytes, &extra, &additional_signed);

    Ok(ExternalSigningRequest {
        call: call.clone(),
        signing_payload_hex: format!("0x{}", hex::encode(&signing_payload)),
        signing_payload,
        context,
        signing_account: signing_account.to_vec(),
        nonce_account: nonce_account.to_vec(),
        scheme,
        extensions,
        extension_overrides: options.extensions.clone(),
    })
}

/// Validate an external signature and finish a previously frozen V4 request.
/// This operation is local and never submits the resulting extrinsic.
pub fn finish_external_signing(
    meta: &Rc<Metadata>,
    request: &ExternalSigningRequest,
    signature: &[u8],
) -> Result<EncodedExtrinsic> {
    ensure_v4(meta)?;
    if signature.len() != request.scheme.signature_len() {
        return Err(Error::Signing(format!(
            "{} signature must be {} bytes, got {}",
            request.scheme.metadata_name(),
            request.scheme.signature_len(),
            signature.len()
        )));
    }

    let current_versions = runtime_versions(meta)?;
    if current_versions != (request.context.spec_version, request.context.tx_version) {
        return Err(Error::RuntimeUpgrade {
            built_spec: request.context.spec_version,
            current_spec: current_versions.0,
        });
    }

    let (extra, additional_signed, extensions) = encode_extensions_detailed(
        &meta.extrinsic.extensions,
        &meta.registry,
        &request.context,
        &request.extension_overrides,
    )?;
    let signing_payload = signing_payload(&request.call.bytes, &extra, &additional_signed);
    if signing_payload != request.signing_payload || extensions != request.extensions {
        return Err(Error::OperationFailed(
            "external signing request no longer matches runtime metadata".into(),
        ));
    }

    let assembled = assemble_signed_v4_parts(
        &request.signing_account,
        signature,
        Some(request.scheme.metadata_name()),
        &request.call.bytes,
        &meta.extrinsic,
        &meta.registry,
        extra,
        extensions,
    )?;
    encode_transaction(
        &request.call,
        &request.context,
        AuthorizationSummary {
            signing_account: request.signing_account.clone(),
            nonce_account: request.nonce_account.clone(),
            scheme: Some(request.scheme.metadata_name().into()),
        },
        assembled,
    )
}

fn ensure_v4(meta: &Metadata) -> Result<()> {
    if meta.extrinsic.version != 4 {
        return Err(Error::OperationFailed(format!(
            "external signing only supports V4 extrinsics; runtime declares V{}",
            meta.extrinsic.version
        )));
    }
    Ok(())
}

fn signing_payload(encoded_call: &[u8], extra: &[u8], additional_signed: &[u8]) -> Vec<u8> {
    let payload = [encoded_call, extra, additional_signed].concat();
    if payload.len() > 256 {
        hash(&Hasher::Blake2_256, &payload)
    } else {
        payload
    }
}

fn encode_transaction(
    call: &PreparedCall,
    ctx: &ChainContext,
    authorization: AuthorizationSummary,
    assembled: AssembledExtrinsic,
) -> Result<EncodedExtrinsic> {
    let len = Compact(
        u32::try_from(assembled.bytes.len())
            .map_err(|_| Error::Encode("extrinsic too large".into()))?,
    )
    .encode();
    let bytes = [len, assembled.bytes].concat();
    let expires_at = mortality_expiry(ctx.mortality, ctx.checkpoint_number);
    Ok(EncodedExtrinsic {
        hex: format!("0x{}", hex::encode(&bytes)),
        bytes,
        call: call.clone(),
        checkpoint_hash: ctx.checkpoint_hash,
        checkpoint_number: ctx.checkpoint_number,
        expires_at,
        genesis_hash: ctx.genesis_hash,
        spec_version: ctx.spec_version,
        transaction_version: ctx.tx_version,
        nonce: ctx.account_nonce,
        authorization,
        extensions: assembled.extensions,
    })
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
) -> Result<AssembledExtrinsic> {
    // Encode extensions
    let (extra_bytes, additional_signed, extensions) =
        encode_extensions_detailed(&meta.extensions, registry, ctx, overrides)?;

    // Sign
    let payload = signing_payload(encoded_call, &extra_bytes, &additional_signed);
    let signature = signer.sign(payload).await?;

    assemble_signed_v4_parts(
        signer.account().as_ref(),
        signature.as_ref(),
        signer.signature_variant(),
        encoded_call,
        meta,
        registry,
        extra_bytes,
        extensions,
    )
}

fn assemble_signed_v4_parts(
    account: &[u8],
    signature: &[u8],
    signature_variant: Option<&str>,
    encoded_call: &[u8],
    meta: &meta::ExtrinsicMeta,
    registry: &scales::Registry,
    extra_bytes: Vec<u8>,
    extensions: Vec<EncodedExtension>,
) -> Result<AssembledExtrinsic> {
    // Assemble the metadata-declared address and signature types. This supports
    // AccountId/newtype addresses as well as MultiAddress, and any signature
    // enum variant selected by the signer.
    let address_ty = meta.address_ty.ok_or(Error::BadMetadata)?;
    let signature_ty = meta.signature_ty.ok_or(Error::BadMetadata)?;

    let address_value = match registry.resolve(address_ty) {
        Some(scales::TypeDef::Variant(def)) => {
            let id = def
                .variants()
                .find(|variant| variant.name().eq_ignore_ascii_case("Id"))
                .ok_or_else(|| {
                    Error::Encode("address enum has no metadata-declared Id variant".into())
                })?;
            DynValue::obj(&[(id.name(), DynValue::from(account))])
        }
        _ => DynValue::from(account),
    };
    let address_bytes = scales::to_vec_with_info(&address_value, Some((registry, address_ty)))
        .map_err(|error| Error::Encode(format!("address: {error}")))?;

    let signature_value = match registry.resolve(signature_ty) {
        Some(scales::TypeDef::Variant(def)) => {
            let requested = signature_variant
                .ok_or_else(|| Error::Encode("signer did not select a signature variant".into()))?;
            let variant = def
                .variants()
                .find(|variant| variant.name().eq_ignore_ascii_case(requested))
                .ok_or_else(|| {
                    Error::Encode(format!(
                        "signature variant {requested} is absent from runtime metadata"
                    ))
                })?;
            DynValue::obj(&[(variant.name(), DynValue::from(signature))])
        }
        _ => DynValue::from(signature),
    };
    let signature_bytes =
        scales::to_vec_with_info(&signature_value, Some((registry, signature_ty)))
            .map_err(|error| Error::Encode(format!("signature: {error}")))?;

    let bytes = [
        vec![0b10000000 | meta.version],
        address_bytes,
        signature_bytes,
        extra_bytes,
        encoded_call.to_vec(),
    ]
    .concat();
    Ok(AssembledExtrinsic { bytes, extensions })
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    fn test_ctx() -> ChainContext {
        ChainContext {
            spec_version: 100,
            tx_version: 2,
            genesis_hash: [0xab; 32],
            account_nonce: 42,
            checkpoint_number: 128,
            checkpoint_hash: [0xcd; 32],
            mortality_checkpoint_hash: [0xce; 32],
            mortality: Mortality::Mortal { period: 64 },
            tip: 0,
        }
    }

    #[test]
    fn default_extra_check_mortality() {
        let ctx = test_ctx();
        assert_eq!(
            default_extra("CheckMortality", &ctx),
            Some(DynValue::obj(&[(
                "Mortal",
                DynValue::Seq(vec![DynValue::from(64u64), DynValue::from(0u64)])
            )]))
        );
    }

    #[test]
    fn mortal_era_uses_period_and_checkpoint_phase() {
        assert_eq!(encode_mortal_era(64, 0), [0x05, 0x00]);
        assert_eq!(encode_mortal_era(64, 42), [0xa5, 0x02]);
        assert_eq!(encode_mortal_era(3, 5), encode_mortal_era(4, 5));
    }

    #[test]
    fn mortal_era_expiry_is_anchored_at_its_encoded_birth() {
        assert_eq!(
            mortal_era(64, 42),
            MortalEra {
                period: 64,
                phase: 42,
                birth: 42,
                death: 106,
            }
        );
        assert_eq!(
            mortality_expiry(Mortality::Mortal { period: 64 }, 42),
            Some(106)
        );
    }

    #[test]
    fn long_mortal_era_quantizes_birth_and_death_together() {
        assert_eq!(
            mortal_era(8_192, 10_005),
            MortalEra {
                period: 8_192,
                phase: 1_812,
                birth: 10_004,
                death: 18_196,
            }
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
            Some(DynValue::from(0u64))
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
        assert_eq!(
            default_additional("CheckGenesis", &ctx),
            Some(DynValue::from([0xab; 32]))
        );
    }

    #[test]
    fn default_additional_check_mortality_uses_era_birth_hash() {
        let ctx = test_ctx();
        assert_eq!(
            default_additional("CheckMortality", &ctx),
            Some(DynValue::from([0xce; 32]))
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

    #[test]
    fn metadata_typed_option_defaults_to_none() {
        let meta = Metadata::from_bytes(include_bytes!("../tests/fixtures/kreivo.scale")).unwrap();
        let extension = meta
            .extrinsic
            .extensions
            .iter()
            .find(|extension| option_none(extension.ty, &meta.registry).is_some())
            .unwrap();
        assert_eq!(
            option_none(extension.ty, &meta.registry),
            Some(DynValue::Null)
        );
    }

    #[test]
    fn detailed_extension_summary_uses_exact_encoded_bytes() {
        let meta = Metadata::from_bytes(include_bytes!("../tests/fixtures/kreivo.scale")).unwrap();
        let (extra, additional, summaries) = encode_extensions_detailed(
            &meta.extrinsic.extensions,
            &meta.registry,
            &test_ctx(),
            &[],
        )
        .unwrap();

        let summarized_extra = summaries
            .iter()
            .flat_map(|summary| hex::decode(summary.extra_hex.trim_start_matches("0x")).unwrap())
            .collect::<Vec<_>>();
        let summarized_additional = summaries
            .iter()
            .flat_map(|summary| {
                hex::decode(summary.additional_signed_hex.trim_start_matches("0x")).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(summarized_extra, extra);
        assert_eq!(summarized_additional, additional);
        assert_eq!(
            summaries
                .iter()
                .map(|summary| summary.identifier.as_str())
                .collect::<Vec<_>>(),
            meta.extrinsic
                .extensions
                .iter()
                .map(|extension| extension.identifier.as_str())
                .collect::<Vec<_>>()
        );
    }
}

/// Fetch spec/tx version, genesis hash, and account nonce.
pub async fn build_context(
    chain: &mut (impl Backend + ?Sized),
    meta: &Rc<Metadata>,
    options: &TransactionOptions,
    account: &[u8],
) -> Result<ChainContext> {
    let checkpoint = chain.block_info(None).await?;
    build_context_at(chain, meta, options, account, checkpoint).await
}

async fn build_context_at(
    chain: &mut (impl Backend + ?Sized),
    meta: &Rc<Metadata>,
    options: &TransactionOptions,
    account: &[u8],
    checkpoint: crate::BlockInfo,
) -> Result<ChainContext> {
    let (spec_version, tx_version) = runtime_versions(meta)?;

    // Genesis hash
    let genesis_block: Vec<u8> = chain.block_info(Some(0u32)).await?.into();
    let genesis_hash: [u8; 32] = genesis_block
        .try_into()
        .map_err(|_| Error::Decode("genesis block hash is not 32 bytes".into()))?;

    // Resolve the nonce at the same authenticated state/runtime snapshot as
    // the mortality checkpoint and metadata.
    let account_nonce = resolve_nonce(
        chain,
        meta,
        options.nonce,
        &options.extensions,
        account,
        checkpoint.hash,
    )
    .await?;

    let mortality_checkpoint_hash = match options.mortality {
        Mortality::Immortal => genesis_hash,
        Mortality::Mortal { period } => {
            let era = mortal_era(period, checkpoint.number);
            if era.birth == checkpoint.number {
                checkpoint.hash
            } else {
                let birth = u32::try_from(era.birth).map_err(|_| {
                    Error::OperationFailed(
                        "mortal era birth exceeds the backend block-number range".into(),
                    )
                })?;
                let birth_block = chain.block_info(Some(birth)).await?;
                if birth_block.number != era.birth {
                    return Err(Error::OperationFailed(
                        "backend returned the wrong mortal-era birth block".into(),
                    ));
                }
                birth_block.hash
            }
        }
    };

    Ok(ChainContext {
        spec_version,
        tx_version,
        genesis_hash,
        account_nonce,
        checkpoint_number: checkpoint.number,
        checkpoint_hash: checkpoint.hash,
        mortality_checkpoint_hash,
        mortality: options.mortality,
        tip: options.tip,
    })
}

/// Decode the runtime versions captured by the current metadata.
pub fn runtime_versions(meta: &Metadata) -> Result<(u32, u32)> {
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
    if let (Some(sv), Some(tv)) = (
        version.field("spec_version").and_then(|v| v.as_u32()),
        version
            .field("transaction_version")
            .and_then(|v| v.as_u32()),
    ) {
        Ok((sv, tv))
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
        Ok((sv, tv))
    }
}

/// Resolve nonce from: explicit field, extension override, or on-chain query.
async fn resolve_nonce(
    chain: &mut (impl Backend + ?Sized),
    meta: &Rc<Metadata>,
    nonce: Option<u64>,
    extensions: &[(String, DynValue)],
    account: &[u8],
    block_hash: [u8; 32],
) -> Result<u64> {
    if let Some(nonce) = nonce {
        return Ok(nonce);
    }
    if let Some(val) = find_override(extensions, "CheckNonce") {
        return val
            .as_u64()
            .ok_or(Error::Mapping("CheckNonce override is not a number".into()));
    }

    let response = crate::query_at_hash(
        chain,
        meta,
        &format!("system/account/0x{}", hex::encode(account)),
        block_hash,
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
