# Sube CLI

An unpublished workspace tool for querying and interactively exploring
Substrate-compatible chains with `sube`.

```sh
# One query
cargo run -p sube-cli -- --chain wss://kreivo.io system/number
cargo run -p sube-cli -- --chain wss://kreivo.io query system/number

# Watch a path
cargo run -p sube-cli -- --chain wss://kreivo.io --watch system/number

# Validate a call and print its call hex (never submits)
cargo run -p sube-cli -- --chain wss://kreivo.io tx system/remark \
  --body '(remark:0x68656c6c6f)'

# Select a chain-bound signer, then explicitly review and submit
cargo run -p sube-cli -- profile list
cargo run -p sube-cli -- --chain wss://kreivo.io connect alice
cargo run -p sube-cli -- --chain wss://kreivo.io tx system/remark \
  --body '(remark:0x68656c6c6f)' --submit

# Open the terminal UI
cargo run -p sube-cli -- --chain wss://kreivo.io
```

Create an ordinary wallet profile by piping its mnemonic to the secure store.
The profile signs with the mnemonic's root sr25519 key, the account wallets
such as polkadot.js, Talisman or subkey show for it. The profile file contains
only that account and the genesis hash:

```sh
printf '%s\n' "$MNEMONIC" | cargo run -p sube-cli -- profile add-wallet alice \
  --genesis 0x...
```

Pass enrollment always requires an exact 32-byte hashed user ID and an
ordinary registrar profile. A Substrate-key device names another wallet
profile:

```sh
cargo run -p sube-cli -- --chain wss://kreivo.io pass enroll \
  --name alice-pass --user-id 0x... --registrar alice \
  --device wallet:alice-device --submit
```

Default desktop builds can instead create a WebAuthn credential. Only the RP
ID, origin, and opaque credential ID are written to the profile. The RP ID must
be the origin's domain, which is what pallet-pass checks. Linux and macOS use
USB security keys; Windows uses the system's WebAuthn API. pallet-pass requires
user verification, so a security key needs a PIN (or fingerprint), which it
asks for on the terminal for every operation; the TUI hands the terminal over
while it does:

```sh
cargo run -p sube-cli -- --chain wss://kreivo.io pass enroll \
  --name alice-pass --user-id 0x... --registrar alice \
  --device webauthn:wallet.example.com@https://wallet.example.com --submit
```

Without `--submit`, enrollment and device commands only print the review;
nothing is stored, so a later `--submit` prompts the device again. Profiles
change only after the transaction finalizes successfully. Connecting a pass
profile with a session policy registers that session, or reuses the exact
stored one, and calls are then signed with the session key. Without a policy
or a stored session, the device itself authenticates each call:

```sh
cargo run -p sube-cli -- --chain wss://kreivo.io connect alice-pass \
  --session-policy 'calls:Balances/transfer_keep_alive'

cargo run -p sube-cli -- --chain wss://kreivo.io pass session forget alice-pass
# Without a session, the security key signs each call.
cargo run -p sube-cli -- --chain wss://kreivo.io connect alice-pass
```

The network connection is created and owned by a dedicated worker thread.
Only an owned metadata snapshot and formatted results cross into the UI thread.
Completing a call form prepares and validates the typed call and shows its hex;
it does not generate or execute a shell command. With an active signing
profile, the result panel shows the signed transaction review and `s` submits
those reviewed bytes for finalization. `sube tx --json` prints the same review
as a JSON artifact.

Press `p` in the TUI to manage chain-bound profiles. From the profile dialog,
`a` imports an ordinary wallet, `e` enrolls a pass account, `s` registers or
reuses an explicitly scoped session, `f` forgets it, and `d`/`r` add or remove
devices. Devices are entered as for `--device`; `←`/`→` cycles a field's
presets (device kinds the chain accepts, the stored or selected session
policy, known devices to remove). Pass enrollment, session, and device mutations always enter the same full-screen
review and require a successful finalized receipt before changing the local
profile. Adding an Admin device requires typing `ADMIN`; removal requires
typing `REMOVE` after reviewing the last-device warning.

Typed forms accept SS58 or hex AccountId32 values; amounts are entered in the
chain's smallest unit. Outside text fields, Space toggles enums and booleans,
and `+`/`-` add and remove list items. If the node connection drops, the TUI
reconnects and says so in its header. `Vec<u8>` fields accept plain UTF-8 as well as explicit `utf8:` and `hex:`
modes. Conversion errors stay inline and do not reach the chain
worker.

Profiles contain no secrets and are bound to a genesis hash. Wallet and session
keys stay in the platform secure store. `tx` is non-mutating unless `--submit`
is supplied, and asks for confirmation unless `--yes` is also supplied.

Build the generic explorer without wallet, pass, or desktop WebAuthn
support with `--no-default-features`.

An ignored destructive smoke test exercises Substrate-key enrollment, exact
session registration, a locally rejected call, an allowed finalized call,
adding and removing a scoped device, and receipt inspection. Use disposable
credentials: enrollment and the allowed remark remain on the selected chain.
The registrar's account (the mnemonic's root key) pays for enrollment and
transfers 0.02 KSM to the new pass account, which pays its own fees unless a
subscribed community covers them. The test stops before
submitting anything if the registrar holds less than 0.1 KSM.

```sh
SUBE_E2E_URL=wss://... \
SUBE_E2E_REGISTRAR_MNEMONIC='funded mnemonic' \
SUBE_E2E_DEVICE_MNEMONIC='disposable device mnemonic' \
cargo test -p sube-cli --all-features \
  live_pass_enrollment_session_and_submission -- --ignored --nocapture
```

`live_pass_webauthn_flow` runs the same flow with a WebAuthn device, and also
signs a call with the device itself. The device is an in-memory software
authenticator from the test-only `soft-webauthn` feature, which also turns
every `webauthn:` device of a `sube` build into one; its keys are lost when the
process exits. Never use such a build with real accounts.

```sh
SUBE_E2E_URL=wss://... \
SUBE_E2E_REGISTRAR_MNEMONIC='funded mnemonic' \
cargo test -p sube-cli --features soft-webauthn \
  live_pass_webauthn_flow -- --ignored --nocapture
```
