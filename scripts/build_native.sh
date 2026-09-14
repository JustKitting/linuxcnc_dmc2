#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
rust_dir="${project_dir}/rust"
native_bin_dir="${project_dir}/native/bin"
h100_project="${project_dir}/../h100_modbus"
dmc2_module="${rust_dir}/target/release/libdmc2_rt.so"
h100_module="${h100_project}/target/release/h100_spindle.so"
hm2_eth_module="${project_dir}/native/modules/hm2_eth.so"
tooldata_library="${project_dir}/native/lib/libtooldata.so.0"

"${project_dir}/scripts/build_linuxcnc_driver_overlays.sh"
"${h100_project}/scripts/build_release.sh"

env RUSTFLAGS=-Dwarnings \
    cargo build --manifest-path "${rust_dir}/Cargo.toml" --workspace --release

install -D -m 0755 \
    "${rust_dir}/target/release/dmc2-serial-bridge" \
    "${native_bin_dir}/dmc2-serial-bridge"
install -D -m 0755 \
    "${rust_dir}/target/release/dmc2-task-monitor" \
    "${native_bin_dir}/dmc2-task-monitor"
install -D -m 0755 \
    "${rust_dir}/target/release/dmc2-process-supervisor" \
    "${native_bin_dir}/dmc2-process-supervisor"
install -D -m 0755 \
    "${rust_dir}/target/release/dmc2-session-supervisor" \
    "${native_bin_dir}/dmc2-session-supervisor"
install -D -m 0755 \
    "${rust_dir}/target/release/dmc2-linuxcnc" \
    "${native_bin_dir}/dmc2-linuxcnc"
install -D -m 0755 \
    "${rust_dir}/target/release/dmc2ctl" \
    "${native_bin_dir}/dmc2ctl"
install -D -m 0755 \
    "${rust_dir}/target/release/dmc2-probe-capture" \
    "${native_bin_dir}/dmc2-probe-capture"
cmp --silent \
    "${rust_dir}/target/release/dmc2-serial-bridge" \
    "${native_bin_dir}/dmc2-serial-bridge"
cmp --silent \
    "${rust_dir}/target/release/dmc2-task-monitor" \
    "${native_bin_dir}/dmc2-task-monitor"
cmp --silent \
    "${rust_dir}/target/release/dmc2-process-supervisor" \
    "${native_bin_dir}/dmc2-process-supervisor"
cmp --silent \
    "${rust_dir}/target/release/dmc2-session-supervisor" \
    "${native_bin_dir}/dmc2-session-supervisor"
cmp --silent \
    "${rust_dir}/target/release/dmc2-linuxcnc" \
    "${native_bin_dir}/dmc2-linuxcnc"
cmp --silent \
    "${rust_dir}/target/release/dmc2ctl" \
    "${native_bin_dir}/dmc2ctl"
cmp --silent \
    "${rust_dir}/target/release/dmc2-probe-capture" \
    "${native_bin_dir}/dmc2-probe-capture"

echo "native release build passed"
echo "userspace adapters installed in ${native_bin_dir}"
echo "DMC2 realtime module staged at ${dmc2_module}"
echo "H100 realtime module staged at ${h100_module}"
echo "patched LinuxCNC hm2_eth module staged at ${hm2_eth_module}"
echo "isolated LinuxCNC tooldata library staged at ${tooldata_library}"
echo "launch with native/bin/dmc2-linuxcnc --live --persistent to synchronize all modules"
