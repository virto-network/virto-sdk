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

# Pass enrollment and device/session management (default desktop features)
cargo run -p sube-cli -- pass --help

# Open the terminal UI
cargo run -p sube-cli -- --chain wss://kreivo.io
```

The network connection is created and owned by a dedicated worker thread.
Only an owned metadata snapshot and formatted results cross into the UI thread.
Completing a call form prepares and validates the typed call and shows its hex;
it does not generate or execute a shell command. With an active signing
profile, the result panel shows the signed transaction review and `s` submits
those reviewed bytes for finalization. `c` and `x` copy the call or full
extrinsic via the terminal clipboard, while `e` writes a collision-safe JSON
artifact beside the profile store.

Profiles contain no secrets and are bound to a genesis hash. Wallet and session
keys stay in the platform secure store. `tx` is non-mutating unless `--submit`
is supplied, and asks for confirmation unless `--yes` is also supplied.

Build the generic explorer without wallet, pass, SSH-agent, or desktop
WebAuthn support with `--no-default-features`.
