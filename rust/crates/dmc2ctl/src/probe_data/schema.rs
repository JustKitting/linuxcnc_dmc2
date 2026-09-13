use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workflow {
    Circle,
    Surface,
    Block,
    ToolSetter,
}

impl Workflow {
    pub fn name(self) -> &'static str {
        match self {
            Self::Circle => "circle",
            Self::Surface => "surface",
            Self::Block => "block",
            Self::ToolSetter => "tool-setter",
        }
    }
    pub fn magic(self) -> &'static str {
        match self {
            Self::Circle => "DMC2_CIRCLE_RECORD_V2",
            Self::Surface => "DMC2_SURFACE_RECORD_V1",
            Self::Block => "DMC2_BLOCK_RECORD_V1",
            Self::ToolSetter => "DMC2_TOOL_SETTER_RECORD_V1",
        }
    }
    pub fn ledger_magic(self) -> &'static str {
        match self {
            Self::Circle => "DMC2_CIRCLE_LEDGER_V2",
            Self::Surface => "DMC2_SURFACE_LEDGER_V1",
            Self::Block => "DMC2_BLOCK_LEDGER_V1",
            Self::ToolSetter => "DMC2_TOOL_SETTER_LEDGER_V1",
        }
    }
    pub fn request(self) -> String {
        format!("{}-request.txt", self.name())
    }
    pub fn active(self) -> String {
        format!("{}-active.txt", self.name())
    }
}

