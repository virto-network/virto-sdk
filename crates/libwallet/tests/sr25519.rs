//! The typed sr25519 pair, from outside the crate.

use libwallet::{sr25519, Pair, Signer};

#[test]
fn a_seed_gives_a_typed_pair_that_signs() {
    let pair = <sr25519::Pair as Pair>::from_bytes(&[7; 32]).expect("a 32-byte seed");
    let public: sr25519::Public = pair.public();
    // The account subkey derives from the seed 0x0707…07.
    assert_eq!(
        public,
        [
            124, 15, 70, 157, 59, 211, 64, 186, 231, 24, 32, 63, 163, 12, 160, 113, 165, 227, 124,
            117, 30, 137, 29, 189, 237, 131, 123, 33, 61, 69, 217, 29
        ]
    );
    smol::block_on(async {
        let signature: sr25519::Signature =
            Signer::sign_msg(&pair, b"a message").await.expect("signs");
        assert!(Signer::verify(&pair, b"a message", signature).await);
        assert!(!Signer::verify(&pair, b"another message", signature).await);
    });
    assert!(<sr25519::Pair as Pair>::from_bytes(&[7; 31]).is_none());
}
