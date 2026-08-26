mod protocol;
mod state;

pub(super) const IDLE: &str = "P3,1,20,0,0,0,0,X,X1,0,0,1";

// The helper spells out every wire field so tests cannot silently inherit a
// default for a pendant packet field they intend to exercise.
#[allow(clippy::too_many_arguments)]
pub(super) fn packet(
    sequence: u32,
    latest_detent: i32,
    axis: &str,
    multiplier: &str,
    deadman: bool,
    estop: bool,
    valid: bool,
    quadrature_errors: u32,
) -> String {
    format!(
        "P3,{sequence},{},0,0,{quadrature_errors},{latest_detent},{axis},{multiplier},{},{},{}",
        sequence.wrapping_mul(20),
        u8::from(deadman),
        u8::from(estop),
        u8::from(valid),
    )
}
