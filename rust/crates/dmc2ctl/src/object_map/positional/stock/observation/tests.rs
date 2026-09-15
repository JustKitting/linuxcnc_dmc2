//! Numerical selection cases only; no machine or measurement evidence.
use super::{request::Request, select::choose};
use std::collections::BTreeSet;
fn links(rows: &[&[usize]]) -> Vec<BTreeSet<usize>> {
    rows.iter().map(|r| r.iter().copied().collect()).collect()
}
#[test]
fn chooses_shared_check_before_redundant_single_patch_repeats() {
    let links = links(&[&[0], &[1], &[0, 1], &[2], &[2]]);
    assert_eq!(choose(&links, 4, 5), (vec![2, 3], vec![3]));
}
#[test]
fn budget_preserves_unplanned_requirements_and_no_eligible_path_is_not_completion() {
    let links = links(&[&[0, 1], &[1, 2], &[]]);
    assert_eq!(choose(&links, 4, 1), (vec![0], vec![2, 3]));
    assert_eq!(choose(&links[2..], 4, 4), (vec![], vec![0, 1, 2, 3]));
}
#[test]
fn request_requires_explicit_computation_and_observation_budgets() {
    let request = "DMC2_OBSERVATION_REQUEST_V1\nmaterial_analysis=material\nmax_observations=1\nmax_candidate_need_comparisons=10\n\n";
    assert!(Request::read(request.as_bytes()).is_ok());
    assert!(
        Request::read(
            request
                .replace("max_observations=1", "max_observations=REQUIRED")
                .as_bytes()
        )
        .is_err()
    );
    assert!(
        Request::read(
            request
                .replace("comparisons=10", "comparisons=0")
                .as_bytes()
        )
        .is_err()
    );
}
