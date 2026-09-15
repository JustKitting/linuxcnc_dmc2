//! Binary edge brackets, connected top grid and independent face checks.
use super::super::geometry::{Point, Rectangle};
use super::model::{close, Mode, Phase, Request, Sample, Settings};
use std::collections::{BTreeMap, BTreeSet};

pub use dmc2ctl::probe_data::mapper_trace::Progress;

pub struct Survey<'a> {
    pub(super) s: &'a Settings,
    pub(super) samples: &'a [Sample],
    pub(super) cursor: usize,
    cache: BTreeMap<(u64, u64), (bool, usize)>,
    pub(super) brackets: Vec<super::free_surface::Bracket>,
    pub(super) boundary_contacts: Vec<super::free_surface::BoundaryContact>,
    pub(super) censored_grid: BTreeSet<(i32, i32)>,
    pub hits: Vec<Point>,
    pub misses: Vec<Point>,
}

impl<'a> Survey<'a> {
    pub fn new(s: &'a Settings, samples: &'a [Sample]) -> Self {
        Self {
            s,
            samples,
            cursor: 0,
            cache: BTreeMap::new(),
            brackets: Vec::new(),
            boundary_contacts: Vec::new(),
            censored_grid: BTreeSet::new(),
            hits: Vec::new(),
            misses: Vec::new(),
        }
    }
    fn measure(&mut self, request: Request) -> Result<bool, Progress> {
        if self.cursor >= i32::MAX as usize {
            return Err(Progress::Invalid("The retained mapper sample index is full. Preserve this run and use a larger grid spacing for a new Run; Abort and Pendant Mode remain available.".into()));
        }
        self.s.bounds(request.target)?;
        self.s
            .bounds([request.approach[0], request.approach[1], self.s.origin[2]])?;
        let Some(sample) = self.samples.get(self.cursor) else {
            return Err(Progress::Need(request));
        };
        if request.phase != sample.request.phase
            || request.edge != sample.request.edge
            || !(0..3).all(|i| close(request.target[i], sample.request.target[i]))
            || !(0..2).all(|i| close(request.approach[i], sample.request.approach[i]))
        {
            return Err(Progress::Invalid(format!("Retained sample {} disagrees with the deterministic scan plan; start a new Run after recovery.", self.cursor)));
        }
        self.cursor += 1;
        Ok(sample.trigger.is_some())
    }
    pub(super) fn top(&mut self, phase: Phase, xy: Point, fresh: bool) -> Result<bool, Progress> {
        let key = (xy[0].to_bits(), xy[1].to_bits());
        if !fresh {
            if let Some((hit, _)) = self.cache.get(&key) {
                return Ok(*hit);
            }
        }
        let hit = self.measure(Request::top(self.s, phase, xy))?;
        self.cache
            .insert(key, (hit, self.samples[self.cursor - 1].sequence));
        if hit {
            self.hits.push(xy);
        } else {
            self.misses.push(xy);
        }
        Ok(hit)
    }
    pub(super) fn bisect(
        &mut self,
        phase: Phase,
        mut hit: Point,
        mut miss: Point,
    ) -> Result<(Point, Point), Progress> {
        let bracket = if self.s.mode == Mode::FreeSurface {
            self.brackets.push(self.bracket(hit, miss)?);
            Some(self.brackets.len() - 1)
        } else {
            None
        };
        while (hit[0] - miss[0]).hypot(hit[1] - miss[1]) > self.s.resolution {
            let mid = [0, 1].map(|i| (hit[i] + miss[i]) / 2.0);
            if mid == hit || mid == miss {
                return Err(Progress::Invalid("Binary edge search exhausted representable coordinates before the requested resolution.".into()));
            }
            if self.top(phase, mid, false)? {
                hit = mid;
            } else {
                miss = mid;
            }
            if let Some(i) = bracket {
                self.brackets[i] = self.bracket(hit, miss)?;
            }
        }
        Ok((hit, miss))
    }

