use std::ptr;

use dmc2_hal_sys as hal;

pub(super) const SNAPSHOT_GENERATION_PIN: &str = "snapshot-generation";
pub(super) const CONNECTION_BIT_OUTPUT_PINS: [&str; 2] = ["connected", "fault"];
pub(super) const RUNTIME_U32_OUTPUT_PINS: [&str; 3] =
    ["task-heartbeat", "publications", "poll-errors"];
pub(super) const DIAGNOSTIC_BIT_OUTPUT_PINS: [&str; 4] = [
    "nml-error-known",
    "linuxcnc-error-active",
    "linuxcnc-warning-active",
    "unknown-code-active",
];
pub(super) const DIAGNOSTIC_U32_OUTPUT_PINS: [&str; 20] = [
    "active-error-mask-low",
    "active-error-mask-high",
    "active-warning-mask-low",
    "active-warning-mask-high",
    "latched-error-mask-low",
    "latched-error-mask-high",
    "latched-warning-mask-low",
    "latched-warning-mask-high",
    "unknown-domain-mask-low",
    "unknown-domain-mask-high",
    "diagnostic-count",
    "unknown-code-count",
    "diagnostic-transitions",
    "latest-code-low",
    "latest-code-high",
    "catalog-code-count",
    "catalog-fingerprint-low",
    "catalog-fingerprint-high",
    "snapshot-abi-version",
    "snapshot-struct-size",
];
pub(super) const DIAGNOSTIC_S32_OUTPUT_PINS: [&str; 4] = [
    "nml-error-code",
    "latest-code-domain",
    "latest-severity",
    "latest-action",
];
pub(super) const CLEAR_LATCHED_INPUT_PIN: &str = "clear-latched";
pub(super) const MACHINE_BIT_OUTPUT_PINS: [&str; 6] = [
    "machine-on",
    "estopped",
    "manual-mode",
    "joint-mode",
    "teleop-mode",
    "interp-idle",
];

pub(super) struct HalPins {
    pub(super) snapshot_generation: *mut hal::hal_u32_t,
    pub(super) connected: *mut hal::hal_bit_t,
    pub(super) fault: *mut hal::hal_bit_t,
    pub(super) task_heartbeat: *mut hal::hal_u32_t,
    pub(super) publications: *mut hal::hal_u32_t,
    pub(super) poll_errors: *mut hal::hal_u32_t,
    pub(super) nml_error_code: *mut hal::hal_s32_t,
    pub(super) nml_error_known: *mut hal::hal_bit_t,
    pub(super) linuxcnc_error_active: *mut hal::hal_bit_t,
    pub(super) linuxcnc_warning_active: *mut hal::hal_bit_t,
    pub(super) unknown_code_active: *mut hal::hal_bit_t,
    pub(super) active_error_mask_low: *mut hal::hal_u32_t,
    pub(super) active_error_mask_high: *mut hal::hal_u32_t,
    pub(super) active_warning_mask_low: *mut hal::hal_u32_t,
    pub(super) active_warning_mask_high: *mut hal::hal_u32_t,
    pub(super) latched_error_mask_low: *mut hal::hal_u32_t,
    pub(super) latched_error_mask_high: *mut hal::hal_u32_t,
    pub(super) latched_warning_mask_low: *mut hal::hal_u32_t,
    pub(super) latched_warning_mask_high: *mut hal::hal_u32_t,
    pub(super) unknown_domain_mask_low: *mut hal::hal_u32_t,
    pub(super) unknown_domain_mask_high: *mut hal::hal_u32_t,
    pub(super) diagnostic_count: *mut hal::hal_u32_t,
    pub(super) unknown_code_count: *mut hal::hal_u32_t,
    pub(super) diagnostic_transitions: *mut hal::hal_u32_t,
    pub(super) latest_code_domain: *mut hal::hal_s32_t,
    pub(super) latest_code_low: *mut hal::hal_u32_t,
    pub(super) latest_code_high: *mut hal::hal_u32_t,
    pub(super) latest_severity: *mut hal::hal_s32_t,
    pub(super) latest_action: *mut hal::hal_s32_t,
    pub(super) clear_latched: *mut hal::hal_bit_t,
    pub(super) catalog_code_count: *mut hal::hal_u32_t,
    pub(super) catalog_fingerprint_low: *mut hal::hal_u32_t,
    pub(super) catalog_fingerprint_high: *mut hal::hal_u32_t,
    pub(super) snapshot_abi_version: *mut hal::hal_u32_t,
    pub(super) snapshot_struct_size: *mut hal::hal_u32_t,
    pub(super) machine_on: *mut hal::hal_bit_t,
    pub(super) estopped: *mut hal::hal_bit_t,
    pub(super) manual_mode: *mut hal::hal_bit_t,
    pub(super) joint_mode: *mut hal::hal_bit_t,
    pub(super) teleop_mode: *mut hal::hal_bit_t,
    pub(super) interp_idle: *mut hal::hal_bit_t,
    pub(super) homed: [*mut hal::hal_bit_t; 3],
    pub(super) homing: [*mut hal::hal_bit_t; 3],
    pub(super) axis_stopped: [*mut hal::hal_bit_t; 3],
}

