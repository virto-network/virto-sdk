# Changelog

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
