# Changelog

## Unreleased

### Breaking changes

- **Ethereum, Bitcoin, Solana and Cosmos accounts change.** These vaults now
  derive their seed with standard BIP39 (PBKDF2 over the mnemonic *phrase*), so
  a phrase produces the same accounts as MetaMask, Ledger, Phantom and other
  BIP39 wallets. They previously used the Substrate seed (PBKDF2 over the
  mnemonic *entropy*), so accounts derived by earlier versions will not match.
  To recover funds held by an old account, derive it with
  `seed_from_entropy` and the chain's BIP32/SLIP-10 path. Substrate accounts are
  unchanged.
- The non-Substrate chain features (`ethereum`, `bitcoin`, `solana`, `cosmos`)
  now enable `mnemonic`.
- `Language` is now `bip0039::BuiltInLanguage` and no longer implements
  `Default`; use `Language::English`.
- `util::gen_phrase` returns `AnyMnemonic`. `Mnemonic` is the English mnemonic
  type, as before.
- secp256k1 `verify` treats the message as a prehash, matching `sign_msg`.
  Previously it hashed the message with SHA-256 first, so it rejected
  signatures produced by `sign_msg`.

### Added

- `bip39_seed` derives the standard BIP39 seed from mnemonic entropy.
- Known-answer tests (`tests/golden_vectors.rs`) pinning seeds, keys and
  deterministic signatures for every chain.

### Maintenance

- Dependencies updated: `k256` 0.14, `ed25519-dalek` 3, `hmac` 0.13,
  `pbkdf2` 0.13, `sha2` 0.11, `sha3` 0.12, `bip0039` 0.14, `keyring` 4,
  `prs-lib` 0.5.
- `keyring` 4 finds credentials stored by `keyring` 1 (verified against the
  Linux Secret Service). `OSKeyring::new` stays infallible: a credential store
  that fails to initialize is reported as `Error::Keyring` on first use.
- secp256k1 signing still rejects prehashes shorter than 16 bytes.
