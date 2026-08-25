#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
h100_project="${project_dir}/../h100_modbus"
source_modules=(
    "${project_dir}/rust/target/release/libdmc2_rt.so"
    "${h100_project}/target/release/h100_spindle.so"
)
target_modules=(
    "/usr/lib/linuxcnc/modules/dmc2_rt.so"
    "/usr/lib/linuxcnc/modules/h100_spindle.so"
)

transaction_staged=()
transaction_backups=()
transaction_original_present=()
transaction_committed=0
transaction_active=0

require_realtime_host_stopped() {
    local status
    if pgrep -x rtapi_app >/dev/null 2>&1; then
        echo "refusing to replace realtime modules while rtapi_app is active" >&2
        return 1
    else
        status=$?
    fi
    if [[ "${status}" -ne 1 ]]; then
        echo "cannot prove rtapi_app is stopped: pgrep exited ${status}" >&2
        return 1
    fi
}

reset_transaction_state() {
    transaction_staged=()
    transaction_backups=()
    transaction_original_present=()
    transaction_committed=0
    transaction_active=0
}

remove_transaction_file() {
    local path="$1"
    local description="$2"
    if [[ -z "${path}" || ( ! -e "${path}" && ! -L "${path}" ) ]]; then
        return 0
    fi
    if ! rm -f -- "${path}"; then
        echo "failed to remove ${description}: ${path}" >&2
        return 1
    fi
}

cleanup_transaction_artifacts() {
    local failed=0
    local path
    for path in "${transaction_staged[@]}"; do
        if ! remove_transaction_file "${path}" "staged realtime module"; then
            failed=1
        fi
    done
    for path in "${transaction_backups[@]}"; do
        if ! remove_transaction_file "${path}" "realtime-module rollback backup"; then
            failed=1
        fi
    done
    return "${failed}"
}

stage_module() {
    local index="$1"
    local source_path="${source_modules[index]}"
    local target_path="${target_modules[index]}"
    local target_directory
    local target_name
    local temporary_path

    if ! target_directory="$(dirname -- "${target_path}")"; then
        echo "failed to resolve realtime-module target directory" >&2
        return 1
    fi
    if ! target_name="$(basename -- "${target_path}")"; then
        echo "failed to resolve realtime-module target name" >&2
        return 1
    fi
    if ! temporary_path="$(mktemp "${target_directory}/.${target_name}.stage.XXXXXX")"; then
        echo "failed to create staged realtime module in ${target_directory}" >&2
        return 1
    fi
    transaction_staged[index]="${temporary_path}"
    if ! install -m 0755 "${source_path}" "${temporary_path}"; then
        echo "failed to stage verified realtime module: ${source_path}" >&2
        return 1
    fi
    if ! cmp --silent "${source_path}" "${temporary_path}"; then
        echo "staged realtime module differs from verified release: ${source_path}" >&2
        return 1
    fi
}

backup_installed_module() {
    local index="$1"
    local target_path="${target_modules[index]}"
    local target_directory
    local target_name
    local backup_path

    if [[ -L "${target_path}" ]]; then
        echo "installed realtime-module target is a symlink: ${target_path}" >&2
        return 1
    fi
    if [[ ! -e "${target_path}" ]]; then
        transaction_original_present[index]=0
        transaction_backups[index]=""
        return 0
    fi
    if [[ ! -f "${target_path}" ]]; then
        echo "installed realtime-module target is not a regular file: ${target_path}" >&2
        return 1
    fi
    if ! target_directory="$(dirname -- "${target_path}")"; then
        echo "failed to resolve rollback target directory" >&2
        return 1
    fi
    if ! target_name="$(basename -- "${target_path}")"; then
        echo "failed to resolve rollback target name" >&2
        return 1
    fi
    if ! backup_path="$(mktemp "${target_directory}/.${target_name}.rollback.XXXXXX")"; then
        echo "failed to create realtime-module rollback backup" >&2
        return 1
    fi
    transaction_backups[index]="${backup_path}"
    transaction_original_present[index]=1
    if ! cp -p -- "${target_path}" "${backup_path}"; then
        echo "failed to copy realtime-module rollback backup: ${target_path}" >&2
        return 1
    fi
    if ! cmp --silent "${target_path}" "${backup_path}"; then
        echo "realtime-module rollback backup failed byte verification: ${target_path}" >&2
        return 1
    fi
}

rollback_transaction() {
    local index
    local failed=0
    for ((index = transaction_committed - 1; index >= 0; index--)); do
        if [[ "${transaction_original_present[index]}" -eq 1 ]]; then
            if mv -f -- "${transaction_backups[index]}" "${target_modules[index]}"; then
                transaction_backups[index]=""
            else
                echo "failed to restore realtime-module rollback backup: ${target_modules[index]}" >&2
                failed=1
            fi
        elif ! rm -f -- "${target_modules[index]}"; then
            echo "failed to remove newly created realtime module during rollback: ${target_modules[index]}" >&2
            failed=1
        fi
    done
    transaction_committed=0
    if [[ "${failed}" -ne 0 ]]; then
        echo "rollback was incomplete; preserved rollback artifacts require manual recovery" >&2
        return 1
    fi
}

