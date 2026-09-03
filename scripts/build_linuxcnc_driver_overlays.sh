#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
catalog_path="${project_dir}/config/linuxcnc-driver-overlays.tsv"
vendor_dir="${project_dir}/vendor/linuxcnc-2.9.10"

expected_signature=$'DMC2_LINUXCNC_DRIVER_OVERLAY_CATALOG\t1'
expected_columns=$'module\tlinuxcnc_version\tbase_commit\tpatch\tpatch_sha256\tupstream_commits\tupstream_scope\tbuild_directory\tentry_source\tstaged_artifact\tdeployed_artifact\treason'

mapfile -t catalog_lines < "${catalog_path}"
if [[ "${#catalog_lines[@]}" -lt 3 ]]; then
    echo "LinuxCNC driver-overlay catalog has no data rows" >&2
    exit 1
fi
if [[ "${catalog_lines[0]}" != "${expected_signature}" ]]; then
    echo "LinuxCNC driver-overlay catalog signature/version mismatch" >&2
    exit 1
fi
if [[ "${catalog_lines[1]}" != "${expected_columns}" ]]; then
    echo "LinuxCNC driver-overlay catalog columns mismatch" >&2
    exit 1
fi

declare -a modules=()
declare -a patches=()
declare -a build_directories=()
declare -a entry_sources=()
declare -a staged_artifacts=()
declare -a deployed_artifacts=()
catalog_linuxcnc_version=""
catalog_base_commit=""

require_relative_path() {
    local value="$1"
    local label="$2"
    if [[ -z "${value}" || "${value}" == /* || "${value}" == ".." ||
          "${value}" == ../* || "${value}" == */../* || "${value}" == */.. ]]; then
        echo "invalid ${label} in LinuxCNC driver-overlay catalog: ${value}" >&2
        exit 1
    fi
}

