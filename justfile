prefix := env("HOME") / ".local"
bin_dir := prefix / "bin"
bin := bin_dir / "archive"

default:
    @just --list

# Build frontend + production binary (UI embedded) and install to ~/.local/bin/archive
install:
    npm run build
    cargo build --release --manifest-path src-tauri/Cargo.toml
    mkdir -p {{bin_dir}}
    install -m 755 src-tauri/target/release/archive {{bin}}
    @echo "installed {{bin}}"

# GUI dev mode (Vite on :1420, no frontend embed)
dev:
    cargo tauri dev --manifest-path src-tauri/Cargo.toml --no-default-features
