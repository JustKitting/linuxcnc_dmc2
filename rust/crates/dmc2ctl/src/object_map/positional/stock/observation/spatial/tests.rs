//! Analytical planning cases, never evidence of stock or physical execution.
use super::{
    grid::{Budget, Grid},
    request::Request,
};
use crate::probe_data::{
    mapper_settings::{Mode, Settings},
    mapper_trace::observation::TopColumn,
};

fn settings() -> Settings {
    Settings {
        mode: Mode::FreeSurface,
        origin: [25., 28., 48.],
        offset: [10., -3., 2.],
        min: [24., 27., 1.],
        max: [26., 29., 100.],
        grid: 1.,
        resolution: 0.1,
        floor: 45.,
        reach_floor: 26.,
        radius: 1.,
        side_depth: 0.,
        backoff: 0.,
        feeds: [800., 50., 1500.],
        downward_feed: 400.,
        outline: None,
        step: [0.001; 3],
    }
}
#[test]
fn new_column_keeps_source_floor_feeds_and_clearance() {
    let s = settings();
    let p = TopColumn::new(&s, [24.25, 28.25]).unwrap();
    assert_eq!(p.request.target, [24.25, 28.25, 45.]);
    assert_eq!(p.start, s.origin);
    let values = p.request.values(&s, 0, 0);
    assert!(values.contains(&("coarse-feed", 400.)));
    assert!(values.contains(&("fine-feed", 50.)));
    assert!(TopColumn::new(&s, [23.99, 28.]).is_err());
    let mut side = s;
    side.mode = Mode::Outline;
    assert!(TopColumn::new(&side, [25., 28.]).is_err());
}
#[test]
fn projected_disk_keeps_only_intersecting_cells_and_censors_outside() {
    let g = Grid::new(&settings(), 0.5).unwrap();
    assert_eq!(
        g.range([24.25, 27.25], 0.1).unwrap(),
        Some(([-2, -2], [-2, -2]))
    );
    assert_eq!(g.xy([-2, -2]).unwrap(), [24.25, 27.25]);
    assert!(!g.intersects([-1, -1], [24.25, 27.25], 0.1).unwrap());
    assert!(g.range([30., 30.], 0.1).unwrap().is_none());
}
#[test]
fn explicit_budgets_and_representability_fail_without_truncation() {
    let text = "DMC2_SPATIAL_OBSERVATION_REQUEST_V1\nmaterial_analysis=material\nsource_capture=top\nsample_spacing_mm=0.5\nmax_observations=4\nmax_grid_cells=16\nmax_candidate_comparisons=100\n\n";
    assert!(Request::read(text.as_bytes()).is_ok());
    assert!(Request::read(text.replace("spacing_mm=0.5", "spacing_mm=NaN").as_bytes()).is_err());
    assert!(Request::read(text.replace("cells=16", "cells=0").as_bytes()).is_err());
    assert!(Grid::new(&settings(), 0.0001).is_err());
    let mut b = Budget::new(2);
    b.take(Some(2)).unwrap();
    assert!(b.take(Some(1)).is_err());
    assert!(Budget::new(usize::MAX).take(None).is_err());
}
