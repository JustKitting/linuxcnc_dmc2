use super::model::{Calibration, ToolOffset};

fn calibration() -> Calibration {
    Calibration {
        height_mm: 64.0,
        home_z_mm: 135.0,
    }
}

fn touch(sequence: u64, stage: u8, z: f64) -> String {
    format!("BEGIN {sequence}\nDMC2_TOOL_SETTER_RECORD_V1\nsequence={sequence}\nkind=touch\nstage={stage}\naxis=2\ndirection=-1\nsuccess=1\nwork_x=1\nwork_y=2\nwork_z={z}\nmachine_x=1\nmachine_y=2\nmachine_z={z}\nfeed=50\nexact_source=emcStatus.motion.traj.probedPosition;machine-mm\nmachine_x_exact=1\nmachine_x_f64_bits={:016x}\nmachine_y_exact=2\nmachine_y_f64_bits={:016x}\nmachine_z_exact={z}\nmachine_z_f64_bits={:016x}\nEND {sequence}\n", 1.0_f64.to_bits(), 2.0_f64.to_bits(), z.to_bits())
}

#[test]
fn offset_uses_fine_trigger_and_height_once_with_correct_sign() {
    let text = format!(
        "DMC2_TOOL_SETTER_LEDGER_V1\n{}{}",
        touch(0, 0, 85.3),
        touch(1, 1, 85.32)
    );
    let result = ToolOffset::from_ledger(&text, calibration()).unwrap();
    assert_eq!(result.offset_z_mm, 85.32 - 64.0);
    // At the saved setter trigger, applying the offset reports setter height.
    assert_eq!(result.trigger_mm[2] - result.offset_z_mm, 64.0);
    assert_eq!(result.tip_height_at_home_mm, 135.0 - result.offset_z_mm);
}

#[test]
fn missing_fine_duplicate_and_corrupt_trigger_bits_are_rejected() {
    let header = "DMC2_TOOL_SETTER_LEDGER_V1\n";
    assert!(
        ToolOffset::from_ledger(&format!("{header}{}", touch(0, 0, 85.0)), calibration()).is_err()
    );
    assert!(ToolOffset::from_ledger(
        &format!("{header}{}{}", touch(0, 1, 85.0), touch(1, 1, 85.0)),
        calibration()
    )
    .is_err());
    let corrupt = format!("{header}{}", touch(0, 1, 85.0))
        .replace("machine_z_exact=85", "machine_z_exact=86");
    assert!(ToolOffset::from_ledger(&corrupt, calibration()).is_err());
}

#[test]
fn calibration_has_one_runtime_source_and_rejects_ambiguous_values() {
    let ini = "[TOOL_SETTER]\nHEIGHT_ABOVE_PLATE_MM=64\n[JOINT_2]\nHOME=135\n";
    assert_eq!(Calibration::read(ini).unwrap().height_mm, 64.0);
    assert_eq!(
        Calibration::read(&ini.replace("=64", "=63"))
            .unwrap()
            .height_mm,
        63.0
    );
    assert!(Calibration::read(&ini.replace("=64", "=NaN")).is_err());
    assert!(Calibration::read(&ini.replace("=64", "=64\nHEIGHT_ABOVE_PLATE_MM=63")).is_err());
}
