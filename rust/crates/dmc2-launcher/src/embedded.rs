#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Root {
    Project,
    H100,
}

#[derive(Debug, Clone, Copy)]
pub struct File {
    pub root: Root,
    pub relative: &'static str,
    pub bytes: &'static [u8],
}

macro_rules! project_file {
    ($path:literal) => {
        File {
            root: Root::Project,
            relative: $path,
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../", $path)),
        }
    };
}

macro_rules! h100_file {
    ($path:literal) => {
        File {
            root: Root::H100,
            relative: $path,
            bytes: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../../h100_modbus/",
                $path
            )),
        }
    };
}

pub const FILES: &[File] = &[
    project_file!("config/operations.tsv"),
    project_file!("config/script-panel.json"),
    project_file!("config/block-plan-fields.txt"),
    project_file!("config/processes.tsv"),
    project_file!("config/linuxcnc-driver-overlays.tsv"),
    project_file!("packaging/dmc2-linuxcnc-autostart.desktop"),
    project_file!("live_requirements.json"),
    project_file!("live/dmc2.ini"),
    project_file!("live/machine.hal"),
    project_file!("live/pendant.hal"),
    project_file!("live/hal/pendant_input_sources.hal"),
    project_file!("live/hal/pendant_motion_contract.hal"),
    project_file!("live/hal/mesa_status_sources.hal"),
    project_file!("live/hal/status_latches.hal"),
    project_file!("live/hal/status_postgui.hal"),
    project_file!("live/hal/probe_mode.hal"),
    project_file!("live/ui/status_panel.xml"),
    project_file!("live/nc_files/dmc2_abort.ngc"),
    project_file!("live/nc_files/find-circle-center.ngc"),
    project_file!("live/nc_files/map-top-surface.ngc"),
    project_file!("live/nc_files/measure-gauge-block.ngc"),
    project_file!("live/nc_files/go-to-home.ngc"),
    project_file!("scripts/install_native_module.sh"),
    project_file!("scripts/build_linuxcnc_driver_overlays.sh"),
    project_file!("patches/linuxcnc-2.9.10/hm2-eth-buffer-safety.patch"),
    project_file!("patches/linuxcnc-2.9.10/tooldata-standalone-isolation.patch"),
    project_file!("python/dmc2_axis/__init__.py"),
    project_file!("python/dmc2_axis/axis_user_command.py"),
    project_file!("python/dmc2_axis/base_controls.py"),
    project_file!("python/dmc2_axis/custom_scripts.py"),
    project_file!("python/dmc2_axis/block_plan.py"),
    project_file!("python/dmc2_axis/script_panel_layout.py"),
    project_file!("python/dmc2_axis/script_panel_model.py"),
    project_file!("python/dmc2_axis/constants.py"),
    project_file!("python/dmc2_axis/diagnostic_journal.py"),
    project_file!("python/dmc2_axis/error_journal.py"),
    project_file!("python/dmc2_axis/notifications.py"),
    project_file!("python/dmc2_axis/notification_paint.tcl"),
    project_file!("python/dmc2_axis/operation_catalog.py"),
    project_file!("python/dmc2_axis/pendant_mode.py"),
    project_file!("python/dmc2_axis/probe_mode.py"),
    project_file!("python/dmc2_axis/pendant_icon.xbm"),
    project_file!("python/dmc2_axis/recovery_contract.py"),
    project_file!("python/dmc2_axis/recovery_ui.py"),
    project_file!("python/dmc2_axis/run_guard.py"),
    project_file!("python/dmc2_axis/script_contract.py"),
    project_file!("python/dmc2_axis/script_loader.py"),
    project_file!("python/dmc2_axis/spindle_feedback.py"),
    project_file!("python/dmc2_axis/ui_fault.py"),
    project_file!("var/log/linuxcnc/README.md"),
    h100_file!("maps/live/h100-spindle.mbccb"),
    h100_file!("maps/live/h100-spindle.mbccs"),
];