for ((line_index = 2; line_index < ${#catalog_lines[@]}; line_index++)); do
    line_number=$((line_index + 1))
    line="${catalog_lines[line_index]}"
    if [[ -z "${line}" ]]; then
        echo "blank row in LinuxCNC driver-overlay catalog at line ${line_number}" >&2
        exit 1
    fi
    IFS=$'\t' read -r \
        module linuxcnc_version base_commit patch_relative patch_sha256 \
        upstream_commits upstream_scope build_directory entry_source \
        staged_artifact deployed_artifact reason <<< "${line}"
    for required in \
        module linuxcnc_version base_commit patch_relative patch_sha256 \
        upstream_commits upstream_scope build_directory entry_source \
        staged_artifact deployed_artifact reason; do
        if [[ -z "${!required}" ]]; then
            echo "missing ${required} in LinuxCNC driver-overlay catalog line ${line_number}" >&2
            exit 1
        fi
    done
    require_relative_path "${patch_relative}" "patch path"
    require_relative_path "${build_directory}" "build directory"
    require_relative_path "${entry_source}" "entry source"
    require_relative_path "${staged_artifact}" "staged artifact"
    if [[ "${entry_source}" != "${module}.c" ]]; then
        echo "module and entry source disagree at catalog line ${line_number}" >&2
        exit 1
    fi
    if [[ "${deployed_artifact}" != "/usr/lib/linuxcnc/modules/${module}.so" ]]; then
        echo "unsupported deployed artifact at catalog line ${line_number}: ${deployed_artifact}" >&2
        exit 1
    fi
    if [[ -z "${catalog_linuxcnc_version}" ]]; then
        catalog_linuxcnc_version="${linuxcnc_version}"
        catalog_base_commit="${base_commit}"
    elif [[ "${linuxcnc_version}" != "${catalog_linuxcnc_version}" ||
            "${base_commit}" != "${catalog_base_commit}" ]]; then
        echo "all LinuxCNC driver overlays must share one pinned base" >&2
        exit 1
    fi
    patch_path="${project_dir}/${patch_relative}"
    observed_patch_sha256="$(sha256sum "${patch_path}" | awk '{print $1}')"
    if [[ "${observed_patch_sha256}" != "${patch_sha256}" ]]; then
        echo "patch checksum mismatch: ${patch_relative}" >&2
        exit 1
    fi
    modules+=("${module}")
    patches+=("${patch_path}")
    build_directories+=("${build_directory}")
    entry_sources+=("${entry_source}")
    staged_artifacts+=("${staged_artifact}")
    deployed_artifacts+=("${deployed_artifact}")
done

observed_linuxcnc_version="$(linuxcnc_var LINUXCNCVERSION)"
if [[ "${observed_linuxcnc_version}" != "${catalog_linuxcnc_version}" ]]; then
    echo "installed LinuxCNC version ${observed_linuxcnc_version} does not match overlay base ${catalog_linuxcnc_version}" >&2
    exit 1
fi
observed_base_commit="$(git -C "${vendor_dir}" rev-parse HEAD)"
if [[ "${observed_base_commit}" != "${catalog_base_commit}" ]]; then
    echo "vendored LinuxCNC commit ${observed_base_commit} does not match overlay base ${catalog_base_commit}" >&2
    exit 1
fi
if [[ -n "$(git -C "${vendor_dir}" status --porcelain=v1 --untracked-files=all)" ]]; then
    echo "vendored LinuxCNC source is not pristine" >&2
    exit 1
fi

temporary_root="$(mktemp -d "${project_dir}/var/tmp/linuxcnc-driver-overlay.XXXXXX")"
cleanup() {
    if [[ -n "${temporary_root:-}" && "${temporary_root}" == "${project_dir}/var/tmp/linuxcnc-driver-overlay."* ]]; then
        rm -rf -- "${temporary_root}"
    fi
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

source_root="${temporary_root}/source"
mkdir -p -- "${source_root}"
git -C "${vendor_dir}" archive --format=tar "${catalog_base_commit}" |
    tar -xf - -C "${source_root}"

for patch_path in "${patches[@]}"; do
    patch --directory="${source_root}" --strip=1 --batch --forward --fuzz=0 < "${patch_path}"
done

declare -a built_artifacts=()
for index in "${!modules[@]}"; do
    module="${modules[index]}"
    build_directory="${source_root}/${build_directories[index]}"
    entry_source="${entry_sources[index]}"
    (
        cd -- "${build_directory}"
        halcompile --compile "${entry_source}"
    )
    built_artifact="${build_directory}/${module}.so"
    if [[ ! -f "${built_artifact}" || -L "${built_artifact}" ]]; then
        echo "halcompile did not produce a regular ${module}.so" >&2
        exit 1
    fi
    # halcompile deliberately adds debug paths from its random temporary
    # directory. Remove only those non-runtime sections so identical reviewed
    # source produces an identical deployable module and checksum.
    objcopy --strip-debug --remove-section=.note.gnu.build-id "${built_artifact}"
    if ! nm -D --defined-only "${built_artifact}" |
        awk '$3 == "rtapi_app_main" { found = 1 } END { exit !found }'; then
        echo "built ${module}.so does not export rtapi_app_main" >&2
        exit 1
    fi
    deployed_artifact="${deployed_artifacts[index]}"
    if [[ -f "${deployed_artifact}" && ! -L "${deployed_artifact}" ]]; then
        built_exports="${temporary_root}/${module}.built.exports"
        deployed_exports="${temporary_root}/${module}.deployed.exports"
        nm -D --defined-only "${built_artifact}" | awk '{print $2, $3}' | sort > "${built_exports}"
        nm -D --defined-only "${deployed_artifact}" | awk '{print $2, $3}' | sort > "${deployed_exports}"
        if ! cmp --silent "${built_exports}" "${deployed_exports}"; then
            echo "built ${module}.so changes the installed module's exported-symbol contract" >&2
            exit 1
        fi
    fi
    staged_copy="${temporary_root}/artifacts/${staged_artifacts[index]}"
    install -D -m 0644 -- "${built_artifact}" "${staged_copy}"
    built_artifacts+=("${staged_copy}")
done

for index in "${!modules[@]}"; do
    destination="${project_dir}/${staged_artifacts[index]}"
    install -D -m 0644 -- "${built_artifacts[index]}" "${destination}"
    artifact_sha256="$(sha256sum "${destination}" | awk '{print $1}')"
    printf 'built LinuxCNC %s overlay: %s sha256=%s\n' \
        "${catalog_linuxcnc_version}" "${destination}" "${artifact_sha256}"
done
