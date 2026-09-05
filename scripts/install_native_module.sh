#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
h100_project="${project_dir}/../h100_modbus"
source_modules=(
    "${project_dir}/rust/target/release/libdmc2_rt.so"
    "${h100_project}/target/release/h100_spindle.so"
    "${project_dir}/native/modules/hm2_eth.so"
    "${project_dir}/native/lib/libtooldata.so.0"
)
target_modules=(
    "/usr/lib/linuxcnc/modules/dmc2_rt.so"
    "/usr/lib/linuxcnc/modules/h100_spindle.so"
    "/usr/lib/linuxcnc/modules/hm2_eth.so"
    "/usr/lib/libtooldata.so.0"
)
artifact_kinds=(
    native-artifact
    native-artifact
    native-artifact
    userspace-library
)
staged_artifacts=()

select_artifacts() {
    if [[ "$#" -eq 0 ]]; then
        return 0
    fi
    if [[ "$#" -ne 1 || "$1" != "--userspace-only" ]]; then
        echo "unknown install selection; use no arguments for all system artifacts or --userspace-only for atomic library replacement without restarting processes" >&2
        return 1
    fi
    local index
    local -a selected_sources=() selected_targets=() selected_kinds=()
    for index in "${!artifact_kinds[@]}"; do
        if [[ "${artifact_kinds[index]}" == "userspace-library" ]]; then
            selected_sources+=("${source_modules[index]}")
            selected_targets+=("${target_modules[index]}")
            selected_kinds+=("${artifact_kinds[index]}")
        fi
    done
    source_modules=("${selected_sources[@]}")
    target_modules=("${selected_targets[@]}")
    artifact_kinds=("${selected_kinds[@]}")
}

require_realtime_host_stopped() {
    local kind
    local has_realtime=0
    for kind in "${artifact_kinds[@]}"; do
        case "${kind}" in
            native-artifact) has_realtime=1 ;;
            userspace-library) ;;
            *) echo "unknown artifact kind: ${kind}; restore the installer inventory" >&2; return 1 ;;
        esac
    done
    # Renaming a userspace library preserves existing processes' mapped inode.
    # Never use this exception for native artifacts or truncate a loaded file.
    if [[ "${has_realtime}" -eq 0 ]]; then
        return 0
    fi
    local status
    if pgrep -x rtapi_app >/dev/null 2>&1; then
        echo "refusing to replace native artifacts while rtapi_app is active; close the previous LinuxCNC session through its UI" >&2
        return 1
    else
        status=$?
    fi
    if [[ "${status}" -ne 1 ]]; then
        echo "cannot determine whether rtapi_app is stopped: pgrep exited ${status}; resolve this process-list error before retrying" >&2
        return 1
    fi
}

cleanup_staged_artifacts() {
    local failed=0
    local path
    for path in "${staged_artifacts[@]}"; do
        if [[ -n "${path}" && ( -e "${path}" || -L "${path}" ) ]]; then
            if ! rm -f -- "${path}"; then
                echo "failed to remove unused staged release: ${path}; correct the reported filesystem error and retry installation" >&2
                failed=1
            fi
        fi
    done
    return "${failed}"
}

validate_installed_target() {
    local index="$1"
    local target_path="${target_modules[index]}"
    if [[ -L "${target_path}" ]]; then
        echo "installed native-artifact target is a symlink: ${target_path}; restore the expected regular installation target before retrying" >&2
        return 1
    fi
    if [[ -e "${target_path}" && ! -f "${target_path}" ]]; then
        echo "installed native-artifact target is not a regular file: ${target_path}; restore the expected installation target before retrying" >&2
        return 1
    fi
}

