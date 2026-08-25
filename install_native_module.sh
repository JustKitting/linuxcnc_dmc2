#!/usr/bin/env bash
set -euo pipefail

if [[ "${EUID}" -ne 0 ]]; then
    echo "run this exact installer as root" >&2
    exit 1
fi

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source_module="${project_dir}/rust/target/release/libdmc2_rt.so"
target_module="/usr/lib/linuxcnc/modules/dmc2_rt.so"

if [[ ! -f "${source_module}" ]]; then
    echo "missing staged realtime module; run ./build_native.sh first" >&2
    exit 1
fi

install -m 0755 "${source_module}" "${target_module}"
cmp --silent "${source_module}" "${target_module}"
echo "installed exact verified module: ${target_module}"
