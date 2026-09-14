//! Plate-referenced tool Z geometry from retained normal-contact triggers.
use crate::ledger::{number, records};
use crate::schema::Workflow;

pub(super) use dmc2ctl::calibration::Calibration;

#[derive(Debug)]
pub(super) struct ToolOffset {
    pub trigger_mm: [f64; 3],
    pub feed_mm_min: f64,
    pub offset_z_mm: f64,
    pub tip_height_at_home_mm: f64,
    pub coarse_z_mm: Option<f64>,
}

impl ToolOffset {
    pub fn from_ledger(text: &str, calibration: Calibration) -> Result<Self, String> {
        let rows = records(text, Workflow::ToolSetter)?;
        let touches: Vec<_> = rows.iter().filter(|r| r["kind"] == "touch").collect();
        let fine = match touches.as_slice() {
            [fine] if fine["stage"] == "1" => *fine,
            [coarse, fine] if coarse["stage"] == "0" && fine["stage"] == "1" => *fine,
            _ => return Err("tool offset requires one final fine contact, optionally preceded by one coarse contact; missing or repeated contacts are not accepted".into()),
        };
        for touch in &touches {
            if touch.get("exact_source").map(String::as_str)
                != Some("emcStatus.motion.traj.probedPosition;machine-mm")
            {
                return Err("tool contact lacks the original LinuxCNC G38 trigger source".into());
            }
        }
        let trigger_mm = [
            number(fine, "machine_x_exact")?,
            number(fine, "machine_y_exact")?,
            number(fine, "machine_z_exact")?,
        ];
        let feed_mm_min = number(fine, "feed")?;
        let offset_z_mm = trigger_mm[2] - calibration.height_mm;
        let tip_height_at_home_mm = calibration.home_z_mm - offset_z_mm;
        if feed_mm_min <= 0.0 || !offset_z_mm.is_finite() || !tip_height_at_home_mm.is_finite() {
            return Err(
                "retained tool contact produces an invalid feed or nonfinite Z reference".into(),
            );
        }
        let coarse_z_mm = touches
            .iter()
            .find(|r| r["stage"] == "0")
            .map(|r| number(r, "machine_z_exact"))
            .transpose()?;
        Ok(Self {
            trigger_mm,
            feed_mm_min,
            offset_z_mm,
            tip_height_at_home_mm,
            coarse_z_mm,
        })
    }

    pub fn json(
        &self,
        id: &str,
        calibration: Calibration,
        timing: &str,
        has_reference: bool,
    ) -> String {
        let source_reference = if has_reference {
            format!("\"{id}.setter-reference.json\"")
        } else {
            "null".into()
        };
        let coarse = self.coarse_z_mm.map_or("null".into(), |z| z.to_string());
        let difference = self.coarse_z_mm.map_or("null".into(), |z| {
            (self.trigger_mm[2] - z).abs().to_string()
        });
        format!(
            r#"{{
  "schema": "dmc2.plate-referenced-tool-offset.v1",
  "measurement_id": "{id}",
  "units": "mm",
  "source_ledger": "{id}.txt",
  "source_calibration_ini": "{id}.tool-reference.ini",
  "source_setter_reference": {source_reference},
  "calibration_snapshot_timing": "{timing}",
  "tool_identity": "installed tool at this measurement; no tool-table number assigned",
  "contact_input": "BTER normal contact IN0; overtravel IN2 is not the measurement source",
  "trigger_source": "emcStatus.motion.traj.probedPosition; machine XYZ millimetres",
  "approach": "physical DOWN / LinuxCNC -Z",
  "fine_trigger_machine_xyz_mm": [{x}, {y}, {z}],
  "fine_trigger_machine_z_f64_bits": "{z_bits:016x}",
  "fine_feed_mm_min": {feed},
  "coarse_trigger_machine_z_mm": {coarse},
  "coarse_to_fine_difference_mm": {difference},
  "accepted_setter_height_above_plate_mm": {height},
  "plate_contact_machine_z_mm": {offset},
  "plate_referenced_tool_offset_z_mm": {offset},
  "offset_z_f64_bits": "{offset_bits:016x}",
  "machine_home_z_mm": {home},
  "tool_tip_height_above_plate_at_home_mm": {tip_height},
  "offset_formula": "fine_trigger_machine_z - accepted_setter_height_above_plate",
  "tip_height_formula": "machine_home_z - plate_contact_machine_z",
  "machining_formula": "machine_z = desired_tip_height_above_plate + plate_referenced_tool_offset_z",
  "work_coordinate_convention": "G54 Z translation and G52/G92 Z translation zero for plate Z0; XY origins are independent",
  "scope": "This installed tool and accepted sampled plate reference; no full plate plane or spindle-gauge length inferred",
  "offset_application": "separate operator Run of the paired .tool-offset.ngc, or explicit use by a machining program",
  "applied_by_capture": false
}}
"#,
            x = self.trigger_mm[0],
            y = self.trigger_mm[1],
            z = self.trigger_mm[2],
            z_bits = self.trigger_mm[2].to_bits(),
            feed = self.feed_mm_min,
            height = calibration.height_mm,
            offset = self.offset_z_mm,
            offset_bits = self.offset_z_mm.to_bits(),
            home = calibration.home_z_mm,
            tip_height = self.tip_height_at_home_mm
        )
    }

    pub fn apply_program(&self, id: &str) -> String {
        format!(
            r#"%
(DMC2 SCRIPT 1)
(DMC2 EFFECTS coordinate-state)
(DMC2 REQUIRES running-session;estop-clear;machine-on;interpreter-idle;all-homed)
(DMC2 RECOVERY abort-task)
(DMC2 END)
(Apply plate-referenced Z tool offset from {id}.)
(Use with the same tool clamping and homed reference as this measurement.)
(This file sets compensation only; it commands no axis travel.)
o<tool_offset_preview> if [#<_task> EQ 0]
    (PREVIEW,stop)
    M2
o<tool_offset_preview> endif
o<tool_offset_homed> if [#<_hal[motion.is-all-homed]> NE 1]
    (ABORT,Tool offset requires the measured homed reference. Abort then Pendant Mode; establish that reference before applying the measurement.)
o<tool_offset_homed> endif
o<tool_offset_spindle> if [#<_spindle_on> NE 0]
    (ABORT,Stop the spindle before applying the recorded tool offset. Abort and Pendant Mode remain available.)
o<tool_offset_spindle> endif
o<tool_offset_frame> if [[#5220 NE 1] OR [#5223 NE 0] OR [[#5210 NE 0] AND [#5213 NE 0]]]
    (ABORT,This offset file uses G54 with zero Z translation and no active G52/G92 Z shift. Select that plate frame before Run; Abort and Pendant Mode remain available.)
o<tool_offset_frame> endif
G21
G43.1 Z{offset}
(MSG,Recorded Z tool offset applied for plate Z0. No axis travel commanded.)
M2
%
"#,
            offset = self.offset_z_mm
        )
    }
}