pub fn validate(workflow: Workflow, request: &str, sequence: u64) -> Result<(), String> {
    let mut lines = request.lines();
    if lines.next() != Some(workflow.magic()) || !request.ends_with('\n') {
        return Err("staged capture is missing its complete versioned header/terminator".into());
    }
    let mut values = BTreeMap::new();
    for line in lines {
        let (key, value) = line
            .split_once('=')
            .ok_or("malformed staged capture field")?;
        if values.insert(key, value).is_some() {
            return Err(format!("duplicate capture field {key}"));
        }
    }
    if values
        .remove("sequence")
        .and_then(|v| v.parse::<u64>().ok())
        != Some(sequence)
    {
        return Err("staged record has a stale or missing capture sequence".into());
    }
    let kind = values
        .remove("kind")
        .ok_or("capture record kind is missing")?;
    let required: &[&str] = match (workflow, kind) {
        (Workflow::Circle, "start") => &[
            "x",
            "y",
            "z",
            "offset_x",
            "offset_y",
            "offset_z",
            "feed",
            "coarse_feed",
            "search",
            "ball_diameter",
            "step_x",
            "step_y",
        ],
        (Workflow::Circle, "touch") => &[
            "pass",
            "stage",
            "axis",
            "direction",
            "success",
            "work_x",
            "work_y",
            "work_z",
            "machine_x",
            "machine_y",
            "machine_z",
            "feed",
        ],
        (Workflow::Circle, "sweep") => &[
            "pass",
            "x",
            "y",
            "center_x",
            "center_y",
            "dx",
            "dy",
            "span_error",
            "balance",
            "radius_x",
            "radius_y",
        ],
        (Workflow::Circle, "selection" | "result") => &[
            "reason",
            "x",
            "y",
            "machine_x",
            "machine_y",
            "dx",
            "dy",
            "span_error",
            "balance",
            "diameter",
            "pass",
        ],
        (Workflow::Surface, "start") => &[
            "x",
            "y",
            "z",
            "offset_x",
            "offset_y",
            "offset_z",
            "x_min",
            "x_max",
            "y_min",
            "y_max",
            "spacing",
            "columns",
            "rows",
            "reference_search",
            "max_drop",
            "clearance",
            "usable_reach",
            "reach_reserve",
            "feed",
            "coarse_feed",
            "ball_diameter",
        ],
        (Workflow::Surface, "touch") => &[
            "point",
            "row",
            "column",
            "stage",
            "axis",
            "direction",
            "success",
            "work_x",
            "work_y",
            "work_z",
            "machine_x",
            "machine_y",
            "machine_z",
            "feed",
        ],
        (Workflow::Surface, "miss") => &[
            "point",
            "row",
            "column",
            "work_x",
            "work_y",
            "floor_work_z",
            "machine_x",
            "machine_y",
            "floor_machine_z",
            "feed",
        ],
        (Workflow::Surface, "obstruction") => &[
            "success",
            "work_x",
            "work_y",
            "work_z",
            "machine_x",
            "machine_y",
            "machine_z",
            "from_x",
            "from_y",
            "from_z",
            "target_x",
            "target_y",
            "target_z",
            "feed",
        ],
        (Workflow::Surface, "result") => &[
            "points",
            "hits",
            "misses",
            "reference_work_z",
            "clearance_work_z",
            "floor_work_z",
            "final_work_x",
            "final_work_y",
            "final_work_z",
        ],
        (Workflow::Block, "start") => super::block_schema::START_FIELDS,
        (Workflow::ToolSetter, "start") => &[
            "style",
            "setter_x",
            "setter_y",
            "setter_height",
            "home_z",
            "target_z",
            "start_x",
            "start_y",
            "start_z",
            "offset_x",
            "offset_y",
            "offset_z",
            "first_feed",
            "second_feed",
            "backoff",
            "backoff_feed",
            "position_tolerance",
        ],
        (Workflow::ToolSetter, "touch") => &[
            "stage",
            "axis",
            "direction",
            "success",
            "work_x",
            "work_y",
            "work_z",
            "machine_x",
            "machine_y",
            "machine_z",
            "feed",
        ],
        (Workflow::ToolSetter, "result") => &[
            "touches",
            "trigger_machine_z",
            "plate_contact_machine_z",
            "tool_tip_height_at_home",
            "repeat_difference",
        ],
        (
            Workflow::Block,
            "touch" | "miss" | "travel" | "obstruction" | "ready" | "recovery" | "result",
        ) => super::block_schema::EVENT_FIELDS,
        _ => return Err(format!("unknown capture record kind {kind}")),
    };
    if values.len() != required.len() {
        return Err(format!("{kind} capture has missing or unexpected fields"));
    }
    for key in required {
        let value = values
            .get(key)
            .ok_or_else(|| format!("missing {kind} field {key}"))?;
        if !value.parse::<f64>().map(|v| v.is_finite()).unwrap_or(false) {
            return Err(format!(
                "{kind} field {key} is not a finite number: {value}"
            ));
        }
    }
    if matches!(kind, "touch" | "obstruction") && values["success"].parse::<f64>() != Ok(1.0) {
        return Err("G38 did not report a contact; no reconstructed position is accepted".into());
    }
    if kind == "touch" && !matches!(values["stage"], "0" | "1") {
        return Err("touch stage must be coarse-location=0 or fine-measurement=1".into());
    }
    if workflow == Workflow::Block {
        super::block_schema::validate_fields(kind, &values)?;
    }
    for key in [
        "point", "row", "column", "rows", "columns", "points", "hits", "misses",
    ] {
        // The adaptive block grid is signed around its starting cell. Its own
        // schema validates full signed indices; fixed surface grids use -1 only
        // for their reference measurement.
        if workflow == Workflow::Block {
            continue;
        }
        if let Some(raw) = values.get(key) {
            let number = raw.parse::<f64>().unwrap();
            if number.fract() != 0.0 || number < -1.0 {
                return Err(format!(
                    "{kind} field {key} must be an integer grid identifier/count"
                ));
            }
        }
    }
    if workflow == Workflow::Surface
        && kind == "touch"
        && (values["axis"] != "2" || values["direction"] != "-1")
    {
        return Err("surface samples must be downward Z contacts".into());
    }
    if workflow == Workflow::ToolSetter {
        match kind {
            "start" => {
                if !matches!(values["style"], "1" | "2") {
                    return Err("tool-setter style must be single touch=1 or double touch=2".into());
                }
                for key in [
                    "setter_height",
                    "first_feed",
                    "second_feed",
                    "backoff",
                    "backoff_feed",
                    "position_tolerance",
                ] {
                    if values[key].parse::<f64>().unwrap() <= 0.0 {
                        return Err(format!(
                            "tool-setter {key} must be positive; reopen the calibrated program"
                        ));
                    }
                }
            }
            "touch" if values["axis"] != "2" || values["direction"] != "-1" => {
                return Err("tool-setter measurements must be downward Z contacts".into());
            }
            "result" if !matches!(values["touches"], "1" | "2") => {
                return Err("tool-setter result must retain one or two contacts".into());
            }
            _ => (),
        }
    }
    Ok(())
}
