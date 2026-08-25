//! Deterministic inventory of every logical field in the C snapshot ABI.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Debug)]
struct Field {
    c_type: String,
    name: String,
    extent: usize,
}

#[derive(Clone, Debug)]
struct Leaf {
    path: String,
    expression: String,
    c_type: String,
    element_count: usize,
}

fn parse_decimal_constants(source: &str) -> BTreeMap<String, usize> {
    let mut constants = BTreeMap::new();
    for line in source.lines() {
        let tokens = line.split_whitespace().collect::<Vec<_>>();
        if tokens.len() != 3 || tokens[0] != "#define" || !tokens[1].starts_with("DMC2_") {
            continue;
        }
        let raw = tokens[2].trim_end_matches('U');
        if let Ok(value) = raw.parse::<usize>() {
            assert!(
                constants.insert(tokens[1].to_owned(), value).is_none(),
                "duplicate snapshot constant {}",
                tokens[1]
            );
        }
    }
    constants
}

fn parse_declarator(declaration: &str, constants: &BTreeMap<String, usize>) -> Field {
    let tokens = declaration.split_whitespace().collect::<Vec<_>>();
    assert_eq!(
        tokens.len(),
        2,
        "snapshot field declaration must contain exactly a type and declarator: {declaration:?}"
    );
    let c_type = tokens[0].to_owned();
    let declarator = tokens[1];
    if let Some(open) = declarator.find('[') {
        assert!(declarator.ends_with(']'), "malformed array: {declaration}");
        let name = declarator[..open].to_owned();
        let extent_name = &declarator[open + 1..declarator.len() - 1];
        let extent = constants
            .get(extent_name)
            .copied()
            .unwrap_or_else(|| panic!("unknown array extent {extent_name} in {declaration}"));
        assert!(extent > 0, "zero-length snapshot array: {declaration}");
        Field {
            c_type,
            name,
            extent,
        }
    } else {
        Field {
            c_type,
            name: declarator.to_owned(),
            extent: 1,
        }
    }
}

fn parse_structs(
    source: &str,
    constants: &BTreeMap<String, usize>,
) -> BTreeMap<String, Vec<Field>> {
    let mut structs = BTreeMap::new();
    let mut current: Option<(String, Vec<Field>)> = None;
    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.starts_with("typedef struct dmc2_") && line.ends_with('{') {
            assert!(current.is_none(), "nested snapshot typedef is unsupported");
            let tokens = line.split_whitespace().collect::<Vec<_>>();
            assert_eq!(tokens.len(), 4, "malformed snapshot typedef: {line}");
            current = Some((tokens[2].to_owned(), Vec::new()));
            continue;
        }
        let Some((name, fields)) = current.as_mut() else {
            continue;
        };
        if line.starts_with('}') {
            let alias = line.trim_start_matches('}').trim().trim_end_matches(';');
            assert_eq!(alias, name, "snapshot typedef tag/alias mismatch");
            let (name, fields) = current.take().unwrap();
            assert!(!fields.is_empty(), "empty snapshot struct {name}");
            assert!(structs.insert(name.clone(), fields).is_none());
            continue;
        }
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        assert!(line.ends_with(';'), "malformed field in {name}: {line}");
        fields.push(parse_declarator(
            line.trim_end_matches(';').trim(),
            constants,
        ));
    }
    assert!(current.is_none(), "unterminated snapshot typedef");
    assert_eq!(
        structs.len(),
        15,
        "snapshot ABI must contain the 15 audited data structures"
    );
    structs
}

fn join(prefix: &str, field: &str) -> String {
    if prefix.is_empty() {
        field.to_owned()
    } else {
        format!("{prefix}.{field}")
    }
}

fn flatten(
    structs: &BTreeMap<String, Vec<Field>>,
    struct_name: &str,
    path_prefix: &str,
    expression_prefix: &str,
    active: &mut BTreeSet<String>,
    output: &mut Vec<Leaf>,
) {
    assert!(
        active.insert(struct_name.to_owned()),
        "recursive snapshot structure involving {struct_name}"
    );
    let fields = structs
        .get(struct_name)
        .unwrap_or_else(|| panic!("snapshot schema omitted {struct_name}"));
    for field in fields {
        let base_path = join(path_prefix, &field.name);
        let base_expression = join(expression_prefix, &field.name);
        if structs.contains_key(&field.c_type) {
            for index in 0..field.extent {
                let path = if field.extent == 1 {
                    base_path.clone()
                } else {
                    format!("{base_path}[{index}]")
                };
                let expression = if field.extent == 1 {
                    base_expression.clone()
                } else {
                    format!("{base_expression}[{index}]")
                };
                flatten(structs, &field.c_type, &path, &expression, active, output);
            }
        } else {
            assert!(
                matches!(
                    field.c_type.as_str(),
                    "double"
                        | "float"
                        | "int32_t"
                        | "int64_t"
                        | "uint8_t"
                        | "uint32_t"
                        | "uint64_t"
                ),
                "unaudited primitive snapshot type {}",
                field.c_type
            );
            output.push(Leaf {
                path: base_path,
                expression: base_expression,
                c_type: field.c_type.clone(),
                element_count: field.extent,
            });
        }
    }
    active.remove(struct_name);
}

