#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
rust_dir="${project_dir}/rust"
native_bin_dir="${project_dir}/native/bin"

installed_version="$(linuxcnc_var LINUXCNCVERSION)"
if [[ "${installed_version}" != "2.9.10" ]]; then
    echo "refusing unaudited LinuxCNC version: ${installed_version}" >&2
    exit 1
fi

cargo test --manifest-path "${rust_dir}/Cargo.toml" --workspace
cargo build --manifest-path "${rust_dir}/Cargo.toml" --workspace --release
"${rust_dir}/target/release/dmc2-task-monitor" --validate

module="${rust_dir}/target/release/libdmc2_rt.so"
for symbol in rtapi_app_main rtapi_app_exit; do
    if ! readelf -Ws "${module}" | grep -q " ${symbol}$"; then
        echo "realtime module is missing ${symbol}" >&2
        exit 1
    fi
done

install -D -m 0755 \
    "${rust_dir}/target/release/dmc2-serial-bridge" \
    "${native_bin_dir}/dmc2-serial-bridge"
install -D -m 0755 \
    "${rust_dir}/target/release/dmc2-task-monitor" \
    "${native_bin_dir}/dmc2-task-monitor"

echo "native build and offline ABI tests passed"
echo "userspace adapters installed in ${native_bin_dir}"
echo "realtime module staged at ${module}"
