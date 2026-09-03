#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source_file="${project_dir}/native_src/dmc2_signal_evidence.c"
release_file="${project_dir}/rust/target/release/libdmc2_signal_evidence.so"
native_file="${project_dir}/native/bin/libdmc2_signal_evidence.so"

temporary_root="$(mktemp -d "${project_dir}/var/tmp/signal-evidence-build.XXXXXX")"
cleanup() {
    if [[ "${temporary_root}" == "${project_dir}/var/tmp/signal-evidence-build."* ]]; then
        rm -rf -- "${temporary_root}"
    fi
}
trap cleanup EXIT

common_flags=(
    -std=c11
    -O2
    -fPIC
    -fvisibility=hidden
    -fstack-protector-strong
    -D_FORTIFY_SOURCE=3
    -Wall
    -Wextra
    -Wpedantic
    -Werror
    -shared
    -Wl,-z,defs
    -Wl,-z,relro,-z,now,-z,noexecstack
    -Wl,--build-id=none
)

for output in first second; do
    artifact="${temporary_root}/${output}.so"
    cc "${common_flags[@]}" -o "${artifact}" "${source_file}" -ldl
    objcopy --strip-debug --remove-section=.comment "${artifact}"
done

cmp --silent "${temporary_root}/first.so" "${temporary_root}/second.so"
exports="$(nm -D --defined-only "${temporary_root}/first.so" | awk '{print $3}')"
if [[ "${exports}" != "signal" ]]; then
    echo "signal-evidence shim exports an unexpected symbol set" >&2
    exit 1
fi
if ! readelf -W -l "${temporary_root}/first.so" |
    grep -Eq 'GNU_STACK.* RW[[:space:]]'; then
    echo "signal-evidence shim has an executable or missing GNU stack declaration" >&2
    exit 1
fi

install -D -m 0755 "${temporary_root}/first.so" "${release_file}"
install -D -m 0755 "${temporary_root}/first.so" "${native_file}"
cmp --silent "${release_file}" "${native_file}"

checksum="$(sha256sum "${native_file}" | awk '{print $1}')"
printf 'reproducible caught-signal evidence shim built: %s sha256=%s\n' \
    "${native_file}" "${checksum}"
