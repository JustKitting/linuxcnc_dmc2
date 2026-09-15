use super::{number, Mode, Plan, Rows};
const BLOB: &str = "(FOLLOWUP-BYTES ";
const EXECUTOR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../live/nc_files/mapper-run.ngc"
));
const LOCAL_EXECUTOR: &str = "o<top-followup-run>";
impl Plan {
    pub fn program(&self) -> Result<String, String> {
        let encoded = self.encode()?;
        let s = self.settings()?;
        let mut out = String::from("%\n(DMC2 SCRIPT 1)\n(DMC2 EFFECTS axis-motion;digital-output;coordinate-state;external-command)\n(DMC2 REQUIRES running-session;estop-clear;machine-on;interpreter-idle;all-homed)\n(DMC2 RECOVERY abort-task)\n(DMC2 END)\n(Top follow-up: fresh columns in the explicit order below.)\n(Review the same stock, probe and frame, plus every transfer at original clearance.)\n(No positioning move to the start is supplied. Abort then Pendant Mode remains available.)\n(Physical RIGHT / LinuxCNC -X; physical LEFT / LinuxCNC +X.)\n");
        if let Some(role) = self.role {
            out.push_str(&format!(
                "(Fresh fine contact role: {}. {})\n",
                role.name(),
                role.description()
            ));
        }
        if self.rows.directed() {
            out.push_str("(Adaptive top/side observations: explicit ray geometry follows; each returns to starting clearance.)\n");
        }
        out.push_str(&format!(
            "(Required starting work XYZ mm: {:?}; work-to-machine translation mm: {:?})\n",
            s.origin, s.offset
        ));
        out.push_str(&format!(
            "(Fixed work Z floor: {}; downward/fine/travel feeds mm/min: {:?}; backoff mm: {})\n",
            s.floor,
            [s.downward_feed, s.feeds[1], s.feeds[2]],
            s.outline_backoff()
        ));
        for (i, q) in self.requests()?.iter().enumerate() {
            let p = q.approach;
            if self.rows.directed() {
                out.push_str(&format!(
                    "(Adaptive row {i}: phase {}; approach XY {:?}; target XYZ {:?})\n",
                    q.phase as u8, p, q.target
                ));
                if q.target[0] > q.approach[0] {
                    out.push_str("(This side approach: physical LEFT / LinuxCNC +X.)\n");
                }
                if q.target[0] < q.approach[0] {
                    out.push_str("(This side approach: physical RIGHT / LinuxCNC -X.)\n");
                }
            }
            out.push_str(&format!(
                "(Row {i}: work XY mm {:?}; clear Z {}; floor Z {})\n",
                p, s.origin[2], s.floor
            ));
            if let Rows::Repeats(rows) = &self.rows {
                let source = &rows[i];
                out.push_str(&format!(
                    "(Repeat proposal {}: original capture {} record {}; phase {} retained.)\n",
                    source.proposal, source.capture, source.sequence, q.phase as u8
                ));
            }
        }
        // Hex comments bind all plan bytes into the standard loader's program
        // revision. 48 source bytes keep each line below LinuxCNC LINELEN.
        for chunk in encoded.as_bytes().chunks(48) {
            out.push_str(BLOB);
            for b in chunk {
                out.push_str(&format!("{b:02x}"));
            }
            out.push_str(")\n");
        }
        // LinuxCNC reports the current interpreter filename, including an
        // external subroutine. Keep P12 inside this file by materializing the
        // one shared executor template; implementation is never forked here.
        let definition = EXECUTOR.strip_suffix("M2\n").ok_or("The shared mapper executor lacks its expected standalone terminator. Restore the matching source template and rebuild before exporting a follow-up program.")?;
        out.push_str(&definition.replace("o<mapper-run>", LOCAL_EXECUTOR));
        out.push('\n');
        out.push_str("o<top-followup-preview> if [#<_task> EQ 0]\n    (PREVIEW,stop)\n    M2\no<top-followup-preview> endif\n");
        let arguments = [
            "drop",
            "grid",
            "resolution",
            "usable_reach",
            "reach_reserve",
            "side_depth",
            "backoff",
        ]
        .map(|k| number(&self.start, k));
        out.push_str("(X increasing: physical LEFT / LinuxCNC +X; decreasing: physical RIGHT / LinuxCNC -X.)\n");
        out.push_str(&format!(
            "{LOCAL_EXECUTOR} call [{}]",
            Mode::TopFollowup as u8
        ));
        for a in arguments {
            out.push_str(&format!(" [{}]", a?));
        }
        out.push_str("\nM2\n%\n");
        // LinuxCNC 2.9.10 src/emc/linuxcnc.h: LINELEN is 255 bytes.
        if out.lines().any(|line| line.len() >= 255) {
            return Err("A generated follow-up line exceeds LinuxCNC's line buffer. Inspect the source coordinate scale before exporting; no numeric value was rounded or truncated.".into());
        }
        Ok(out)
    }
    pub fn from_program(text: &str) -> Result<Self, String> {
        let mut bytes = Vec::new();
        for line in text.lines().filter_map(|l| l.strip_prefix(BLOB)) {
            let hex = line.strip_suffix(')').ok_or(
                "Follow-up plan comment is unfinished. Re-export the original program before Run.",
            )?;
            if !hex.is_ascii() || hex.len() % 2 != 0 {
                return Err("Follow-up plan bytes are malformed. Re-export the original program before Run.".into());
            }
            for i in (0..hex.len()).step_by(2) {
                bytes.push(u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| {
                    "Follow-up plan bytes are malformed. Re-export the original program before Run."
                })?);
            }
        }
        let plan = Self::read(std::str::from_utf8(&bytes).map_err(|_| {
            "Follow-up plan is not UTF-8. Re-export the original program before Run."
        })?)?;
        if plan.program()? != text {
            return Err("The loaded follow-up program differs from its retained plan. No next target was supplied. Re-export and open the intact program through File Open; Abort then Pendant Mode remains available.".into());
        }
        Ok(plan)
    }
}
