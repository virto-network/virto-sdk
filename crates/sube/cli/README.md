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
The profile file contains only the account, genesis hash, secure-store entry,
and signing scheme:

```sh
printf '%s\n' "$MNEMONIC" | cargo run -p sube-cli -- profile add-wallet alice \
  --genesis 0x... --account 0x... --mnemonic-stdin
```

Pass enrollment always requires an exact 32-byte hashed user ID and an
ordinary registrar profile. A Substrate-key device names another wallet
profile:

```sh
cargo run -p sube-cli -- --chain wss://kreivo.io pass enroll \
  --name alice-pass --user-id 0x... --registrar alice \
  --device-wallet alice-device --submit
```

Default desktop builds can instead create a WebAuthn credential. Only the RP
ID, origin, and opaque credential ID are written to the profile:

```sh
cargo run -p sube-cli -- --chain wss://kreivo.io pass enroll \
  --name alice-pass --user-id 0x... --registrar alice \
  --webauthn-rp-id wallet.example.com \
  --webauthn-origin https://wallet.example.com --submit
```

Unix builds with `--features ssh-agent` accept an Ed25519 key selected by its
exact OpenSSH fingerprint. The runtime must advertise the `Ssh` credential
variant; otherwise the command stops before prompting the agent:

```sh
cargo run -p sube-cli --features ssh-agent -- \
  --chain wss://example.invalid pass enroll \
  --name alice-pass --user-id 0x... --registrar alice \
  --ssh-fingerprint SHA256:... --ssh-namespace virto-pass --submit
```

Enrollment and device-add calls retain a retryable non-secret draft while
their checkpoint remains valid. Profiles are persisted only after successful
finalization. Connecting a pass profile requires an explicit session policy
unless the exact stored policy can be reused:

```sh
cargo run -p sube-cli -- --chain wss://kreivo.io connect alice-pass \
  --session-policy 'calls:Balances/transfer_keep_alive'

cargo run -p sube-cli -- --chain wss://kreivo.io pass session forget alice-pass
```

The network connection is created and owned by a dedicated worker thread.
Only an owned metadata snapshot and formatted results cross into the UI thread.
Completing a call form prepares and validates the typed call and shows its hex;
it does not generate or execute a shell command. With an active signing
profile, the result panel shows the signed transaction review and `s` submits
those reviewed bytes for finalization. `c` and `x` copy the call or full
extrinsic via the terminal clipboard, while `e` writes a collision-safe JSON
artifact beside the profile store.

Press `p` in the TUI to manage chain-bound profiles. From the profile dialog,
`a` imports an ordinary wallet, `e` enrolls a pass account, `s` registers or
reuses an explicitly scoped session, and `d`/`r` add or remove devices. Pass
enrollment, session, and device mutations always enter the same full-screen
review and require a successful finalized receipt before changing the local
profile. Adding an Admin device requires typing `ADMIN`; removal requires
typing `REMOVE` after reviewing the last-device warning.

Typed forms accept SS58 or hex AccountId32 values and convert decimal token
amounts using the chain's advertised precision without floating point.
`Vec<u8>` fields accept plain UTF-8 as well as explicit `utf8:`, `hex:`, and
`file:` modes. Conversion errors stay inline and do not reach the chain
worker.

Profiles contain no secrets and are bound to a genesis hash. Wallet and session
keys stay in the platform secure store. `tx` is non-mutating unless `--submit`
is supplied, and asks for confirmation unless `--yes` is also supplied.

Build the generic explorer without wallet, pass, SSH-agent, or desktop
WebAuthn support with `--no-default-features`.

An ignored destructive smoke test exercises Substrate-key enrollment, exact
session registration, a locally rejected call, an allowed finalized call, and
receipt inspection. Use disposable credentials: enrollment and the allowed
remark remain on the selected chain.

```sh
SUBE_E2E_URL=wss://... \
SUBE_E2E_REGISTRAR_MNEMONIC='funded mnemonic derived at //default' \
SUBE_E2E_DEVICE_MNEMONIC='disposable device mnemonic' \
cargo test -p sube-cli --all-features \
  live_pass_enrollment_session_and_submission -- --ignored --nocapture
```
