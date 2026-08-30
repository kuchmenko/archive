prefix := env("HOME") / ".local"
bin_dir := prefix / "bin"
bin := bin_dir / "archive"

default:
    @just --list

build:
    cargo build --release --manifest-path src-tauri/Cargo.toml

install:
    just build
    mkdir -p {{bin_dir}}
    install -m 755 src-tauri/target/release/archive {{bin}}
    @echo "installed {{bin}}"

mcp:
    cargo run --manifest-path src-tauri/Cargo.toml -- mcp

test:
    cargo test --manifest-path src-tauri/Cargo.toml

mcp-stdio:
    cargo test --manifest-path src-tauri/Cargo.toml --test mcp_stdio

fmt:
    cargo fmt --manifest-path src-tauri/Cargo.toml --check

clippy:
    cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

check: fmt test clippy
