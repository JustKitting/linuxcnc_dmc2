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

test_root="$(mktemp -d /tmp/dmc2-module-installer-test.XXXXXX)"
case_number=0
cleanup_test_root() {
    rm -rf -- "${test_root}"
}
trap cleanup_test_root EXIT

test_pgrep_status=1
test_pgrep_calls=0
test_pgrep_sequence=()
pgrep() {
    local index="${test_pgrep_calls}"
    local status
    test_pgrep_calls=$((test_pgrep_calls + 1))
    status="${test_pgrep_sequence[index]-}"
    if [[ -z "${status}" ]]; then
        status="${test_pgrep_status}"
    fi
    return "${status}"
}

new_case() {
    case_number=$((case_number + 1))
    case_dir="${test_root}/case-${case_number}"
    mkdir -p "${case_dir}"
    source_zero="${case_dir}/source-zero.so"
    source_one="${case_dir}/source-one.so"
    target_zero="${case_dir}/target-zero.so"
    target_one="${case_dir}/target-one.so"
    original_zero="${case_dir}/original-zero.so"
    original_one="${case_dir}/original-one.so"
    printf 'verified module zero\n' > "${source_zero}"
    printf 'verified module one\n' > "${source_one}"
    printf 'original module zero\n' > "${target_zero}"
    printf 'original module one\n' > "${target_one}"
    cp -p -- "${target_zero}" "${original_zero}"
    cp -p -- "${target_one}" "${original_one}"
    chmod 0600 "${source_zero}" "${source_one}" "${target_zero}" "${target_one}"
    source_modules=("${source_zero}" "${source_one}")
    target_modules=("${target_zero}" "${target_one}")
    test_pgrep_status=1
    test_pgrep_calls=0
    test_pgrep_sequence=()
    reset_transaction_state
}

assert_original_modules() {
    cmp --silent "${original_zero}" "${target_zero}"
    cmp --silent "${original_one}" "${target_one}"
}

assert_verified_modules() {
    cmp --silent "${source_zero}" "${target_zero}"
    cmp --silent "${source_one}" "${target_one}"
}

assert_no_transaction_artifacts() {
    if find "${case_dir}" -maxdepth 1 -type f \
        \( -name '*.stage.*' -o -name '*.rollback.*' \) -print -quit |
        grep -q .; then
        echo "installer left a transaction artifact in ${case_dir}" >&2
        return 1
    fi
}

assert_transaction_fails() {
    local expected="$1"
    local output
    if output="$(install_modules_transactionally 2>&1)"; then
        echo "module transaction unexpectedly succeeded" >&2
        return 1
    fi
    if [[ "${output}" != *"${expected}"* ]]; then
        echo "unexpected transaction failure: ${output}" >&2
        return 1
    fi
}

test_pgrep_status=1
require_realtime_host_stopped
for status in 0 2 3 127; do
    test_pgrep_status="${status}"
    if [[ "${status}" -eq 0 ]]; then
        assert_command_fails "rtapi_app is active" require_realtime_host_stopped
    else
        assert_command_fails "pgrep exited ${status}" require_realtime_host_stopped
    fi
done

new_case
validate_module_inventory
saved_sources=("${source_modules[@]}")
saved_targets=("${target_modules[@]}")
source_modules=()
target_modules=()
assert_command_fails "inventory is inconsistent" validate_module_inventory
source_modules=("${saved_sources[0]}")
target_modules=("${saved_targets[@]}")
assert_command_fails "inventory is inconsistent" validate_module_inventory
source_modules=("${saved_sources[@]}")
target_modules=("${saved_targets[@]}")
mv "${source_modules[0]}" "${source_modules[0]}.missing"
assert_command_fails "missing regular staged" validate_module_inventory
mv "${source_modules[0]}.missing" "${source_modules[0]}"
ln -s "${source_modules[0]}" "${case_dir}/source-link.so"
source_modules[0]="${case_dir}/source-link.so"
assert_command_fails "missing regular staged" validate_module_inventory

new_case
mktemp() {
    return 73
}
assert_transaction_fails "failed to create staged realtime module"
unset -f mktemp
assert_original_modules
assert_no_transaction_artifacts

new_case
install() {
    return 74
}
assert_transaction_fails "failed to stage verified realtime module"
unset -f install
assert_original_modules
assert_no_transaction_artifacts

new_case
cmp() {
    return 75
}
assert_transaction_fails "staged realtime module differs"
unset -f cmp
assert_original_modules
assert_no_transaction_artifacts

new_case
dirname() {
    return 76
}
assert_transaction_fails "failed to resolve realtime-module target directory"
unset -f dirname
assert_original_modules
assert_no_transaction_artifacts

new_case
basename() {
    return 77
}
assert_transaction_fails "failed to resolve realtime-module target name"
unset -f basename
assert_original_modules
assert_no_transaction_artifacts

new_case
cp() {
    return 78
}
assert_transaction_fails "failed to copy realtime-module rollback backup"
unset -f cp
assert_original_modules
assert_no_transaction_artifacts

new_case
cmp() {
    local last="${@: -1}"
    if [[ "${last}" == *.rollback.* ]]; then
        return 79
    fi
    command cmp "$@"
}
assert_transaction_fails "rollback backup failed byte verification"
unset -f cmp
assert_original_modules
assert_no_transaction_artifacts

new_case
rm "${target_modules[0]}"
mkdir "${target_modules[0]}"
assert_transaction_fails "not a regular file"
rmdir "${target_modules[0]}"
cp -p -- "${original_zero}" "${target_modules[0]}"
assert_original_modules
assert_no_transaction_artifacts

