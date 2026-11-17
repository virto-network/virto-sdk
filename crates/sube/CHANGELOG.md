# Changelog

## Unreleased

### Breaking changes

- Awaiting a call builder no longer submits. Transactions now use explicit
  `prepare_call`, `build_transaction`, `inspect_transaction`, and
  `submit_transaction` stages.
- `ExtrinsicAssembler::account` was replaced by the required
  `ExtrinsicAssembler::nonce_account`, which separates nonce identity from
  signing identity. There is no `account` alias; implementors rename the
  method and callers use `nonce_account`.
- `ExtrinsicAssembler::assemble` returns an `AssembledExtrinsic` instead of
  `Vec<u8>`.
- `OneShotCall`, `CallBuilder` and `Sube::call` were removed. Use
  `Sube::prepare_call(path, &body)` (with `Text(..)` for a text body) and
  pass signing and options to `build_transaction`.
- `Backend::submit(ext, wait_for_finalization: bool) -> Result<()>` is now
  `Backend::submit(ext, wait_for: WaitFor, metadata: &Metadata) ->
  Result<TransactionReceipt>` and is the only submission method a backend
  implements; `metadata` decodes the inclusion block's events into the
  receipt. Applications should call `Sube::submit_transaction`, which also
  checks the runtime version and genesis hash the bytes were built for.
- `extrinsic::encode_extensions` returns `(extra, additional_signed,
  extensions)` instead of `(extra, additional_signed)`; the third item holds
  each extension's exact bytes. There is no separate
  `encode_extensions_detailed`.
- Transaction artifacts hold bytes only: `PreparedCall::hex` and
  `EncodedExtrinsic::hex` are methods instead of fields, and
  `EncodedExtension` has `extra`/`additional_signed` byte fields with
  `extra_hex()`/`additional_signed_hex()` methods.
- Transactions are mortal by default (64 blocks) instead of immortal.
- `ChainContext` gained `checkpoint_number`, `checkpoint_hash`, `mortality`
  and `tip` fields.
- `Error` gained `RuntimeUpgrade` and `GenesisMismatch` variants.
- `scales` `RawPallet` gained an `errors_ty` field; filtered metadata keeps
  pallet error types so dispatch errors can be decoded.

### Added

- Metadata-driven V4 address/signature encoding, 64-block mortal checkpoints,
  optional-extension `None` defaults, transaction diagnostics and receipts.
- `Signer::signature_variant` names the metadata signature variant a signer
  produces (`&str`, default `"Sr25519"`).
- Optional `libwallet` adapter for ordinary wallet signers.
- Non-mutating `sube query` and `sube tx` CLI commands.

### Fixed

- Signed transactions could not be built from V14 metadata (what
  `Metadata_metadata` returns), which declares no address or signature type;
  they now fall back to `MultiAddress::Id` and `MultiSignature`.
- Transaction extensions that are newtypes were serialized without type
  information: `CheckNonce` went out as 8 raw bytes instead of a compact
  integer, so no runtime could decode the transaction.
- Struct fields given as a map were encoded by position, ignoring their
  names: a misnamed field encoded silently into wrong bytes, and JSON
  bodies (whose keys are sorted) failed or mis-encoded. Fields are now
  matched by name; unknown or missing fields are errors, and a missing
  `Option` field is `None`. An `Option` takes a bare value, not
  `{"Some": ..}`.
- Serializing dynamic values for types wrapped in more than one newtype (e.g.
  a `BoundedBTreeSet` of tuples) produced wrong SCALE bytes.

## 1.0.0

First stable release.

### Breaking changes

- The default feature set is empty. Enable `wss`, `ws-web`, `ws-edge`, or a
  smoldot feature for a connected backend.
- `SignerFn::new` accepts an exact `[u8; 32]` account ID; the panicking generic
  tuple conversion was removed.
- `Signer::sign` and `ExtrinsicAssembler::assemble` are async trait methods.
- `Sube::set_metadata` replaces mutable access to its internal `Rc`.
- The lossy `Response -> Vec<u8>` conversion was removed. Use
  `Response::into_value` or match the response explicitly.
- Embedded `connect_edge` consumes `EdgeResources` containing unique static
  stack/RX/TX resources. `EdgeNet` was removed, and `wss://` now requires an
  explicit DER-encoded CA certificate instead of disabling verification.

### Maintenance

- Metadata is owned per connection rather than stored in an unsafe global
  cache, preventing stale runtime and cross-chain metadata reuse.
- Query resolution is shared across normal and block-hash queries.
- JSON-RPC envelopes are parsed structurally, including nested and escaped
  values.
- Native TLS uses Rustls/WebPKI; edge transport dependencies and smoldot are
  updated to their current release lines.
- The CLI is part of the workspace and keeps the non-`Send` client entirely on
  its worker thread.
