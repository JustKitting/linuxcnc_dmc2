//! Retained-data model for the two automatic stock scans. No machine commands.
use super::super::ledger::{number, Fields};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Surface,
    Rim,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    Reference,
    Boundary,
    Grid,
    Verify,
    Rim,
    Finished,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub mode: Mode,
    pub origin: [f64; 3],
    pub offset: [f64; 3],
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub grid: f64,
    pub resolution: f64,
    pub floor: f64,
    pub radius: f64,
    pub side_depth: f64,
    pub backoff: f64,
    pub feeds: [f64; 3], // horizontal coarse, fine, travel, mm/min
    pub downward_feed: f64,
    pub step: [f64; 3],
}

pub fn data(text: &str, magic: &str, required: &[&str]) -> Result<Fields, String> {
    let mut lines = text.lines();
    if lines.next() != Some(magic) {
        return Err(format!("Missing {magic} data header."));
    }
    let mut fields = BTreeMap::new();
    for line in lines {
        let (k, v) = line
            .split_once('=')
            .ok_or("Malformed mapper settings field.")?;
        if !required.contains(&k) || fields.insert(k.into(), v.into()).is_some() {
            return Err(format!("Unknown or repeated mapper setting {k}."));
        }
    }
    for key in required {
        number(&fields, key)?;
    }
    Ok(fields)
}

impl Settings {
    pub fn read(start: &Fields, plate: &Fields, policy: &Fields) -> Result<Self, String> {
        let n = |k: &str| number(start, k);
        let xyz = |prefix: &str| -> Result<[f64; 3], String> {
            Ok([
                n(&format!("{prefix}x"))?,
                n(&format!("{prefix}y"))?,
                n(&format!("{prefix}z"))?,
            ])
        };
        let origin = xyz("")?;
        let offset = xyz("offset_")?;
        let step = xyz("step_")?;
        let radius = number(plate, "ball_diameter")? / 2.0;
        if radius <= 0.0 {
            return Err("The plate envelope has no positive probe ball diameter.".into());
        }
        let mut min = [0.0; 3];
        let mut max = [0.0; 3];
        for (i, axis) in ["x", "y", "z"].iter().enumerate() {
            if step[i] <= 0.0 {
                return Err(format!("Invalid {axis} step scale."));
            }
            min[i] = n(&format!("{axis}_min"))? + step[i];
            max[i] = n(&format!("{axis}_max"))? - step[i];
            if i < 2 {
                // Keep the ball inside the operator's plate envelope. Machine
                // travel is a further constraint, never a replacement for it.
                min[i] = min[i].max(number(plate, &format!("{axis}_min"))? + radius);
                max[i] = max[i].min(number(plate, &format!("{axis}_max"))? - radius);
            }
            min[i] -= offset[i];
            max[i] -= offset[i];
            if min[i] >= max[i] {
                return Err(format!("Empty {axis} plate/travel intersection."));
            }
        }
        for key in [
            "drop",
            "grid",
            "resolution",
            "usable_reach",
            "reach_reserve",
        ] {
            if n(key)? <= 0.0 {
                return Err(format!("Set a positive {key} in Scripts before Run."));
            }
        }
        if n("drop")? + n("reach_reserve")? > n("usable_reach")? {
            return Err("The descent budget plus reserve exceeds the mounted usable probe reach. Correct these Scripts fields before Run.".into());
        }
        let result = Self {
            mode: match n("mode")? {
                0.0 => Mode::Surface,
                1.0 => Mode::Rim,
                _ => return Err("Unknown automatic mapper mode.".into()),
            },
            origin,
            offset,
            min,
            max,
            step,
            radius,
            grid: n("grid")?,
            resolution: n("resolution")?,
            floor: origin[2] - n("drop")?,
            side_depth: n("side_depth")?,
            backoff: n("backoff")?,
            downward_feed: number(policy, "downward_feed")?,
            feeds: [
                number(policy, "coarse_feed")?,
                number(policy, "fine_feed")?,
                number(policy, "travel_feed")?,
            ],
        };
        let max_feed = n("max_feed")?;
        for (name, feed) in ["horizontal search", "fine", "travel"]
            .into_iter()
            .zip(result.feeds)
            .chain([("downward search", result.downward_feed)])
        {
            if feed <= 0.0 {
                return Err(format!("Mapper {name} feed must be positive."));
            }
            if feed > max_feed {
                return Err(format!("Mapper {name} feed {feed} mm/min exceeds the configured machine maximum {max_feed} mm/min."));
            }
        }
        if result.feeds[1] > result.feeds[0].min(result.downward_feed) {
            return Err(
                "The fine re-touch feed exceeds the search feed in this run's retained settings."
                    .into(),
            );
        }
        if result.grid < step[0].max(step[1])
            || result.resolution < step[0].max(step[1])
            || result.resolution > result.grid
        {
            return Err("Grid and boundary resolution must be at least one XY step; boundary resolution must not exceed grid spacing.".into());
        }
        if result.mode == Mode::Rim && (result.side_depth <= 0.0 || result.backoff <= 0.0) {
            return Err("Set positive rim depth and backoff before Run.".into());
        }
        result.bounds(origin)?;
        result.bounds([origin[0], origin[1], result.floor])?;
        Ok(result)
    }

