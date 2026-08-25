#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
rust_dir="${project_dir}/rust"
module="${rust_dir}/target/release/libdmc2_rt.so"

installed_version="$(linuxcnc_var LINUXCNCVERSION)"
if [[ "${installed_version}" != "2.9.10" ]]; then
    echo "refusing unaudited LinuxCNC version: ${installed_version}" >&2
    exit 1
fi

cargo fmt --manifest-path "${rust_dir}/Cargo.toml" --all -- --check
cargo test --manifest-path "${rust_dir}/Cargo.toml" --workspace
cargo build --manifest-path "${rust_dir}/Cargo.toml" --workspace --release
"${rust_dir}/target/release/dmc2-task-monitor" --validate

for symbol in rtapi_app_main rtapi_app_exit; do
    if ! readelf -Ws "${module}" | grep -q " ${symbol}$"; then
        echo "realtime module is missing ${symbol}" >&2
        exit 1
    fi
done

"${project_dir}/scripts/check_source_layout.sh"

PYTHONDONTWRITEBYTECODE=1 python3 "${project_dir}/tests/run_python.py"
PYTHONDONTWRITEBYTECODE=1 python3 "${project_dir}/scripts/validate_offline.py"
PYTHONDONTWRITEBYTECODE=1 python3 "${project_dir}/scripts/check_readiness.py"

echo "all hardware-free project verification passed"
