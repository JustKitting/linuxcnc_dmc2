use super::super::ledger::{number, Fields};
use super::geometry::{self, Cell, Frontier, Rectangle, Station};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Phase {
    Reference = 0,
    Grid = 1,
    Side = 2,
    Finished = 3,
}

impl Phase {
    fn read(record: &Fields) -> Result<Self, String> {
        match number(record, "phase")? as i32 {
            0 => Ok(Self::Reference),
            1 => Ok(Self::Grid),
            2 => Ok(Self::Side),
            _ => Err("Unknown retained gauge-block measurement phase.".into()),
        }
    }
}

pub(super) fn point(record: &Fields, prefix: &str) -> Result<[f64; 3], String> {
    Ok([
        number(record, &format!("{prefix}x"))?,
        number(record, &format!("{prefix}y"))?,
        number(record, &format!("{prefix}z"))?,
    ])
}

pub(super) fn trigger(record: &Fields) -> Result<[f64; 3], String> {
    Ok([
        number(record, "machine_x_exact")?,
        number(record, "machine_y_exact")?,
        number(record, "machine_z_exact")?,
    ])
}

#[derive(Clone, Debug)]
pub(super) struct Plan {
    pub phase: Phase,
    pub sample: usize,
    pub cell: Cell,
    pub edge: i32,
    pub layer: usize,
    pub approach: [f64; 2],
    pub target: [f64; 3],
    pub clear: f64,
}

impl Plan {
    pub fn values(&self, sequence: u64) -> Vec<(&'static str, f64)> {
        vec![
            ("sequence", sequence as f64),
            ("phase", self.phase as u8 as f64),
            ("sample", self.sample as f64),
            ("column", self.cell.0 as f64),
            ("row", self.cell.1 as f64),
            ("edge", self.edge as f64),
            ("layer", self.layer as f64),
            ("approach-x", self.approach[0]),
            ("approach-y", self.approach[1]),
            ("target-x", self.target[0]),
            ("target-y", self.target[1]),
            ("target-z", self.target[2]),
            ("clear-z", self.clear),
        ]
    }
}

pub(super) struct State<'a> {
    pub start: &'a Fields,
    pub samples: Vec<&'a Fields>,
    pub grid: BTreeMap<Cell, bool>,
    pub top: Vec<&'a Fields>,
    pub sides: Vec<&'a Fields>,
    pub failed: bool,
    pub ended: bool,
    ready: bool,
    frontier: Frontier,
    last: &'a Fields,
}

