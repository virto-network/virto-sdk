# scales - SCALE Serialization

Dynamic [SCALE](https://docs.substrate.io/reference/scale-codec/) encoding and decoding
driven by type registry metadata, without depending on `parity-scale-codec` at runtime.

The core library is `no_std` compatible and has been verified on ARM Cortex-M, RISC-V,
and WebAssembly targets.

## Compressed Registry

Instead of carrying the full `scale-info` `PortableRegistry` (which includes documentation
strings, path segments, and type parameters), scales uses a compact `Registry` that maps
directly to serde's data model. On a real Substrate metadata registry (~880 types) this
achieves around 64% wire size reduction.

The `scale-info` crate is only needed at build time or when compressing a
`PortableRegistry` into the compact format. It is behind an optional `scale-info` feature
flag.

## From SCALE

`Value` wraps raw SCALE encoded bytes together with a type id and a reference to the
registry, producing an object that implements `serde::Serialize`.

```rust
let value = scales::Value::new(scale_bytes, type_id, &registry);
serde_json::to_string(&value)?;
```

`Value` also provides typed accessors for reading primitive fields, struct members,
sequences, tuples, and enum variants without full deserialization.

```rust
let amount = value.field("amount").and_then(|v| v.as_u128());
let first  = value.sequence_get(0);
let name   = value.variant_name();
```

## To SCALE

The `serializer` feature (enabled by default) provides functions to convert any
`serde::Serialize` type into SCALE bytes. When type information is supplied the serializer
coerces values to match the target type (e.g. JSON numbers to the correct integer width).

```rust
// simple conversion
let scale_bytes = scales::to_vec(some_serializable_input)?;

// with type info for coercion
let scale_bytes = scales::to_vec_with_info(input, Some((&registry, type_id)))?;

// from an unordered list of named fields
let fields = vec![("dest", dest_value), ("value", amount_value)];
let scale_bytes = scales::to_vec_from_iter(fields, (&registry, type_id))?;
```

## Features

| Feature      | Default | Description                                      |
|--------------|---------|--------------------------------------------------|
| `std`        | yes     | Enable standard library support                  |
| `serializer` | yes    | SCALE serializer (Rust/JSON to SCALE)            |
| `json`       | yes     | JSON support via `serde_json`                    |
| `hex`        | yes     | Hex string decoding for byte arrays              |
| `scale-info` | yes    | `PortableRegistry` compression from `scale-info` |
