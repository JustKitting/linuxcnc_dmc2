mod artifacts;
mod failure;
mod linuxcnc;
mod pendant;
mod presentation;
mod pty;
mod run_directory;

#[cfg(test)]
mod route_contract_tests;

use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use failure::{Failure, FailureCode, Result};
use linuxcnc::{hal_bool, hal_f64, hal_i32, hal_u32};
use pendant::{AxisSelection, MultiplierSelection};

const CONTROLLER_READY_TIMEOUT: Duration = Duration::from_secs(12);
const MOTION_TIMEOUT: Duration = Duration::from_secs(4);
const SELECTION_TIMEOUT: Duration = Duration::from_secs(2);
const ESTOP_TIMEOUT: Duration = Duration::from_secs(4);
const LIMIT_BOUNCE_TIMEOUT: Duration = Duration::from_secs(6);
const HOMING_TIMEOUT: Duration = Duration::from_secs(8);
const PULSES_PER_MM: f64 = 1000.0;
const BOUNCE_PULSES: i32 = 250;
const SUPERVISOR_IDLE: i32 = 0;
const SUPERVISOR_BOUNCING: i32 = 4;
const SUPERVISOR_BOUNCE_RELEASE_WAIT: i32 = 13;
const SELECTED_POSITION_TOLERANCE_RATIO: f64 = 0.20;
const MINIMUM_POSITION_TOLERANCE_MM: f64 = 0.002;
const UNSELECTED_POSITION_TOLERANCE_MM: f64 = 0.000_001;

const SCENARIOS: &[Scenario] = &[
    Scenario::new(AxisSelection::X, MultiplierSelection::X1, 1),
    Scenario::new(AxisSelection::X, MultiplierSelection::X1, 1),
    Scenario::new(AxisSelection::X, MultiplierSelection::X1, -1),
    Scenario::new(AxisSelection::X, MultiplierSelection::X1, -1),
    Scenario::new(AxisSelection::Y, MultiplierSelection::X1, 1),
    Scenario::new(AxisSelection::Y, MultiplierSelection::X1, -1),
    Scenario::new(AxisSelection::Z, MultiplierSelection::X1, 1),
    Scenario::new(AxisSelection::Z, MultiplierSelection::X1, -1),
    Scenario::new(AxisSelection::X, MultiplierSelection::X10, 1),
    Scenario::new(AxisSelection::X, MultiplierSelection::X10, -1),
    Scenario::new(AxisSelection::Y, MultiplierSelection::X10, 1),
    Scenario::new(AxisSelection::Y, MultiplierSelection::X10, -1),
    Scenario::new(AxisSelection::Z, MultiplierSelection::X10, 1),
    Scenario::new(AxisSelection::Z, MultiplierSelection::X10, -1),
    Scenario::new(AxisSelection::X, MultiplierSelection::X100, 1),
    Scenario::new(AxisSelection::X, MultiplierSelection::X100, -1),
    Scenario::new(AxisSelection::Y, MultiplierSelection::X100, 1),
    Scenario::new(AxisSelection::Y, MultiplierSelection::X100, -1),
    Scenario::new(AxisSelection::Z, MultiplierSelection::X100, 1),
    Scenario::new(AxisSelection::Z, MultiplierSelection::X100, -1),
];

const LIMIT_BOUNCE_AXES: &[AxisSelection] = &[AxisSelection::X, AxisSelection::Y, AxisSelection::Z];
const HOMING_CANCEL_EVENTS: usize = 1;

