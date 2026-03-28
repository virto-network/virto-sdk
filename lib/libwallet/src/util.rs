#[cfg(feature = "rand")]
pub fn random_bytes<R, const S: usize>(rng: &mut R) -> [u8; S]
where
    R: rand_core::CryptoRng + rand_core::RngCore,
{
    let mut bytes = [0u8; S];
    rng.fill_bytes(&mut bytes);
    bytes
}

#[cfg(feature = "rand")]
pub fn gen_phrase<R>(rng: &mut R, lang: mnemonic::Language) -> mnemonic::Mnemonic
where
    R: rand_core::CryptoRng + rand_core::RngCore,
{
    let seed = random_bytes::<_, 32>(rng);
    mnemonic::Mnemonic::from_entropy_in(lang, seed.as_ref()).expect("seed valid")
}