impl<'a> State<'a> {
    pub fn read(records: &'a [Fields]) -> Result<Self, String> {
        let start = records
            .first()
            .filter(|r| r["kind"] == "start")
            .ok_or("Block start settings are missing.")?;
        let mut state = Self {
            start,
            samples: Vec::new(),
            grid: BTreeMap::new(),
            top: Vec::new(),
            sides: Vec::new(),
            failed: false,
            ended: false,
            ready: true,
            frontier: Frontier::default(),
            last: start,
        };
        let mut coarse = BTreeSet::new();
        for record in &records[1..] {
            if state.ended {
                return Err("Block records continue after the final result; retained result is quarantined.".into());
            }
            state.last = record;
            match record["kind"].as_str() {
                "start" => return Err("Block ledger repeats its starting settings.".into()),
                "obstruction" | "recovery" => {
                    state.failed = true;
                    state.ready = false;
                }
                "result" => state.ended = true,
                "ready" => {
                    if state.samples.is_empty()
                        || number(record, "sample")? != (state.samples.len() - 1) as f64
                    {
                        return Err(
                            "Block clearance record does not follow its accepted sample.".into(),
                        );
                    }
                    state.ready = true;
                }
                "touch" if number(record, "stage")? == 0.0 => {
                    let sample = number(record, "sample")? as usize;
                    if sample != state.samples.len() || !coarse.insert(sample) {
                        return Err("Coarse block contact is duplicated or out of sequence.".into());
                    }
                    state.ready = false;
                }
                "touch" | "miss" => {
                    let sample = number(record, "sample")? as usize;
                    let hit = record["kind"] == "touch";
                    if sample != state.samples.len()
                        || (hit && !coarse.remove(&sample))
                        || (!hit && coarse.contains(&sample))
                    {
                        return Err("Block slow contact/miss does not follow the expected sample and coarse-contact sequence.".into());
                    }
                    let phase = Phase::read(record)?;
                    let cell = (
                        number(record, "column")? as i32,
                        number(record, "row")? as i32,
                    );
                    match phase {
                        Phase::Reference => {
                            if sample != 0 || cell != (0, 0) {
                                return Err("The initial block reference was not captured by a slow top re-touch.".into());
                            }
                            if hit {
                                state.grid.insert((0, 0), true);
                                state.frontier.observe((0, 0), true, &state.grid);
                                state.top.push(record);
                            } else {
                                state.failed = true;
                            }
                        }
                        Phase::Grid => {
                            if state.top.is_empty()
                                || !state.sides.is_empty()
                                || state.frontier.next() != Some(cell)
                            {
                                return Err("Block grid observations do not follow the outward hit/miss frontier.".into());
                            }
                            state.grid.insert(cell, hit);
                            state.frontier.observe(cell, hit, &state.grid);
                            if hit {
                                state.top.push(record);
                            }
                        }
                        Phase::Side => {
                            if state.frontier.next().is_some() {
                                return Err("Side contacts started before the grid had a closed miss boundary.".into());
                            }
                            if hit {
                                state.sides.push(record);
                            } else {
                                state.failed = true;
                            }
                        }
                        Phase::Finished => unreachable!(),
                    }
                    state.samples.push(record);
                    state.ready = false;
                }
                "travel" => (),
                _ => return Err("Unknown block measurement event.".into()),
            }
        }
        Ok(state)
    }

    pub fn offset(&self) -> Result<[f64; 3], String> {
        point(self.start, "offset_")
    }

    pub fn work_trigger(&self, record: &Fields) -> Result<[f64; 3], String> {
        let p = trigger(record)?;
        let o = self.offset()?;
        Ok([0, 1, 2].map(|i| p[i] - o[i]))
    }

    pub fn reference(&self) -> Result<f64, String> {
        Ok(self.work_trigger(self.top.first().ok_or("No retained fine top reference.")?)?[2])
    }

    pub fn clear(&self) -> Result<f64, String> {
        let highest = self
            .top
            .iter()
            .map(|r| self.work_trigger(r).map(|p| p[2]))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .reduce(f64::max)
            .ok_or("No measured top from which to define block clearance.")?;
        Ok(highest + number(self.start, "clearance")?)
    }

    pub fn footprint(&self) -> Result<Rectangle, String> {
        let origin = point(self.start, "")?;
        let spacing = number(self.start, "grid")?;
        let mut hits = Vec::new();
        let mut misses = Vec::new();
        for (&(col, row), &hit) in &self.grid {
            let p = [
                origin[0] + col as f64 * spacing,
                origin[1] + row as f64 * spacing,
            ];
            if hit {
                hits.push(p);
            } else {
                misses.push(p);
            }
        }
        Rectangle::fit(&hits, &misses, spacing)
    }

    pub fn stations(&self) -> Result<Vec<Station>, String> {
        self.footprint()?.stations(
            number(self.start, "grid")?,
            number(self.start, "ball_diameter")? / 2.0,
            number(self.start, "clearance")?,
        )
    }