fn main() {
    if let Err(error) = run() {
        eprintln!("dmc2-motion-acceptance: FAIL: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let project_root = project_root_from_arguments()?;
    artifacts::require_linuxcnc_2_9_10()?;
    artifacts::require_no_active_realtime_host()?;
    artifacts::require_test_assets(&project_root)?;
    let deployment_issues = artifacts::deployment_identity_issues(&project_root);

    let mut terminal = pty::PseudoTerminal::open()?;
    let rsh_port = linuxcnc::reserve_loopback_port()?;
    let mut run_directory = run_directory::RunDirectory::create(&project_root)?;
    let ini_path = run_directory.render_ini(&project_root, terminal.slave_path(), rsh_port)?;
    let pendant = pendant::PendantStream::start(terminal.take_master()?);
    let mut linuxcnc = linuxcnc::LinuxCncSession::start(&ini_path, run_directory.path(), rsh_port)?;
    linuxcnc.wait_until_ready()?;
    linuxcnc.configure_manual_joint_mode()?;

    linuxcnc::wait_for_bool("dmc2-pendant.connected", true, CONTROLLER_READY_TIMEOUT)?;
    linuxcnc::wait_for_bool(
        "dmc2-task-monitor.connected",
        true,
        CONTROLLER_READY_TIMEOUT,
    )?;
    linuxcnc::wait_for_bool("motion.motion-enabled", true, CONTROLLER_READY_TIMEOUT)?;
    linuxcnc::wait_for_bool(
        "dmc2-pendant-control.control-ready",
        true,
        CONTROLLER_READY_TIMEOUT,
    )?;
    require_controller_healthy("before jog")?;
    execute_canonical_estop_recovery(&pendant)?;

    let start = MotionEvidence::read()?;
    let mut finish = start;
    for (index, scenario) in SCENARIOS.iter().copied().enumerate() {
        prepare_selection(&pendant, scenario)?;
        finish = execute_scenario(&pendant, index, scenario, finish)?;
    }
    finish = execute_homing_cancel(&mut linuxcnc, &pendant)?;
    execute_shared_home_limit_diagnostic(&mut linuxcnc)?;
    for axis in LIMIT_BOUNCE_AXES.iter().copied() {
        finish = execute_limit_bounce(&pendant, axis)?;
    }

    linuxcnc.shutdown()?;
    pendant.finish()?;
    presentation::validate_error_journal(
        &project_root,
        run_directory.path(),
        LIMIT_BOUNCE_AXES.len(),
    )?;
    presentation::validate_diagnostic_journal(&project_root, run_directory.path())?;

    if !deployment_issues.is_empty() {
        return Err(Failure::new(
            FailureCode::DeploymentIdentity,
            format!(
                "real motmod movement was observed, but the running artifacts are stale: {}",
                deployment_issues.join(" | ")
            ),
        ));
    }

    run_directory.mark_success();
    println!(
        "dmc2-motion-acceptance: PASS: 1 canonical pendant E-stop trip/recovery, {} pendant moves, {} real LinuxCNC homing cancel, 1 shared home/limit diagnostic path, and {} real LinuxCNC limit-stop/bounce/latch-reset paths covered production P3 -> dmc2_rt -> LinuxCNC 2.9.10 motmod -> software-stepgen across X/Y/Z, x1/x10/x100, both directions, and repeated X/x1 input; command_mm={:?}->{:?}; stepgen_counts={:?}->{:?}",
        SCENARIOS.len(),
        HOMING_CANCEL_EVENTS,
        LIMIT_BOUNCE_AXES.len(),
        start.command_mm,
        finish.command_mm,
        start.stepgen_count,
        finish.stepgen_count,
    );
    Ok(())
}

fn execute_canonical_estop_recovery(pendant: &pendant::PendantStream) -> Result<()> {
    set_pendant_controls_and_wait(
        pendant,
        AxisSelection::X,
        MultiplierSelection::X1,
        false,
        true,
    )?;
    linuxcnc::wait_for_bool("dmc2-pendant-control.recovery-active", true, ESTOP_TIMEOUT)?;
    linuxcnc::wait_for_bool("dmc2-task-monitor.estopped", true, ESTOP_TIMEOUT)?;
    require_estop_state("estop-latch.0.ok-out", false, "physical press latch output")?;
    require_estop_state(
        "dmc2-pendant-control.external-enable",
        false,
        "physical press controller gate",
    )?;

    // Releasing the physical switch alone must leave LinuxCNC's canonical
    // E-stop latched. Recovery begins only from the exact operator sequence.
    set_pendant_controls_and_wait(
        pendant,
        AxisSelection::X,
        MultiplierSelection::X1,
        false,
        false,
    )?;
    require_estop_state(
        "dmc2-task-monitor.estopped",
        true,
        "physical release without recovery",
    )?;
    require_estop_state(
        "dmc2-pendant-control.recovery-active",
        true,
        "physical release retained recovery",
    )?;

    set_pendant_controls_and_wait(
        pendant,
        AxisSelection::X,
        MultiplierSelection::X10,
        false,
        false,
    )?;
    set_pendant_controls_and_wait(
        pendant,
        AxisSelection::X,
        MultiplierSelection::X1,
        false,
        false,
    )?;
    set_pendant_controls_and_wait(
        pendant,
        AxisSelection::Off,
        MultiplierSelection::Off,
        false,
        false,
    )?;
    request_recovery_detent(pendant, 1)?;
    request_recovery_detent(pendant, -1)?;
    for _ in 0..3 {
        set_pendant_controls_and_wait(
            pendant,
            AxisSelection::Off,
            MultiplierSelection::X1,
            true,
            false,
        )?;
        set_pendant_controls_and_wait(
            pendant,
            AxisSelection::Off,
            MultiplierSelection::Off,
            false,
            false,
        )?;
    }

    linuxcnc::wait_for_bool("dmc2-task-monitor.estopped", false, ESTOP_TIMEOUT)?;
    linuxcnc::wait_for_bool("dmc2-task-monitor.machine-on", true, ESTOP_TIMEOUT)?;
    linuxcnc::wait_for_bool("dmc2-pendant-control.recovery-active", false, ESTOP_TIMEOUT)?;
    linuxcnc::wait_for_bool("estop-latch.0.ok-out", true, ESTOP_TIMEOUT)?;

    set_pendant_controls_and_wait(
        pendant,
        AxisSelection::X,
        MultiplierSelection::X1,
        true,
        false,
    )?;
    linuxcnc::wait_for_bool(
        "dmc2-pendant-control.control-ready",
        true,
        CONTROLLER_READY_TIMEOUT,
    )?;
    require_controller_healthy("after canonical pendant E-stop recovery")
}

fn set_pendant_controls_and_wait(
    pendant: &pendant::PendantStream,
    axis: AxisSelection,
    multiplier: MultiplierSelection,
    deadman_held: bool,
    estop_pressed: bool,
) -> Result<()> {
    let sequence = hal_u32("dmc2-pendant.sequence")?;
    pendant.set_controls(axis, multiplier, deadman_held, estop_pressed)?;
    linuxcnc::wait_for_u32_advance("dmc2-pendant.sequence", sequence, 1, SELECTION_TIMEOUT)?;
    linuxcnc::wait_for_i32("dmc2-pendant.axis-code", axis as i32, SELECTION_TIMEOUT)?;
    linuxcnc::wait_for_i32(
        "dmc2-pendant.multiplier-code",
        multiplier as i32,
        SELECTION_TIMEOUT,
    )?;
    linuxcnc::wait_for_bool("dmc2-pendant.deadman-held", deadman_held, SELECTION_TIMEOUT)?;
    linuxcnc::wait_for_bool(
        "dmc2-pendant.estop-pressed",
        estop_pressed,
        SELECTION_TIMEOUT,
    )?;
    linuxcnc::wait_for_bool(
        "dmc2-pendant.selector-valid",
        axis != AxisSelection::Off,
        SELECTION_TIMEOUT,
    )?;
    let confirmed_sequence = hal_u32("dmc2-pendant.sequence")?;
    linuxcnc::wait_for_u32_advance(
        "dmc2-pendant.sequence",
        confirmed_sequence,
        1,
        SELECTION_TIMEOUT,
    )
}

fn request_recovery_detent(pendant: &pendant::PendantStream, direction: i32) -> Result<()> {
    let start = hal_i32("dmc2-pendant.detent-count")?;
    pendant.request_detent(direction)?;
    linuxcnc::wait_for_i32(
        "dmc2-pendant.detent-count",
        start.wrapping_add(direction),
        SELECTION_TIMEOUT,
    )?;
    let sequence = hal_u32("dmc2-pendant.sequence")?;
    linuxcnc::wait_for_u32_advance("dmc2-pendant.sequence", sequence, 1, SELECTION_TIMEOUT)
}

fn require_estop_state(pin: &str, expected: bool, stage: &str) -> Result<()> {
    let observed = hal_bool(pin)?;
    if observed == expected {
        Ok(())
    } else {
        Err(Failure::new(
            FailureCode::EstopContract,
            format!("stage={stage}; pin={pin}; expected={expected}; observed={observed}"),
        ))
    }
}

fn execute_shared_home_limit_diagnostic(linuxcnc: &mut linuxcnc::LinuxCncSession) -> Result<()> {
    const JOINT: usize = 1;
    const MOTOR: usize = 0;

    linuxcnc.home_joint(JOINT)?;
    linuxcnc::wait_for_bool("dmc2-task-monitor.joint-1-homing", true, HOMING_TIMEOUT)?;
    let heartbeat = hal_u32("dmc2-task-monitor.task-heartbeat")?;
    linuxcnc::set_hal_signal_bool(raw_limit_signal(MOTOR), true)?;
    let observation = (|| {
        linuxcnc::wait_for_bool(hard_limit_pin(JOINT), true, MOTION_TIMEOUT)?;
        linuxcnc::wait_for_u32_advance(
            "dmc2-task-monitor.task-heartbeat",
            heartbeat,
            100,
            MOTION_TIMEOUT,
        )
    })();
    let clear = linuxcnc::set_hal_signal_bool(raw_limit_signal(MOTOR), false);
    observation?;
    clear?;

    linuxcnc::wait_for_bool(hard_limit_pin(JOINT), false, MOTION_TIMEOUT)?;
    linuxcnc::wait_for_bool("dmc2-task-monitor.joint-1-homing", false, HOMING_TIMEOUT)?;
    linuxcnc::wait_for_bool("joint.1.homed", true, HOMING_TIMEOUT)?;
    linuxcnc::wait_for_bool(latched_limit_pin(MOTOR), false, LIMIT_BOUNCE_TIMEOUT)?;
    require_controller_healthy("after shared home/limit diagnostic path")
}

fn execute_homing_cancel(
    linuxcnc: &mut linuxcnc::LinuxCncSession,
    pendant: &pendant::PendantStream,
) -> Result<MotionEvidence> {
    let axis = AxisSelection::X;
    let scenario = Scenario::new(axis, MultiplierSelection::X100, axis.clockwise_sign());
    prepare_selection(pendant, scenario)?;
    let start = MotionEvidence::read()?;
    pendant.request_detent(scenario.detent)?;
    linuxcnc::wait_for_bool("dmc2-pendant-control.jog-active", true, MOTION_TIMEOUT)?;
    linuxcnc::wait_for_position_change(
        command_pin(axis.index()),
        start.command_mm[axis.index()],
        0.001,
        MOTION_TIMEOUT,
    )?;

    // Real LinuxCNC homemod remains level-active while the final homing move
    // runs. The controller must publish one controlled stop transition, not
    // one stop on every servo cycle while LinuxCNC decelerates and homes.
    linuxcnc.home_joint(0)?;
    linuxcnc::wait_for_bool("dmc2-task-monitor.joint-0-homing", true, HOMING_TIMEOUT)?;
    linuxcnc::wait_for_bool("dmc2-pendant-control.jog-active", false, MOTION_TIMEOUT)?;
    linuxcnc::wait_for_bool("dmc2-task-monitor.joint-0-homing", false, HOMING_TIMEOUT)?;
    linuxcnc::wait_for_bool("joint.0.homed", true, HOMING_TIMEOUT)?;
    require_controller_healthy("after real LinuxCNC homing cancellation")?;
    Ok(MotionEvidence::read()?)
}

fn execute_limit_bounce(
    pendant: &pendant::PendantStream,
    axis: AxisSelection,
) -> Result<MotionEvidence> {
    let toward_positive = axis.clockwise_sign();
    let scenario = Scenario::new(axis, MultiplierSelection::X100, toward_positive);
    prepare_selection(pendant, scenario)?;
    let before_jog = MotionEvidence::read()?;
    pendant.request_detent(toward_positive)?;
    linuxcnc::wait_for_bool("dmc2-pendant-control.jog-active", true, MOTION_TIMEOUT)?;
    linuxcnc::wait_for_position_change(
        command_pin(axis.index()),
        before_jog.command_mm[axis.index()],
        0.001,
        MOTION_TIMEOUT,
    )?;

    let motor = axis.motor_index();
    linuxcnc::set_hal_signal_bool(raw_limit_signal(motor), true)?;
    linuxcnc::wait_for_i32(
        "dmc2-pendant-control.supervisor-phase",
        SUPERVISOR_BOUNCING,
        LIMIT_BOUNCE_TIMEOUT,
    )?;
    let bounce_start = MotionEvidence::read_selected_first(axis.index())?;

    // Hold the modeled physical switch active through the complete automatic
    // backoff. This is the live failure that the prior acceptance cleared too
    // early to exercise.
    linuxcnc::wait_for_i32(
        "dmc2-pendant-control.supervisor-phase",
        SUPERVISOR_BOUNCE_RELEASE_WAIT,
        LIMIT_BOUNCE_TIMEOUT,
    )?;
    require_controller_healthy(&format!(
        "after {axis:?} automatic backoff with raw limit held"
    ))?;
    linuxcnc::wait_for_bool(hard_limit_pin(axis.index()), false, LIMIT_BOUNCE_TIMEOUT)?;
    let automatic_finish = MotionEvidence::read_selected_first(axis.index())?;
    validate_limit_bounce(axis, bounce_start, automatic_finish)?;

    // No additional automatic movement is permitted. Send one real pendant x1
    // detent in the away direction while the raw switch is still asserted and
    // require LinuxCNC motion to accept it without exposing its native hard
    // limit input.
    let release = Scenario::new(axis, MultiplierSelection::X1, -toward_positive);
    prepare_selection(pendant, release)?;
    let release_start = MotionEvidence::read_selected_first(axis.index())?;
    pendant.request_detent(release.detent)?;
    linuxcnc::wait_for_position_change(
        command_pin(axis.index()),
        release_start.command_mm[axis.index()],
        0.001,
        MOTION_TIMEOUT,
    )?;
    linuxcnc::wait_for_i32(
        "dmc2-pendant-control.supervisor-phase",
        SUPERVISOR_BOUNCE_RELEASE_WAIT,
        LIMIT_BOUNCE_TIMEOUT,
    )?;
    require_controller_healthy(&format!("after {axis:?} operator-commanded limit release"))?;
    linuxcnc::wait_for_bool(hard_limit_pin(axis.index()), false, LIMIT_BOUNCE_TIMEOUT)?;
    let release_finish = MotionEvidence::read_selected_first(axis.index())?;
    validate_motion(
        SCENARIOS.len() + axis.index(),
        release,
        release_start,
        release_finish,
    )?;

    linuxcnc::set_hal_signal_bool(raw_limit_signal(motor), false)?;
    linuxcnc::wait_for_i32(
        "dmc2-pendant-control.supervisor-phase",
        SUPERVISOR_IDLE,
        LIMIT_BOUNCE_TIMEOUT,
    )?;
    linuxcnc::wait_for_bool(latched_limit_pin(motor), false, LIMIT_BOUNCE_TIMEOUT)?;
    linuxcnc::wait_for_bool(
        "dmc2-pendant-control.control-ready",
        true,
        LIMIT_BOUNCE_TIMEOUT,
    )?;
    require_controller_healthy(&format!("after {axis:?} limit bounce"))?;
    Ok(MotionEvidence::read()?)
}

fn validate_limit_bounce(
    axis: AxisSelection,
    start: MotionEvidence,
    finish: MotionEvidence,
) -> Result<()> {
    let selected = axis.index();
    let expected_mm = -(BOUNCE_PULSES as f64) / PULSES_PER_MM;
    let observed_mm = finish.command_mm[selected] - start.command_mm[selected];
    let position_tolerance = expected_mm.abs() * SELECTED_POSITION_TOLERANCE_RATIO;
    if (observed_mm - expected_mm).abs() > position_tolerance {
        return Err(Failure::new(
            FailureCode::MotionNotObserved,
            format!(
                "axis={axis:?}; limit bounce command mismatch: expected={expected_mm:.6}mm tolerance={position_tolerance:.6}mm observed={observed_mm:.6}mm"
            ),
        ));
    }
    let observed_count = finish.stepgen_count[selected].wrapping_sub(start.stepgen_count[selected]);
    let count_tolerance =
        ((BOUNCE_PULSES as f64) * SELECTED_POSITION_TOLERANCE_RATIO).ceil() as i32;
    if observed_count.wrapping_add(BOUNCE_PULSES).abs() > count_tolerance {
        return Err(Failure::new(
            FailureCode::MotionNotObserved,
            format!(
                "axis={axis:?}; downstream bounce count mismatch: expected=-{BOUNCE_PULSES} tolerance={count_tolerance} observed={observed_count}"
            ),
        ));
    }
    for other in 0..3 {
        if other == selected {
            continue;
        }
        let position_delta = finish.command_mm[other] - start.command_mm[other];
        let count_delta = finish.stepgen_count[other].wrapping_sub(start.stepgen_count[other]);
        if position_delta.abs() > UNSELECTED_POSITION_TOLERANCE_MM || count_delta != 0 {
            return Err(Failure::new(
                FailureCode::MotionNotObserved,
                format!(
                    "limit_axis={axis:?}; unselected_axis={other}; position_delta={position_delta:.9}mm count_delta={count_delta}"
                ),
            ));
        }
    }
    Ok(())
}

fn prepare_selection(pendant: &pendant::PendantStream, scenario: Scenario) -> Result<()> {
    pendant.select(scenario.axis, scenario.multiplier)?;
    linuxcnc::wait_for_i32(
        "dmc2-pendant.axis-code",
        scenario.axis as i32,
        SELECTION_TIMEOUT,
    )?;
    linuxcnc::wait_for_i32(
        "dmc2-pendant.multiplier-code",
        scenario.multiplier as i32,
        SELECTION_TIMEOUT,
    )?;
    let sequence = hal_u32("dmc2-pendant.sequence")?;
    linuxcnc::wait_for_u32_advance("dmc2-pendant.sequence", sequence, 2, SELECTION_TIMEOUT)?;
    require_controller_healthy("after selector baseline")
}

fn execute_scenario(
    pendant: &pendant::PendantStream,
    index: usize,
    scenario: Scenario,
    start: MotionEvidence,
) -> Result<MotionEvidence> {
    pendant.request_detent(scenario.detent)?;
    let axis = scenario.axis.index();
    linuxcnc::wait_for_position_change(
        command_pin(axis),
        start.command_mm[axis],
        0.001,
        MOTION_TIMEOUT,
    )?;
    linuxcnc::wait_for_bool("dmc2-pendant-control.jog-active", false, MOTION_TIMEOUT)?;
    linuxcnc::wait_for_bool(in_position_pin(axis), true, MOTION_TIMEOUT)?;
    require_controller_healthy(&format!(
        "after scenario={index} axis={:?} multiplier={:?} detent={}",
        scenario.axis, scenario.multiplier, scenario.detent
    ))?;
    let finish = MotionEvidence::read()?;
    validate_motion(index, scenario, start, finish)?;
    Ok(finish)
}

fn project_root_from_arguments() -> Result<PathBuf> {
    let mut arguments = env::args_os().skip(1);
    let Some(first) = arguments.next() else {
        return artifacts::default_project_root();
    };
    if first != "--project-root" {
        return Err(Failure::new(
            FailureCode::Argument,
            format!(
                "unknown argument: {first:?}; usage: dmc2-motion-acceptance [--project-root PATH]"
            ),
        ));
    }
    let path = arguments
        .next()
        .ok_or_else(|| Failure::new(FailureCode::Argument, "--project-root requires a path"))?;
    if let Some(extra) = arguments.next() {
        return Err(Failure::new(
            FailureCode::Argument,
            format!("unexpected extra argument: {extra:?}"),
        ));
    }
    Path::new(&path)
        .canonicalize()
        .map_err(|error| Failure::io(FailureCode::Argument, "canonicalize --project-root", error))
}

fn require_controller_healthy(stage: &str) -> Result<()> {
    if !hal_bool("dmc2-pendant-control.fault")? {
        return Ok(());
    }
    let code = hal_i32("dmc2-pendant-control.fault-code")?;
    Err(Failure::new(
        FailureCode::MotionNotObserved,
        format!("stage={stage}; controller_fault=true; controller_fault_code={code}"),
    ))
}

#[derive(Clone, Copy, Debug)]
struct MotionEvidence {
    command_mm: [f64; 3],
    stepgen_count: [i32; 3],
}

impl MotionEvidence {
    fn read() -> Result<Self> {
        Ok(Self {
            command_mm: [
                hal_f64("joint.0.motor-pos-cmd")?,
                hal_f64("joint.1.motor-pos-cmd")?,
                hal_f64("joint.2.motor-pos-cmd")?,
            ],
            stepgen_count: [
                hal_i32("stepgen.1.counts")?,
                hal_i32("stepgen.0.counts")?,
                hal_i32("stepgen.2.counts")?,
            ],
        })
    }

    fn read_selected_first(selected: usize) -> Result<Self> {
        let mut command_mm = [0.0; 3];
        let mut stepgen_count = [0; 3];
        stepgen_count[selected] = hal_i32(stepgen_count_pin(selected))?;
        command_mm[selected] = hal_f64(command_pin(selected))?;
        for axis in 0..3 {
            if axis == selected {
                continue;
            }
            command_mm[axis] = hal_f64(command_pin(axis))?;
            stepgen_count[axis] = hal_i32(stepgen_count_pin(axis))?;
        }
        Ok(Self {
            command_mm,
            stepgen_count,
        })
    }
}

fn validate_motion(
    index: usize,
    scenario: Scenario,
    start: MotionEvidence,
    finish: MotionEvidence,
) -> Result<()> {
    let axis = scenario.axis.index();
    let expected_pulses = scenario.expected_pulses();
    let expected_mm = expected_pulses as f64 / PULSES_PER_MM;
    let observed_mm = finish.command_mm[axis] - start.command_mm[axis];
    let position_tolerance =
        (expected_mm.abs() * SELECTED_POSITION_TOLERANCE_RATIO).max(MINIMUM_POSITION_TOLERANCE_MM);
    if (observed_mm - expected_mm).abs() > position_tolerance {
        return Err(Failure::new(
            FailureCode::MotionNotObserved,
            format!(
                "scenario={index}; axis={:?}; multiplier={:?}; detent={}; command distance mismatch: expected={expected_mm:.6}mm tolerance={position_tolerance:.6}mm observed={observed_mm:.6}mm",
                scenario.axis, scenario.multiplier, scenario.detent,
            ),
        ));
    }

    let observed_count = finish.stepgen_count[axis].wrapping_sub(start.stepgen_count[axis]);
    let count_tolerance =
        ((expected_pulses.abs() as f64) * SELECTED_POSITION_TOLERANCE_RATIO).ceil() as i32;
    if observed_count.wrapping_sub(expected_pulses).abs() > count_tolerance {
        return Err(Failure::new(
            FailureCode::MotionNotObserved,
            format!(
                "scenario={index}; axis={:?}; multiplier={:?}; detent={}; downstream step count mismatch: expected={expected_pulses} tolerance={count_tolerance} observed={observed_count}",
                scenario.axis, scenario.multiplier, scenario.detent,
            ),
        ));
    }

    for other in 0..3 {
        if other == axis {
            continue;
        }
        let position_delta = finish.command_mm[other] - start.command_mm[other];
        let count_delta = finish.stepgen_count[other].wrapping_sub(start.stepgen_count[other]);
        if position_delta.abs() > UNSELECTED_POSITION_TOLERANCE_MM || count_delta != 0 {
            return Err(Failure::new(
                FailureCode::MotionNotObserved,
                format!(
                    "scenario={index}; selected_axis={:?}; unselected_axis={other}; position_delta={position_delta:.9}mm count_delta={count_delta}",
                    scenario.axis,
                ),
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct Scenario {
    axis: AxisSelection,
    multiplier: MultiplierSelection,
    detent: i32,
}

impl Scenario {
    const fn new(axis: AxisSelection, multiplier: MultiplierSelection, detent: i32) -> Self {
        Self {
            axis,
            multiplier,
            detent,
        }
    }

    const fn expected_pulses(self) -> i32 {
        self.detent * self.axis.clockwise_sign() * self.multiplier.pulses()
    }
}

const fn command_pin(axis: usize) -> &'static str {
    match axis {
        0 => "joint.0.motor-pos-cmd",
        1 => "joint.1.motor-pos-cmd",
        2 => "joint.2.motor-pos-cmd",
        _ => "",
    }
}

const fn in_position_pin(axis: usize) -> &'static str {
    match axis {
        0 => "joint.0.in-position",
        1 => "joint.1.in-position",
        2 => "joint.2.in-position",
        _ => "",
    }
}

const fn stepgen_count_pin(axis: usize) -> &'static str {
    match axis {
        0 => "stepgen.1.counts",
        1 => "stepgen.0.counts",
        2 => "stepgen.2.counts",
        _ => "",
    }
}

const fn raw_limit_signal(motor: usize) -> &'static str {
    match motor {
        0 => "acceptance-limit-raw-0",
        1 => "acceptance-limit-raw-1",
        2 => "acceptance-limit-raw-2",
        _ => "",
    }
}

const fn hard_limit_pin(axis: usize) -> &'static str {
    match axis {
        0 => "joint.0.pos-lim-sw-in",
        1 => "joint.1.pos-lim-sw-in",
        2 => "joint.2.pos-lim-sw-in",
        _ => "",
    }
}

const fn latched_limit_pin(motor: usize) -> &'static str {
    match motor {
        0 => "dmc2-pendant-control.motor-0-limit-latched",
        1 => "dmc2-pendant-control.motor-1-limit-latched",
        2 => "dmc2-pendant-control.motor-2-limit-latched",
        _ => "",
    }
}