new_case
rm "${target_modules[0]}"
ln -s "${case_dir}/missing-target.so" "${target_modules[0]}"
assert_transaction_fails "target is a symlink"
rm "${target_modules[0]}"
cp -p -- "${original_zero}" "${target_modules[0]}"
assert_original_modules
assert_no_transaction_artifacts

new_case
test_stage_mv_calls=0
mv() {
    local source="${@: -2:1}"
    if [[ "${source}" == *.stage.* ]]; then
        test_stage_mv_calls=$((test_stage_mv_calls + 1))
        if [[ "${test_stage_mv_calls}" -eq 2 ]]; then
            return 80
        fi
    fi
    command mv "$@"
}
assert_transaction_fails "failed to commit realtime module"
unset -f mv
assert_original_modules
assert_no_transaction_artifacts

new_case
cmp() {
    local last="${@: -1}"
    if [[ "${last}" == "${target_one}" ]]; then
        return 81
    fi
    command cmp "$@"
}
assert_transaction_fails "failed final byte verification"
unset -f cmp
assert_original_modules
assert_no_transaction_artifacts

new_case
test_pgrep_sequence=(1 1 0)
assert_transaction_fails "realtime host changed state during"
assert_original_modules
assert_no_transaction_artifacts

new_case
test_pgrep_sequence=(1 1 1 0)
assert_transaction_fails "realtime host changed state before transaction completion"
assert_original_modules
assert_no_transaction_artifacts

new_case
test_pgrep_sequence=(0)
assert_transaction_fails "transaction refused before commit"
assert_original_modules
assert_no_transaction_artifacts

new_case
test_pgrep_sequence=(2)
assert_transaction_fails "transaction refused before commit"
assert_original_modules
assert_no_transaction_artifacts

new_case
rm "${target_zero}"
test_stage_mv_calls=0
mv() {
    local source="${@: -2:1}"
    if [[ "${source}" == *.stage.* ]]; then
        test_stage_mv_calls=$((test_stage_mv_calls + 1))
        if [[ "${test_stage_mv_calls}" -eq 2 ]]; then
            return 82
        fi
    fi
    command mv "$@"
}
assert_transaction_fails "failed to commit realtime module"
unset -f mv
if [[ -e "${target_zero}" ]]; then
    echo "rollback did not remove a newly created first module" >&2
    exit 1
fi
cmp --silent "${original_one}" "${target_one}"
assert_no_transaction_artifacts

new_case
test_stage_mv_calls=0
mv() {
    local source="${@: -2:1}"
    if [[ "${source}" == *.stage.* ]]; then
        test_stage_mv_calls=$((test_stage_mv_calls + 1))
        if [[ "${test_stage_mv_calls}" -eq 2 ]]; then
            return 83
        fi
    elif [[ "${source}" == *.rollback.* ]]; then
        return 84
    fi
    command mv "$@"
}
assert_transaction_fails "rollback was incomplete"
unset -f mv
if ! find "${case_dir}" -maxdepth 1 -type f -name '*.rollback.*' -print -quit |
    grep -q .; then
    echo "incomplete rollback did not preserve its recovery artifact" >&2
    exit 1
fi
rollback_backup="$(find "${case_dir}" -maxdepth 1 -type f \
    -name '.target-zero.so.rollback.*' -print -quit)"
mv -f -- "${rollback_backup}" "${target_zero}"
find "${case_dir}" -maxdepth 1 -type f \
    \( -name '*.stage.*' -o -name '*.rollback.*' \) -delete
assert_original_modules

new_case
install_modules_transactionally
assert_verified_modules
if [[ "$(stat -c '%a' "${target_zero}")" != "755" ||
      "$(stat -c '%a' "${target_one}")" != "755" ]]; then
    echo "transactional installer did not set the required module mode" >&2
    exit 1
fi
assert_no_transaction_artifacts

new_case
rm() {
    local path="${@: -1}"
    if [[ "${path}" == *.rollback.* ]]; then
        return 85
    fi
    command rm "$@"
}
assert_transaction_fails "cleanup failed"
unset -f rm
assert_verified_modules
find "${case_dir}" -maxdepth 1 -type f -name '*.rollback.*' -delete

new_case
signal_output="${case_dir}/signal-output.txt"
set +e
(
    trap - EXIT HUP INT TERM
    install_transaction_traps
    mv() {
        local source="${@: -2:1}"
        if [[ "${source}" == *.stage.* ]]; then
            command mv "$@"
            kill -TERM "${BASHPID}"
        else
            command mv "$@"
        fi
    }
    install_modules_transactionally
) >"${signal_output}" 2>&1
signal_status=$?
set -e
if [[ "${signal_status}" -ne 143 ]]; then
    echo "TERM transaction returned ${signal_status}, expected 143" >&2
    exit 1
fi
if [[ "$(<"${signal_output}")" != *"interrupted by TERM"* ]]; then
    echo "TERM transaction did not report its interruption" >&2
    exit 1
fi
assert_original_modules
assert_no_transaction_artifacts

new_case
exit_output="${case_dir}/exit-output.txt"
set +e
(
    trap - EXIT HUP INT TERM
    install_transaction_traps
    reset_transaction_state
    transaction_active=1
    stage_module 0
    backup_installed_module 0
    transaction_committed=1
    mv -f -- "${transaction_staged[0]}" "${target_modules[0]}"
    transaction_staged[0]=""
    exit 47
) >"${exit_output}" 2>&1
exit_status=$?
set -e
if [[ "${exit_status}" -ne 47 ]]; then
    echo "EXIT transaction returned ${exit_status}, expected 47" >&2
    exit 1
fi
assert_original_modules
assert_no_transaction_artifacts

reset_transaction_state
emergency_transaction_cleanup

assert_command_fails "run this exact installer as root" \
    bash "${project_dir}/scripts/install_native_module.sh"

echo "transactional native module installer tests passed"
