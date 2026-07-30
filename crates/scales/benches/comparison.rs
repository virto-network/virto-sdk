use codec::{Decode, Encode};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use scale_info::{meta_type, PortableRegistry, Registry as SiRegistry, TypeInfo};
use scale_serialization::{frame::compress, Registry, Value};
use std::collections::BTreeMap;

// -- helpers --

fn make_portable<T: TypeInfo + 'static>() -> (u32, PortableRegistry) {
    let mut reg = SiRegistry::new();
    let sym = reg.register_type(&meta_type::<T>());
    (sym.id, reg.into())
}

fn make_compressed(portable: &PortableRegistry, id: u32) -> (u32, Registry) {
    (id, compress::compress(portable).expect("compress"))
}

// -- test types --

#[derive(Encode, Decode, TypeInfo, serde::Serialize, Clone)]
struct Transfer {
    from: [u8; 32],
    to: [u8; 32],
    amount: u128,
    nonce: u64,
    memo: String,
}

#[derive(Encode, Decode, TypeInfo, serde::Serialize, Clone)]
enum RuntimeCall {
    Transfer(Transfer),
    Stake { validator: [u8; 32], amount: u128 },
    Vote(u32),
}

#[derive(Encode, Decode, TypeInfo, serde::Serialize, Clone)]
struct Block {
    number: u64,
    parent_hash: [u8; 32],
    extrinsics: Vec<RuntimeCall>,
}

fn sample_transfer() -> Transfer {
    Transfer {
        from: [1u8; 32],
        to: [2u8; 32],
        amount: 1_000_000_000_000,
        nonce: 42,
        memo: "benchmark transfer".into(),
    }
}

fn sample_block() -> Block {
    Block {
        number: 12345,
        parent_hash: [0xAB; 32],
        extrinsics: vec![
            RuntimeCall::Transfer(sample_transfer()),
            RuntimeCall::Stake {
                validator: [3u8; 32],
                amount: 500_000,
            },
            RuntimeCall::Vote(7),
            RuntimeCall::Transfer(Transfer {
                from: [4u8; 32],
                to: [5u8; 32],
                amount: 999,
                nonce: 100,
                memo: "another one".into(),
            }),
        ],
    }
}

// ==================== Encoding Benchmarks ====================

fn bench_encode_primitives(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode/primitives");

    let val = 0xDEAD_BEEF_u32;
    group.bench_function("parity-scale-codec", |b| b.iter(|| black_box(val).encode()));
    group.bench_function("scales (serde)", |b| {
        b.iter(|| scale_serialization::to_vec(black_box(&val)).unwrap())
    });

    group.finish();
}

fn bench_encode_struct(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode/transfer_struct");
    let transfer = sample_transfer();
    let data = transfer.encode();

    group.bench_function("parity-scale-codec", |b| {
        b.iter(|| black_box(&transfer).encode())
    });
    group.bench_function("scales (serde)", |b| {
        b.iter(|| scale_serialization::to_vec(black_box(&transfer)).unwrap())
    });

    // Encode from JSON with type info (the dynamic path)
    let (id, portable) = make_portable::<Transfer>();
    let (id, registry) = make_compressed(&portable, id);
    let json: serde_json::Value = serde_json::to_value(&transfer).unwrap();
    group.bench_function("scales (json + type_info)", |b| {
        b.iter(|| {
            scale_serialization::to_vec_with_info(black_box(&json), Some((&registry, id))).unwrap()
        })
    });

    // scale-value encode
    let sv_value: scale_value::Value<u32> =
        scale_value::scale::decode_as_type(&mut &data[..], id, &portable).unwrap();
    group.bench_function("scale-value (encode)", |b| {
        b.iter(|| {
            let mut out = Vec::new();
            scale_value::scale::encode_as_type(black_box(&sv_value), id, &portable, &mut out)
                .unwrap();
            black_box(out);
        })
    });

    group.finish();
}

