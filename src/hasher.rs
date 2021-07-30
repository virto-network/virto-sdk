use crate::meta_ext::Hasher;
use blake2::{Blake2b, Digest};
use core::hash::Hasher as _;

/// hashes and encodes the provided input with the specified hasher
pub fn hash(hasher: &Hasher, input: &str) -> Vec<u8> {
    let input = if input.starts_with("0x") {
        hex::decode(&input[2..]).unwrap_or_else(|_| input.into())
    } else {
        input.into()
    };

    match hasher {
        Hasher::Blake2_128 => Blake2b::digest(&input).as_slice().to_owned(),
        Hasher::Blake2_256 => unimplemented!(),
        Hasher::Blake2_128Concat => blake2_concat(&input),
        Hasher::Twox128 => twox_hash(&input),
        Hasher::Twox256 => unimplemented!(),
        Hasher::Twox64Concat => twox_hash_concat(&input),
        Hasher::Identity => input.into(),
    }
}

fn blake2_concat(input: &[u8]) -> Vec<u8> {
    [Blake2b::digest(input).as_slice(), input].concat()
}

fn twox_hash_concat(input: &[u8]) -> Vec<u8> {
    let mut dest = [0; 8];
    let mut h = twox_hash::XxHash64::with_seed(0);

    h.write(input);
    let r = h.finish();
    use byteorder::{ByteOrder, LittleEndian};
    LittleEndian::write_u64(&mut dest, r);

    [&dest[..], input].concat()
}

fn twox_hash(input: &[u8]) -> Vec<u8> {
    let mut dest: [u8; 16] = [0; 16];

    let mut h0 = twox_hash::XxHash64::with_seed(0);
    let mut h1 = twox_hash::XxHash64::with_seed(1);
    h0.write(input);
    h1.write(input);
    let r0 = h0.finish();
    let r1 = h1.finish();
    use byteorder::{ByteOrder, LittleEndian};
    LittleEndian::write_u64(&mut dest[0..8], r0);
    LittleEndian::write_u64(&mut dest[8..16], r1);

    dest.into()
}
