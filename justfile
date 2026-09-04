prefix := env("HOME") / ".local"
bin_dir := prefix / "bin"
bin := bin_dir / "archive"

default:
    @just --list

build:
    cargo build --release

install:
    just build
    mkdir -p {{bin_dir}}
    install -m 755 target/release/archive {{bin}}
    @echo "installed {{bin}}"

mcp:
    cargo run -- mcp

test:
    cargo test

mcp-stdio:
    cargo test --test mcp_stdio

fmt:
    cargo fmt --check

clippy:
    cargo clippy --all-targets -- -D warnings

check: fmt test clippy
