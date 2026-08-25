#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
maximum_lines=1000
failures=0

while IFS= read -r -d '' source; do
    relative="${source#"${project_dir}/"}"
    case "${relative}" in
        rust/target/*|archive/*|reference/*|tests/*|vendor/*)
            continue
            ;;
    esac
    lines="$(wc -l < "${source}")"
    if (( lines > maximum_lines )); then
        echo "source module exceeds ${maximum_lines} lines: ${relative} (${lines})" >&2
        failures=1
    fi
done < <(
    find "${project_dir}" -type f \
        \( -name '*.rs' -o -name '*.py' -o -name '*.c' -o -name '*.cc' \) \
        -print0
)

if find "${project_dir}" -type d \
    \( -name __pycache__ -o -name .pytest_cache \) \
    -not -path "${project_dir}/rust/target/*" \
    -not -path "${project_dir}/vendor/*" -print -quit | grep -q .; then
    echo "generated Python cache exists inside the project tree" >&2
    failures=1
fi

if grep -RInE '^[[:space:]]*loadusr([^#]*[[:space:]])python(3)?([[:space:]]|$)' \
    "${project_dir}/live"; then
    echo "live LinuxCNC configuration invokes Python as a HAL component" >&2
    failures=1
fi

if grep -RInE '(archive/|reference/|var/tmp/)' \
    "${project_dir}/live"; then
    echo "live configuration depends on a reference, archive, or temporary file" >&2
    failures=1
fi

if (( failures != 0 )); then
    exit 1
fi

echo "source layout and live-path boundaries passed"
