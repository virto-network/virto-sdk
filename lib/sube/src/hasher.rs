use crate::metadata::Hasher;
use crate::prelude::*;
use blake2::{
    digest::{
        typenum::{U16, U32},
        Output,
    },
    Blake2b, Digest,
};
use core::hash::Hasher as _;

/// hashes and encodes the provided input with the specified hasher
pub fn hash<I: AsRef<[u8]>>(hasher: &Hasher, input: I) -> Vec<u8> {
    let mut input = input.as_ref();
    // input might be a hex encoded string
    let mut data = vec![];
    if input.starts_with(b"0x") {
        if let Ok(mut decoded) = hex::decode(&input[2..]) {
            data.append(&mut decoded);
        }
        // if hex decode fails, just use the raw input
        input = data.as_ref();
    };

    #[inline]
    fn digest<T: Digest>(input: &[u8]) -> Output<T> {
        let mut hasher = T::new();
        hasher.update(input);
        hasher.finalize()
    }

    match hasher {
        Hasher::Blake2_128 => digest::<Blake2b<U16>>(input).to_vec(),
        Hasher::Blake2_256 => digest::<Blake2b<U32>>(input).to_vec(),
        Hasher::Blake2_128Concat => [digest::<Blake2b<U16>>(input).as_slice(), input].concat(),
        Hasher::Twox128 => twox_hash(input),
        Hasher::Twox256 => twox256_hash(input),
        Hasher::Twox64Concat => twox_hash_concat(input),
        Hasher::Identity => input.into(),
    }
}

fn twox_hash_concat(input: &[u8]) -> Vec<u8> {
    let mut dest = [0; 8];
    let mut h = twox_hash::XxHash64::with_seed(0);

    h.write(input);
    let r = h.finish();
    dest.copy_from_slice(&r.to_le_bytes());
    [dest.as_ref(), input].concat()
}

fn twox256_hash(input: &[u8]) -> Vec<u8> {
    let mut dest = [0u8; 32];
    for (i, chunk) in dest.chunks_exact_mut(8).enumerate() {
        let mut h = twox_hash::XxHash64::with_seed(i as u64);
        h.write(input);
        chunk.copy_from_slice(&h.finish().to_le_bytes());
    }
    dest.into()
}

fn twox_hash(input: &[u8]) -> Vec<u8> {
    let mut dest: [u8; 16] = [0; 16];

    let mut h0 = twox_hash::XxHash64::with_seed(0);
    let mut h1 = twox_hash::XxHash64::with_seed(1);
    h0.write(input);
    h1.write(input);
    let r0 = h0.finish();
    let r1 = h1.finish();

    let (first, last) = dest.split_at_mut(8);
    first.copy_from_slice(&r0.to_le_bytes());
    last.copy_from_slice(&r1.to_le_bytes());
    dest.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn hash_blake128_hex_vs_raw() {
        let out1 = hash(&Hasher::Blake2_128, "0x68656c6c6f");
        let out2 = hash(&Hasher::Blake2_128, hex!("68656c6c6f"));
        assert_eq!(out1, out2);
        assert_eq!(out1.len(), 16);
    }

    #[test]
    fn hash_blake256() {
        let out = hash(&Hasher::Blake2_256, b"hello");
        assert_eq!(out.len(), 32);
    }

    #[test]
    fn hash_blake128_concat_includes_input() {
        let input = b"hello";
        let out = hash(&Hasher::Blake2_128Concat, input);
        // 16 bytes hash + original input
        assert_eq!(out.len(), 16 + input.len());
        assert_eq!(&out[16..], input);
    }

    #[test]
    fn hash_twox128() {
        let out = hash(&Hasher::Twox128, b"System");
        assert_eq!(out.len(), 16);
        // Well-known twox128 hash of "System"
        assert_eq!(out, hex!("26aa394eea5630e07c48ae0c9558cef7"));
    }

    #[test]
    fn hash_twox64_concat_includes_input() {
        let input = b"hello";
        let out = hash(&Hasher::Twox64Concat, input);
        // 8 bytes hash + original input
        assert_eq!(out.len(), 8 + input.len());
        assert_eq!(&out[8..], input);
    }

    #[test]
    fn hash_identity_passthrough() {
        let input = b"unchanged";
        let out = hash(&Hasher::Identity, input);
        assert_eq!(out, input);
    }

    #[test]
    fn hash_twox256() {
        let out = hash(&Hasher::Twox256, b"hello");
        assert_eq!(out.len(), 32);
        // First 16 bytes should match twox128
        let twox128 = hash(&Hasher::Twox128, b"hello");
        assert_eq!(&out[..16], &twox128[..]);
    }
}
