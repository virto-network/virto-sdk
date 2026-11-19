# Changelog

## Unreleased

### License

- sube is now GPL-3.0-or-later, like the rest of the Virto SDK. Releases up
  to 1.0.0 remain Apache-2.0.

### Breaking changes

- Submitting is an explicit stage. Transactions go through `prepare_call`,
  `build_transaction`, `inspect_transaction` and `submit_transaction`, and
  only the last one sends anything; in 1.0.0, awaiting a `Sube::call`
  builder signed and submitted.
- `ExtrinsicAssembler::account` was replaced by the required
  `ExtrinsicAssembler::nonce_account`, which separates nonce identity from
  signing identity. There is no `account` alias; implementors rename the
  method and callers use `nonce_account`.
- `ExtrinsicAssembler::assemble` returns an `AssembledExtrinsic` instead of
  `Vec<u8>`. Besides the bytes it carries each extension's bytes and the
  `signing_account` and `scheme` that `EncodedExtrinsic::authorization`
  reports, so assemblers describe the identity they actually encoded; the
  unreleased `ExtrinsicAssembler::authorization` method was removed.
- `OneShotCall`, `CallBuilder` and `Sube::call` were removed. Use
  `Sube::prepare_call(path, &body)` (with `Text(..)` for a text body) and
  pass signing and options to `build_transaction`.
- `Backend::submit(ext, wait_for_finalization: bool) -> Result<()>` is now
  `Backend::submit(ext, wait_for: WaitFor, metadata: &Metadata) ->
  Result<TransactionReceipt>` and is the only submission method a backend
  implements; `metadata` decodes the inclusion block's events into the
  receipt. The unreleased `Backend::submit_transaction` and
  `Backend::enrich_receipt` hooks were folded into it. Applications should
  call `Sube::submit_transaction`, which also checks the runtime version and
  genesis hash the bytes were built for.
- `extrinsic::encode_extensions` returns `(extra, additional_signed,
  extensions)` instead of `(extra, additional_signed)`; the third item holds
  each extension's exact bytes. There is no separate
  `encode_extensions_detailed`.
- Transaction artifacts hold bytes only: `PreparedCall::hex` and
  `EncodedExtrinsic::hex` are methods instead of fields, and
  `EncodedExtension` has `extra`/`additional_signed` byte fields with
  `extra_hex()`/`additional_signed_hex()` methods.
- `Sube` no longer caches chain properties: `Sube::chain_properties`
  fetches them from the backend each call and returns an owned
  `ChainProperties`. There is no `Sube::cached_chain_properties`.
- `TransactionEvent` has no `data` field; it was always empty. The decoded
  fields are in `decoded`.
- `TransactionValidity` has no `Unknown` variant. An undecodable validation
  result leaves `TransactionReport::validity` as `None` with a warning, as an
  unavailable validation API already did.
- `TransactionOptions` keeps only the `nonce` and `immortal` builder
  methods; `tip`, `mortal` and `with_extension` were removed. Set the public
  `tip`, `mortality` and `extensions` fields directly.
- Transactions are mortal by default (64 blocks) instead of immortal.
- Mortal periods are limited to 4096 blocks (they were rounded up to a
  power of two up to 65536). Longer eras quantize their phase, so the
  runtime checked the signature against an earlier block than the one
  signed.
- `TransactionReceipt` gained `events_error`, which says why an included
  transaction's outcome is `Unknown`.
- `ChainContext` gained `checkpoint_number`, `checkpoint_hash`, `mortality`
  and `tip` fields.
- `Error` gained `RuntimeUpgrade` and `GenesisMismatch` variants.
- `scales` `RawPallet` gained an `errors_ty` field; filtered metadata keeps
  pallet error types so dispatch errors can be decoded.
- The minimum supported Rust version is now 1.91, because `smoldot-light` 4.0
  does not build on older compilers.

### Added

- `fork` feature: `Sube::connect("fork:<snapshot>")` boots an in-process
  fork of a live chain with `sube-fork`, so development builds can use a
  fork and production builds a node or light client with the same code.
  `Sube::connect_fork` opens a session on a shared fork, `Sube::fork`
  returns it for dev operations, and `sube::fork` re-exports the engine.
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
- A serialized Rust `Option` with type information got its `Some` tag twice,
  and a null for an `Option` type was encoded as `Some` with no payload; it
  is now `None`.
- Receipts never had events: they were read through `archive_v1_storage`
  with the wrong event names, and the error was dropped. Events are now read
  at the transaction's block with `chainHead_v1_storage`, falling back to the
  archive API, which not every node offers.
- Queries at a block hash hold that block's pin, so the unpin flush before
  the query can't release it.

### Maintenance

- Dependencies updated: `blake2` 0.11, `smoldot-light` 4.0, `gloo-net` 0.8,
  `mbedtls-rs` 0.3 (`ws-edge`). Storage hashing output is unchanged and now
  covered by BLAKE2 known-answer tests.
- The workspace uses the MSRV-aware resolver (`resolver = "3"`), so fresh
  resolves pick dependency versions compatible with the declared
  `rust-version`.
- The bare-metal QEMU smoke test applies its linker script from `build.rs`, so
  it no longer depends on the directory cargo runs from.
- Fixed lints reported by newer Clippy releases.

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