fn bench_encode_block(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode/block");
    let block = sample_block();
    let data = block.encode();

    group.bench_function("parity-scale-codec", |b| {
        b.iter(|| black_box(&block).encode())
    });
    group.bench_function("scales (serde)", |b| {
        b.iter(|| scale_serialization::to_vec(black_box(&block)).unwrap())
    });

    let (id, portable) = make_portable::<Block>();
    let (id, registry) = make_compressed(&portable, id);
    let json: serde_json::Value = serde_json::to_value(&block).unwrap();
    group.bench_function("scales (json + type_info)", |b| {
        b.iter(|| {
            scale_serialization::to_vec_with_info(black_box(&json), Some((&registry, id))).unwrap()
        })
    });

    let sv_value: scale_value::Value<u32> =
        scale_value::scale::decode_as_type(&mut &data[..], id, &portable).unwrap();
    group.bench_function("scale-value (encode)", |b| {
        b.iter(|| {
            let mut out = Vec::new();
            scale_value::scale::encode_as_type(black_box(&sv_value), id, &portable, &mut out)
                .unwrap();
            black_box(out);
        })
    });

    group.finish();
}

// ==================== Decoding Benchmarks ====================

fn bench_decode_primitives(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode/primitives");

    let data = 0xDEAD_BEEF_u32.encode();
    group.bench_function("parity-scale-codec", |b| {
        b.iter(|| u32::decode(&mut black_box(&data[..])).unwrap())
    });

    let (id, portable) = make_portable::<u32>();
    let (id, registry) = make_compressed(&portable, id);
    group.bench_function("scales::Value (zero-copy)", |b| {
        b.iter(|| {
            let v = Value::new(black_box(&data), id, &registry);
            black_box(v.as_u32().unwrap());
        })
    });

    group.bench_function("scale-value", |b| {
        b.iter(|| {
            scale_value::scale::decode_as_type(&mut black_box(&data[..]), id, &portable).unwrap()
        })
    });

    group.finish();
}

fn bench_decode_struct(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode/transfer_struct");
    let transfer = sample_transfer();
    let data = transfer.encode();

    group.bench_function("parity-scale-codec", |b| {
        b.iter(|| Transfer::decode(&mut black_box(&data[..])).unwrap())
    });

    let (id, portable) = make_portable::<Transfer>();
    let (id, registry) = make_compressed(&portable, id);

    // Zero-copy field access (by name, re-walks from start each time)
    group.bench_function("scales::Value (field by name)", |b| {
        b.iter(|| {
            let v = Value::new(black_box(&data), id, &registry);
            black_box(v.field("amount").unwrap().as_u128().unwrap());
            black_box(v.field("nonce").unwrap().as_u64().unwrap());
            black_box(v.field("memo").unwrap().as_str().unwrap());
        })
    });

    // Sequential field iteration (cursor-based, O(n))
    group.bench_function("scales::Value (fields_iter)", |b| {
        b.iter(|| {
            let v = Value::new(black_box(&data), id, &registry);
            for (name, val) in v.fields_iter().unwrap() {
                match name {
                    "amount" => {
                        black_box(val.as_u128());
                    }
                    "nonce" => {
                        black_box(val.as_u64());
                    }
                    "memo" => {
                        black_box(val.as_str());
                    }
                    _ => {
                        black_box(&val);
                    }
                }
            }
        })
    });

    // Full decode to JSON
    group.bench_function("scales::Value (to json)", |b| {
        b.iter(|| {
            let v = Value::new(black_box(&data), id, &registry);
            black_box(serde_json::to_value(&v).unwrap());
        })
    });

    group.bench_function("scale-value (decode)", |b| {
        b.iter(|| {
            scale_value::scale::decode_as_type(&mut black_box(&data[..]), id, &portable).unwrap()
        })
    });

    group.bench_function("scale-value (decode + to json)", |b| {
        b.iter(|| {
            let v = scale_value::scale::decode_as_type(&mut black_box(&data[..]), id, &portable)
                .unwrap();
            black_box(serde_json::to_value(&v).unwrap());
        })
    });

    group.finish();
}

