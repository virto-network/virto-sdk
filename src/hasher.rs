use crate::meta_ext::Hasher;
use crate::prelude::*;
use sp_core::hashing::{blake2_128, blake2_256};
use core::hash::Hasher as _;

/// hashes and encodes the provided input with the specified hasher
pub fn hash<I: AsRef<[u8]>>(hasher: &Hasher, input: I) -> Vec<u8> {
    let mut input = input.as_ref();
    // input might be a hex encoded string
    let mut data = vec![];
    if input.starts_with(b"0x") {
        data.append(&mut hex::decode(&input[2..]).expect("hex string"));
        input = data.as_ref();
    };

    match hasher {
        Hasher::Blake2_128 => blake2_128(input).to_vec(),
        Hasher::Blake2_256 => blake2_256(input).to_vec(),
        Hasher::Blake2_128Concat => [blake2_128(input).as_slice(), input].concat(),
        Hasher::Twox128 => twox_hash(&input),
        Hasher::Twox256 => unimplemented!(),
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
    fn hash_blake_hex() {
        let out1 = hash(&Hasher::Blake2_128, "0x68656c6c6f");
        let out2 = hash(&Hasher::Blake2_128, hex!("68656c6c6f"));
        assert_eq!(out1, out2,);
    }
}
