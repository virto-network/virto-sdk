# Sube

A lightweight, metadata-driven Substrate client for `no_std`, embedded, browser,
and native Rust applications.

Sube uses runtime metadata v15+ and
[`scale-serialization`](../scales/) to encode calls and decode storage without
generated runtime types.

## Quick start

The default feature set is intentionally empty. Native TLS applications should
enable `wss`:

```toml
[dependencies]
sube = { version = "1.0", features = ["wss"] }
```

```rust
use sube::Sube;

# async fn example() -> sube::Result<()> {
let mut chain = Sube::connect("wss://kreivo.io").await?;
let response = chain.query("system/account/0x1284...b2b").await?;

if let Some(text) = response.to_text()? {
    println!("{text}");
}
# Ok(())
# }
```

Reuse a connected `Sube` handle for repeated queries so the transport and
metadata remain local to that connection. One-shot queries are also available:

```rust
# async fn example() -> sube::Result<()> {
let response = sube::sube("wss://kreivo.io/system/number").await?;
assert!(response.to_text()?.is_some());
# Ok(())
# }
```

## Features

| Feature | Environment | Description |
| --- | --- | --- |
| *(default)* | `no_std` | Metadata, SCALE values, offline backends, and builders |
| `ws` | native `std` | Plain WebSocket using `async-tungstenite` and `smol` |
| `wss` | native `std` | WebSocket with Rustls and WebPKI roots; implies `ws` |
| `ws-edge` | embedded `no_std` | Embassy TCP, mbedTLS, and `edge-ws` |
| `ws-web` | browser | Browser WebSocket for `wasm32-unknown-unknown` |
| `smoldot` | custom `no_std` platform | Smoldot light-client backend |
| `smoldot-std` | native `std` | Smoldot default platform with Wasmtime |
| `libwallet` | any | Adapter for ordinary libwallet-backed V4 signing |

Text-format encoding and decoding are always available; there is no separate
`text` feature.

### Browser

Build with:

```sh
cargo build --target wasm32-unknown-unknown --features ws-web
```

Browser applications use `Sube::connect("wss://…")` normally. Smoldot's
provided default platform is native-only, so browser builds should use
`ws-web` or provide a custom `smoldot` platform.

### Embedded

`connect_edge` consumes unique stack and socket-buffer resources, preventing
overlapping connections from aliasing the same buffers:

```rust,ignore
static RX: StaticCell<[u8; 2048]> = StaticCell::new();
static TX: StaticCell<[u8; 2048]> = StaticCell::new();

let resources = sube::EdgeResources::new(
    stack,
    RX.init([0; 2048]),
    TX.init([0; 2048]),
)
.with_ca_certificate_der(include_bytes!("root-ca.der"));
let chain = sube::connect_edge(
    "wss://kreivo.io",
    resources,
    rng,
    &["Balances"],
)
.await?;
```

The filtered pallet list is fetched during connection; `System` is retained
automatically. A `wss://` connection requires a trusted DER-encoded CA
certificate; plain `ws://` remains available for local development.

## Transactions

Preparing, building, inspecting, and submitting are deliberately separate.
Neither building nor inspecting can submit:

```rust,ignore
use sube::{Text, TransactionOptions, WaitFor};

let call = chain.prepare_call(
    "system/remark",
    &Text("(remark:0x68656c6c6f)"),
)?;
let extrinsic = chain
    .build_transaction(&call, &signer, TransactionOptions::default())
    .await?;
let report = chain.inspect_transaction(&extrinsic).await?;

// The only mutating operation:
let receipt = chain
    .submit_transaction(&extrinsic, WaitFor::Finalized)
    .await?;
```

Transactions are 64-block mortal by default and use the finalized header as
their checkpoint. `TransactionOptions::default().immortal()` opts out.
Metadata-typed optional extensions default to `None`; unknown required
extensions must be supplied explicitly.

## CLI

The workspace includes an unpublished explorer:

```sh
cargo run -p sube-cli -- --chain wss://kreivo.io system/number
cargo run -p sube-cli -- --chain wss://kreivo.io query system/number
cargo run -p sube-cli -- --chain wss://kreivo.io tx system/remark \
  --body '(remark:0x68656c6c6f)'
cargo run -p sube-cli -- --chain wss://kreivo.io
```

`tx` validates and prints call hex without submitting. The final form opens
the terminal UI.

## Verification

```sh
just ci
```

Live-chain tests are ignored by default:

```sh
SUBE_TEST_CHAIN=wss://kreivo.io \
  cargo test --features wss --test integration -- --ignored
```

The CLI also has an ignored, destructive pallet-pass smoke test. It requires a
disposable funded registrar and device mnemonic; see
[`cli/README.md`](cli/README.md) for the exact environment variables and
warning.

The crate requires Rust 1.88 or newer. See [CHANGELOG.md](CHANGELOG.md) for the
1.0 migration notes.
