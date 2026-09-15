//! Validate capture/release/clearance transitions before planning another move.
use crate::probe_data::mapper_schema::CaptureFailure;
use crate::probe_data::mapper_settings::{close, xyz, Mode, Phase, Request, Sample, Settings};
use crate::probe_data::{
    ledger::{number, Fields},
    schema::Workflow,
};

enum Cycle<'a> {
    Ready,
    Coarse(&'a Fields),
    CoarseBackedOff(&'a Fields),
    Measured,
    MeasuredBackedOff,
    Finished,
}

pub fn samples(
    records: &[Fields],
    s: &Settings,
    require_result: bool,
) -> Result<Vec<Sample>, String> {
    let first = records
        .first()
        .ok_or("The mapper has no retained start record.")?;
    if first["kind"] != "start" {
        return Err("The mapper ledger must begin with its settings.".into());
    }
    let mut samples: Vec<Sample> = Vec::new();
    let mut cycle = Cycle::Ready;
    for r in &records[1..] {
        let kind = r["kind"].as_str();
        if matches!(cycle, Cycle::Finished) {
            return Err("Records follow a terminal mapper result.".into());
        }
        if let Some(failure) = CaptureFailure::from_event(kind, number(r, "stage")?) {
            return Err(format!("{} Record {}: {} Exact contacts remain in the ledger. Use Abort then Pendant Mode; recapture the required geometry after recovery.", failure.name(), r["sequence"], failure.message()));
        }
        let index = number(r, "sample")? as usize;
        let phase = Phase::read(number(r, "phase")?)?;
        if Workflow::Mapper.requires_exact_trigger(kind) {
            if r.get("exact_source").map(String::as_str)
                != Some("emcStatus.motion.traj.probedPosition;machine-mm")
            {
                return Err("Mapper contact lacks the original G38 trigger source.".into());
            }
            let work = xyz(r, "work_", "")?;
            let machine = xyz(r, "machine_", "_exact")?;
            if !(0..3).all(|i| close(work[i] + s.offset[i], machine[i])) {
                return Err(
                    "Mapper trigger coordinates disagree with the retained work frame.".into(),
                );
            }
        }
        match kind {
            "recontact" | "withdrawal-release" | "withdrawal-complete" => {
                let (expected_sample, expected_target) = match &cycle {
                    Cycle::Coarse(coarse) | Cycle::CoarseBackedOff(coarse) => {
                        let expected = if s.full_outline_backoff() && phase.is_outline() {
                            Some(super::withdrawal::target(
                                s,
                                super::withdrawal::request(coarse)?,
                                xyz(coarse, "machine_", "_exact")?,
                            )?)
                        } else {
                            None
                        };
                        (number(coarse, "sample")? as usize, expected)
                    }
                    Cycle::Measured | Cycle::MeasuredBackedOff => {
                        let sample = samples.last().ok_or("Withdrawal has no retained sample.")?;
                        let expected = if s.full_outline_backoff() && phase.is_outline() {
                            Some(super::withdrawal::target(
                                s,
                                sample.request,
                                sample
                                    .trigger
                                    .ok_or("A miss cannot authorize a contact backoff.")?,
                            )?)
                        } else {
                            None
                        };
                        (samples.len() - 1, expected)
                    }
                    Cycle::Ready if s.mode == Mode::Outline && phase == Phase::Finished => {
                        (samples.len(), None)
                    }
                    _ => return Err(
                        "Withdrawal is outside a retained probing cycle; Abort then Pendant Mode."
                            .into(),
                    ),
                };
                if index != expected_sample {
                    return Err("Withdrawal sample differs from its retained contact; Abort then Pendant Mode.".into());
                }
                super::withdrawal::validate(r, s, expected_target)?;
                if kind == "withdrawal-complete" {
                    cycle = match cycle {
                        Cycle::Coarse(coarse) | Cycle::CoarseBackedOff(coarse) => Cycle::CoarseBackedOff(coarse),
                        Cycle::Measured | Cycle::MeasuredBackedOff => Cycle::MeasuredBackedOff,
                        Cycle::Ready => Cycle::Ready,
                        _ => return Err("Withdrawal completion has no recoverable capture state; Abort then Pendant Mode.".into()),
                    };
                }
            }
            "travel" => (),
            "touch" | "miss" => {
                if index != samples.len() {
                    return Err("A mapper contact is duplicated or out of sequence.".into());
                }
                let stage = number(r, "stage")?;
                if kind == "touch" && stage == 0.0 {
                    if !matches!(cycle, Cycle::Ready)
                        || !close(number(r, "feed")?, s.coarse_feed(phase))
                    {
                        return Err(
                            "Coarse mapper contact is out of sequence or has the wrong feed."
                                .into(),
                        );
                    }
                    cycle = Cycle::Coarse(r);
                    continue;
                }
                match (&cycle,kind) {
                    (Cycle::Coarse(coarse) | Cycle::CoarseBackedOff(coarse),"touch") if stage == 1.0 => {
                        if s.full_outline_backoff() && phase.is_outline() && !matches!(cycle,Cycle::CoarseBackedOff(_)) {
                            return Err("Fine outline touch began before the full probe-diameter backoff was retained; Abort then Pendant Mode.".into());
                        }
                        for field in ["phase","edge","approach_x","approach_y","target_x","target_y","target_z"] {
                            if !close(number(r,field)?,number(coarse,field)?) { return Err("Fine touch does not repeat the retained coarse target and direction.".into()); }
                        }
                        if !close(number(r,"feed")?,s.feeds[1]) { return Err("Fine touch has the wrong retained feed.".into()); }
                    }
                    (Cycle::Ready,"miss") if stage == 0.0 => (),
                    _ => return Err("A fine contact or coarse miss does not follow the required capture sequence.".into()),
                }
                if phase == Phase::Finished {
                    return Err("A finished plan cannot supply a measurement.".into());
                }
                let target = xyz(r, "target_", "")?;
                s.bounds(target)?;
                samples.push(Sample {
                    sequence: r["sequence"]
                        .parse()
                        .map_err(|_| "The original contact record identity is invalid. Preserve this ledger and import an intact capture; for an active run use Abort then Pendant Mode.")?,
                    returned: None,
                    request: Request {
                        phase,
                        edge: number(r, "edge")? as i32,
                        approach: [number(r, "approach_x")?, number(r, "approach_y")?],
                        target,
                    },
                    trigger: if kind == "touch" {
                        Some(xyz(r, "machine_", "_exact")?)
                    } else {
                        None
                    },
                });
                cycle = Cycle::Measured;
            }
            "ready" => {
                let sample_count = samples.len();
                let sample = samples
                    .last_mut()
                    .ok_or("A mapper ready record has no measured sample.")?;
                let clearance = if sample.request.phase.is_outline() {
                    sample.request.target[2]
                } else {
                    s.origin[2]
                };
                let returned = xyz(r, "work_", "")?;
                s.bounds(returned)?;
                if !matches!(cycle, Cycle::Measured | Cycle::MeasuredBackedOff)
                    || index + 1 != sample_count
                    || phase != sample.request.phase
                    || (returned[2] - clearance).abs() > s.step[2] / 2.0 + 1e-9
                {
                    return Err(
                        "The previous sample lacks its matching released endpoint at the planned Z plane."
                            .into(),
                    );
                }
                if s.full_outline_backoff() && phase.is_outline() && sample.trigger.is_some() {
                    let expected =
                        super::withdrawal::target(s, sample.request, sample.trigger.unwrap())?;
                    if !matches!(cycle, Cycle::MeasuredBackedOff)
                        || !s.endpoint_matches(returned, expected)
                    {
                        return Err("Outline ready was reported before a full probe-diameter backoff; Abort then Pendant Mode.".into());
                    }
                }
                sample.returned = Some(returned);
                cycle = Cycle::Ready;
            }
            "result" => {
                if !matches!(cycle, Cycle::Ready)
                    || index != samples.len()
                    || (number(r, "work_z")? - s.origin[2]).abs() > s.step[2] / 2.0 + 1e-9
                {
                    return Err(
                        "The mapper result lacks its final clearance return or sample count."
                            .into(),
                    );
                }
                cycle = Cycle::Finished;
            }
            _ => return Err(format!("Unexpected mapper event {kind}.")),
        }
    }
    if !matches!(cycle, Cycle::Ready | Cycle::Finished) {
        return Err("The previous capture/return cycle is unfinished. No next target will be issued; Abort then Pendant Mode.".into());
    }
    if require_result && !matches!(cycle, Cycle::Finished) {
        return Err("No final mapper result was retained.".into());
    }
    Ok(samples)
}
