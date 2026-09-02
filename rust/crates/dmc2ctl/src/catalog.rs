use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

const MAGIC: &str = "DMC2_OPERATION_CATALOG\t1";
const HEADER: &str = "id\tkind\tlabel\tdriver\ttarget\tui_scope\teffects\tprerequisites";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationKind {
    Control,
    Program,
    Internal,
}

impl OperationKind {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "control" => Some(Self::Control),
            "program" => Some(Self::Program),
            "internal" => Some(Self::Internal),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Control => "control",
            Self::Program => "program",
            Self::Internal => "internal",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Operation {
    pub id: String,
    pub kind: OperationKind,
    pub label: String,
    pub driver: String,
    pub target: String,
    pub ui_scope: String,
    pub effects: Vec<String>,
    pub prerequisites: Vec<String>,
}

#[derive(Debug)]
pub struct Catalog {
    path: PathBuf,
    project_root: PathBuf,
    operations: BTreeMap<String, Operation>,
}

impl Catalog {
    pub fn open(path: PathBuf) -> Result<Self, CatalogError> {
        let content = fs::read_to_string(&path).map_err(|source| CatalogError::Read {
            path: path.clone(),
            source,
        })?;
        Self::parse(path, &content)
    }

    fn parse(path: PathBuf, content: &str) -> Result<Self, CatalogError> {
        let mut lines = content.lines();
        if lines.next() != Some(MAGIC) {
            return Err(CatalogError::Magic { path });
        }
        if lines.next() != Some(HEADER) {
            return Err(CatalogError::Header { path });
        }

        let mut operations = BTreeMap::new();
        for (index, line) in lines.enumerate() {
            let line_number = index + 3;
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields = line.split('\t').collect::<Vec<_>>();
            if fields.len() != 8 {
                return Err(CatalogError::ColumnCount {
                    path,
                    line: line_number,
                    observed: fields.len(),
                });
            }
            let kind =
                OperationKind::parse(fields[1]).ok_or_else(|| CatalogError::OperationKind {
                    path: path.clone(),
                    line: line_number,
                    value: fields[1].to_owned(),
                })?;
            let operation = Operation {
                id: fields[0].to_owned(),
                kind,
                label: fields[2].to_owned(),
                driver: fields[3].to_owned(),
                target: fields[4].to_owned(),
                ui_scope: fields[5].to_owned(),
                effects: split_list(fields[6]),
                prerequisites: split_list(fields[7]),
            };
            validate_operation(&path, line_number, &operation)?;
            if operations.insert(operation.id.clone(), operation).is_some() {
                return Err(CatalogError::DuplicateId {
                    path,
                    line: line_number,
                    id: fields[0].to_owned(),
                });
            }
        }

        let project_root = path
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| CatalogError::ProjectRoot { path: path.clone() })?
            .to_path_buf();
        Ok(Self {
            path,
            project_root,
            operations,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    pub fn operations(&self) -> impl Iterator<Item = &Operation> {
        self.operations.values()
    }

    pub fn operation(&self, id: &str) -> Result<&Operation, CatalogError> {
        self.operations
            .get(id)
            .ok_or_else(|| CatalogError::UnknownId {
                path: self.path.clone(),
                id: id.to_owned(),
            })
    }
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(';')
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn validate_operation(path: &Path, line: usize, operation: &Operation) -> Result<(), CatalogError> {
    for (field, value) in [
        ("id", operation.id.as_str()),
        ("label", operation.label.as_str()),
        ("driver", operation.driver.as_str()),
        ("target", operation.target.as_str()),
        ("ui_scope", operation.ui_scope.as_str()),
    ] {
        if value.is_empty() {
            return Err(CatalogError::EmptyField {
                path: path.to_path_buf(),
                line,
                field,
            });
        }
    }
    Ok(())
}

pub fn default_catalog_path() -> PathBuf {
    if let Some(path) = env::var_os("DMC2_OPERATION_CATALOG") {
        return PathBuf::from(path);
    }
    if let Some(path) = installed_catalog_path(env::current_exe().ok()) {
        if path.is_file() {
            return path;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../config/operations.tsv")
}

fn installed_catalog_path(executable: Option<PathBuf>) -> Option<PathBuf> {
    let binary_directory = executable?.parent()?.to_path_buf();
    let native_directory = binary_directory.parent()?;
    let project_root = native_directory.parent()?;
    Some(project_root.join("config/operations.tsv"))
}

#[derive(Debug)]
pub enum CatalogError {
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    Magic {
        path: PathBuf,
    },
    Header {
        path: PathBuf,
    },
    ColumnCount {
        path: PathBuf,
        line: usize,
        observed: usize,
    },
    OperationKind {
        path: PathBuf,
        line: usize,
        value: String,
    },
    EmptyField {
        path: PathBuf,
        line: usize,
        field: &'static str,
    },
    DuplicateId {
        path: PathBuf,
        line: usize,
        id: String,
    },
    UnknownId {
        path: PathBuf,
        id: String,
    },
    ProjectRoot {
        path: PathBuf,
    },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(
                    formatter,
                    "cannot read operation catalog {}: {source}",
                    path.display()
                )
            }
            Self::Magic { path } => write!(
                formatter,
                "operation catalog {} has an unsupported format/version",
                path.display()
            ),
            Self::Header { path } => {
                write!(
                    formatter,
                    "operation catalog {} has invalid columns",
                    path.display()
                )
            }
            Self::ColumnCount {
                path,
                line,
                observed,
            } => write!(
                formatter,
                "operation catalog {} line {line} has {observed} columns; expected 8",
                path.display()
            ),
            Self::OperationKind { path, line, value } => write!(
                formatter,
                "operation catalog {} line {line} has unknown kind {value:?}",
                path.display()
            ),
            Self::EmptyField { path, line, field } => write!(
                formatter,
                "operation catalog {} line {line} has empty {field}",
                path.display()
            ),
            Self::DuplicateId { path, line, id } => write!(
                formatter,
                "operation catalog {} line {line} duplicates {id}",
                path.display()
            ),
            Self::UnknownId { path, id } => {
                write!(formatter, "operation {id:?} is not in {}", path.display())
            }
            Self::ProjectRoot { path } => write!(
                formatter,
                "cannot derive project root from operation catalog {}",
                path.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_catalog_is_strict_and_contains_home() {
        let catalog = Catalog::open(default_catalog_path()).expect("live catalog should parse");
        let home = catalog
            .operation("machine.home-all")
            .expect("home operation should exist");
        assert_eq!(home.kind, OperationKind::Control);
        assert_eq!(home.driver, "linuxcnc.home-all");
    }

    #[test]
    fn installed_binary_resolves_the_shared_project_catalog() {
        let executable = PathBuf::from("/project/native/bin/dmc2ctl");
        assert_eq!(
            installed_catalog_path(Some(executable)),
            Some(PathBuf::from("/project/config/operations.tsv"))
        );
    }
}
