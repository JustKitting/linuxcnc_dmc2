#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source_module="${project_dir}/rust/target/release/libdmc2_rt.so"
target_module="/usr/lib/linuxcnc/modules/dmc2_rt.so"

require_realtime_host_stopped() {
    local status
    if pgrep -x rtapi_app >/dev/null 2>&1; then
        echo "refusing to replace dmc2_rt.so while rtapi_app is active" >&2
        return 1
    else
        status=$?
    fi
    if [[ "${status}" -ne 1 ]]; then
        echo "cannot prove rtapi_app is stopped: pgrep exited ${status}" >&2
        return 1
    fi
}

install_module_atomically() {
    local source_path="$1"
    local target_path="$2"
    local target_directory
    local temporary_path

    if ! target_directory="$(dirname -- "${target_path}")"; then
        echo "failed to resolve realtime-module target directory" >&2
        return 1
    fi
    if ! temporary_path="$(mktemp "${target_directory}/.dmc2_rt.so.XXXXXX")"; then
        echo "failed to create temporary realtime module in ${target_directory}" >&2
        return 1
    fi
    if ! install -m 0755 "${source_path}" "${temporary_path}"; then
        echo "failed to stage verified realtime module" >&2
        if ! rm -f -- "${temporary_path}"; then
            echo "failed to remove temporary realtime module: ${temporary_path}" >&2
        fi
        return 1
    fi
    if ! cmp --silent "${source_path}" "${temporary_path}"; then
        echo "staged realtime module differs from verified release module" >&2
        if ! rm -f -- "${temporary_path}"; then
            echo "failed to remove temporary realtime module: ${temporary_path}" >&2
        fi
        return 1
    fi
    if ! mv -f -- "${temporary_path}" "${target_path}"; then
        echo "failed to atomically replace installed realtime module" >&2
        if ! rm -f -- "${temporary_path}"; then
            echo "failed to remove temporary realtime module: ${temporary_path}" >&2
        fi
        return 1
    fi
    if ! cmp --silent "${source_path}" "${target_path}"; then
        echo "installed realtime module failed final byte verification" >&2
        return 1
    fi
}

main() {
    if [[ "${EUID}" -ne 0 ]]; then
        echo "run this exact installer as root" >&2
        return 1
    fi
    if [[ ! -f "${source_module}" ]]; then
        echo "missing staged realtime module; run scripts/build_native.sh first" >&2
        return 1
    fi

    require_realtime_host_stopped
    install_module_atomically "${source_module}" "${target_module}"
    require_realtime_host_stopped
    echo "installed exact verified module: ${target_module}"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