    fn bounds(&self, p: [f64; 3]) -> Result<(), String> {
        let offset = self.offset()?;
        for (i, axis) in ["x", "y", "z"].into_iter().enumerate() {
            let machine = p[i] + offset[i];
            let half_step = number(self.start, &format!("step_{axis}"))? / 2.0;
            if !machine.is_finite()
                || machine <= number(self.start, &format!("{axis}_min"))? + half_step
                || machine >= number(self.start, &format!("{axis}_max"))? - half_step
            {
                let direction = if axis == "x" {
                    " X: physical RIGHT / LinuxCNC -X; physical LEFT / LinuxCNC +X."
                } else {
                    ""
                };
                return Err(format!("The discovered scan needs {axis}={machine} machine mm at or beyond its travel boundary. The range was not clipped.{direction}"));
            }
        }
        Ok(())
    }

    pub fn next(&self) -> Result<Plan, String> {
        if self.failed || self.ended {
            return Err("This run is terminal after an obstruction, missing side contact or final result. It cannot resume.".into());
        }
        if !self.ready {
            return Err("The previous measurement has no retained clearance return. No next move was planned.".into());
        }
        let origin = point(self.start, "")?;
        let mut p = Plan {
            phase: Phase::Reference,
            sample: self.samples.len(),
            cell: (0, 0),
            edge: -1,
            layer: 0,
            approach: [origin[0], origin[1]],
            target: [
                origin[0],
                origin[1],
                origin[2] - number(self.start, "search")?,
            ],
            clear: origin[2],
        };
        if !self.samples.is_empty() {
            p.clear = self.clear()?;
            if number(self.last, "work_z")? < p.clear - number(self.start, "step_z")? / 2.0 {
                return Err("The script's retained return is below the measured block clearance plane. No lateral transfer was planned.".into());
            }
            if let Some(cell) = self.frontier.next() {
                let grid = number(self.start, "grid")?;
                p.phase = Phase::Grid;
                p.cell = cell;
                p.approach = [
                    origin[0] + cell.0 as f64 * grid,
                    origin[1] + cell.1 as f64 * grid,
                ];
                p.target = [
                    p.approach[0],
                    p.approach[1],
                    self.reference()? - number(self.start, "drop")?,
                ];
            } else {
                let stations = self.stations()?;
                // Every target in all three circuits is bounded BEFORE the first
                // side descent. Their XY station/direction is never re-fitted.
                for layer in 0..3 {
                    let z = self.reference()?
                        - number(self.start, "ball_diameter")? / 2.0
                        - number(self.start, "side_depth")?
                        - layer as f64;
                    for station in &stations {
                        if geometry::dot(
                            [0, 1].map(|i| station.target[i] - station.approach[i]),
                            station.normal,
                        ) >= 0.0
                        {
                            return Err("The planned side approach does not point toward the block. No side descent was issued.".into());
                        }
                        for xy in [station.approach, station.target] {
                            self.bounds([xy[0], xy[1], z])?;
                        }
                        self.bounds([station.approach[0], station.approach[1], p.clear])?;
                    }
                }
                let expected = 3 * stations.len();
                if self.sides.len() > expected {
                    return Err(
                        "More side stations were retained than the three planned circuits.".into(),
                    );
                }
                for (index, record) in self.sides.iter().enumerate() {
                    if number(record, "edge")? != stations[index % stations.len()].edge as f64
                        || number(record, "layer")? != (index / stations.len()) as f64
                    {
                        return Err("Retained side measurements do not follow the same stations on all three circuits.".into());
                    }
                }
                if self.sides.len() == expected {
                    p.phase = Phase::Finished;
                    p.target = point(self.last, "work_")?;
                    p.approach = [p.target[0], p.target[1]];
                } else {
                    let station = stations[self.sides.len() % stations.len()];
                    p.phase = Phase::Side;
                    p.edge = station.edge as i32;
                    p.layer = self.sides.len() / stations.len();
                    p.approach = station.approach;
                    p.target = [
                        station.target[0],
                        station.target[1],
                        self.reference()?
                            - number(self.start, "ball_diameter")? / 2.0
                            - number(self.start, "side_depth")?
                            - p.layer as f64,
                    ];
                }
            }
        }
        self.bounds(p.target)?;
        self.bounds([p.approach[0], p.approach[1], p.clear])?;
        Ok(p)
    }
}
