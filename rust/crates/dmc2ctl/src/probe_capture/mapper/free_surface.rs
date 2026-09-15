//! Connected top acquisition without a rectangle or nominal stock shape.
use super::super::geometry::Point;
use super::{
    model::{Mode, Phase},
    search::{Progress, Survey},
};

#[derive(Debug)]
pub(super) struct Bracket {
    pub hit_sequence: usize,
    pub miss_sequence: usize,
    // Requested sampling locations, not substituted physical trigger positions.
    pub hit_xy: Point,
    pub miss_xy: Point,
    pub gap: f64,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum BoundarySide {
    Low,
    High,
}

#[derive(Debug)]
pub(super) struct BoundaryContact {
    pub axis: usize,
    pub side: BoundarySide,
    pub sequence: usize,
    pub requested_xy: Point,
}

impl Survey<'_> {
    pub(super) fn bracket(&self, hit: Point, miss: Point) -> Result<Bracket, Progress> {
        Ok(Bracket {
            hit_sequence: self.source(hit)?,
            miss_sequence: self.source(miss)?,
            hit_xy: hit,
            miss_xy: miss,
            gap: (hit[0] - miss[0]).hypot(hit[1] - miss[1]),
        })
    }

    fn free_preflight(&self) -> Result<(), Progress> {
        if self.s.mode != Mode::FreeSurface {
            return Err(Progress::Invalid("Automatic top coverage requires its retained acquisition mode. Reopen Automatic top map before a new Run; Abort and Pendant Mode remain available.".into()));
        }
        if self.s.grid / 2.0 < self.s.step[0].max(self.s.step[1]) {
            return Err(Progress::Invalid("The grid cell centre is less than one XY step from a grid line. Increase grid spacing to at least twice the larger XY step before a new Run.".into()));
        }
        // The ledger's sample index is i32. Include the neighbouring lattice
        // layer used to mark censored coverage; this is a storage bound, not a
        // machine-travel limit or a retry count.
        let cells =
            [0, 1].map(|axis| ((self.s.max[axis] - self.s.min[axis]) / self.s.grid).ceil() + 3.0);
        if cells
            .iter()
            .any(|n| !n.is_finite() || *n > f64::from(i32::MAX))
            || cells[0] * cells[1] > f64::from(i32::MAX)
        {
            return Err(Progress::Invalid("The selected grid exceeds retained sample-index capacity. Increase grid spacing before a new Run; no top-search request was issued.".into()));
        }
        Ok(())
    }

    fn free_cardinal(&mut self, axis: usize, side: BoundarySide) -> Result<(), Progress> {
        let seed = [self.s.origin[0], self.s.origin[1]];
        let (endpoint, sign) = match side {
            BoundarySide::Low => (self.s.min[axis], -1.0),
            BoundarySide::High => (self.s.max[axis], 1.0),
        };
        let extent = (endpoint - seed[axis]).abs();
        let mut distance = self.s.grid.min(extent);
        let mut hit = seed;
        loop {
            let mut p = seed;
            p[axis] = if distance == extent {
                endpoint
            } else {
                seed[axis] + sign * distance
            };
            // X low: physical RIGHT / LinuxCNC -X.
            // X high: physical LEFT / LinuxCNC +X.
            if !self.top(Phase::Boundary, p, false)? {
                self.bisect(Phase::Boundary, hit, p)?;
                return Ok(());
            }
            if distance == extent {
                self.boundary_contacts.push(BoundaryContact {
                    axis,
                    side,
                    sequence: self.source(p)?,
                    requested_xy: p,
                });
                return Ok(());
            }
            hit = p;
            let next = (distance * 2.0).min(extent);
            if next <= distance || seed[axis] + sign * next == p[axis] {
                return Err(Progress::Invalid("Growing top search exhausted representable coordinates. Preserve the partial run and increase grid spacing before a new Run.".into()));
            }
            distance = next;
        }
    }

    pub(super) fn free_surface(&mut self) -> Result<(), Progress> {
        self.free_preflight()?;
        let seed = [self.s.origin[0], self.s.origin[1]];
        if !self.top(Phase::Reference, seed, true)? {
            return Err(Progress::Invalid("No starting top contact within the selected downward budget. Return above the stock in Pendant Mode and start a new Run.".into()));
        }
        for axis in 0..2 {
            for side in [BoundarySide::Low, BoundarySide::High] {
                self.free_cardinal(axis, side)?;
            }
        }
        let grid = self.discover_grid()?;
        // A fresh cell-centre contact is withheld from fitting. A centre miss
        // exposes a gap that four corner hits alone would have concealed.
        for (&(x, y), &hit) in &grid {
            if !hit
                || [(x + 1, y), (x, y + 1), (x + 1, y + 1)]
                    .iter()
                    .any(|p| grid.get(p) != Some(&true))
            {
                continue;
            }
            let centre = [
                seed[0] + (f64::from(x) + 0.5) * self.s.grid,
                seed[1] + (f64::from(y) + 0.5) * self.s.grid,
            ];
            if !self.top(Phase::Verify, centre, true)? {
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let corner = [
                        seed[0] + f64::from(x + dx) * self.s.grid,
                        seed[1] + f64::from(y + dy) * self.s.grid,
                    ];
                    self.bisect(Phase::Boundary, corner, centre)?;
                }
            }
        }
        if self.cursor != self.samples.len() {
            return Err(Progress::Invalid("Unexpected extra samples after automatic top coverage. Preserve the ledger and begin a new Run after recovery.".into()));
        }
        Ok(())
    }
}
