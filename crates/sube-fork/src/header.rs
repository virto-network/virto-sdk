//! Block headers, decoded just far enough to rewrite them.

use codec::{Compact, Decode, Encode};

use crate::Hash;
use crate::util::blake2_256;

const DIGEST_PRE_RUNTIME: u8 = 6;

#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    pub parent: Hash,
    pub number: u32,
    pub state_root: Hash,
    pub extrinsics_root: Hash,
    /// The SCALE-encoded `Vec<DigestItem>`, kept as is.
    pub digest: Vec<u8>,
}

impl Header {
    /// A header for `Core_initialize_block`: the runtime fills in the roots,
    /// and the digest carries the slot for `engine` (e.g. `b"aura"`).
    pub fn pre_runtime(parent: Hash, number: u32, engine: &[u8; 4], slot: u64) -> Self {
        let mut digest = Compact(1u32).encode();
        digest.push(DIGEST_PRE_RUNTIME);
        digest.extend(engine);
        digest.extend(slot.encode().encode());
        Header {
            parent,
            number,
            state_root: [0; 32],
            extrinsics_root: [0; 32],
            digest,
        }
    }

    pub fn decode(input: &mut &[u8]) -> Result<Self, codec::Error> {
        let parent = Hash::decode(input)?;
        let number = Compact::<u32>::decode(input)?.0;
        let state_root = Hash::decode(input)?;
        let extrinsics_root = Hash::decode(input)?;
        let start = *input;
        for _ in 0..Compact::<u32>::decode(input)?.0 {
            match u8::decode(input)? {
                // PreRuntime, Consensus and Seal carry an engine id and a payload.
                4..=6 => {
                    <[u8; 4]>::decode(input)?;
                    skip_bytes(input)?;
                }
                // Other(Vec<u8>)
                0 => skip_bytes(input)?,
                // RuntimeEnvironmentUpdated
                8 => {}
                _ => return Err("unknown digest item".into()),
            }
        }
        let digest = start[..start.len() - input.len()].to_vec();
        Ok(Header {
            parent,
            number,
            state_root,
            extrinsics_root,
            digest,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut v = self.parent.to_vec();
        Compact(self.number).encode_to(&mut v);
        v.extend(self.state_root);
        v.extend(self.extrinsics_root);
        v.extend(&self.digest);
        v
    }

    pub fn hash(&self) -> Hash {
        blake2_256(&self.encode())
    }
}

/// Skip a SCALE `Vec<u8>` without copying it.
fn skip_bytes(input: &mut &[u8]) -> Result<(), codec::Error> {
    let len = Compact::<u32>::decode(input)?.0 as usize;
    *input = input.get(len..).ok_or("truncated bytes")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_keeps_the_digest() {
        let h = Header::pre_runtime([7; 32], 41_000_000, b"aura", 299_181_089);
        let encoded = h.encode();
        let decoded = Header::decode(&mut &encoded[..]).unwrap();
        assert_eq!(decoded, h);
        assert_eq!(decoded.hash(), blake2_256(&encoded));
    }
}
