#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
rust_dir="${project_dir}/rust"
native_bin_dir="${project_dir}/native/bin"

"${project_dir}/scripts/verify.sh"

install -D -m 0755 \
    "${rust_dir}/target/release/dmc2-serial-bridge" \
    "${native_bin_dir}/dmc2-serial-bridge"
install -D -m 0755 \
    "${rust_dir}/target/release/dmc2-task-monitor" \
    "${native_bin_dir}/dmc2-task-monitor"

echo "native build and offline ABI tests passed"
echo "userspace adapters installed in ${native_bin_dir}"
echo "realtime module staged at ${module}"
