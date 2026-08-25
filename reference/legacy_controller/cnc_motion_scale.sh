#!/usr/bin/env bash

# Shared exact integer scaling for Bash motion programs. This file is sourced;
# it never starts HAL or sends a hardware command.
CNC_MOTION_SCALE_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
CNC_MOTION_CONFIG_PATH="${CNC_MOTION_SCALE_DIR}/../../config/machine-pulses.conf"

if [[ ! -r "$CNC_MOTION_CONFIG_PATH" ]]; then
    echo "REFUSED: cannot read ${CNC_MOTION_CONFIG_PATH}; no motion sent." >&2
    return 1 2>/dev/null || exit 1
fi

# shellcheck source=../../config/machine-pulses.conf
source "$CNC_MOTION_CONFIG_PATH"

for cnc_motion_value_name in MOTOR_PULSES_PER_REV REFERENCE_PULSES_PER_REV; do
    cnc_motion_value=${!cnc_motion_value_name:-}
    if [[ ! "$cnc_motion_value" =~ ^[1-9][0-9]*$ ]]; then
        echo "REFUSED: ${cnc_motion_value_name} must be a positive integer; no motion sent." >&2
        return 1 2>/dev/null || exit 1
    fi
done
readonly MOTOR_PULSES_PER_REV REFERENCE_PULSES_PER_REV

scale_reference_value() {
    local reference_value=$1
    local scaled_numerator

    if [[ ! "$reference_value" =~ ^[0-9]+$ ]]; then
        echo "REFUSED: reference pulse value must be a non-negative integer; no motion sent." >&2
        return 1
    fi
    scaled_numerator=$((reference_value * MOTOR_PULSES_PER_REV))
    if (( scaled_numerator % REFERENCE_PULSES_PER_REV != 0 )); then
        echo "REFUSED: ${reference_value} reference pulses cannot be scaled exactly from ${REFERENCE_PULSES_PER_REV} to ${MOTOR_PULSES_PER_REV} pulses/revolution; no motion sent." >&2
        return 1
    fi
    printf '%d\n' "$((scaled_numerator / REFERENCE_PULSES_PER_REV))"
}
