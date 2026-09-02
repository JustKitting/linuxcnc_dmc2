#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
rust_dir="${project_dir}/rust"

cargo fmt --manifest-path "${rust_dir}/Cargo.toml" --all --check
env RUSTFLAGS=-Dwarnings \
    cargo build --manifest-path "${rust_dir}/Cargo.toml" --workspace --release

"${rust_dir}/target/release/dmc2-motion-acceptance" \
    --project-root "${project_dir}"