stage_artifact() {
    local index="$1"
    local source_path="${source_modules[index]}"
    local target_path="${target_modules[index]}"
    local target_directory="${target_path%/*}"
    local target_name="${target_path##*/}"
    local temporary_path

    if ! temporary_path="$(mktemp "${target_directory}/.${target_name}.stage.XXXXXX")"; then
        echo "failed to stage new release in ${target_directory}; resolve the reported filesystem error and retry installation" >&2
        return 1
    fi
    staged_artifacts[index]="${temporary_path}"
    if ! install -m 0755 "${source_path}" "${temporary_path}"; then
        echo "failed to stage new release: ${source_path}; resolve the reported copy error and retry installation" >&2
        return 1
    fi
    if ! cmp --silent "${source_path}" "${temporary_path}"; then
        echo "staged artifact differs from release: ${source_path}; rebuild the named release and retry installation" >&2
        return 1
    fi
}

install_artifacts() {
    local index
    require_realtime_host_stopped || return 1
    for index in "${!target_modules[@]}"; do
        validate_installed_target "${index}" || return 1
    done
    # Check every new artifact before replacing any installed file.
    for index in "${!source_modules[@]}"; do
        stage_artifact "${index}" || return 1
    done
    for index in "${!source_modules[@]}"; do
        require_realtime_host_stopped || return 1
        validate_installed_target "${index}" || return 1
        # Each rename is atomic and preserves already-loaded inodes. No copy
        # of an old artifact is created. A partial installation is retried by
        # the standard launcher, which refuses mismatched system artifacts.
        if ! mv -fT -- "${staged_artifacts[index]}" "${target_modules[index]}"; then
            echo "failed to replace artifact: ${target_modules[index]}; resolve the reported rename error and retry installation" >&2
            return 1
        fi
        staged_artifacts[index]=""
        if ! cmp --silent "${source_modules[index]}" "${target_modules[index]}"; then
            echo "installed artifact differs from release: ${target_modules[index]}; rebuild the named release and retry installation" >&2
            return 1
        fi
    done
    require_realtime_host_stopped
}

handle_install_exit() {
    local exit_status="$1"
    trap - EXIT HUP INT TERM
    if ! cleanup_staged_artifacts; then
        if [[ "${exit_status}" -eq 0 ]]; then
            exit_status=1
        fi
    fi
    if [[ "${exit_status}" -ne 0 ]]; then
        echo "Installation stopped; any artifacts already replaced remain installed. Resolve the reported cause, then open DMC2 LinuxCNC from Applications to retry synchronization. The standard launcher refuses to start LinuxCNC until every artifact matches its staged release." >&2
    fi
    exit "${exit_status}"
}

handle_install_signal() {
    echo "native-artifact installation interrupted by $1" >&2
    exit "$2"
}

validate_module_inventory() {
    local index
    if [[ "${#source_modules[@]}" -ne "${#target_modules[@]}" ||
          "${#source_modules[@]}" -ne "${#artifact_kinds[@]}" ||
          "${#source_modules[@]}" -eq 0 ]]; then
        echo "native-artifact source/target inventory is inconsistent; restore the installer inventory before retrying" >&2
        return 1
    fi
    for index in "${!source_modules[@]}"; do
        if [[ ! -f "${source_modules[index]}" || -L "${source_modules[index]}" ]]; then
            echo "missing regular staged artifact: ${source_modules[index]}; rebuild and stage the named release before retrying" >&2
            return 1
        fi
    done
}

main() {
    local index
    trap 'handle_install_exit "$?"' EXIT
    trap 'handle_install_signal HUP 129' HUP
    trap 'handle_install_signal INT 130' INT
    trap 'handle_install_signal TERM 143' TERM
    if [[ "${EUID}" -ne 0 ]]; then
        echo "installation requires root privileges; launch DMC2 LinuxCNC from Applications to use the configured installer" >&2
        return 1
    fi
    select_artifacts "$@" || return 1
    validate_module_inventory || return 1
    install_artifacts || return 1
    for index in "${!target_modules[@]}"; do
        echo "installed matching artifact: ${target_modules[index]}"
    done
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
