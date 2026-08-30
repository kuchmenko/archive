# Archive

Archive is a local SQLite knowledge store exposed to agents through a Model Context Protocol server over standard input and output.

## Build

```sh
cargo build --release --manifest-path src-tauri/Cargo.toml
```

The executable is written to `src-tauri/target/release/archive`.

## Run the MCP server

```sh
archive mcp
```

Archive stores its SQLite database at the platform data directory under `dev.kuchmenko.archive/archive.sqlite3`. Set `XDG_DATA_HOME` to select a different data root on Linux.

## Test

```sh
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
```
