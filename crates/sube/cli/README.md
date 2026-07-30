# Sube CLI

An unpublished workspace tool for querying and interactively exploring
Substrate-compatible chains with `sube`.

```sh
# One query
cargo run -p sube-cli -- --chain wss://kreivo.io system/number

# Watch a path
cargo run -p sube-cli -- --chain wss://kreivo.io --watch system/number

# Open the terminal UI
cargo run -p sube-cli -- --chain wss://kreivo.io
```

The network connection is created and owned by a dedicated worker thread.
Only an owned metadata snapshot and formatted results cross into the UI thread.
