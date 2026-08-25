use std::ptr;

use dmc2_hal_sys as hal;

pub(super) const SNAPSHOT_GENERATION_PIN: &str = "snapshot-generation";
pub(super) const BIT_OUTPUT_PINS: [&str; 8] = [
    "connected",
    "serial-fault",
    "quadrature-fault",
    "link-healthy",
    "heartbeat",
    "estop-pressed",
    "deadman-held",
    "selector-valid",
];
pub(super) const AXIS_OUTPUT_PINS: [&str; 7] = [
    "axis-x",
    "axis-y",
    "axis-z",
    "axis-4",
    "axis-5",
    "axis-off",
    "axis-invalid",
];
pub(super) const MULTIPLIER_OUTPUT_PINS: [&str; 5] = [
    "multiplier-x1",
    "multiplier-x10",
    "multiplier-x100",
    "multiplier-off",
    "multiplier-invalid",
];
pub(super) const S32_OUTPUT_PINS: [&str; 5] = [
    "axis-code",
    "multiplier-code",
    "latest-detent",
    "detent-count",
    "transition-count",
];
pub(super) const U32_OUTPUT_PINS: [&str; 6] = [
    "quadrature-errors",
    "sequence",
    "milliseconds",
    "protocol-errors",
    "dropped-packets",
    "timeouts",
];
pub(super) const PACKET_AGE_PIN: &str = "packet-age-ms";

pub(super) struct HalPins {
    pub(super) snapshot_generation: *mut hal::hal_u32_t,
    pub(super) connected: *mut hal::hal_bit_t,
    pub(super) serial_fault: *mut hal::hal_bit_t,
    pub(super) quadrature_fault: *mut hal::hal_bit_t,
    pub(super) link_healthy: *mut hal::hal_bit_t,
    pub(super) heartbeat: *mut hal::hal_bit_t,
    pub(super) estop_pressed: *mut hal::hal_bit_t,
    pub(super) deadman_held: *mut hal::hal_bit_t,
    pub(super) selector_valid: *mut hal::hal_bit_t,
    pub(super) axis: [*mut hal::hal_bit_t; 7],
    pub(super) multiplier: [*mut hal::hal_bit_t; 5],
    pub(super) axis_code: *mut hal::hal_s32_t,
    pub(super) multiplier_code: *mut hal::hal_s32_t,
    pub(super) latest_detent: *mut hal::hal_s32_t,
    pub(super) detent_count: *mut hal::hal_s32_t,
    pub(super) transition_count: *mut hal::hal_s32_t,
    pub(super) quadrature_errors: *mut hal::hal_u32_t,
    pub(super) sequence: *mut hal::hal_u32_t,
    pub(super) milliseconds: *mut hal::hal_u32_t,
    pub(super) protocol_errors: *mut hal::hal_u32_t,
    pub(super) dropped_packets: *mut hal::hal_u32_t,
    pub(super) timeouts: *mut hal::hal_u32_t,
    pub(super) packet_age_ms: *mut hal::real_t,
}

impl HalPins {
    pub(super) const fn empty() -> Self {
        Self {
            snapshot_generation: ptr::null_mut(),
            connected: ptr::null_mut(),
            serial_fault: ptr::null_mut(),
            quadrature_fault: ptr::null_mut(),
            link_healthy: ptr::null_mut(),
            heartbeat: ptr::null_mut(),
            estop_pressed: ptr::null_mut(),
            deadman_held: ptr::null_mut(),
            selector_valid: ptr::null_mut(),
            axis: [ptr::null_mut(); 7],
            multiplier: [ptr::null_mut(); 5],
            axis_code: ptr::null_mut(),
            multiplier_code: ptr::null_mut(),
            latest_detent: ptr::null_mut(),
            detent_count: ptr::null_mut(),
            transition_count: ptr::null_mut(),
            quadrature_errors: ptr::null_mut(),
            sequence: ptr::null_mut(),
            milliseconds: ptr::null_mut(),
            protocol_errors: ptr::null_mut(),
            dropped_packets: ptr::null_mut(),
            timeouts: ptr::null_mut(),
            packet_age_ms: ptr::null_mut(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Kind {
        Bit,
        S32,
        U32,
        Float,
    }

    fn schema() -> Vec<(&'static str, Kind, &'static str)> {
        let mut pins = vec![(SNAPSHOT_GENERATION_PIN, Kind::U32, "out")];
        pins.extend(BIT_OUTPUT_PINS.map(|name| (name, Kind::Bit, "out")));
        pins.extend(AXIS_OUTPUT_PINS.map(|name| (name, Kind::Bit, "out")));
        pins.extend(MULTIPLIER_OUTPUT_PINS.map(|name| (name, Kind::Bit, "out")));
        pins.extend(S32_OUTPUT_PINS.map(|name| (name, Kind::S32, "out")));
        pins.extend(U32_OUTPUT_PINS.map(|name| (name, Kind::U32, "out")));
        pins.push((PACKET_AGE_PIN, Kind::Float, "out"));
        pins
    }

    #[test]
    fn exported_hal_schema_is_exact_complete_and_unique() {
        let expected = vec![
            ("snapshot-generation", Kind::U32, "out"),
            ("connected", Kind::Bit, "out"),
            ("serial-fault", Kind::Bit, "out"),
            ("quadrature-fault", Kind::Bit, "out"),
            ("link-healthy", Kind::Bit, "out"),
            ("heartbeat", Kind::Bit, "out"),
            ("estop-pressed", Kind::Bit, "out"),
            ("deadman-held", Kind::Bit, "out"),
            ("selector-valid", Kind::Bit, "out"),
            ("axis-x", Kind::Bit, "out"),
            ("axis-y", Kind::Bit, "out"),
            ("axis-z", Kind::Bit, "out"),
            ("axis-4", Kind::Bit, "out"),
            ("axis-5", Kind::Bit, "out"),
            ("axis-off", Kind::Bit, "out"),
            ("axis-invalid", Kind::Bit, "out"),
            ("multiplier-x1", Kind::Bit, "out"),
            ("multiplier-x10", Kind::Bit, "out"),
            ("multiplier-x100", Kind::Bit, "out"),
            ("multiplier-off", Kind::Bit, "out"),
            ("multiplier-invalid", Kind::Bit, "out"),
            ("axis-code", Kind::S32, "out"),
            ("multiplier-code", Kind::S32, "out"),
            ("latest-detent", Kind::S32, "out"),
            ("detent-count", Kind::S32, "out"),
            ("transition-count", Kind::S32, "out"),
            ("quadrature-errors", Kind::U32, "out"),
            ("sequence", Kind::U32, "out"),
            ("milliseconds", Kind::U32, "out"),
            ("protocol-errors", Kind::U32, "out"),
            ("dropped-packets", Kind::U32, "out"),
            ("timeouts", Kind::U32, "out"),
            ("packet-age-ms", Kind::Float, "out"),
        ];
        let actual = schema();
        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 33);
        assert_eq!(
            actual
                .iter()
                .map(|(name, _, _)| *name)
                .collect::<BTreeSet<_>>()
                .len(),
            actual.len(),
            "HAL pin suffixes must be unique"
        );
    }
}
