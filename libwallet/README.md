# Libwallet

A lightweight and very portable library with simple to understand and use
abstractions that allow creating chain-agnostic crypto wallets able to run in
all kinds of environments like native applications, hosted wallets, the browser
or even embedded hardware.

Core principles:

- **Ease of use**  
  A high level public API abstracts blockchain and cryptography heavy concepts.
  i.e. `Account` abstracts handling of private keys and their metadata. 
  `Vault` makes it simple to support multiple types of credentials and
  back-ends that store private keys in different ways, whether it is a
  cryptographic hardware module, a database or a file system.  
  In the future this abstractions could be used to integrate with different
  ledger-like back-ends including regular bank accounts.

- **Security**  
  Written in safe Rust and based on of production ready crypto primitives. Also
  encourages good practices like the use of pin protected sub-accounts derived
  from the root account as a form of second factor authentication or a two step
  signing process where transactions are added to a queue for review before
  being signed.

- **Multi-chain and extensibility**  
  No assumptions are made about what the kind of private keys and signatures
  different vaults use to be able to support any chain's cryptography.  
  Core functionality of wallets and accounts can be extended to support chain
  specific features. There is initial focus on supporting [Substrate][1] based
  chains adding features like formatting addresses using their network prefix
  or use metadata to validate what's being signed and simplify the creation of
  signed extrinsics.
   
- **Portability and small footprint**  
  Being `no_std` and WASM friendly users can create wallets for the wide range
  of platforms and computer architectures supported by Rust. Dependencies are
  kept to a minimum and any extra non-core functionality is set behind feature
  flags that users can enable only when needed.
  
[1]: https://substrate.io/

## Use in Virto

libwallet is developed by the Virto Network team, a parachain focused in
*decentralized commerce* infrastructure for the Polkadot ecosystem, Virto aims
to bridge real world products/services and the common folk to the growing but
not so user friendly world of interconnected blockchains.  
This library will be used as the core component of the **Wallet API**, one of
Virto's *decentralizable*, composable and easy to use [HTTP APIs][2] that run
as plugins of the [Valor][3] runtime.
`SummaVault` will be our main vault implementation that integrates the Matrix
protocol allowing users to sign-up to a homeserver using familiar credentials
and have out of the box key management, multi-device support and encrypted
account backup among others.

[2]: https://github.com/virto-network/virto-apis
[3]: https://github.com/virto-network/valor
