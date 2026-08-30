prefix := env("HOME") / ".local"
bin_dir := prefix / "bin"
bin := bin_dir / "archive"

default:
    @just --list

install:
    cargo build --release --manifest-path src-tauri/Cargo.toml
    mkdir -p {{bin_dir}}
    install -m 755 src-tauri/target/release/archive {{bin}}
    @echo "installed {{bin}}"

mcp:
    cargo run --manifest-path src-tauri/Cargo.toml -- mcp

test:
    cargo test --manifest-path src-tauri/Cargo.toml
