//! Validate released endpoints against the retained contact and diameter.
use crate::probe_data::ledger::{number, Fields};
use crate::probe_data::mapper_settings::{close, xyz, Phase, Request, Settings};

pub fn target(
    s: &Settings,
    request: Request,
    machine_trigger: [f64; 3],
) -> Result<[f64; 3], String> {
    let d = [
        request.approach[0] - request.target[0],
        request.approach[1] - request.target[1],
    ];
    let length = d[0].hypot(d[1]);
    if !length.is_finite() || length == 0.0 {
        return Err(
            "The captured outline approach has no horizontal direction; Abort then Pendant Mode."
                .into(),
        );
    }
    let p = [
        machine_trigger[0] - s.offset[0] + d[0] / length * s.outline_backoff(),
        machine_trigger[1] - s.offset[1] + d[1] / length * s.outline_backoff(),
        request.target[2],
    ];
    s.bounds(p)?;
    Ok(p)
}
pub fn request(r: &Fields) -> Result<Request, String> {
    Ok(Request {
        phase: Phase::read(number(r, "phase")?)?,
        edge: number(r, "edge")? as i32,
        approach: [number(r, "approach_x")?, number(r, "approach_y")?],
        target: xyz(r, "target_", "")?,
    })
}
pub fn validate(r: &Fields, s: &Settings, expected: Option<[f64; 3]>) -> Result<(), String> {
    let from = xyz(r, "from_", "")?;
    let to = xyz(r, "target_", "")?;
    s.bounds(from)?;
    s.bounds(to)?;
    let phase = Phase::read(number(r, "phase")?)?;
    let complete = r["kind"] == "withdrawal-complete";
    let lateral = s.full_outline_backoff() && phase.is_outline();
    if lateral {
        let wanted =
            expected.ok_or("No retained contact defines this backoff; Abort then Pendant Mode.")?;
        if !s.endpoint_matches(to, wanted)
            || !close(from[2], to[2])
            || !close(number(r, "feed")?, s.feeds[1])
        {
            return Err("The lateral withdrawal differs from its retained probe-diameter endpoint, Z or feed; Abort then Pendant Mode.".into());
        }
    } else if !close(to[2], s.origin[2])
        || !close(from[0], to[0])
        || !close(from[1], to[1])
        || (!complete && to[2] <= from[2])
        || !(close(number(r, "feed")?, s.feeds[1]) || close(number(r, "feed")?, s.feeds[2]))
    {
        return Err("Withdrawal record differs from the retained upward path, clearance or feed; Abort then Pendant Mode.".into());
    }
    if complete && !s.endpoint_matches(xyz(r, "work_", "")?, to) {
        return Err("Release was reported but the full withdrawal endpoint was not reached; Abort then Pendant Mode.".into());
    }
    Ok(())
}
