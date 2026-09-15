//! Numerical protocol examples only; these do not establish machine behavior.
use super::*;
fn plan() -> Plan {
    let fields = [
        ("mode", 3.0),
        ("x", 25.0),
        ("y", 28.0),
        ("z", 48.0),
        ("offset_x", 0.0),
        ("offset_y", 0.0),
        ("offset_z", 0.0),
        ("x_min", 0.0),
        ("x_max", 300.0),
        ("y_min", 0.0),
        ("y_max", 170.0),
        ("z_min", 0.0),
        ("z_max", 135.0),
        ("drop", 3.0),
        ("grid", 1.0),
        ("resolution", 0.1),
        ("usable_reach", 24.0),
        ("reach_reserve", 2.0),
        ("side_depth", 0.0),
        ("backoff", 0.0),
        ("step_x", 0.001),
        ("step_y", 0.001),
        ("step_z", 0.001),
        ("max_feed", 1800.0),
    ];
    Plan {
        role: None,
        source: ["example","setup","analysis","capture"].map(String::from),
        start: fields.into_iter().map(|(k,v)|(k.into(),v.to_string())).collect(),
        plate: "DMC2_PLATE_ENVELOPE_V1\nx_min=23\nx_max=27\ny_min=26\ny_max=30\nball_diameter=2\n".into(),
        feeds: "DMC2_MAPPER_FEEDS_V2\ncoarse_feed=800\ndownward_feed=400\nfine_feed=50\ntravel_feed=1500\n".into(),
        rows: Rows::Columns(vec![[24.5,28.5]]),
    }
}
fn start(p: &Plan) -> Fields {
    let mut s = p.start.clone();
    s.insert("mode".into(), (Mode::TopFollowup as u8).to_string());
    s.insert("kind".into(), "start".into());
    s.insert("sequence".into(), "0".into());
    s
}
fn missed(p: &Plan) -> Vec<Fields> {
    let r = p.requests().unwrap()[0];
    let mut miss: Fields = [
        ("sample", 0.0),
        ("sequence", 1.0),
        ("phase", 2.0),
        ("stage", 0.0),
        ("edge", -1.0),
        ("approach_x", r.approach[0]),
        ("approach_y", r.approach[1]),
        ("target_x", r.target[0]),
        ("target_y", r.target[1]),
        ("target_z", r.target[2]),
        ("from_x", r.approach[0]),
        ("from_y", r.approach[1]),
        ("from_z", 48.0),
        ("work_x", r.target[0]),
        ("work_y", r.target[1]),
        ("work_z", r.target[2]),
        ("feed", 400.0),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.to_string()))
    .collect();
    miss.insert("kind".into(), "miss".into());
    let mut ready = miss.clone();
    ready.insert("kind".into(), "ready".into());
    ready.insert("sequence".into(), "2".into());
    ready.insert("work_z".into(), "48".into());
    vec![start(p), miss, ready]
}
#[test]
fn program_binds_plan_bytes_and_motion_body() {
    let p = plan();
    let raw = p.encode().unwrap();
    let text = p.program().unwrap();
    assert_eq!(Plan::read(&raw).unwrap().encode().unwrap(), raw);
    assert_eq!(
        Plan::from_program(&text).unwrap().rows.encode(),
        p.rows.encode()
    );
    assert!(Plan::from_program(&text.replace("M2\n%", "G0 Z0\nM2\n%")).is_err());
    assert!(Plan::from_program(&text.replace("[4]", "[3]")).is_err());
    assert!(Plan::read(&raw.replace("24.5,28.5", "NaN,28.5")).is_err());
    assert!(Plan::read(&raw.replace("24.5,28.5", "300,28.5")).is_err());
}
#[test]
fn changed_start_is_rejected_before_a_target() {
    let p = plan();
    let s = start(&p);
    assert!(p.samples(&[s.clone()], false).unwrap().is_empty());
    for &key in START_FIELDS.iter().filter(|&&k| k != "mode") {
        let mut changed = s.clone();
        changed.insert(key.into(), (number(&s, key).unwrap() + 1.0).to_string());
        assert!(p.samples(&[changed], false).is_err(), "{key}");
    }
}
#[test]
fn complete_miss_is_not_a_trigger_and_bad_rows_cannot_supply_evidence() {
    let p = plan();
    let records = missed(&p);
    let samples = p.samples(&records, false).unwrap();
    assert_eq!(samples.len(), 1);
    assert!(samples[0].trigger.is_none());
    assert!(p.samples(&records[..2], false).is_err());
    assert!(p.samples(&records, true).is_err());
    for field in ["sample", "target_x", "from_z", "feed"] {
        let mut changed = records.clone();
        changed[1].insert(field.into(), "99".into());
        assert!(p.samples(&changed, false).is_err(), "{field}");
    }
    let mut end = records.last().unwrap().clone();
    end.insert("kind".into(), "result".into());
    end.insert("sequence".into(), "3".into());
    end.insert("sample".into(), "1".into());
    let mut ended = records;
    ended.push(end.clone());
    assert_eq!(p.samples(&ended, true).unwrap().len(), 1);
    end.insert("sample".into(), "0".into());
    assert!(p.samples(&[start(&p), end], true).is_err());
}
