//! Known-answer tests for key derivation and signing across every chain.
//!
//! These pin the observable outputs of the crypto stack (seeds via
//! PBKDF2-HMAC-SHA512, BIP32/SLIP-10 via HMAC-SHA512, sr25519, secp256k1
//! ECDSA, ed25519, Keccak-256) so dependency upgrades can't silently change
//! derived accounts.
//!
//! Substrate uses substrate-bip39 (seed from the mnemonic entropy); the other
//! chains use standard BIP39 (seed from the phrase) and are pinned to published
//! "abandon ×11 about" wallet vectors. Signatures are pinned only where signing
//! is deterministic (ed25519, RFC 6979 ECDSA); sr25519 signing is randomized.

use libwallet::{
    vault::{Simple, Vault},
    Bitcoin, Cosmos, Ethereum, Mnemonic, Signer, Solana, Substrate,
};

const PHRASE: &str = "bottom drive obey lake curtain smoke basket hold race lonely fit walk";
const ABANDON: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const MSG: &[u8] = b"libwallet golden vector";
/// secp256k1 signers sign a chain-specific prehash (keccak/sha256 digest).
const DIGEST: &[u8; 32] = b"libwallet golden vector digest!!";

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

fn keys() -> Simple<(), 16> {
    Simple::from_phrase(PHRASE).expect("valid phrase")
}

fn abandon() -> Simple<(), 16> {
    Simple::from_phrase(ABANDON).expect("valid phrase")
}

fn check(label: &str, actual: &str, expected: &str) {
    assert_eq!(actual, expected, "{label}");
}

#[test]
fn mnemonic_roundtrip() {
    let m: Mnemonic = PHRASE.parse().unwrap();
    // BIP39 English: this phrase encodes this 128-bit entropy (external vector).
    assert_eq!(hex(m.entropy()), "1a486a5fbe53639984cb64b070755f7b");
    assert_eq!(m.phrase(), PHRASE);
}

#[test]
fn substrate_seed_from_entropy() {
    let m: Mnemonic = PHRASE.parse().unwrap();
    let seed = libwallet::seed_from_entropy(m.entropy(), "");
    check("seed", &hex(*seed), "fac7959dbfe72f052e5a0c3c8d6530f202b02fd8f9f5ca3580ec8deb7797479e0b9f67282f42d1214e457243b9eb38a0a5ed13de66c01cfb213a6fe73cedef5a");
    let seed_pw = libwallet::seed_from_entropy(m.entropy(), "hunter2");
    check("seed(pw)", &hex(*seed_pw), "87c2da7b75b1caac6e5f78b4c979ac67e1aec40ce0210453fd54046b56aa6810b69c3f18a5e83825193d5a3e3bc505e7667c4c832412403c87adf6cdd044b3b4");
}

#[test]
fn substrate_sr25519() {
    smol::block_on(async {
        let mut v = Substrate::new(keys());
        // The well-known Substrate dev phrase: //Alice is 5GrwvaEF… (external vector).
        let alice = v.unlock(Some("//Alice"), ()).await.unwrap();
        check(
            "substrate //Alice",
            &alice.to_string(),
            "d43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d",
        );
        let root = v.unlock(None, ()).await.unwrap();
        check(
            "substrate root",
            &root.to_string(),
            "701f7cc645a26b6bc9c2c61beb25ac9c7564890eccf962fe4f16549b41b29b65",
        );
        let sig = root.sign_msg(MSG).await.unwrap();
        assert!(root.verify(MSG, &sig).await);
    });
}

#[test]
fn standard_bip39_seed() {
    // Official BIP39 test vectors (trezor/python-mnemonic vectors.json).
    let m: Mnemonic = ABANDON.parse().unwrap();
    check(
        "bip39 seed",
        &hex(*libwallet::bip39_seed(m.entropy(), "").unwrap()),
        "5eb00bbddcf069084889a8ab9155568165f5c453ccb85e70811aaed6f6da5fc19a5ac40b389cd370d086206dec8aa6c43daea6690f20ad3d8d48b2d2ce9e38e4",
    );
    check(
        "bip39 seed(TREZOR)",
        &hex(*libwallet::bip39_seed(m.entropy(), "TREZOR").unwrap()),
        "c55257c360c07c72029aebc1b53c05ed0362ada38ead3e3e9efa3708e53495531f09a6987599d18264c1e1c92f2cf141630c7a3c4ab7c81b2f001698e7463b04",
    );
    assert!(libwallet::bip39_seed(&[0u8; 15], "").is_none());
}

#[test]
fn ethereum_secp256k1() {
    smol::block_on(async {
        // m/44'/60'/0'/0/0 of "abandon ×11 about": address 0x9858EfFD232B4033E47d90003D41EC34EcaEda94,
        // as derived by MetaMask/ethers (external vector).
        let s = Ethereum::new(abandon()).unlock(None, ()).await.unwrap();
        check(
            "eth pub",
            &s.to_string(),
            "0237b0bb7a8288d38ed49a524b5dc98cff3eb5ca824c9f9dc0dfdb3d9cd600f299",
        );
        let sig = s.sign_msg(DIGEST).await.unwrap();
        check("eth sig", &hex(&sig), "e806d7145c6145c6c9cd273ad327827bd1be78765954daf36adcad0cdd109ffb6d1423d93212ef5e4b65dee741c95a6abe1fd631973f997f5ef96cb74eaad60b00");
        assert!(s.verify(DIGEST, &sig).await);
        assert!(!s.verify(MSG, &sig).await);
    });
}

