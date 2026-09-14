//! Numerical fixtures only. These do not establish probe or machine accuracy.
use super::*;
const STL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../examples/positional-mapper/reference.stl"
));
const LEDGER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../examples/positional-mapper/synthetic-ledger.txt"
));
const REQUEST: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../examples/positional-mapper/fit-request.txt"
));
fn samples(req: &Request) -> Vec<fit::Sample> {
    let capture = super::super::capture::Capture::read(LEDGER).unwrap();
    req.selected
        .iter()
        .map(|selected| {
            req.probe
                .measure(&selected.capture, &capture, selected)
                .unwrap()
        })
        .collect()
}
#[test]
fn local_pose_recovers_known_fixture_without_stock_rows_pulling_the_fit() {
    let mesh = mesh::Mesh::read(STL, 1.).unwrap();
    let req = Request::read(REQUEST).unwrap();
    let s = samples(&req);
    let f = fit::run(&mesh, &s, &req).unwrap();
    assert!(f.outcome == fit::Outcome::Converged);
    assert!(norm(sub(f.model_to_machine.t, [70., 60., 40.])) < req.convergence);
    let expected = Pose::from_euler([0., 0., 7.], [70., 60., 40.]);
    assert!(
        norm(sub(
            f.model_to_machine.point([40., 30., 20.]),
            expected.point([40., 30., 20.])
        )) < req.convergence
    );
    let report = report::build(&mesh, &s, &f, &req).unwrap();
    assert!(report.json.contains("\"reconstructed_stock\":null"));
    assert!(report.json.contains("\"contains_partial_capture\":true"));
    // Independent check contacts are not part of the fitting objective.
    for (s, o) in s.iter().zip(&f.observations) {
        if s.usage == request::Use::Check {
            assert!(o.residual.abs() < req.convergence);
        }
    }
}
#[test]
fn three_dimensional_pose_and_inverse_keep_frame_direction() {
    let p = Pose::from_euler([15., -12., 8.], [70., 60., 40.]);
    let q = [11., 13., 17.];
    assert!(norm(sub(p.inverse().point(p.point(q)), q)) < 64. * f64::EPSILON * norm(q));
    let mesh = mesh::Mesh::read(STL, 1.).unwrap();
    let mut req = Request::read(REQUEST).unwrap();
    let mut s = samples(&req);
    let old = Pose::from_euler([0., 0., 7.], [70., 60., 40.]);
    for x in &mut s {
        x.center = p.point(old.inverse().point(x.center));
        x.approach = mv(p.r, mv(old.inverse().r, x.approach));
    }
    req.planar = false;
    req.initial = Pose::from_euler([14.7, -11.8, 7.7], [70.1, 59.8, 40.2]);
    let f = fit::run(&mesh, &s, &req).unwrap();
    assert!(f.outcome == fit::Outcome::Converged);
    assert!(norm(sub(f.model_to_machine.point(q), p.point(q))) < req.convergence);
}
#[test]
fn missing_constraints_bad_radius_and_duplicate_validation_are_errors() {
    let mesh = mesh::Mesh::read(STL, 1.).unwrap();
    let req = Request::read(REQUEST).unwrap();
    let mut s = samples(&req);
    s.retain(|s| s.sequence >= 7 && s.sequence <= 9);
    assert!(fit::run(&mesh, &s, &req).is_err());
    let raw = std::str::from_utf8(REQUEST).unwrap();
    assert!(Request::read(
        raw.replace("ball_radius_mm=1", "ball_radius_mm=0")
            .as_bytes()
    )
    .is_err());
    assert!(Request::read(format!("{raw}synthetic,1,check\n").as_bytes()).is_err());
    assert!(Request::read(raw.replace("huber_mm=0.1", "huber_mm=REQUIRED").as_bytes()).is_err());
}
#[test]
fn unmatched_contacts_are_not_silently_removed() {
    let mesh = mesh::Mesh::read(STL, 1.).unwrap();
    let req = Request::read(REQUEST).unwrap();
    let mut s = samples(&req);
    s[0].center[0] += 100.;
    assert!(fit::run(&mesh, &s, &req)
        .unwrap_err_text()
        .contains("not silently excluded"));
}
#[test]
fn search_boundary_does_not_become_a_convergence_claim() {
    let mesh = mesh::Mesh::read(STL, 1.).unwrap();
    let mut req = Request::read(REQUEST).unwrap();
    let samples = samples(&req);
    // The example seed needs a larger correction than this deliberately
    // insufficient numerical search bound. It must remain an unresolved fit.
    req.max_translation = req.convergence;
    let fitted = fit::run(&mesh, &samples, &req).unwrap();
    assert!(fitted.outcome != fit::Outcome::Converged);
}
trait ErrorText {
    fn unwrap_err_text(self) -> String;
}
impl<T> ErrorText for Result<T, Error> {
    fn unwrap_err_text(self) -> String {
        match self {
            Ok(_) => panic!("expected rejection"),
            Err(e) => e.to_string(),
        }
    }
}
#[test]
fn binary_solid_header_and_open_mesh_have_distinct_meanings() {
    let mut bytes = vec![0; 84];
    bytes[..5].copy_from_slice(b"solid");
    bytes[80..84].copy_from_slice(&1_u32.to_le_bytes());
    for x in [0_f32, 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 0.] {
        bytes.extend(x.to_le_bytes());
    }
    bytes.extend([0, 0]);
    let m = mesh::Mesh::read(&bytes, 1.).unwrap();
    assert_eq!(m.boundary_edges, 3);
    assert!(m.fitting_geometry().is_err());
    assert!(mesh::Mesh::read(&bytes[..bytes.len() - 1], 1.).is_err());
    assert!(mesh::Mesh::read(STL, f64::NAN).is_err());
}
#[test]
fn pivoted_qr_reports_unobservable_planes_instead_of_damping_them() {
    let rows = vec![vec![1., 0., 1.], vec![2., 0., 2.], vec![3., 0., 3.]];
    assert!(least_squares(&rows, &[1., 2., 3.]).is_err());
    let rows = vec![vec![0., 2.], vec![1., 1.], vec![1., 0.]];
    let (x, _) = least_squares(&rows, &[6., 5., 2.]).unwrap();
    assert!((x[0] - 2.).abs() < 16. * f64::EPSILON);
    assert!((x[1] - 3.).abs() < 16. * f64::EPSILON);
}
