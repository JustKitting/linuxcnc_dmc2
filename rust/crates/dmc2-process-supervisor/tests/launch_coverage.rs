use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use dmc2_process_supervisor::Invocation;

#[derive(Debug)]
struct CatalogRow {
    program: String,
    launch_site: String,
    ownership: String,
    criticality: String,
}

#[test]
fn every_live_userspace_launch_and_every_direct_catalog_row_are_bidirectionally_covered() {
    let project = project_root();
    let catalog = read_catalog(&project);
    let mut live_roles = BTreeSet::new();

    let ini = fs::read_to_string(project.join("live/dmc2.ini")).expect("read live DMC2 INI");
    for (key, expected_role, expected_site) in [
        ("DISPLAY", "axis", "ini-display"),
        ("TASK", "milltask", "ini-task"),
        ("EMCIO", "iocontrol", "ini-emcio"),
        ("HALUI", "halui", "ini-halui"),
    ] {
        let prefix = format!("{key} = ");
        let configured = ini
            .lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .unwrap_or_else(|| panic!("live INI has no {key} process"));
        let invocation = parse_configured_owner(configured, expected_role == "axis");
        assert_eq!(invocation.role.name(), expected_role, "live INI key {key}");
        let row = catalog.get(expected_role).expect("INI role in catalog");
        assert_eq!(
            row.launch_site, expected_site,
            "catalog launch site for {key}"
        );
        assert_eq!(invocation.program, OsString::from(&row.program));
        assert!(
            live_roles.insert(expected_role.to_owned()),
            "duplicate live process role {expected_role}"
        );
    }

    let mut hal_files = Vec::new();
    collect_hal_files(&project.join("live"), &mut hal_files);
    for path in hal_files {
        let contents = fs::read_to_string(&path).expect("read live HAL file");
        for (line_number, line) in contents.lines().enumerate() {
            let line = line.split('#').next().unwrap_or_default().trim();
            if !line.starts_with("loadusr ") {
                continue;
            }
            let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
            let owner_index = fields
                .iter()
                .position(|field| field.ends_with("/dmc2-process-supervisor"))
                .unwrap_or_else(|| {
                    panic!(
                        "unowned live loadusr at {}:{}: {line}",
                        path.display(),
                        line_number + 1
                    )
                });
            let invocation = Invocation::parse(
                fields[owner_index + 1..]
                    .iter()
                    .copied()
                    .map(OsString::from),
            )
            .unwrap_or_else(|error| {
                panic!(
                    "invalid process owner at {}:{}: {error}",
                    path.display(),
                    line_number + 1
                )
            });
            let role = invocation.role.name();
            let row = catalog.get(role).expect("HAL role in catalog");
            assert_eq!(row.launch_site, "pendant-hal", "HAL role {role}");
            assert_eq!(invocation.program, OsString::from(&row.program));
            assert!(
                live_roles.insert(role.to_owned()),
                "duplicate live process role {role}"
            );
        }
    }

    let expected_roles = catalog
        .iter()
        .filter(|(_, row)| {
            row.ownership == "direct-child" && row.criticality != "verification-only"
        })
        .map(|(role, _)| role.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        live_roles, expected_roles,
        "live configuration and production direct-child catalog diverged"
    );
}

#[test]
fn linuxcncs_two_hardcoded_persistent_processes_remain_catalogued() {
    let project = project_root();
    let catalog = read_catalog(&project);
    let launcher = fs::read_to_string(project.join("vendor/linuxcnc-2.9.10/scripts/linuxcnc.in"))
        .expect("read pinned LinuxCNC launcher source");
    let halcmd =
        fs::read_to_string(project.join("vendor/linuxcnc-2.9.10/src/hal/utils/halcmd_commands.cc"))
            .expect("read pinned halcmd source");

    let server = catalog.get("linuxcncsvr").expect("linuxcncsvr catalog row");
    assert_eq!(server.program, "/usr/bin/linuxcncsvr");
    assert_eq!(server.launch_site, "linuxcnc-hardcoded-server");
    assert_eq!(server.ownership, "self-daemonizing-descendant");
    assert!(launcher.contains("EMCSERVER=linuxcncsvr"));

    let realtime = catalog.get("rtapi-app").expect("rtapi-app catalog row");
    assert_eq!(realtime.program, "/usr/bin/rtapi_app");
    assert_eq!(realtime.launch_site, "halcmd-hardcoded-loadrt");
    assert_eq!(realtime.ownership, "persistent-master");
    assert!(halcmd.contains("EMC2_BIN_DIR \"/rtapi_app\""));
}

fn parse_configured_owner(configured: &str, linuxcnc_prepends_ini: bool) -> Invocation {
    let mut fields = configured.split_ascii_whitespace();
    let owner = fields.next().expect("configured process owner executable");
    assert!(
        owner.ends_with("/dmc2-process-supervisor"),
        "configured process bypasses the lifecycle owner: {configured}"
    );
    let mut arguments = fields.map(OsString::from).collect::<Vec<_>>();
    if linuxcnc_prepends_ini {
        arguments.splice(0..0, [OsString::from("-ini"), OsString::from("dmc2.ini")]);
    }
    Invocation::parse(arguments)
        .unwrap_or_else(|error| panic!("invalid configured process owner {configured:?}: {error}"))
}

fn collect_hal_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read live configuration directory") {
        let entry = entry.expect("read live configuration entry");
        let path = entry.path();
        if path.is_dir() {
            collect_hal_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "hal") {
            files.push(path);
        }
    }
}

fn read_catalog(project: &Path) -> BTreeMap<String, CatalogRow> {
    let contents =
        fs::read_to_string(project.join("config/processes.tsv")).expect("read process catalog");
    contents
        .lines()
        .skip(2)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 10, "invalid process-catalog row: {line}");
            (
                fields[0].to_owned(),
                CatalogRow {
                    program: fields[1].to_owned(),
                    launch_site: fields[2].to_owned(),
                    ownership: fields[3].to_owned(),
                    criticality: fields[4].to_owned(),
                },
            )
        })
        .collect()
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}
