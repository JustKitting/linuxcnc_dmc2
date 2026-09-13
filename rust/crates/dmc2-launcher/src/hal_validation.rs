use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::layout::Layout;
use crate::platform::Platform;

const HAL_SOURCES: &[&str] = &[
    "live/machine.hal",
    "live/pendant.hal",
    "live/hal/pendant_input_sources.hal",
    "live/hal/pendant_motion_contract.hal",
    "live/hal/mesa_status_sources.hal",
    "live/hal/status_latches.hal",
    "live/hal/status_postgui.hal",
    "live/hal/probe_mode.hal",
    "live/hal/tool_setter.hal",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Assignment {
    signal: String,
    path: PathBuf,
    line: usize,
}

pub fn validate(platform: &dyn Platform, layout: &Layout) -> Result<(), Error> {
    let mut assignments = BTreeMap::new();
    for relative in HAL_SOURCES {
        let path = layout.project.join(relative);
        let bytes = platform
            .read_file(&path)
            .map_err(|error| Error::os("read HAL source", path.clone(), error))?;
        let source = std::str::from_utf8(&bytes).map_err(|error| Error::HalSourceInvalidUtf8 {
            path: path.clone(),
            valid_up_to: error.valid_up_to(),
        })?;
        validate_source(&path, source, &mut assignments)?;
    }
    Ok(())
}

fn validate_source(
    path: &Path,
    source: &str,
    assignments: &mut BTreeMap<String, Assignment>,
) -> Result<(), Error> {
    for (zero_based_line, raw_line) in source.lines().enumerate() {
        let line_number = zero_based_line + 1;
        let command = raw_line.split_once('#').map_or(raw_line, |(code, _)| code);
        let mut words = command.split_ascii_whitespace();
        if words.next() != Some("net") {
            continue;
        }
        let Some(signal) = words.next() else {
            continue;
        };
        for pin in words.filter(|word| !matches!(*word, "=>" | "<=" | "<=>")) {
            let observed = Assignment {
                signal: signal.to_owned(),
                path: path.to_path_buf(),
                line: line_number,
            };
            if let Some(first) = assignments.get(pin) {
                if first.signal != signal {
                    return Err(Error::HalPinSignalConflict {
                        pin: pin.to_owned(),
                        first_signal: first.signal.clone(),
                        first_path: first.path.clone(),
                        first_line: first.line,
                        conflicting_signal: signal.to_owned(),
                        conflicting_path: path.to_path_buf(),
                        conflicting_line: line_number,
                    });
                }
            } else {
                assignments.insert(pin.to_owned(), observed);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_both_exact_locations_when_a_pin_is_assigned_twice() {
        let mut assignments = BTreeMap::new();
        validate_source(
            Path::new("first.hal"),
            "net spindle-ready source.one => motion.digital-in-02\n",
            &mut assignments,
        )
        .expect("first assignment");

        let error = validate_source(
            Path::new("second.hal"),
            "# comment\nnet probe-selected source.two => motion.digital-in-02\n",
            &mut assignments,
        )
        .expect_err("conflicting assignment must be rejected");

        assert_eq!(error.identity(), "HAL_PIN_SIGNAL_CONFLICT");
        let rendered = error.to_string();
        assert!(rendered.contains("motion.digital-in-02"));
        assert!(rendered.contains("first.hal:1"));
        assert!(rendered.contains("second.hal:2"));
        assert!(rendered.contains("spindle-ready"));
        assert!(rendered.contains("probe-selected"));
    }

    #[test]
    fn accepts_repeating_the_same_signal_to_add_more_pins() {
        let mut assignments = BTreeMap::new();
        validate_source(
            Path::new("one.hal"),
            "net shared <= producer\nnet shared => consumer producer\n",
            &mut assignments,
        )
        .expect("same-signal reuse is valid");
    }

    #[test]
    fn all_live_hal_sources_assign_each_pin_to_only_one_signal() {
        let sources = [
            (
                "live/hal/tool_setter.hal",
                include_str!("../../../../live/hal/tool_setter.hal"),
            ),
            (
                "live/hal/probe_mode.hal",
                include_str!("../../../../live/hal/probe_mode.hal"),
            ),
            (
                "live/machine.hal",
                include_str!("../../../../live/machine.hal"),
            ),
            (
                "live/pendant.hal",
                include_str!("../../../../live/pendant.hal"),
            ),
            (
                "live/hal/pendant_input_sources.hal",
                include_str!("../../../../live/hal/pendant_input_sources.hal"),
            ),
            (
                "live/hal/pendant_motion_contract.hal",
                include_str!("../../../../live/hal/pendant_motion_contract.hal"),
            ),
            (
                "live/hal/mesa_status_sources.hal",
                include_str!("../../../../live/hal/mesa_status_sources.hal"),
            ),
            (
                "live/hal/status_latches.hal",
                include_str!("../../../../live/hal/status_latches.hal"),
            ),
            (
                "live/hal/status_postgui.hal",
                include_str!("../../../../live/hal/status_postgui.hal"),
            ),
        ];
        let mut assignments = BTreeMap::new();
        for (path, source) in sources {
            validate_source(Path::new(path), source, &mut assignments)
                .unwrap_or_else(|error| panic!("{error}"));
        }
    }
}
