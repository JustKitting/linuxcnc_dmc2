//! Offline model/capture fixtures; these do not establish machine behaviour.
use super::super::{
    ledger::Fields,
    schema::{validate, Workflow},
};
use super::{
    geometry::dot,
    model::{Phase, Plan, State},
};

fn start() -> Fields {
    let mut fields = Fields::from([
        ("kind".into(), "start".into()),
        ("sequence".into(), "0".into()),
    ]);
    for &key in super::START_FIELDS {
        fields.insert(key.into(), "1".into());
    }
    for (key, value) in [
        ("x", 100.0),
        ("y", 80.0),
        ("z", 70.0),
        ("offset_x", 0.0),
        ("offset_y", 0.0),
        ("offset_z", 0.0),
        ("x_min", 0.0),
        ("x_max", 300.0),
        ("y_min", 0.0),
        ("y_max", 173.0),
        ("z_min", 0.0),
        ("z_max", 135.0),
        ("search", 25.0),
        ("clearance", 2.0),
        ("feed", 50.0),
        ("coarse_feed", 200.0),
        ("ball_diameter", 2.0),
        ("step_x", 0.001),
        ("step_y", 0.001),
        ("step_z", 0.001),
    ] {
        fields.insert(key.into(), value.to_string());
    }
    fields
}

fn event(plan: &Plan, kind: &str, stage: i32, xyz: [f64; 3], sequence: usize) -> Fields {
    let mut f = Fields::new();
    for &key in super::EVENT_FIELDS {
        f.insert(key.into(), "0".into());
    }
    for (key, value) in [
        ("sample", plan.sample as i32),
        ("phase", plan.phase as i32),
        ("column", plan.cell.0),
        ("row", plan.cell.1),
        ("edge", plan.edge),
        ("layer", plan.layer as i32),
        ("stage", stage),
        (
            "success",
            i32::from(matches!(kind, "touch" | "obstruction")),
        ),
    ] {
        f.insert(key.into(), value.to_string());
    }
    f.insert("kind".into(), kind.into());
    f.insert("sequence".into(), sequence.to_string());
    f.insert("feed".into(), "50".into());
    for (i, axis) in ["x", "y", "z"].iter().enumerate() {
        for prefix in ["work_", "machine_", "from_", "target_"] {
            f.insert(format!("{prefix}{axis}"), xyz[i].to_string());
        }
    }
    let body = format!(
        "{}\n{}",
        Workflow::Block.magic(),
        f.iter()
            .map(|(k, v)| format!("{k}={v}\n"))
            .collect::<String>()
    );
    validate(Workflow::Block, &body, sequence as u64).unwrap();
    if matches!(kind, "touch" | "obstruction") {
        for (i, axis) in ["x", "y", "z"].iter().enumerate() {
            f.insert(format!("machine_{axis}_exact"), xyz[i].to_string());
        }
    }
    f
}

fn top_scan() -> Vec<Fields> {
    let mut records = vec![start()];
    let angle: f64 = 0.37;
    let u = [angle.cos(), angle.sin()];
    let v = [-angle.sin(), angle.cos()];
    loop {
        let plan = State::read(&records).unwrap().next().unwrap();
        if plan.phase == Phase::Side {
            return records;
        }
        let p = [plan.target[0] - 100.0, plan.target[1] - 80.0];
        let hit = dot(p, u).abs() <= 12.0 && dot(p, v).abs() <= 5.0;
        let xyz = [
            plan.target[0],
            plan.target[1],
            if hit { 50.0 } else { 49.0 },
        ];
        if hit {
            records.push(event(&plan, "touch", 0, xyz, records.len()));
            records.push(event(&plan, "touch", 1, xyz, records.len()));
        } else {
            records.push(event(&plan, "miss", 0, xyz, records.len()));
        }
        records.push(event(
            &plan,
            "ready",
            -1,
            [plan.approach[0], plan.approach[1], 52.0],
            records.len(),
        ));
        assert!(records.len() < 3000);
    }
}

#[test]
fn signed_grid_closes_and_all_circuits_reuse_exact_xy_stations() {
    let mut records = top_scan();
    let mut first = Vec::new();
    loop {
        let state = State::read(&records).unwrap();
        let plan = state.next().unwrap();
        if plan.phase == Phase::Finished {
            break;
        }
        assert_eq!(plan.phase, Phase::Side);
        let station_count = state.stations().unwrap().len();
        if plan.layer == 0 {
            first.push(plan.clone());
        } else {
            let previous = &first[state.sides.len() % station_count];
            assert_eq!(plan.approach, previous.approach);
            assert_eq!(
                [plan.target[0], plan.target[1]],
                [previous.target[0], previous.target[1]]
            );
            assert_eq!(plan.target[2], previous.target[2] - plan.layer as f64);
        }
        records.push(event(&plan, "touch", 0, plan.target, records.len()));
        records.push(event(&plan, "touch", 1, plan.target, records.len()));
        records.push(event(
            &plan,
            "ready",
            -1,
            [plan.approach[0], plan.approach[1], plan.clear],
            records.len(),
        ));
    }
    let state = State::read(&records).unwrap();
    assert_eq!(state.sides.len(), first.len() * 3);
    assert!(state.grid.keys().any(|&(x, y)| x < -1 && y < -1));
}

#[test]
fn obstruction_and_recovery_are_terminal_and_missing_clearance_blocks_planning() {
    let mut records = top_scan();
    let plan = State::read(&records).unwrap().next().unwrap();
    let saved = records.clone();
    records.push(event(&plan, "obstruction", -1, plan.target, records.len()));
    records.push(event(
        &plan,
        "recovery",
        -1,
        [plan.approach[0], plan.approach[1], plan.clear],
        records.len(),
    ));
    assert!(State::read(&records)
        .unwrap()
        .next()
        .unwrap_err()
        .contains("terminal"));
    let mut incomplete = saved;
    incomplete.pop();
    assert!(State::read(&incomplete)
        .unwrap()
        .next()
        .unwrap_err()
        .contains("clearance"));
}

#[test]
fn rejects_travel_boundary_instead_of_clipping() {
    let mut records = vec![start()];
    records[0].insert("z".into(), "20".into());
    assert!(State::read(&records)
        .unwrap()
        .next()
        .unwrap_err()
        .contains("travel boundary"));
}
