//! Benchmarks for metadata parsing and SCALE decoding.
//!
//! Run with: cargo bench --features json,text

use std::hint::black_box;
use std::time::Instant;

use sube::{Metadata, StorageEntry};

const META_BYTES: &[u8] = include_bytes!("../tests/fixtures/kreivo.scale");
const ITERATIONS: u32 = 100;

fn bench<F: FnMut()>(name: &str, mut f: F) {
    // Warm up
    for _ in 0..3 {
        f();
    }
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        f();
    }
    let elapsed = start.elapsed();
    let per_iter = elapsed / ITERATIONS;
    println!("{name}: {per_iter:?} per iteration ({ITERATIONS} iterations)");
}

fn main() {
    println!("sube benchmarks (metadata: {} bytes)\n", META_BYTES.len());

    // Parse metadata from SCALE bytes
    bench("metadata_parse", || {
        let _ = black_box(Metadata::from_bytes(META_BYTES).unwrap());
    });

    let meta = Metadata::from_bytes(META_BYTES).unwrap();

    // Look up a pallet by name
    bench("pallet_lookup", || {
        let _ = black_box(meta.pallet_by_name("System"));
    });

    // Decode a constant (System::Version)
    let system = meta.pallet_by_name("System").unwrap();
    let version = system
        .constants
        .iter()
        .find(|c| c.name == "Version")
        .unwrap();
    let entry = StorageEntry::new(version.value.clone(), version.ty);

    bench("constant_decode_json", || {
        let _ = black_box(entry.to_json(&meta.registry).unwrap());
    });

    bench("constant_decode_text", || {
        let _ = black_box(entry.to_text(&meta.registry).unwrap());
    });

    // Memory analysis
    println!("\n--- Memory Analysis ---");
    println!("pallets: {}", meta.pallets.len());
    println!("SCALE input: {} bytes", META_BYTES.len());

    // Registry type count
    let mut type_count = 0u32;
    loop {
        if meta.registry.resolve(type_count).is_none() { break; }
        type_count += 1;
    }
    println!("registry types: {type_count}");

    // Estimate registry memory: enum discriminants + inline data + heap strings/vecs
    let mut string_bytes = 0usize;
    let mut vec_overhead = 0usize;
    let mut enum_overhead = type_count as usize * std::mem::size_of::<sube::scales::TypeDef>();
    for i in 0..type_count {
        if let Some(td) = meta.registry.resolve(i) {
            match td {
                sube::scales::TypeDef::Struct(fields) => {
                    for f in fields {
                        string_bytes += f.name.len();
                    }
                    vec_overhead += fields.len() * std::mem::size_of::<sube::scales::registry::Field>();
                }
                sube::scales::TypeDef::Variant(vdef) => {
                    string_bytes += vdef.name.len();
                    for v in &vdef.variants {
                        string_bytes += v.name.len();
                        match &v.fields {
                            sube::scales::registry::Fields::Struct(fields) => {
                                for f in fields {
                                    string_bytes += f.name.len();
                                }
                                vec_overhead += fields.len() * std::mem::size_of::<sube::scales::registry::Field>();
                            }
                            sube::scales::registry::Fields::Tuple(ids) => {
                                vec_overhead += ids.len() * 4;
                            }
                            _ => {}
                        }
                    }
                    vec_overhead += vdef.variants.len() * std::mem::size_of::<sube::scales::registry::Variant>();
                }
                sube::scales::TypeDef::Tuple(ids) | sube::scales::TypeDef::StructTuple(ids) => {
                    vec_overhead += ids.len() * 4;
                }
                _ => {}
            }
        }
    }

    println!("enum_overhead (TypeDef array): {} bytes", enum_overhead);
    println!("string_bytes (field/variant names): {} bytes", string_bytes);
    println!("vec_overhead (nested vecs): {} bytes", vec_overhead);
    println!("estimated total registry: {} bytes", enum_overhead + string_bytes + vec_overhead);

    // Pallets
    let pallets_size: usize = meta.pallets.iter().map(|p| {
        p.name.len()
            + p.storage.as_ref().map(|s| {
                s.entries.iter().map(|e| e.name.len() + 64).sum::<usize>()
            }).unwrap_or(0)
            + p.constants.iter().map(|c| c.name.len() + c.value.len() + 16).sum::<usize>()
            + 64 // struct overhead
    }).sum();
    println!("pallets (estimated): {} bytes", pallets_size);
    println!("estimated total metadata: {} bytes", enum_overhead + string_bytes + vec_overhead + pallets_size);

    // Filtered metadata benchmark
    println!("\n--- Filtered Metadata (System + Balances only) ---");
    let filtered = Metadata::from_bytes_filtered(META_BYTES, &["Balances"]).unwrap();

    let mut f_type_count = 0u32;
    loop {
        if filtered.registry.resolve(f_type_count).is_none() { break; }
        f_type_count += 1;
    }
    println!("pallets: {}", filtered.pallets.len());
    println!("registry types: {f_type_count} (was {type_count})");

    let mut f_string_bytes = 0usize;
    let mut f_vec_overhead = 0usize;
    let f_enum_overhead = f_type_count as usize * std::mem::size_of::<sube::scales::TypeDef>();
    for i in 0..f_type_count {
        if let Some(td) = filtered.registry.resolve(i) {
            match td {
                sube::scales::TypeDef::Struct(fields) => {
                    for f in fields { f_string_bytes += f.name.len(); }
                    f_vec_overhead += fields.len() * std::mem::size_of::<sube::scales::registry::Field>();
                }
                sube::scales::TypeDef::Variant(vdef) => {
                    f_string_bytes += vdef.name.len();
                    for v in &vdef.variants {
                        f_string_bytes += v.name.len();
                        match &v.fields {
                            sube::scales::registry::Fields::Struct(fields) => {
                                for f in fields { f_string_bytes += f.name.len(); }
                                f_vec_overhead += fields.len() * std::mem::size_of::<sube::scales::registry::Field>();
                            }
                            sube::scales::registry::Fields::Tuple(ids) => {
                                f_vec_overhead += ids.len() * 4;
                            }
                            _ => {}
                        }
                    }
                    f_vec_overhead += vdef.variants.len() * std::mem::size_of::<sube::scales::registry::Variant>();
                }
                sube::scales::TypeDef::Tuple(ids) | sube::scales::TypeDef::StructTuple(ids) => {
                    f_vec_overhead += ids.len() * 4;
                }
                _ => {}
            }
        }
    }
    let f_total = f_enum_overhead + f_string_bytes + f_vec_overhead;
    println!("estimated registry: {} bytes (was {} bytes, {:.0}% reduction)",
        f_total, enum_overhead + string_bytes + vec_overhead,
        (1.0 - f_total as f64 / (enum_overhead + string_bytes + vec_overhead) as f64) * 100.0);
}
