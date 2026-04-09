# Sube

A lightweight Substrate client focused on size and portability.
Runs in `no_std` environments including embedded targets (Cortex-M), the browser, and standard servers.

Uses runtime metadata (≥ v15) and our [Scales](../scales/) library to automatically convert between SCALE binary and human-readable formats (JSON, text) without hardcoded type information.

## Quick Start

```rust
use sube::sube;

// One-liner query
let response = sube("wss://kreivo.io/system/account/0x1234...").await?;
if let Some(text) = response.to_text()? {
    println!("{text}");
}

// Reusable handle
let mut chain = sube::Sube::connect("wss://kreivo.io").await?;
let account = chain.query("system/account/0x1234...").await?;

// Historical block query
let old = chain.query_at("system/account/0x1234...", 1000).await?;

// Submit an extrinsic with a text-format body (waits for finalization)
chain.call("balances/transfer_keep_alive")
    .body_text("(dest:MultiAddress::Id(0xd435...);value:1000)")
    .signer(my_signer)
    .await?;

// Any serde::Serialize body works too
chain.call("system/remark")
    .body(json!({ "remark": "0x68656c6c6f" }))
    .signer(my_signer)
    .await?;
```

## Backends

| Feature | Description |
|---------|-------------|
| `ws` | WebSocket via `async-tungstenite` + `smol` (std) |
| `wss` | WebSocket with TLS (implies `ws`) |
| `ws-edge` | WebSocket via `edge-ws` for embedded targets (no_std) |
| `ws-web` | Browser WebSocket via `gloo-net` (wasm32-unknown-unknown) |
| `smoldot-std` | Embedded light client via `smoldot-light` (std, no external node) |

### Browser / wasm

Build with `--target wasm32-unknown-unknown --features ws-web`. The browser
handles TCP, TLS and framing natively, so `Sube::connect("wss://...")` works
unchanged from a wasm-bindgen app:

```rust
// In a wasm-bindgen entry point
let mut chain = sube::Sube::connect("wss://kreivo.io").await?;
let r = chain.query("system/account/0x1234").await?;
```

Full in-browser light client (smoldot in wasm) is **not yet supported** —
upstream `smoldot-light` 0.19 only ships `DefaultPlatform`, which is std-only.
A browser-capable `PlatformRef` impl is tracked as future work; in the
meantime, browser apps should use `ws-web` against a public RPC endpoint.

### Light Client

```rust
let chain = sube::Sube::connect_light(include_str!("chain_spec.json")).await?;
let r = chain.query("system/account/0x1234").await?;
```

## Other Features

| Feature | Description |
|---------|-------------|
| `text` | Compact text format via `scales` (call bodies, response decoding) |
| `std` | Standard library support |

## Testing

```sh
# Unit tests
cargo test

# Integration tests (requires network)
cargo test --features test --test integration -- --ignored

# Embedded smoke test (requires qemu-system-arm)
cd tests/qemu && cargo build --release
qemu-system-arm -cpu cortex-m4 -machine lm3s6965evb \
  -nographic -semihosting-config enable=on,target=native \
  -kernel target/thumbv7em-none-eabihf/release/sube-qemu-test
```
