#!/usr/bin/env bash
# Reusable OFFLINE example. All geometry and trigger records are synthetic.
# Calls only dmc2ctl object-map file operations; never a machine command.
set -euo pipefail
if [[ $# != 1 ]]; then
    echo 'Usage: bash examples/positional-mapper/run-example.sh NEW_DIRECTORY' >&2
    exit 1
fi
project=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
example="$project/examples/positional-mapper"
output="$1"
if [[ -e "$output" || -L "$output" ]]; then
    echo "Output already exists: $output. Choose a new directory; existing data was preserved." >&2
    exit 1
fi
if [[ ! -x "$project/native/bin/dmc2ctl" ]]; then
    echo "Standard dmc2ctl binary is absent. Build/install the project's native program, then retry." >&2
    exit 1
fi
mkdir -- "$output"
output=$(CDPATH='' cd -- "$output" && pwd)
ctl=("$project/native/bin/dmc2ctl" object-map --store "$output/store")
"${ctl[@]}" create example 'Synthetic positional-mapper example' > "$output/create.json"
"${ctl[@]}" add-setup example initial 'Synthetic setup, not machine observations' > "$output/setup.json"
"${ctl[@]}" attach-design example reference "$example/reference.stl" > "$output/design.json"
"${ctl[@]}" import-capture example initial synthetic "$example/synthetic-ledger.txt" > "$output/capture.json"
"${ctl[@]}" inspect-stl "$example/reference.stl" 1 > "$output/mesh.json"
"${ctl[@]}" prepare-fit example initial reference > "$output/template.txt"
"${ctl[@]}" fit example initial fitted "$example/fit-request.txt" > "$output/fit.json"
"${ctl[@]}" locate example initial fitted "$example/locations.csv" "$output/locations.csv"
"${ctl[@]}" export-fit example initial fitted "$output/exchange"
echo "Synthetic analysis output: $output/exchange/manifest.json"
