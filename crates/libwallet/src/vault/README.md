# Vaults

To support a wide variety of platforms `libwallet` has the concept of a vault,
an abstraction used to retreive the private keys used for signing.

## Backends

### Simple

An in memmory key storage that will forget keys at the end of a program's
execution. It's useful for tests and generating addresses.

### OS Keyring

A cross platform storage that uses the operating system's default keyring to
store the secret seed used to generate accounts. Useful for desktop wallets.

### Pass

A cross platform secret vault storage that uses pass-like implementation (using
GPG as backend) to encrypt the secret seed used to generate accounts. Requires
`gnupg` or `gpgme` as dependencies.