impl HalPins {
    pub(super) const fn empty() -> Self {
        Self {
            snapshot_generation: ptr::null_mut(),
            connected: ptr::null_mut(),
            fault: ptr::null_mut(),
            task_heartbeat: ptr::null_mut(),
            publications: ptr::null_mut(),
            poll_errors: ptr::null_mut(),
            nml_error_code: ptr::null_mut(),
            nml_error_known: ptr::null_mut(),
            linuxcnc_error_active: ptr::null_mut(),
            linuxcnc_warning_active: ptr::null_mut(),
            unknown_code_active: ptr::null_mut(),
            active_error_mask_low: ptr::null_mut(),
            active_error_mask_high: ptr::null_mut(),
            active_warning_mask_low: ptr::null_mut(),
            active_warning_mask_high: ptr::null_mut(),
            latched_error_mask_low: ptr::null_mut(),
            latched_error_mask_high: ptr::null_mut(),
            latched_warning_mask_low: ptr::null_mut(),
            latched_warning_mask_high: ptr::null_mut(),
            unknown_domain_mask_low: ptr::null_mut(),
            unknown_domain_mask_high: ptr::null_mut(),
            diagnostic_count: ptr::null_mut(),
            unknown_code_count: ptr::null_mut(),
            diagnostic_transitions: ptr::null_mut(),
            latest_code_domain: ptr::null_mut(),
            latest_code_low: ptr::null_mut(),
            latest_code_high: ptr::null_mut(),
            latest_severity: ptr::null_mut(),
            latest_action: ptr::null_mut(),
            clear_latched: ptr::null_mut(),
            catalog_code_count: ptr::null_mut(),
            catalog_fingerprint_low: ptr::null_mut(),
            catalog_fingerprint_high: ptr::null_mut(),
            snapshot_abi_version: ptr::null_mut(),
            snapshot_struct_size: ptr::null_mut(),
            machine_on: ptr::null_mut(),
            estopped: ptr::null_mut(),
            manual_mode: ptr::null_mut(),
            joint_mode: ptr::null_mut(),
            teleop_mode: ptr::null_mut(),
            interp_idle: ptr::null_mut(),
            homed: [ptr::null_mut(); 3],
            homing: [ptr::null_mut(); 3],
            axis_stopped: [ptr::null_mut(); 3],
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
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct Spec {
        name: String,
        kind: Kind,
        direction: &'static str,
    }

    fn spec(name: impl Into<String>, kind: Kind, direction: &'static str) -> Spec {
        Spec {
            name: name.into(),
            kind,
            direction,
        }
    }

    fn schema() -> Vec<Spec> {
        let mut pins = vec![spec(SNAPSHOT_GENERATION_PIN, Kind::U32, "out")];
        pins.extend(CONNECTION_BIT_OUTPUT_PINS.map(|name| spec(name, Kind::Bit, "out")));
        pins.extend(RUNTIME_U32_OUTPUT_PINS.map(|name| spec(name, Kind::U32, "out")));
        pins.extend(DIAGNOSTIC_BIT_OUTPUT_PINS.map(|name| spec(name, Kind::Bit, "out")));
        pins.extend(DIAGNOSTIC_U32_OUTPUT_PINS.map(|name| spec(name, Kind::U32, "out")));
        pins.extend(DIAGNOSTIC_S32_OUTPUT_PINS.map(|name| spec(name, Kind::S32, "out")));
        pins.push(spec(CLEAR_LATCHED_INPUT_PIN, Kind::Bit, "in"));
        pins.extend(MACHINE_BIT_OUTPUT_PINS.map(|name| spec(name, Kind::Bit, "out")));
        for index in 0..3 {
            pins.push(spec(format!("joint-{index}-homed"), Kind::Bit, "out"));
            pins.push(spec(format!("joint-{index}-homing"), Kind::Bit, "out"));
            pins.push(spec(format!("axis-{index}-stopped"), Kind::Bit, "out"));
        }
        pins
    }

    #[test]
    fn exported_hal_schema_is_exact_complete_and_unique() {
        let expected = [
            ("snapshot-generation", Kind::U32, "out"),
            ("connected", Kind::Bit, "out"),
            ("fault", Kind::Bit, "out"),
            ("task-heartbeat", Kind::U32, "out"),
            ("publications", Kind::U32, "out"),
            ("poll-errors", Kind::U32, "out"),
            ("nml-error-known", Kind::Bit, "out"),
            ("linuxcnc-error-active", Kind::Bit, "out"),
            ("linuxcnc-warning-active", Kind::Bit, "out"),
            ("unknown-code-active", Kind::Bit, "out"),
            ("active-error-mask-low", Kind::U32, "out"),
            ("active-error-mask-high", Kind::U32, "out"),
            ("active-warning-mask-low", Kind::U32, "out"),
            ("active-warning-mask-high", Kind::U32, "out"),
            ("latched-error-mask-low", Kind::U32, "out"),
            ("latched-error-mask-high", Kind::U32, "out"),
            ("latched-warning-mask-low", Kind::U32, "out"),
            ("latched-warning-mask-high", Kind::U32, "out"),
            ("unknown-domain-mask-low", Kind::U32, "out"),
            ("unknown-domain-mask-high", Kind::U32, "out"),
            ("diagnostic-count", Kind::U32, "out"),
            ("unknown-code-count", Kind::U32, "out"),
            ("diagnostic-transitions", Kind::U32, "out"),
            ("latest-code-low", Kind::U32, "out"),
            ("latest-code-high", Kind::U32, "out"),
            ("catalog-code-count", Kind::U32, "out"),
            ("catalog-fingerprint-low", Kind::U32, "out"),
            ("catalog-fingerprint-high", Kind::U32, "out"),
            ("snapshot-abi-version", Kind::U32, "out"),
            ("snapshot-struct-size", Kind::U32, "out"),
            ("nml-error-code", Kind::S32, "out"),
            ("latest-code-domain", Kind::S32, "out"),
            ("latest-severity", Kind::S32, "out"),
            ("latest-action", Kind::S32, "out"),
            ("clear-latched", Kind::Bit, "in"),
            ("machine-on", Kind::Bit, "out"),
            ("estopped", Kind::Bit, "out"),
            ("manual-mode", Kind::Bit, "out"),
            ("joint-mode", Kind::Bit, "out"),
            ("teleop-mode", Kind::Bit, "out"),
            ("interp-idle", Kind::Bit, "out"),
            ("joint-0-homed", Kind::Bit, "out"),
            ("joint-0-homing", Kind::Bit, "out"),
            ("axis-0-stopped", Kind::Bit, "out"),
            ("joint-1-homed", Kind::Bit, "out"),
            ("joint-1-homing", Kind::Bit, "out"),
            ("axis-1-stopped", Kind::Bit, "out"),
            ("joint-2-homed", Kind::Bit, "out"),
            ("joint-2-homing", Kind::Bit, "out"),
            ("axis-2-stopped", Kind::Bit, "out"),
        ]
        .map(|(name, kind, direction)| spec(name, kind, direction));
        let actual = schema();
        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 50);
        assert_eq!(
            actual
                .iter()
                .map(|pin| pin.name.as_str())
                .collect::<BTreeSet<_>>()
                .len(),
            actual.len(),
            "HAL pin suffixes must be unique"
        );
    }
}