fn bench_decode_block(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode/block");
    let block = sample_block();
    let data = block.encode();

    group.bench_function("parity-scale-codec", |b| {
        b.iter(|| Block::decode(&mut black_box(&data[..])).unwrap())
    });

    let (id, portable) = make_portable::<Block>();
    let (id, registry) = make_compressed(&portable, id);

    group.bench_function("scales::Value (to json)", |b| {
        b.iter(|| {
            let v = Value::new(black_box(&data), id, &registry);
            black_box(serde_json::to_value(&v).unwrap());
        })
    });

    group.bench_function("scale-value (decode)", |b| {
        b.iter(|| {
            scale_value::scale::decode_as_type(&mut black_box(&data[..]), id, &portable).unwrap()
        })
    });

    group.bench_function("scale-value (decode + to json)", |b| {
        b.iter(|| {
            let v = scale_value::scale::decode_as_type(&mut black_box(&data[..]), id, &portable)
                .unwrap();
            black_box(serde_json::to_value(&v).unwrap());
        })
    });

    group.finish();
}

// ==================== Memory / Size Benchmarks ====================

fn bench_registry_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("registry/compress");

    let raw = include_bytes!("../src/registry.bin");
    let portable = PortableRegistry::decode(&mut &raw[..]).unwrap();

    group.bench_function("compress PortableRegistry", |b| {
        b.iter(|| compress::compress(black_box(&portable)).expect("compress"))
    });

    group.finish();

    // Print size comparison as a one-off
    let compressed = compress::compress(&portable).expect("compress");
    let portable_size = portable.encode().len();

    // Estimate compressed wire size
    let mut compressed_wire = 0usize;
    let num_types = portable.types.len();
    for i in 0..num_types {
        if let Some(ty) = compressed.resolve(i as u32) {
            compressed_wire += 1; // discriminant
            match ty {
                scale_serialization::TypeDef::Struct(fields) => {
                    compressed_wire += 1;
                    for f in fields {
                        compressed_wire += 1 + f.name.len() + 4;
                    }
                }
                scale_serialization::TypeDef::Variant(vdef) => {
                    compressed_wire += 1 + vdef.name().len() + 1;
                    for v in vdef.variants() {
                        compressed_wire += 1 + 1 + v.name().len() + 1;
                        match v.fields() {
                            scale_serialization::registry::Fields::Unit => {}
                            scale_serialization::registry::Fields::NewType(_) => {
                                compressed_wire += 4;
                            }
                            scale_serialization::registry::Fields::Tuple(ids) => {
                                compressed_wire += 1 + ids.len() * 4;
                            }
                            scale_serialization::registry::Fields::Struct(fields) => {
                                compressed_wire += 1;
                                for f in fields {
                                    compressed_wire += 1 + f.name.len() + 4;
                                }
                            }
                        }
                    }
                }
                scale_serialization::TypeDef::Tuple(ids)
                | scale_serialization::TypeDef::StructTuple(ids) => {
                    compressed_wire += 1 + ids.len() * 4;
                }
                scale_serialization::TypeDef::Sequence(_)
                | scale_serialization::TypeDef::StructNewType(_)
                | scale_serialization::TypeDef::Compact(_) => {
                    compressed_wire += 4;
                }
                scale_serialization::TypeDef::Map(_, _)
                | scale_serialization::TypeDef::BitSequence(_, _)
                | scale_serialization::TypeDef::Array(_, _) => {
                    compressed_wire += 8;
                }
                _ => {}
            }
        }
    }

    eprintln!("\n=== Registry Size (real Substrate metadata) ===");
    eprintln!("  PortableRegistry: {portable_size:>8} bytes");
    eprintln!("  Compressed:       {compressed_wire:>8} bytes");
    eprintln!(
        "  Reduction:        {:>7.1}%",
        (1.0 - compressed_wire as f64 / portable_size as f64) * 100.0
    );
    eprintln!("  Types:            {num_types:>8}");
}

