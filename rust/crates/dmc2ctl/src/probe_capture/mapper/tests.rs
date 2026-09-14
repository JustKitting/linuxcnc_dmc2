//! Pure geometry/record checks. These fixtures provide no machine evidence.
use super::{
    model::{Mode, Phase, Sample, Settings},
    search::{Progress, Survey},
};

fn fixture(mode: Mode) -> Settings {
    Settings {
        mode,
        origin: [0.0, 0.0, 10.0],
        offset: [20.0, 30.0, 40.0],
        min: [-30.0, -30.0, 0.0],
        max: [30.0, 30.0, 20.0],
        grid: 2.0,
        resolution: 0.1,
        floor: 1.0,
        radius: 1.0,
        side_depth: 1.0,
        backoff: 2.0,
        feeds: [800.0, 50.0, 1500.0],
        step: [0.001; 3],
    }
}

#[test]
fn rotated_stock_replay_reaches_face_checks_and_rim_inside_original_envelope() {
    let settings = fixture(Mode::Rim);
    let mut samples = Vec::new();
    let mut verified = false;
    let mut rim = false;
    loop {
        let next = Survey::new(&settings, &samples).run();
        match next {
            Err(Progress::Need(request)) => {
                settings.bounds(request.target).unwrap();
                verified |= request.phase == Phase::Verify;
                rim |= request.phase == Phase::Rim;
                let angle: f64 = 0.31;
                let [x, y, _] = request.target;
                let u = x * angle.cos() + y * angle.sin();
                let v = -x * angle.sin() + y * angle.cos();
                let hit = request.phase == Phase::Rim || (u.abs() <= 11.0 && v.abs() <= 7.0);
                let trigger = hit.then_some([
                    x + settings.offset[0],
                    y + settings.offset[1],
                    8.0 + settings.offset[2],
                ]);
                samples.push(Sample { request, trigger });
                assert!(samples.len() < 2000, "finite fixture did not terminate");
            }
            Err(Progress::Invalid(e)) => panic!("{e}"),
            Ok(rect) => {
                assert!(verified && rim);
                assert!((rect.max[0] - rect.min[0] - 22.0).abs() < 0.3);
                assert!((rect.max[1] - rect.min[1] - 14.0).abs() < 0.3);
                break;
            }
        }
    }
    samples[0].request.phase = Phase::Grid;
    assert!(matches!(
        Survey::new(&settings, &samples).run(),
        Err(Progress::Invalid(_))
    ));
}

#[test]
fn contact_at_plate_boundary_never_becomes_a_measured_stock_edge() {
    let settings = fixture(Mode::Surface);
    let mut samples = Vec::new();
    for _ in 0..2 {
        let Err(Progress::Need(request)) = Survey::new(&settings, &samples).run() else {
            panic!("expected bounded request");
        };
        samples.push(Sample {
            request,
            trigger: Some([20.0, 30.0, 48.0]),
        });
    }
    assert!(matches!(
        Survey::new(&settings, &samples).run(),
        Err(Progress::Invalid(_))
    ));
}

#[test]
fn side_depth_cannot_extend_the_original_descent_budget() {
    let settings = fixture(Mode::Rim);
    assert!(settings.bounds([0.0, 0.0, settings.floor - 0.001]).is_err());
    assert!(settings
        .bounds([0.0, 0.0, settings.origin[2] + 0.001])
        .is_err());
    assert!(settings
        .bounds([settings.max[0] + 0.001, 0.0, settings.floor])
        .is_err());
}
