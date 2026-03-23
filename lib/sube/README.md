# Sube

A lightweight Substrate client focused on size and portability.
Runs in `no_std` environments including embedded targets (Cortex-M), the browser, and standard servers.

Uses runtime metadata (≥ v15) and our [Scales](../scales/) library to automatically convert between SCALE binary and human-readable formats (JSON, text) without hardcoded type information.

## Quick Start

```rust
use sube::sube;

// One-liner query
let response = sube("wss://kreivo.io/system/account/0x1234...").await?;

// Reusable handle
let mut chain = sube::Sube::connect("wss://kreivo.io").await?;
let account = chain.query("system/account/0x1234...").await?;

// Historical block query
let old = chain.query_at("system/account/0x1234...", 1000).await?;

// Submit an extrinsic (waits for finalization)
chain.call("balances/transfer_keep_alive")
    .body(json!({ "dest": {"Id": dest}, "value": 1000 }))
    .signer(my_signer)
    .await?;

// Or use compact text format
chain.call("system/remark")
    .body_text("(remark:0x68656c6c6f)")
    .signer(my_signer)
    .await?;
```

## Backends

| Feature | Description |
|---------|-------------|
| `ws` | WebSocket via `async-tungstenite` + `smol` (std) |
| `wss` | WebSocket with TLS (implies `ws`) |
| `ws-edge` | WebSocket via `edge-ws` for embedded targets (no_std) |
| `smoldot-std` | Embedded light client via `smoldot-light` (no external node) |

### Light Client

```rust
let chain = sube::Sube::connect_light(include_str!("chain_spec.json")).await?;
let r = chain.query("system/account/0x1234").await?;
```

## Other Features

| Feature | Description |
|---------|-------------|
| `json` | JSON serialization via `scales` |
| `text` | Compact text format via `scales` |
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