fn bench_value_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/value_size");

    // Compare the size of Value (scales) vs Value (scale-value)
    let transfer = sample_transfer();
    let data = transfer.encode();

    let (id, portable) = make_portable::<Transfer>();
    let (id, registry) = make_compressed(&portable, id);

    // scales::Value is just a pointer triple — measure its stack size
    let scales_value_size = std::mem::size_of::<Value>();
    let scale_value_size = std::mem::size_of::<scale_value::Value<()>>();

    eprintln!("\n=== Value Type Sizes ===");
    eprintln!("  scales::Value (stack):      {scales_value_size:>4} bytes (borrows data, zero heap alloc)");
    eprintln!("  scale_value::Value (stack):  {scale_value_size:>4} bytes (+ heap allocs for decoded tree)");
    eprintln!("  SCALE encoded data:         {:>4} bytes", data.len());

    // Benchmark: how fast can we create + access a value
    group.bench_function("scales::Value create+access", |b| {
        b.iter(|| {
            let v = Value::new(black_box(&data), id, &registry);
            black_box(v.field("amount").unwrap().as_u128().unwrap());
        })
    });

    group.bench_function("scale-value decode+access", |b| {
        b.iter(|| {
            let v = scale_value::scale::decode_as_type(&mut black_box(&data[..]), id, &portable)
                .unwrap();
            // Access the amount field
            match &v.value {
                scale_value::ValueDef::Composite(scale_value::Composite::Named(fields)) => {
                    black_box(&fields[2].1);
                }
                _ => panic!("expected composite"),
            }
        })
    });

    group.finish();
}

// ==================== Sequence Iteration ====================

fn bench_sequence_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode/sequence_u32x1000");

    let input: Vec<u32> = (0..1000).collect();
    let data = input.encode();

    group.bench_function("parity-scale-codec", |b| {
        b.iter(|| Vec::<u32>::decode(&mut black_box(&data[..])).unwrap())
    });

    let (id, portable) = make_portable::<Vec<u32>>();
    let (id, registry) = make_compressed(&portable, id);

    group.bench_function("scales::Value (sequence_get O(n²))", |b| {
        b.iter(|| {
            let v = Value::new(black_box(&data), id, &registry);
            let len = v.sequence_len().unwrap();
            let mut sum = 0u64;
            for i in 0..len {
                sum += v.sequence_get(i).unwrap().as_u32().unwrap() as u64;
            }
            black_box(sum);
        })
    });

    group.bench_function("scales::Value (sequence_iter O(n))", |b| {
        b.iter(|| {
            let v = Value::new(black_box(&data), id, &registry);
            let mut sum = 0u64;
            for elem in v.sequence_iter().unwrap() {
                sum += elem.as_u32().unwrap() as u64;
            }
            black_box(sum);
        })
    });

    group.bench_function("scale-value", |b| {
        b.iter(|| {
            scale_value::scale::decode_as_type(&mut black_box(&data[..]), id, &portable).unwrap()
        })
    });

    group.finish();
}

fn bench_map_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode/map_50_entries");

    let mut input = BTreeMap::<String, u64>::new();
    for i in 0..50 {
        input.insert(format!("key_{i:03}"), i as u64 * 1000);
    }
    let data = input.encode();

    group.bench_function("parity-scale-codec", |b| {
        b.iter(|| BTreeMap::<String, u64>::decode(&mut black_box(&data[..])).unwrap())
    });

    let (id, portable) = make_portable::<BTreeMap<String, u64>>();
    let (id, registry) = make_compressed(&portable, id);

    group.bench_function("scales::Value (to json)", |b| {
        b.iter(|| {
            let v = Value::new(black_box(&data), id, &registry);
            black_box(serde_json::to_value(&v).unwrap());
        })
    });

    group.bench_function("scale-value (decode + to json)", |b| {
        b.iter(|| {
            let v = scale_value::scale::decode_as_type(&mut black_box(&data[..]), id, &portable)
                .unwrap();
            black_box(serde_json::to_value(&v).unwrap());
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_encode_primitives,
    bench_encode_struct,
    bench_encode_block,
    bench_decode_primitives,
    bench_decode_struct,
    bench_decode_block,
    bench_registry_size,
    bench_value_size,
    bench_sequence_decode,
    bench_map_decode,
);
criterion_main!(benches);
