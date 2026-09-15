//! Boolean probe-response fixtures only; no physical stock or motion evidence.
use super::{
    model::{Mode, Phase, Sample, Settings},
    search::{Progress, Survey},
    tests::fixture,
};

fn acquire(settings: &Settings, hit: impl Fn(f64, f64) -> bool) -> Vec<Sample> {
    let mut samples = Vec::new();
    loop {
        match Survey::new(settings, &samples).free_surface() {
            Ok(()) => return samples,
            Err(Progress::Invalid(e)) => panic!("{e}"),
            Err(Progress::Need(request)) => {
                settings.bounds(request.target).unwrap();
                let [x, y, _] = request.target;
                samples.push(Sample {
                    sequence: samples.len(),
                    request,
                    trigger: hit(x, y).then_some([
                        x + settings.offset[0],
                        y + settings.offset[1],
                        8.0 + settings.offset[2],
                    ]),
                    returned: Some([x, y, settings.origin[2]]),
                });
                // A fixture watchdog, never a production retry policy.
                assert!(
                    samples.len() < 2000,
                    "finite numerical domain did not finish"
                );
            }
        }
    }
}

#[test]
fn concave_contact_domain_is_retained_without_a_rectangle_fit() {
    let s = fixture(Mode::FreeSurface);
    let samples = acquire(&s, |x, y| {
        x.abs() <= 5.0 && y.abs() <= 5.0 && (x <= 1.0 || y <= 1.0)
    });
    let mut survey = Survey::new(&s, &samples);
    assert!(survey.free_surface().is_ok());
    assert!(survey.misses.contains(&[2.0, 2.0]));
    assert!(survey.hits.contains(&[0.0, 4.0]));
    assert!(survey.hits.contains(&[4.0, 0.0]));
    assert!(samples.iter().any(|r| r.request.phase == Phase::Verify));
    for b in &survey.brackets {
        assert!(samples[b.hit_sequence].trigger.is_some());
        assert!(samples[b.miss_sequence].trigger.is_none());
        assert!(b.gap <= s.resolution);
    }
    let first_x: Vec<_> = samples
        .iter()
        .take(4)
        .map(|r| r.request.target[0])
        .collect();
    assert_eq!(first_x, [0.0, -2.0, -4.0, -8.0]);
}

#[test]
fn independent_centre_miss_refines_an_interior_gap_hidden_by_corner_hits() {
    let s = fixture(Mode::FreeSurface);
    let samples = acquire(&s, |x, y| {
        x.abs() <= 3.0 && y.abs() <= 3.0 && (x - 1.0).hypot(y - 1.0) >= 0.4
    });
    let centre = samples
        .iter()
        .find(|r| r.request.phase == Phase::Verify && r.request.approach == [1.0, 1.0])
        .unwrap();
    assert!(centre.trigger.is_none());
    let mut survey = Survey::new(&s, &samples);
    assert!(survey.free_surface().is_ok());
    let gaps: Vec<_> = survey
        .brackets
        .iter()
        .filter(|b| (b.miss_xy[0] - 1.0).hypot(b.miss_xy[1] - 1.0) < 0.4)
        .collect();
    assert_eq!(gaps.len(), 4);
    assert!(gaps.iter().all(|b| b.gap <= s.resolution));
}

#[test]
fn plate_contact_and_outside_lattice_remain_censored_without_invented_misses() {
    let mut s = fixture(Mode::FreeSurface);
    s.min[0] = -2.0;
    s.min[1] = -2.0;
    s.max[0] = 2.0;
    s.max[1] = 2.0;
    let samples = acquire(&s, |_, _| true);
    let mut survey = Survey::new(&s, &samples);
    assert!(survey.free_surface().is_ok());
    assert_eq!(survey.boundary_contacts.len(), 4);
    assert!(!survey.censored_grid.is_empty());
    assert!(survey.misses.is_empty() && survey.brackets.is_empty());
    assert_eq!(
        samples
            .iter()
            .filter(|r| r.request.phase == Phase::Verify)
            .count(),
        4
    );
}

#[test]
fn fractional_grid_uses_the_original_endpoint_record_identity() {
    let mut s = fixture(Mode::FreeSurface);
    s.origin[0] = 0.3;
    s.origin[1] = 0.7;
    s.grid = 0.2;
    s.resolution = 0.02;
    let samples = acquire(&s, |x, y| {
        (x - 0.3).abs() <= 0.55 && (y - 0.7).abs() <= 0.55
    });
    let mut survey = Survey::new(&s, &samples);
    assert!(survey.free_surface().is_ok());
    assert!(!survey.brackets.is_empty());
}

#[test]
fn unresolvable_or_unrepresentable_grid_fails_before_a_request() {
    let mut s = fixture(Mode::FreeSurface);
    s.grid = s.step[0];
    assert!(matches!(
        Survey::new(&s, &[]).free_surface(),
        Err(Progress::Invalid(_))
    ));
    s.grid = 0.002;
    s.max[0] = f64::from(i32::MAX);
    assert!(matches!(
        Survey::new(&s, &[]).free_surface(),
        Err(Progress::Invalid(_))
    ));
}
