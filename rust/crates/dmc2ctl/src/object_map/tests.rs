//! Offline file/serialization cases only; no machine behavior is established.
use super::{capture::Capture, cli, model::CaptureState, record};
use std::{
    ffi::OsString,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dmc2-object-map-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> Result<String, super::Error> {
        cli::run(
            &args.iter().map(OsString::from).collect::<Vec<_>>(),
            &self.0.join("store"),
        )
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn retained() -> String {
    // Small synthetic coordinates exercise exact f64 retention, not calibration.
    let mut text = String::from("DMC2_CIRCLE_LEDGER_V2\nunits=mm,mm/min\nBEGIN 0\nDMC2_CIRCLE_RECORD_V2\nsequence=0\nkind=start\nx=0\ny=0\nz=0\noffset_x=0\noffset_y=0\noffset_z=0\nfeed=50\ncoarse_feed=200\nsearch=25\nball_diameter=2\nstep_x=0.001\nstep_y=0.001\nEND 0\n");
    for (sequence, stage) in [(1, 0), (2, 1)] {
        text.push_str(&format!("BEGIN {sequence}\nDMC2_CIRCLE_RECORD_V2\nsequence={sequence}\nkind=touch\npass=1\nstage={stage}\naxis=1\ndirection=-1\nsuccess=1\nwork_x=1\nwork_y=2\nwork_z=3\nmachine_x=1\nmachine_y=2\nmachine_z=3\nfeed=50\nexact_source=emcStatus.motion.traj.probedPosition;machine-mm\n"));
        for (axis, value) in [("x", 1.0_f64), ("y", 2.0), ("z", 3.0)] {
            text.push_str(&format!(
                "machine_{axis}_exact={value}\nmachine_{axis}_f64_bits={:016x}\n",
                value.to_bits()
            ));
        }
        text.push_str(&format!("END {sequence}\n"));
    }
    text
}

#[test]
fn rejects_bad_original_bits_torn_records_and_trailing_garbage() {
    let raw = retained();
    let capture = Capture::read(&raw).unwrap();
    assert_eq!(capture.state, CaptureState::Partial);
    assert_eq!(capture.contacts[1].trigger_mm, [1.0, 2.0, 3.0]);
    assert!(Capture::read(&raw.replace(
        "machine_x_f64_bits=3ff0000000000000",
        "machine_x_f64_bits=0000000000000000"
    ))
    .is_err());
    assert!(Capture::read(raw.trim_end()).is_err());
    assert!(Capture::read(&(raw + "unexpected trailer\n")).is_err());
}

#[test]
fn imported_snapshot_survives_source_change_and_setups_remain_unregistered() {
    let scratch = Scratch::new();
    scratch
        .run(&["create", "part", "A quoted \"part\""])
        .unwrap();
    scratch
        .run(&["add-setup", "part", "first", "Initial placement"])
        .unwrap();
    scratch
        .run(&["add-setup", "part", "flipped", "Later placement"])
        .unwrap();
    let source = scratch.0.join("input.txt");
    fs::write(&source, retained()).unwrap();
    scratch
        .run(&[
            "import-capture",
            "part",
            "first",
            "scan",
            source.to_str().unwrap(),
        ])
        .unwrap();
    fs::write(&source, "changed source").unwrap();
    let show = scratch.run(&["show", "part"]).unwrap();
    assert!(show.contains("A quoted \\\"part\\\""));
    assert_eq!(show.matches("\"object_to_machine\":null").count(), 2);
    assert!(show.contains("\"fine_contacts\":1"));
    assert!(show.contains("\"cam_ready\":false"));
    let export = scratch.0.join("export");
    scratch
        .run(&["export-freecad", "part", "first", export.to_str().unwrap()])
        .unwrap();
    assert_eq!(
        fs::read_to_string(export.join("scan.ledger.txt")).unwrap(),
        retained()
    );
    assert_eq!(
        fs::read_to_string(export.join("scan.trigger-envelope.asc")).unwrap(),
        "1 2 3\n"
    );
    assert!(fs::read_to_string(export.join("manifest.json"))
        .unwrap()
        .contains("\"cloud_record_sequences\":[2]"));
    assert!(scratch
        .run(&["export-freecad", "part", "first", export.to_str().unwrap()])
        .is_err());
}

#[test]
fn conflicting_names_and_ids_do_not_overwrite_retained_objects() {
    let scratch = Scratch::new();
    scratch.run(&["create", "part", "Original"]).unwrap();
    assert!(scratch.run(&["create", "part", "Replacement"]).is_err());
    assert!(scratch.run(&["create", "../escape", "Invalid"]).is_err());
    assert!(scratch.run(&["show", "part"]).unwrap().contains("Original"));
    assert!(scratch
        .run(&["add-setup", "missing", "first", "Missing object"])
        .is_err());
}

#[test]
fn design_attachment_preserves_binary_payload_and_does_not_claim_inspection() {
    let scratch = Scratch::new();
    scratch.run(&["create", "part", "Part"]).unwrap();
    scratch
        .run(&["add-setup", "part", "first", "First"])
        .unwrap();
    let path = scratch.0.join("example.FCStd");
    // Intentionally not a CAD model: attachment does not claim geometry parsing.
    let bytes = b"PK\0\xff\n\nbinary payload";
    fs::write(&path, bytes).unwrap();
    let show = scratch
        .run(&["attach-design", "part", "original", path.to_str().unwrap()])
        .unwrap();
    assert!(show.contains("\"geometry_inspected\":false"));
    let export = scratch.0.join("export");
    scratch
        .run(&["export-freecad", "part", "first", export.to_str().unwrap()])
        .unwrap();
    assert_eq!(
        fs::read(export.join("design-original.FCStd")).unwrap(),
        bytes
    );
}

#[test]
fn metadata_roundtrips_binary_and_rejects_duplicate_or_newline_fields() {
    let encoded = record::encode("EXAMPLE", &[("label", "a=b")], b"\0\xff\n\n").unwrap();
    let (fields, payload) = record::decode(&encoded, "EXAMPLE", &["label"]).unwrap();
    assert_eq!(fields["label"], "a=b");
    assert_eq!(payload, b"\0\xff\n\n");
    assert!(record::encode("EXAMPLE", &[("label", "a\nb")], &[]).is_err());
    assert!(record::decode(b"EXAMPLE\nlabel=a\nlabel=b\n\n", "EXAMPLE", &["label"]).is_err());
}
