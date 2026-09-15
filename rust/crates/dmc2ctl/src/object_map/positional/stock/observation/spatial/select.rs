use super::super::material::{self, query::State};
use super::{
    Source,
    grid::{Budget, Grid, Key},
    request::Request,
};
use crate::{
    object_map::{Error, positional::geometry::finite},
    probe_data::mapper_trace::observation::TopColumn,
};
use std::collections::{BTreeMap, BTreeSet};

pub struct Cell {
    pub key: Key,
    pub column: TopColumn,
    pub regions: BTreeSet<usize>,
    pub original_sequences: Vec<usize>,
    pub nearest_request_mm: f64,
}
pub struct Projection {
    pub center: [f64; 3],
    pub work_xy: [f64; 2],
    pub envelope_censored: bool,
    pub cells: Vec<usize>,
}
pub struct Selection {
    pub grid: Grid,
    pub cells: Vec<Cell>,
    pub chosen: Vec<usize>,
    pub projections: BTreeMap<usize, Projection>,
    pub comparisons: usize,
}

pub fn run(a: &material::Assessment, source: &Source, r: &Request) -> Result<Selection, Error> {
    let s = &source.settings;
    let grid = Grid::new(s, r.spacing)?;
    let mut budget = Budget::new(r.comparisons);
    let mut grouped: BTreeMap<Key, BTreeSet<usize>> = BTreeMap::new();
    let mut projections = BTreeMap::new();
    for (i, (cover, region)) in a.covers.iter().zip(&a.regions).enumerate() {
        if region.state != State::Unsupported {
            continue;
        }
        let center = a.candidate.pose.point(cover.center);
        // For a downward approach the pretravel term has no XY component.
        // Required geometry supplies only the XY region, never a contact Z.
        let work_xy = std::array::from_fn(|axis| {
            center[axis] - a.surface.request.probe.mount[axis] - s.offset[axis]
        });
        if !finite(center) || !work_xy.iter().all(|v| v.is_finite()) {
            return Err(Error::Data("The material region cannot be translated into the retained acquisition frame. Inspect the candidate transform, mounting vector and original work offset before retrying.".into()));
        }
        let envelope_censored = (0..2).any(|axis| {
            work_xy[axis] - cover.radius < grid.min[axis]
                || work_xy[axis] + cover.radius > grid.max[axis]
        });
        projections.insert(
            i,
            Projection {
                center,
                work_xy,
                envelope_censored,
                cells: Vec::new(),
            },
        );
        if let Some((lo, hi)) = grid.range(work_xy, cover.radius)? {
            let count = (0..2).try_fold(1usize, |n, axis| {
                hi[axis]
                    .checked_sub(lo[axis])
                    .and_then(|v| v.checked_add(1))
                    .and_then(|v| usize::try_from(v).ok())
                    .and_then(|v| n.checked_mul(v))
            });
            budget.take(count)?;
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    let key = [x, y];
                    if !grid.intersects(key, work_xy, cover.radius)? {
                        continue;
                    }
                    if !grouped.contains_key(&key) && grouped.len() >= r.cells {
                        return Err(Error::Input("max_grid_cells cannot retain all projected spatial cells. Increase this computation/storage budget or choose another sampling spacing; the analysis was not published with truncated regions.".into()));
                    }
                    grouped.entry(key).or_default().insert(i);
                }
            }
        }
    }
    budget.take(grouped.len().checked_mul(source.samples.len()))?;
    let mut cells = Vec::with_capacity(grouped.len());
    for (key, regions) in grouped {
        let xy = grid.xy(key)?;
        let column = TopColumn::new(s, xy).map_err(Error::Data)?;
        let mut original_sequences = Vec::new();
        let mut nearest_request_mm = f64::INFINITY;
        for sample in &source.samples {
            let old = sample.request.approach;
            let distance = (xy[0] - old[0]).hypot(xy[1] - old[1]);
            if !distance.is_finite() {
                return Err(Error::Data("Spatial novelty distance overflowed. Inspect the retained coordinate scale and sampling spacing before retrying.".into()));
            }
            nearest_request_mm = nearest_request_mm.min(distance);
            if s.endpoint_matches([xy[0], xy[1], s.floor], [old[0], old[1], s.floor]) {
                original_sequences.push(sample.sequence);
            }
        }
        let id = cells.len();
        for region in &regions {
            let p = projections.get_mut(region).ok_or_else(|| Error::Data("A spatial cell lost its source region during selection. Preserve the material analysis and retry under a new analysis ID; no incomplete plan was published.".into()))?;
            p.cells.push(id);
        }
        cells.push(Cell {
            key,
            column,
            regions,
            original_sequences,
            nearest_request_mm,
        });
    }
    let mut chosen = cells
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.original_sequences.is_empty().then_some(i))
        .collect::<Vec<_>>();
    // Prefer filling near previously searched columns, independently of how
    // many STL triangles project onto a cell. This is priority, not travel order.
    chosen.sort_by(|&i, &j| {
        cells[i]
            .nearest_request_mm
            .total_cmp(&cells[j].nearest_request_mm)
            .then(cells[i].key.cmp(&cells[j].key))
    });
    chosen.truncate(r.observations);
    Ok(Selection {
        grid,
        cells,
        chosen,
        projections,
        comparisons: budget.used,
    })
}
