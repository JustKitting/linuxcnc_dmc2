#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
rust_dir="${project_dir}/rust"
llvm_profdata="/usr/lib/llvm-19/bin/llvm-profdata"
llvm_cov="/usr/lib/llvm-19/bin/llvm-cov"

for tool in "${llvm_profdata}" "${llvm_cov}"; do
    if [[ ! -x "${tool}" ]]; then
        echo "required Rust coverage tool is unavailable: ${tool}" >&2
        exit 1
    fi
done

coverage_root="$(mktemp -d /tmp/dmc2-launcher-coverage.XXXXXX)"
case "${coverage_root}" in
    /tmp/dmc2-launcher-coverage.*) ;;
    *)
        echo "refusing unexpected coverage temporary path: ${coverage_root}" >&2
        exit 1
        ;;
esac
cleanup() {
    rm -rf -- "${coverage_root}"
}
trap cleanup EXIT

target_dir="${coverage_root}/target"
profile_dir="${coverage_root}/profiles"
profile_data="${coverage_root}/launcher.profdata"

env \
    CARGO_TARGET_DIR="${target_dir}" \
    RUSTFLAGS=-Cinstrument-coverage \
    LLVM_PROFILE_FILE="${profile_dir}/%p-%m.profraw" \
    cargo test --manifest-path "${rust_dir}/Cargo.toml" -p dmc2-launcher

mapfile -d '' profiles < <(
    find "${profile_dir}" -maxdepth 1 -type f -name '*.profraw' -print0
)
if (( ${#profiles[@]} == 0 )); then
    echo "launcher coverage produced no raw profiles" >&2
    exit 1
fi
"${llvm_profdata}" merge -sparse "${profiles[@]}" -o "${profile_data}"

launcher_object="$(
    find "${target_dir}/debug/deps" -maxdepth 1 -type f -executable \
        -name 'dmc2_launcher-*' -print -quit
)"
if [[ -z "${launcher_object}" ]]; then
    echo "launcher coverage object is missing" >&2
    exit 1
fi

coverage_command=(
    "${llvm_cov}"
    report
    "${launcher_object}"
)
while IFS= read -r -d '' object; do
    coverage_command+=( -object "${object}" )
done < <(
    find "${target_dir}/debug/deps" -maxdepth 1 -type f -executable \
        \( -name 'dmc2_linuxcnc-*' -o -name 'entrypoint-*' \) -print0
)
coverage_command+=(
    "-instr-profile=${profile_data}"
    "-ignore-filename-regex=(/rustc/|/src/tests/|/tests/entrypoint.rs)"
)

report="$("${coverage_command[@]}")"
printf '%s\n' "${report}"
if ! awk '
    $1 == "TOTAL" {
        found = 1
        if ($3 != 0 || $6 != 0 || $9 != 0) {
            exit 1
        }
    }
    END {
        if (!found) {
            exit 1
        }
    }
' <<<"${report}"; then
    echo "launcher production coverage is not 100% for regions, functions, and lines" >&2
    exit 1
fi

echo "launcher production coverage is 100% for regions, functions, and lines"
