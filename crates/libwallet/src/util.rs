#[cfg(feature = "rand")]
pub fn random_bytes<R, const S: usize>(rng: &mut R) -> [u8; S]
where
    R: rand_core::CryptoRng + rand_core::RngCore,
{
    let mut bytes = [0u8; S];
    rng.fill_bytes(&mut bytes);
    bytes
}

#[cfg(all(feature = "rand", feature = "mnemonic"))]
pub fn gen_phrase<R>(rng: &mut R, lang: crate::Language) -> crate::AnyMnemonic
where
    R: rand_core::CryptoRng + rand_core::RngCore,
{
    let entropy = zeroize::Zeroizing::new(random_bytes::<_, 32>(rng));
    crate::AnyMnemonic::from_entropy(lang, entropy.as_slice()).expect("seed valid")
}
