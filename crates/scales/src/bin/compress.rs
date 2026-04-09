use codec::Decode;
use scale_info::PortableRegistry;
use scale_serialization::frame::compress;
use std::io::{self, Read, Write};

fn main() {
    let input = match std::env::args().nth(1) {
        Some(path) => std::fs::read(&path).unwrap_or_else(|e| {
            eprintln!("{path}: {e}");
            std::process::exit(1);
        }),
        None => {
            let mut buf = Vec::new();
            io::stdin().read_to_end(&mut buf).expect("read stdin");
            buf
        }
    };

    let portable = PortableRegistry::decode(&mut &input[..]).unwrap_or_else(|e| {
        eprintln!("failed to decode PortableRegistry: {e}");
        std::process::exit(1);
    });

    let types = compress::compress_to_types(&portable).unwrap_or_else(|e| {
        eprintln!("failed to compress registry: {e}");
        std::process::exit(1);
    });
    let encoded = serde_json::to_vec(&types).unwrap_or_else(|e| {
        eprintln!("failed to encode registry: {e}");
        std::process::exit(1);
    });

    eprintln!(
        "compressed {} types: {} -> {} bytes ({:.0}% reduction)",
        portable.types.len(),
        input.len(),
        encoded.len(),
        (1.0 - encoded.len() as f64 / input.len() as f64) * 100.0,
    );

    io::stdout().write_all(&encoded).expect("write stdout");
}