fn command(program: &str, arguments: &[&str]) -> std::process::Output {
    Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("failed to execute {program}: {error}"))
}

fn path_text(path: &Path) -> &str {
    path.to_str().expect("build path is not valid UTF-8")
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

pub(crate) fn generate(snapshot_header: &Path, output_directory: &Path) {
    let source = fs::read_to_string(snapshot_header).unwrap_or_else(|error| {
        panic!(
            "failed to read snapshot schema {}: {error}",
            snapshot_header.display()
        )
    });
    let constants = parse_decimal_constants(&source);
    let structs = parse_structs(&source, &constants);
    let mut leaves = Vec::new();
    flatten(
        &structs,
        "dmc2_task_status_snapshot",
        "",
        "",
        &mut BTreeSet::new(),
        &mut leaves,
    );
    let unique_paths = leaves
        .iter()
        .map(|leaf| leaf.path.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        unique_paths.len(),
        leaves.len(),
        "flattened snapshot schema contains duplicate paths"
    );

    let probe_source = output_directory.join("status_snapshot_schema_probe.cc");
    let probe_binary = output_directory.join("status_snapshot_schema_probe");
    let mut probe = String::from(
        "#include <cstddef>\n#include <cstdio>\n#include \"status_snapshot.h\"\nint main() {\ndmc2_task_status_snapshot value{};\nconst auto *base = reinterpret_cast<const unsigned char *>(&value);\n",
    );
    for leaf in &leaves {
        probe.push_str(&format!(
            "std::printf(\"{}\\t%zu\\t%zu\\n\", static_cast<std::size_t>(reinterpret_cast<const unsigned char *>(&value.{}) - base), sizeof(value.{}));\n",
            leaf.path, leaf.expression, leaf.expression
        ));
    }
    probe.push_str("return 0;\n}\n");
    fs::write(&probe_source, probe).expect("failed to write snapshot schema probe");

    let native_directory = snapshot_header
        .parent()
        .expect("snapshot header has no parent directory");
    let compile = command(
        "g++",
        &[
            "-std=c++17",
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-I",
            path_text(native_directory),
            path_text(&probe_source),
            "-o",
            path_text(&probe_binary),
        ],
    );
    assert!(
        compile.status.success(),
        "snapshot schema probe failed to compile: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let result = Command::new(&probe_binary)
        .output()
        .expect("failed to run snapshot schema probe");
    assert!(
        result.status.success(),
        "snapshot schema probe failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut layout = BTreeMap::new();
    for line in String::from_utf8(result.stdout)
        .expect("snapshot schema probe produced non-UTF-8 output")
        .lines()
    {
        let fields = line.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 3, "malformed snapshot schema line: {line}");
        let offset = fields[1]
            .parse::<usize>()
            .unwrap_or_else(|error| panic!("invalid field offset in {line:?}: {error}"));
        let size = fields[2]
            .parse::<usize>()
            .unwrap_or_else(|error| panic!("invalid field size in {line:?}: {error}"));
        assert!(size > 0, "zero-sized snapshot field in {line:?}");
        assert!(
            layout
                .insert(fields[0].to_owned(), (offset, size))
                .is_none(),
            "duplicate snapshot layout result: {line}"
        );
    }
    assert_eq!(
        layout.len(),
        leaves.len(),
        "native schema probe omitted a logical snapshot field"
    );

    let mut generated = format!(
        "pub const SNAPSHOT_SCHEMA_FNV64: u64 = 0x{:016x};\n\
         pub const SNAPSHOT_LOGICAL_FIELD_COUNT: usize = {};\n\
         pub static SNAPSHOT_FIELDS: &[SnapshotFieldSpec] = &[\n",
        fnv1a(source.as_bytes()),
        leaves.len(),
    );
    for leaf in &leaves {
        let (offset, byte_size) = layout[&leaf.path];
        generated.push_str(&format!(
            "SnapshotFieldSpec {{ path: {:?}, c_type: {:?}, element_count: {}, byte_offset: {}, byte_size: {} }},\n",
            leaf.path, leaf.c_type, leaf.element_count, offset, byte_size
        ));
    }
    generated.push_str("];\n");
    fs::write(
        output_directory.join("status_snapshot_fields.rs"),
        generated,
    )
    .expect("failed to write snapshot field inventory");
}