#[test]
fn ethereum_address() {
    use k256::ecdsa::SigningKey;
    // Private key of "abandon ×11 about" at m/44'/60'/0'/0/0 (see bip32 tests).
    let sk = SigningKey::from_slice(&[
        0x1a, 0xb4, 0x2c, 0xc4, 0x12, 0xb6, 0x18, 0xbd, 0xea, 0x3a, 0x59, 0x9e, 0x3c, 0x9b, 0xae,
        0x19, 0x9e, 0xbf, 0x03, 0x08, 0x95, 0xb0, 0x39, 0xe9, 0xdb, 0x1e, 0x30, 0xda, 0xfb, 0x12,
        0xb7, 0x27,
    ])
    .unwrap();
    let addr = libwallet::chain::ethereum::eth_address(sk.verifying_key());
    assert_eq!(hex(addr), "9858effd232b4033e47d90003d41ec34ecaeda94");
}

#[test]
fn solana_ed25519() {
    smol::block_on(async {
        // m/44'/501'/0'/0' of "abandon ×11 about": address
        // HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk (external vector).
        let s = Solana::new(abandon()).unlock(None, ()).await.unwrap();
        check(
            "sol pub",
            &s.to_string(),
            "f036276246a75b9de3349ed42b15e232f6518fc20f5fcd4f1d64e81f9bd258f7",
        );
        let sig = s.sign_msg(MSG).await.unwrap();
        check("sol sig", &hex(&sig), "1e8423855dab106624e1d747ab1eacb3ed747802bde6ebd5353269738215b57d6d01e15f07bd4fb0afac8f363d808881f0eb527c330d953cbae1765e5eefb403");
        assert!(s.verify(MSG, &sig).await);
    });
}

#[test]
fn bitcoin_and_cosmos_secp256k1() {
    smol::block_on(async {
        // BIP44 m/44'/0'/0'/0/0: 1LqBGSKuX5yYUonjxT5qGfpUsXKYYWeabA (BIP44 vector).
        let mut btc = Bitcoin::new(abandon());
        let s = btc.unlock(None, ()).await.unwrap();
        check(
            "btc pub",
            &s.to_string(),
            "03aaeb52dd7494c361049de67cc680e83ebcbbbdbeb13637d92cd845f70308af5e",
        );
        // BIP84 m/84'/0'/0'/0/0: bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu (BIP84 spec vector).
        let s = btc
            .unlock(Some(libwallet::chain::bitcoin::SEGWIT_PATH), ())
            .await
            .unwrap();
        check(
            "btc segwit pub",
            &s.to_string(),
            "0330d54fd0dd420a6e5f8d3624f5f3482cae350f79d5f0753bf5beef9c2d91af3c",
        );

        // m/44'/118'/0'/0/0: cosmos19rl4cm2hmr8afy4kldpxz3fka4jguq0auqdal4 (cosmjs vector).
        let s = Cosmos::new(abandon()).unlock(None, ()).await.unwrap();
        check(
            "cosmos pub",
            &s.to_string(),
            "024f4e2ad99c34d60b9ba6283c9431a8418af8673212961f97a77b6377fcd05b62",
        );
        let sig = s.sign_msg(DIGEST).await.unwrap();
        check("cosmos sig", &hex(&sig), "93c4d795ab3d89587df2567c78204d34fe924a492f63d4713e55994f585d0c7b243f18029d6ce69db2d4d33e3cf633f6e49f277b7bac16185f35315fe16e0a3d01");
        assert!(s.verify(DIGEST, &sig).await);
    });
}

#[test]
fn generated_phrases_are_valid() {
    let (_, phrase) = Simple::<()>::generate_with_phrase(&mut rand_core::OsRng).unwrap();
    assert_eq!(phrase.phrase().split(' ').count(), 24);
    let reparsed: Mnemonic = phrase.phrase().parse().unwrap();
    assert_eq!(reparsed.entropy(), phrase.entropy());
}

/// Prehashes shorter than 32 bytes are zero-padded (RFC 6979 bits2field), as
/// ecdsa 0.16 did (verified byte-identical during the k256 0.14 upgrade).
/// Under 16 bytes is rejected.
#[test]
fn secp256k1_short_prehash() {
    smol::block_on(async {
        let s = Ethereum::new(abandon()).unlock(None, ()).await.unwrap();
        check("eth short", &hex(s.sign_msg(MSG).await.unwrap()), "9b44695125b1b3f04e61fa60421aacfb938aa86a3e1663012482d87bf1b5f54d3ac63b4413ed09c0f5e6c043622006c7ea99d70e107cce649ee4f9f536d66f3000");
        assert!(s.sign_msg(&MSG[..15]).await.is_err());
        let s = Cosmos::new(abandon()).unlock(None, ()).await.unwrap();
        check("cosmos short", &hex(s.sign_msg(MSG).await.unwrap()), "2479863209e82861c91d19e24ea00b02a3f278df25e48674ff3a84cf34ea7fba5240588ba08904686329daf39fd8f5140e08d51f602174e4a91c2f9a4cdb8a6a01");
    });
}
