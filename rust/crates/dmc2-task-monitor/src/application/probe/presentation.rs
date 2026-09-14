use dmc2_core::{
    pendant::{AxisSelector, MultiplierSelector},
    supervisor::Phase,
};
use dmc2_hal_sys::probe_stream::{flag, Frame};

pub fn json(value: &str) -> String {
    let mut result = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c.is_control() => result.push_str(&format!("\\u{:04x}", c as u32)),
            c => result.push(c),
        }
    }
    result.push('"');
    result
}

fn direction(velocity: [f64; 3]) -> String {
    let axes = [
        (
            "physical LEFT / LinuxCNC +X",
            "physical RIGHT / LinuxCNC -X",
        ),
        ("LinuxCNC +Y", "LinuxCNC -Y"),
        ("physical UP / LinuxCNC +Z", "physical DOWN / LinuxCNC -Z"),
    ];
    let parts: Vec<_> = velocity
        .iter()
        .zip(axes)
        .filter_map(|(v, (positive, negative))| {
            if !v.is_finite() || *v == 0.0 {
                None
            } else {
                Some(if *v > 0.0 { positive } else { negative })
            }
        })
        .collect();
    if parts.is_empty() {
        "stationary".into()
    } else {
        parts.join(", ")
    }
}

pub fn touch(frame: Frame, previous: Option<Frame>) -> String {
    let mut text = if frame.valid_position() {
        String::from("Probe touch — machine coordinates (mm)\n")
    } else {
        String::from("CAPTURE FAILED — KEEP THE SETUP IN PLACE.\nUnqualified feedback sample; home/reference, controller or sample continuity is invalid.\n")
    };
    text.push_str(&format!(
        "X {:+.4}   Y {:+.4}   Z {:+.4}\n",
        frame.position[0], frame.position[1], frame.position[2]
    ));
    if frame.has(flag::VELOCITY_VALID) {
        let speed = frame
            .feedback_velocity
            .iter()
            .map(|v| v * v)
            .sum::<f64>()
            .sqrt();
        text.push_str(&format!(
            "At contact: {} · {:.3} mm/s reported\n",
            direction(frame.feedback_velocity),
            speed
        ));
    } else {
        text.push_str("At-contact feedback velocity unavailable\n");
    }
    if let Some(p) = previous.filter(|p| p.cycle.wrapping_add(1) == frame.cycle) {
        let speed = p.command_velocity.iter().map(|v| v * v).sum::<f64>().sqrt();
        let phase = Phase::from_wire_code(p.phase)
            .map(|p| format!("{p:?}"))
            .unwrap_or_else(|| format!("unknown state {}", p.phase));
        let axis = AxisSelector::from_wire_code(p.axis)
            .map(|v| format!("{v:?}"))
            .unwrap_or_else(|| format!("unknown ({})", p.axis));
        let multiplier = MultiplierSelector::from_wire_code(p.multiplier)
            .map(|v| format!("{v:?}"))
            .unwrap_or_else(|| format!("unknown ({})", p.multiplier));
        text.push_str(&format!("Prior cycle: {} · {:.3} mm/s commanded\nLast state: {phase}; contact {}; deadman {}; axis {}; multiplier {}\n", direction(p.command_velocity), speed, if p.has(flag::CONTACT) { "pressed" } else { "released" }, if p.has(flag::DEADMAN) { "held" } else { "released" }, axis, multiplier));
    } else {
        text.push_str("Prior cycle unavailable; no approach direction inferred\n");
    }
    text.push_str(&format!(
        "Servo {:.6} s · cycle {} · {}",
        frame.seconds,
        frame.cycle,
        if frame.has(flag::RECORD) {
            "buffered; saved when Record turns off"
        } else {
            "bubble only; not saved"
        }
    ));
    text
}
