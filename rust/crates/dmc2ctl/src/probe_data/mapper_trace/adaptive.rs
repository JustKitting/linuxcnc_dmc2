//! Growing contact intervals with measured midpoint refinement.
//! Trial contacts remain observations; selection retains contour order explicitly.
use super::{outline::*, Progress};
use crate::probe_data::mapper_settings::{Phase, Sample};
use std::{
    collections::BTreeSet,
    f64::consts::{FRAC_PI_2, TAU},
};
type P = [f64; 2];
struct Trial<'a> {
    sample: &'a Sample,
    outward: P,
    // Previously traversed clear path, ending at the full released endpoint.
    clear_path: Vec<P>,
}
fn sectors(radius: f64, resolution: f64) -> Result<usize, Progress> {
    let angle = (2.0 * (1.0 - resolution / radius).acos()).min(FRAC_PI_2);
    if !angle.is_finite() || angle <= 0. || TAU / angle > i64::MAX as f64 {
        return Err(TraceError::SearchPrecision.into());
    }
    Ok((TAU / angle).ceil() as usize)
}
fn trial<'a>(
    r: &mut Reader<'a>,
    contact: P,
    outward: P,
    radius: f64,
    z: f64,
) -> Result<Trial<'a>, Progress> {
    let anchor = add(contact, scale(outward, r.s.outline_backoff()));
    let mut current = anchor;
    let mut clear_path = vec![anchor];
    let count = sectors(radius, r.s.resolution)?;
    for sector in 1..=count {
        let candidate = add(
            contact,
            scale(rotate(outward, TAU * sector as f64 / count as f64), radius),
        );
        // Physical RIGHT / LinuxCNC -X; physical LEFT / LinuxCNC +X.
        let sample = r.local(Phase::OutlineAdvance, current, candidate, z)?;
        if sample.trigger.is_some() {
            let returned = endpoint(sample)?;
            clear_path.push(returned);
            return Ok(Trial {
                sample,
                outward: unit(sub(current, candidate))?,
                clear_path,
            });
        }
        current = endpoint(sample)?;
        clear_path.push(current);
    }
    Err(TraceError::EmptySweep.into())
}
fn travel(r: &mut Reader<'_>, current: P, target: P, z: f64) -> Result<P, Progress> {
    if r.s
        .endpoint_matches([current[0], current[1], z], [target[0], target[1], z])
    {
        return Ok(current);
    }
    // Revisit only the same recorded clear segments. No unobserved shortcut.
    // Physical RIGHT / LinuxCNC -X; physical LEFT / LinuxCNC +X.
    let sample = r.local(Phase::OutlineBackoff, current, target, z)?;
    if sample.trigger.is_some() {
        return Err(TraceError::BlockedBackoff.into());
    }
    Ok(endpoint(sample)?)
}
fn retrace(r: &mut Reader<'_>, trial: &Trial<'_>, reverse: bool, z: f64) -> Result<(), Progress> {
    let path = if reverse {
        trial.clear_path.iter().rev().copied().collect::<Vec<_>>()
    } else {
        trial.clear_path.clone()
    };
    let mut current = path[0];
    for p in path.iter().skip(1) {
        current = travel(r, current, *p, z)?;
    }
    Ok(())
}
fn deviation(p: P, q: P, m: P) -> Result<(f64, bool), Progress> {
    let d = sub(q, p);
    let length = length(d);
    if !length.is_finite() || length == 0. {
        return Err(TraceError::RepeatedRegion.into());
    }
    let n = [d[0] / length, d[1] / length];
    let delta = sub(m, p);
    let along = dot(delta, n);
    let error = (delta[0] * n[1] - delta[1] * n[0]).abs();
    if !error.is_finite() {
        return Err(TraceError::SearchPrecision.into());
    }
    Ok((error, along > 0. && along < length))
}
pub(super) fn walk<'a>(
    r: &mut Reader<'a>,
    first: &'a Sample,
    outside: P,
    inside: P,
    z: f64,
) -> Result<(), Progress> {
    let s = r.s;
    let mut selected = first;
    let start = xy(point(first, s)?);
    let first_outward = unit(sub(outside, inside))?;
    let mut outward = first_outward;
    let mut radius = s.grid;
    let mut distance = 0.;
    let mut visited = BTreeSet::new();
    let direction_bins = sectors(s.grid, s.resolution)?;
    loop {
        let contact = xy(point(selected, s)?);
        let current = endpoint(selected)?;
        let anchor = add(contact, scale(outward, s.outline_backoff()));
        if !s.endpoint_matches([current[0], current[1], z], [anchor[0], anchor[1], z]) {
            return Err(Progress::Invalid("The selected outline contact lacks its full probe-diameter backoff. No next candidate issued; Abort then Pendant Mode.".into()));
        }
        let seam_distance = length(sub(contact, start));
        let seam_direction = dot(outward, first_outward) > 0.;
        if distance > TAU * s.grid && seam_distance <= s.grid && seam_direction {
            let approach = add(start, scale(first_outward, s.outline_backoff()));
            let target = sub(start, scale(first_outward, s.resolution));
            // Existing independent seam touch. Physical RIGHT / LinuxCNC -X;
            // physical LEFT / LinuxCNC +X, in the retained fixed Z plane.
            let closing = r.local(Phase::OutlineClose, approach, target, z)?;
            if length(sub(xy(point(closing, s)?), start)) > s.resolution {
                return Err(TraceError::ClosureMismatch.into());
            }
            r.select(closing)?;
            return r.finished();
        }
        let bin = ((outward[1].atan2(outward[0]).rem_euclid(TAU) / TAU) * direction_bins as f64)
            .round() as i64
            % direction_bins as i64;
        if !visited.insert((
            (contact[0] / s.resolution).round() as i64,
            (contact[1] / s.resolution).round() as i64,
            bin,
        )) {
            return Err(TraceError::RepeatedRegion.into());
        }
        // A circle is bounded by the nearest recorded plate/travel boundary.
        // Reserve the already-required full backoff when enlarging that circle.
        let margin = (0..2)
            .flat_map(|i| [contact[i] - s.min[i], s.max[i] - contact[i]])
            .fold(f64::INFINITY, f64::min);
        let mut maximum = margin - s.outline_backoff();
        if distance > TAU * s.grid && seam_direction {
            maximum = maximum.min(seam_distance);
        }
        radius = radius.min(maximum);
        if !radius.is_finite() || radius < s.grid {
            return Err(Progress::Invalid("No local search radius remains inside the retained plate bounds with the required full backoff. Partial geometry is retained; Abort then Pendant Mode.".into()));
        }
        let mut coarse = trial(r, contact, outward, radius, z)?;
        loop {
            let q = xy(point(coarse.sample, s)?);
            let half = radius / 2.;
            // The entered trace spacing is a sampling interval; resolution is
            // the midpoint/chord error criterion. Do not silently turn that
            // error tolerance into progressively smaller travel intervals.
            if half < s.grid {
                r.refinements.push(Refinement {
                    from_sequence: selected.sequence,
                    coarse_sequence: coarse.sample.sequence,
                    midpoint_sequence: None,
                    radius_mm: radius,
                    error_mm: None,
                    resolved: false,
                });
                r.select(coarse.sample)?;
                distance += length(sub(q, contact));
                selected = coarse.sample;
                outward = coarse.outward;
                radius = (radius * 2.).min(maximum);
                break;
            }
            retrace(r, &coarse, true, z)?;
            let midpoint = trial(r, contact, outward, half, z)?;
            let m = xy(point(midpoint.sample, s)?);
            let (error, between) = deviation(contact, q, m)?;
            let resolved = between && error <= s.resolution;
            r.refinements.push(Refinement {
                from_sequence: selected.sequence,
                coarse_sequence: coarse.sample.sequence,
                midpoint_sequence: Some(midpoint.sample.sequence),
                radius_mm: radius,
                error_mm: Some(error),
                resolved,
            });
            if resolved {
                // Restore the retained coarse release by its measured clear path.
                retrace(r, &midpoint, true, z)?;
                retrace(r, &coarse, false, z)?;
                r.select(midpoint.sample)?;
                r.select(coarse.sample)?;
                distance += length(sub(m, contact)) + length(sub(q, m));
                selected = coarse.sample;
                outward = coarse.outward;
                radius = (radius * 2.).min(maximum); // requested exponential growth
                break;
            }
            // Refine toward the original anchor; the farther trial remains
            // retained observation data, never a deleted or fabricated contact.
            radius = half;
            // This is the already-measured smaller search circle, even when
            // its contact rejected the larger interval's chord. Do not repeat
            // that same search or claim that the larger chord was resolved.
            coarse = midpoint;
        }
    }
}
