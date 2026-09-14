//! Validate capture/release/clearance transitions before planning another move.
use super::super::ledger::{number, Fields};
use super::model::{close, xyz, Phase, Request, Sample, Settings};

enum Cycle<'a> {
    Ready,
    Coarse(&'a Fields),
    Measured,
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
    let mut samples = Vec::new();
    let mut cycle = Cycle::Ready;
    for r in &records[1..] {
        let kind = r["kind"].as_str();
        if matches!(cycle, Cycle::Finished) {
            return Err("Records follow a terminal mapper result.".into());
        }
        if matches!(kind, "obstruction" | "recovery") {
            return Err("The scan was interrupted by an obstruction. Exact contacts remain in the ledger; start a new Run after operator recovery.".into());
        }
        let index = number(r, "sample")? as usize;
        match kind {
            "travel" => (),
            "touch" | "miss" => {
                if index != samples.len() {
                    return Err("A mapper contact is duplicated or out of sequence.".into());
                }
                let stage = number(r, "stage")?;
                if kind == "touch" {
                    if r.get("exact_source").map(String::as_str)
                        != Some("emcStatus.motion.traj.probedPosition;machine-mm")
                    {
                        return Err("Mapper contact lacks the original G38 trigger source.".into());
                    }
                    let work = xyz(r, "work_", "")?;
                    let machine = xyz(r, "machine_", "_exact")?;
                    if !(0..3).all(|i| close(work[i] + s.offset[i], machine[i])) {
                        return Err(
                            "Mapper trigger coordinates disagree with the retained work frame."
                                .into(),
                        );
                    }
                }
                if kind == "touch" && stage == 0.0 {
                    if !matches!(cycle, Cycle::Ready) || !close(number(r, "feed")?, s.feeds[0]) {
                        return Err(
                            "Coarse mapper contact is out of sequence or has the wrong feed."
                                .into(),
                        );
                    }
                    cycle = Cycle::Coarse(r);
                    continue;
                }
                match (&cycle,kind) {
                    (Cycle::Coarse(coarse),"touch") if stage == 1.0 => {
                        for field in ["phase","edge","approach_x","approach_y","target_x","target_y","target_z"] {
                            if !close(number(r,field)?,number(coarse,field)?) { return Err("Fine touch does not repeat the retained coarse target and direction.".into()); }
                        }
                        if !close(number(r,"feed")?,s.feeds[1]) { return Err("Fine touch has the wrong retained feed.".into()); }
                    }
                    (Cycle::Ready,"miss") if stage == 0.0 => (),
                    _ => return Err("A fine contact or coarse miss does not follow the required capture sequence.".into()),
                }
                let phase = match number(r, "phase")? {
                    0.0 => Phase::Reference,
                    1.0 => Phase::Boundary,
                    2.0 => Phase::Grid,
                    3.0 => Phase::Verify,
                    4.0 => Phase::Rim,
                    _ => return Err("Invalid measurement phase.".into()),
                };
                let target = xyz(r, "target_", "")?;
                s.bounds(target)?;
                samples.push(Sample {
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
                if !matches!(cycle, Cycle::Measured)
                    || index + 1 != samples.len()
                    || (number(r, "work_z")? - s.origin[2]).abs() > s.step[2] / 2.0 + 1e-9
                {
                    return Err(
                        "The previous sample lacks its matching return to starting-Z clearance."
                            .into(),
                    );
                }
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