    pub(super) fn source(&self, p: Point) -> Result<usize, Progress> {
        self.cache.get(&(p[0].to_bits(), p[1].to_bits())).map(|(_, sequence)| *sequence)
            .ok_or_else(|| Progress::Invalid("A boundary endpoint has no original observation. Preserve the run; no inferred contact was used.".into()))
    }
    pub(super) fn discover_grid(&mut self) -> Result<BTreeMap<(i32, i32), bool>, Progress> {
        let seed = [self.s.origin[0], self.s.origin[1]];
        // Eight-connected discovery includes diagonal connectivity for rotated
        // stock. Neighbours are measured; unvisited space is never labelled air.
        let mut grid = BTreeMap::new();
        let mut pending = BTreeSet::from([(0_i32, 0_i32)]);
        let mut last = (0_i32, 0_i32);
        while !pending.is_empty() {
            let cell = *pending
                .iter()
                .min_by_key(|&&(x, y)| {
                    (i128::from(x) - i128::from(last.0)).pow(2)
                        + (i128::from(y) - i128::from(last.1)).pow(2)
                })
                .unwrap();
            pending.remove(&cell);
            let xy = [
                seed[0] + cell.0 as f64 * self.s.grid,
                seed[1] + cell.1 as f64 * self.s.grid,
            ];
            if !self.s.inside_xy(xy) {
                if self.s.mode == Mode::FreeSurface {
                    self.censored_grid.insert(cell);
                    continue;
                }
                return Err(Progress::Invalid("The connected top grid reaches the plate envelope without an enclosing measured miss boundary. Partial contacts are retained; the footprint is not declared complete.".into()));
            }
            last = cell;
            let hit = self.top(Phase::Grid, xy, false)?;
            grid.insert(cell, hit);
            if hit {
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        let next = (cell.0.checked_add(dx).ok_or_else(|| Progress::Invalid("Grid X index exhausted integer storage; retain this run and choose a larger grid spacing before a new Run.".into()))?,
                            cell.1.checked_add(dy).ok_or_else(|| Progress::Invalid("Grid Y index exhausted integer storage; retain this run and choose a larger grid spacing before a new Run.".into()))?);
                        if !grid.contains_key(&next) && !self.censored_grid.contains(&next) {
                            pending.insert(next);
                        }
                    }
                }
            }
        }
        // Refine each measured hit/miss crossing, including rotated boundaries.
        for (&(x, y), &hit) in &grid {
            if !hit {
                continue;
            }
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                if grid.get(&(x + dx, y + dy)) == Some(&false) {
                    let p = [
                        seed[0] + x as f64 * self.s.grid,
                        seed[1] + y as f64 * self.s.grid,
                    ];
                    let q = if self.s.mode == Mode::FreeSurface {
                        // Reuse exactly the lattice location whose source
                        // record supplies this endpoint. Keep old replay's
                        // arithmetic unchanged for historical modes.
                        [
                            seed[0] + (x + dx) as f64 * self.s.grid,
                            seed[1] + (y + dy) as f64 * self.s.grid,
                        ]
                    } else {
                        [
                            p[0] + dx as f64 * self.s.grid,
                            p[1] + dy as f64 * self.s.grid,
                        ]
                    };
                    self.bisect(Phase::Boundary, p, q)?;
                }
            }
        }
        Ok(grid)
    }

    pub fn run(&mut self) -> Result<Rectangle, Progress> {
        let seed = [self.s.origin[0], self.s.origin[1]];
        if !self.top(Phase::Reference, seed, true)? {
            return Err(Progress::Invalid("No starting top contact within the selected downward budget. Return above the stock in Pendant Mode and start a new Run.".into()));
        }
        // Locate four brackets before sweeping. A contact at a plate endpoint
        // is censored data, never an invented outside miss or block dimension.
        for axis in 0..2 {
            for sign in [-1.0, 1.0] {
                let mut outside = seed;
                outside[axis] = if sign < 0.0 {
                    self.s.min[axis]
                } else {
                    self.s.max[axis]
                };
                if self.top(Phase::Boundary, outside, false)? {
                    return Err(Progress::Invalid("A boundary search still contacts at the plate envelope. The stock edge is unbounded at this depth; no plate-limit coordinate is accepted as its edge. Adjust the descent budget or stock position in Pendant Mode before a new Run.".into()));
                }
                self.bisect(Phase::Boundary, seed, outside)?;
            }
        }

        self.discover_grid()?;
        let rect = Rectangle::fit(&self.hits, &self.misses, self.s.grid)?;
        // Guess-and-check: fresh inside/outside touches at each fitted face's
        // midpoint, followed by an independent binary crossing measurement.
        for axis in 0..2 {
            for sign in [-1.0, 1.0] {
                let basis = [rect.u, rect.v];
                let support = if sign < 0.0 {
                    rect.min[axis]
                } else {
                    rect.max[axis]
                };
                let along = (rect.min[1 - axis] + rect.max[1 - axis]) / 2.0;
                let p = [0, 1].map(|i| basis[axis][i] * support + basis[1 - axis][i] * along);
                let reach = self.s.grid * std::f64::consts::SQRT_2;
                let inside = [0, 1].map(|i| p[i] - sign * basis[axis][i] * reach);
                let outside = [0, 1].map(|i| p[i] + sign * basis[axis][i] * reach);
                if !self.top(Phase::Verify, inside, true)?
                    || self.top(Phase::Verify, outside, true)?
                {
                    return Err(Progress::Invalid("Fresh inside/outside probes disagree with the fitted cuboid footprint. The map is retained but dimensions are unconfirmed; inspect the measured hit/miss map before another Run.".into()));
                }
                self.bisect(Phase::Verify, inside, outside)?;
            }
        }
        if self.s.mode == Mode::Rim {
            let reference = self.samples[0]
                .trigger
                .ok_or("No retained reference trigger.".to_string())?[2]
                - self.s.offset[2];
            let z = reference - self.s.radius - self.s.side_depth;
            let stations = rect.stations(self.s.grid, self.s.radius, self.s.backoff)?;
            // Bound the entire circuit before any side descent is requested.
            for station in &stations {
                for xy in [station.approach, station.target] {
                    self.s.bounds([xy[0], xy[1], z])?;
                }
            }
            for station in stations {
                let request = Request {
                    phase: Phase::Rim,
                    edge: station.edge as i32,
                    approach: station.approach,
                    target: [station.target[0], station.target[1], z],
                };
                if !self.measure(request)? {
                    return Err(Progress::Invalid("A rim station had no side contact. The probe returned to starting clearance; the recorded outline is partial. Use Pendant Mode before a new Run.".into()));
                }
            }
        }
        if self.cursor != self.samples.len() {
            return Err(Progress::Invalid(
                "Unexpected extra samples after the planned scan.".into(),
            ));
        }
        Ok(rect)
    }
}
