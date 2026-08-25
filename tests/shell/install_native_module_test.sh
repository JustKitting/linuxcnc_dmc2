#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
source "${project_dir}/scripts/install_native_module.sh"

assert_command_fails() {
    local expected="$1"
    shift
    local output
    if output="$("$@" 2>&1)"; then
        echo "command unexpectedly succeeded: $*" >&2
        return 1
    fi
    if [[ "${output}" != *"${expected}"* ]]; then
        echo "unexpected failure diagnostic for $*: ${output}" >&2
        return 1
    fi
}

assert_guard_fails() {
    local expected="$1"
    test_pgrep_status="$2"
    local output
    if output="$(require_realtime_host_stopped 2>&1)"; then
        echo "realtime-host guard accepted pgrep status ${test_pgrep_status}" >&2
        return 1
    fi
    if [[ "${output}" != *"${expected}"* ]]; then
        echo "unexpected guard diagnostic for pgrep status ${test_pgrep_status}: ${output}" >&2
        return 1
    fi
}

pgrep() {
    return "${test_pgrep_status}"
}

test_pgrep_status=1
require_realtime_host_stopped
assert_guard_fails "rtapi_app is active" 0
assert_guard_fails "pgrep exited 2" 2
assert_guard_fails "pgrep exited 3" 3
assert_guard_fails "pgrep exited 127" 127

test_directory="$(mktemp -d)"
trap 'rm -rf -- "${test_directory}"' EXIT
test_source="${test_directory}/source.so"
test_target="${test_directory}/target.so"
printf 'verified realtime module\n' > "${test_source}"
printf 'stale realtime module\n' > "${test_target}"
chmod 0600 "${test_source}" "${test_target}"

assert_no_temporary_module() {
    if find "${test_directory}" -maxdepth 1 -name '.target.so.*' -print -quit |
        grep -q .; then
        echo "atomic installer left a temporary module behind" >&2
        return 1
    fi
}

if [[ "${#source_modules[@]}" -ne 2 || "${#target_modules[@]}" -ne 2 ]]; then
    echo "installer does not own exactly both realtime module deployments" >&2
    exit 1
fi
if [[ "$(basename -- "${target_modules[0]}")" != "dmc2_rt.so" ||
      "$(basename -- "${target_modules[1]}")" != "h100_spindle.so" ]]; then
    echo "installer realtime module targets are incomplete or reordered" >&2
    exit 1
fi

mktemp() {
    return 73
}
assert_command_fails \
    "failed to create temporary realtime module" \
    install_module_atomically "${test_source}" "${test_target}"
unset -f mktemp
assert_no_temporary_module

install() {
    return 74
}
assert_command_fails \
    "failed to stage verified realtime module" \
    install_module_atomically "${test_source}" "${test_target}"
unset -f install
assert_no_temporary_module

cmp() {
    return 75
}
assert_command_fails \
    "staged realtime module differs" \
    install_module_atomically "${test_source}" "${test_target}"
unset -f cmp
assert_no_temporary_module

mv() {
    return 76
}
assert_command_fails \
    "failed to atomically replace" \
    install_module_atomically "${test_source}" "${test_target}"
unset -f mv
assert_no_temporary_module

test_cmp_calls=0
cmp() {
    test_cmp_calls=$((test_cmp_calls + 1))
    if [[ "${test_cmp_calls}" -eq 1 ]]; then
        command cmp "$@"
    else
        return 77
    fi
}
assert_command_fails \
    "failed final byte verification" \
    install_module_atomically "${test_source}" "${test_target}"
unset -f cmp
assert_no_temporary_module

printf 'stale realtime module\n' > "${test_target}"
chmod 0600 "${test_target}"
install_module_atomically "${test_source}" "${test_target}"
cmp --silent "${test_source}" "${test_target}"
if [[ "$(stat -c '%a' "${test_target}")" != "755" ]]; then
    echo "atomic installer did not preserve the required executable mode" >&2
    exit 1
fi
assert_no_temporary_module

assert_command_fails "run this exact installer as root" \
    bash "${project_dir}/scripts/install_native_module.sh"

echo "native module installer tests passed"