fail_transaction() {
    local message="$1"
    local rollback_failed=0
    echo "${message}" >&2
    if [[ "${transaction_committed}" -gt 0 ]]; then
        if ! rollback_transaction; then
            rollback_failed=1
        fi
    fi
    if [[ "${rollback_failed}" -eq 0 ]]; then
        cleanup_transaction_artifacts || true
    fi
    transaction_active=0
    return 1
}

install_modules_transactionally() {
    local index
    reset_transaction_state
    transaction_active=1

    for index in "${!source_modules[@]}"; do
        if ! stage_module "${index}"; then
            fail_transaction "realtime-module transaction failed during staging"
            return 1
        fi
    done
    for index in "${!target_modules[@]}"; do
        if ! backup_installed_module "${index}"; then
            fail_transaction "realtime-module transaction failed while preparing rollback"
            return 1
        fi
    done
    if ! require_realtime_host_stopped; then
        fail_transaction "realtime-module transaction refused before commit"
        return 1
    fi

    for index in "${!source_modules[@]}"; do
        if ! require_realtime_host_stopped; then
            fail_transaction "realtime host changed state during module transaction"
            return 1
        fi
        # Mark the target as requiring rollback before the atomic rename. This
        # closes the signal window between a successful rename and bookkeeping.
        transaction_committed=$((index + 1))
        if ! mv -f -- "${transaction_staged[index]}" "${target_modules[index]}"; then
            fail_transaction "failed to commit realtime module: ${target_modules[index]}"
            return 1
        fi
        transaction_staged[index]=""
        if ! cmp --silent "${source_modules[index]}" "${target_modules[index]}"; then
            fail_transaction "committed realtime module failed final byte verification: ${target_modules[index]}"
            return 1
        fi
    done
    if ! require_realtime_host_stopped; then
        fail_transaction "realtime host changed state before transaction completion"
        return 1
    fi
    transaction_committed=0
    transaction_active=0
    if ! cleanup_transaction_artifacts; then
        echo "modules were installed and verified, but transaction-backup cleanup failed" >&2
        return 1
    fi
}

emergency_transaction_cleanup() {
    local rollback_failed=0
    local cleanup_failed=0
    if [[ "${transaction_active}" -eq 0 ]]; then
        return 0
    fi
    if [[ "${transaction_committed}" -gt 0 ]]; then
        if ! rollback_transaction; then
            rollback_failed=1
        fi
    fi
    if [[ "${rollback_failed}" -eq 0 ]]; then
        if ! cleanup_transaction_artifacts; then
            cleanup_failed=1
        fi
    else
        echo "emergency cleanup preserved all transaction artifacts after incomplete rollback" >&2
    fi
    transaction_active=0
    [[ "${rollback_failed}" -eq 0 && "${cleanup_failed}" -eq 0 ]]
}

clear_transaction_traps() {
    trap - EXIT HUP INT TERM
}

handle_transaction_exit() {
    local exit_status="$1"
    local cleanup_failed=0
    clear_transaction_traps
    if ! emergency_transaction_cleanup; then
        cleanup_failed=1
    fi
    if [[ "${exit_status}" -eq 0 && "${cleanup_failed}" -ne 0 ]]; then
        exit_status=1
    fi
    exit "${exit_status}"
}

handle_transaction_signal() {
    local signal_name="$1"
    local exit_status="$2"
    clear_transaction_traps
    echo "realtime-module transaction interrupted by ${signal_name}" >&2
    if ! emergency_transaction_cleanup; then
        echo "realtime-module emergency cleanup was incomplete" >&2
    fi
    exit "${exit_status}"
}

install_transaction_traps() {
    trap 'handle_transaction_exit "$?"' EXIT
    trap 'handle_transaction_signal HUP 129' HUP
    trap 'handle_transaction_signal INT 130' INT
    trap 'handle_transaction_signal TERM 143' TERM
}

validate_module_inventory() {
    local index
    if [[ "${#source_modules[@]}" -ne "${#target_modules[@]}" ||
          "${#source_modules[@]}" -eq 0 ]]; then
        echo "realtime-module source/target inventory is inconsistent" >&2
        return 1
    fi
    for index in "${!source_modules[@]}"; do
        if [[ ! -f "${source_modules[index]}" || -L "${source_modules[index]}" ]]; then
            echo "missing regular staged realtime module: ${source_modules[index]}" >&2
            return 1
        fi
    done
}

main() {
    local index
    if [[ "${EUID}" -ne 0 ]]; then
        echo "run this exact installer as root" >&2
        return 1
    fi
    validate_module_inventory

    install_transaction_traps
    if ! install_modules_transactionally; then
        clear_transaction_traps
        return 1
    fi
    clear_transaction_traps
    for index in "${!target_modules[@]}"; do
        echo "installed exact verified module: ${target_modules[index]}"
    done
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