    pub fn coarse_feed(&self, phase: Phase) -> f64 {
        if phase == Phase::Rim {
            self.feeds[0]
        } else {
            self.downward_feed
        }
    }

    pub fn bounds(&self, p: [f64; 3]) -> Result<(), String> {
        for (i, axis) in ["x", "y", "z"].iter().enumerate() {
            if !p[i].is_finite()
                || (p[i] < self.min[i] && !close(p[i], self.min[i]))
                || (p[i] > self.max[i] && !close(p[i], self.max[i]))
            {
                return Err(format!("Required {axis}={} machine mm is outside this scan's plate/travel envelope. No substitute target was issued. X: physical RIGHT / LinuxCNC -X; physical LEFT / LinuxCNC +X.", p[i] + self.offset[i]));
            }
        }
        if (p[2] < self.floor && !close(p[2], self.floor))
            || (p[2] > self.origin[2] && !close(p[2], self.origin[2]))
        {
            return Err(
                "A requested target exceeds the original starting-Z descent envelope.".into(),
            );
        }
        Ok(())
    }

    pub fn inside_xy(&self, p: [f64; 2]) -> bool {
        (0..2).all(|i| p[i] >= self.min[i] && p[i] <= self.max[i])
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub phase: Phase,
    pub edge: i32,
    pub approach: [f64; 2],
    pub target: [f64; 3],
}

impl Request {
    pub fn top(s: &Settings, phase: Phase, xy: [f64; 2]) -> Self {
        Self {
            phase,
            edge: -1,
            approach: xy,
            target: [xy[0], xy[1], s.floor],
        }
    }
    pub fn values(self, s: &Settings, sample: usize, sequence: u64) -> Vec<(&'static str, f64)> {
        vec![
            ("sequence", sequence as f64),
            ("phase", self.phase as u8 as f64),
            ("sample", sample as f64),
            ("edge", self.edge as f64),
            ("approach-x", self.approach[0]),
            ("approach-y", self.approach[1]),
            ("target-x", self.target[0]),
            ("target-y", self.target[1]),
            ("target-z", self.target[2]),
            ("clear-z", s.origin[2]),
            ("coarse-feed", s.coarse_feed(self.phase)),
            ("downward-feed", s.downward_feed),
            ("fine-feed", s.feeds[1]),
            ("travel-feed", s.feeds[2]),
        ]
    }
}

#[derive(Clone, Debug)]
pub struct Sample {
    pub request: Request,
    pub trigger: Option<[f64; 3]>,
}

pub fn xyz(fields: &Fields, prefix: &str, suffix: &str) -> Result<[f64; 3], String> {
    Ok([
        number(fields, &format!("{prefix}x{suffix}"))?,
        number(fields, &format!("{prefix}y{suffix}"))?,
        number(fields, &format!("{prefix}z{suffix}"))?,
    ])
}

pub fn close(a: f64, b: f64) -> bool {
    // Decimal G-code serialization plus floating-point arithmetic, not an
    // invented tolerance for mechanical motion or probe repeatability.
    (a - b).abs() <= 1e-9 + 8.0 * f64::EPSILON * a.abs().max(b.abs()).max(1.0)
}
