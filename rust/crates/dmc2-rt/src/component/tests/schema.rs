//! Exact public HAL schema regression test.

use super::*;
use std::collections::BTreeSet;

type SchemaEntry = (String, PinKind, c_int);

fn add(schema: &mut BTreeSet<SchemaEntry>, kind: PinKind, direction: c_int, names: &[&str]) {
    for name in names {
        assert!(schema.insert(((*name).to_string(), kind, direction)));
    }
}

fn add_indexed(
    schema: &mut BTreeSet<SchemaEntry>,
    kind: PinKind,
    direction: c_int,
    stem: &str,
    suffix: &str,
) {
    for index in 0..3 {
        let separator = if suffix.is_empty() { "" } else { "-" };
        let name = format!("dmc2-pendant-control.{stem}-{index}{separator}{suffix}");
        assert!(schema.insert((name, kind, direction)));
    }
}

fn expected_schema() -> BTreeSet<SchemaEntry> {
    let input = hal::hal_pin_dir_t_HAL_IN;
    let output = hal::hal_pin_dir_t_HAL_OUT;
    let io = hal::hal_pin_dir_t_HAL_IO;
    let mut schema = BTreeSet::new();

    add(
        &mut schema,
        PinKind::U32,
        input,
        &[
            "dmc2-pendant-control.snapshot-generation",
            "dmc2-pendant-control.quadrature-errors",
            "dmc2-pendant-control.sequence",
            "dmc2-pendant-control.milliseconds",
            "dmc2-pendant-control.task-snapshot-generation",
            "dmc2-pendant-control.task-heartbeat",
            "dmc2-pendant-control.mesa-packet-error-total",
        ],
    );
    add(
        &mut schema,
        PinKind::S32,
        input,
        &[
            "dmc2-pendant-control.axis-code",
            "dmc2-pendant-control.multiplier-code",
            "dmc2-pendant-control.latest-detent",
            "dmc2-pendant-control.detent-count",
            "dmc2-pendant-control.transition-count",
        ],
    );
    add_indexed(&mut schema, PinKind::S32, input, "motor", "count");
    add_indexed(
        &mut schema,
        PinKind::Float,
        input,
        "motor",
        "position-feedback",
    );
    add(
        &mut schema,
        PinKind::Bit,
        input,
        &[
            "dmc2-pendant-control.connected",
            "dmc2-pendant-control.serial-fault",
            "dmc2-pendant-control.quadrature-fault",
            "dmc2-pendant-control.estop-pressed",
            "dmc2-pendant-control.deadman-held",
            "dmc2-pendant-control.selector-valid",
            "dmc2-pendant-control.task-monitor-connected",
            "dmc2-pendant-control.task-monitor-fault",
            "dmc2-pendant-control.machine-on",
            "dmc2-pendant-control.estopped",
            "dmc2-pendant-control.manual-mode",
            "dmc2-pendant-control.joint-mode",
            "dmc2-pendant-control.teleop-mode",
            "dmc2-pendant-control.interp-idle",
            "dmc2-pendant-control.pendant-mode-enabled",
            "dmc2-pendant-control.servo-thread-ready",
            "dmc2-pendant-control.mesa-packet-error",
            "dmc2-pendant-control.mesa-packet-error-exceeded",
            "dmc2-pendant-control.software-watchdog-ok",
            "dmc2-pendant-control.ui-ready",
        ],
    );
    add_indexed(&mut schema, PinKind::Bit, input, "joint", "homed");
    add_indexed(&mut schema, PinKind::Bit, input, "joint", "homing");
    add_indexed(&mut schema, PinKind::Bit, input, "axis", "stopped");
    add_indexed(&mut schema, PinKind::Bit, input, "motor", "limit-raw");
    add_indexed(&mut schema, PinKind::Bit, input, "motor", "limit-latched");
    add(
        &mut schema,
        PinKind::Bit,
        io,
        &["dmc2-pendant-control.mesa-watchdog-has-bit"],
    );

    add_indexed(&mut schema, PinKind::Bit, output, "motor", "limit-reset");
    add_indexed(&mut schema, PinKind::Bit, output, "motor", "command-enable");
    add_indexed(&mut schema, PinKind::Bit, output, "motor", "toward-limit");
    add_indexed(&mut schema, PinKind::Bit, output, "axis", "increment-plus");
    add_indexed(&mut schema, PinKind::Bit, output, "axis", "increment-minus");
    add_indexed(&mut schema, PinKind::Bit, output, "joint", "increment-plus");
    add_indexed(
        &mut schema,
        PinKind::Bit,
        output,
        "joint",
        "increment-minus",
    );
    add(
        &mut schema,
        PinKind::Bit,
        output,
        &[
            "dmc2-pendant-control.external-enable",
            "dmc2-pendant-control.watchdog-enable",
            "dmc2-pendant-control.heartbeat",
            "dmc2-pendant-control.position-known",
            "dmc2-pendant-control.position-unknown",
            "dmc2-pendant-control.control-available",
            "dmc2-pendant-control.control-ready",
            "dmc2-pendant-control.fault",
            "dmc2-pendant-control.recovery-active",
            "dmc2-pendant-control.jog-active",
            "dmc2-pendant-control.bounce-active",
            "dmc2-pendant-control.estop-reset-request",
            "dmc2-pendant-control.machine-on-request",
            "dmc2-pendant-control.jog-stop",
            "dmc2-pendant-control.jog-stop-immediate",
        ],
    );
    add(
        &mut schema,
        PinKind::S32,
        output,
        &[
            "dmc2-pendant-control.fault-code",
            "dmc2-pendant-control.supervisor-phase",
            "dmc2-pendant-control.mesa-phase",
            "dmc2-pendant-control.controller-watchdog-phase",
            "dmc2-pendant-control.command-phase",
        ],
    );
    add_indexed(&mut schema, PinKind::Float, output, "axis", "increment");
    add_indexed(&mut schema, PinKind::Float, output, "joint", "increment");
    add(
        &mut schema,
        PinKind::Float,
        output,
        &[
            "dmc2-pendant-control.axis-jog-speed",
            "dmc2-pendant-control.joint-jog-speed",
        ],
    );
    schema
}

#[test]
fn exported_hal_schema_is_exact_complete_and_unique() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    reset_component();

    let records = PINS.lock().expect("mock HAL registry lock").clone();
    let actual = records
        .iter()
        .map(|record| (record.name.clone(), record.kind, record.direction))
        .collect::<BTreeSet<_>>();
    let addresses = records
        .iter()
        .map(|record| record.address)
        .collect::<BTreeSet<_>>();
    let expected = expected_schema();

    assert_eq!(expected.len(), 103, "test schema itself is incomplete");
    assert_eq!(
        records.len(),
        expected.len(),
        "HAL registered wrong pin count"
    );
    assert_eq!(actual.len(), records.len(), "duplicate HAL schema entry");
    assert_eq!(addresses.len(), records.len(), "two HAL pins share storage");
    assert_eq!(actual, expected);

    rtapi_app_exit();
}
