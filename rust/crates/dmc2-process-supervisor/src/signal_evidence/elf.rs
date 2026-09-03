use std::fmt;
use std::fs::{self, File, Metadata};
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::event::Event;

const ELF_HEADER_BYTES: usize = 64;
const O_CLOEXEC: i32 = 0o2_000_000;
const O_NOFOLLOW: i32 = 0o400_000;
const ELF_TYPE_SHARED_OBJECT: u16 = 3;

pub struct LibraryEvidence {
    pub path: PathBuf,
    device: u64,
    inode: u64,
    bytes: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    mtime_seconds: i64,
    mtime_nanoseconds: i64,
    ctime_seconds: i64,
    ctime_nanoseconds: i64,
    identity: ElfIdentity,
    target_path: PathBuf,
    target_device: u64,
    target_inode: u64,
    target_bytes: u64,
    target_mode: u32,
    target_uid: u32,
    target_gid: u32,
    target_identity: ElfIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ElfIdentity {
    class: u8,
    data: u8,
    object_type: u16,
    machine: u16,
}

#[derive(Debug)]
pub enum LibraryError {
    UnsafePath(PathBuf),
    Canonicalize {
        path: PathBuf,
        source: io::Error,
    },
    Metadata {
        path: PathBuf,
        source: io::Error,
    },
    NotRegularFile(PathBuf),
    UnsafePermissions {
        path: PathBuf,
        mode: u32,
    },
    Open {
        path: PathBuf,
        source: io::Error,
    },
    IdentityChanged(PathBuf),
    Read {
        path: PathBuf,
        source: io::Error,
    },
    InvalidElf {
        path: PathBuf,
        detail: &'static str,
    },
    TargetExecutable {
        path: PathBuf,
        source: io::Error,
    },
    AbiMismatch {
        library: PathBuf,
        library_identity: String,
        target: PathBuf,
        target_identity: String,
    },
}

pub fn inspect_library(
    requested_path: &Path,
    requested_target: &Path,
) -> Result<LibraryEvidence, LibraryError> {
    if !requested_path.is_absolute()
        || requested_path
            .as_os_str()
            .as_bytes()
            .iter()
            .any(|byte| *byte == b':' || byte.is_ascii_whitespace())
    {
        return Err(LibraryError::UnsafePath(requested_path.to_path_buf()));
    }
    let path = fs::canonicalize(requested_path).map_err(|source| LibraryError::Canonicalize {
        path: requested_path.to_path_buf(),
        source,
    })?;
    let before = fs::symlink_metadata(&path).map_err(|source| LibraryError::Metadata {
        path: path.clone(),
        source,
    })?;
    validate_file_metadata(&path, &before)?;

    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_CLOEXEC | O_NOFOLLOW)
        .open(&path)
        .map_err(|source| LibraryError::Open {
            path: path.clone(),
            source,
        })?;
    let after = file.metadata().map_err(|source| LibraryError::Metadata {
        path: path.clone(),
        source,
    })?;
    validate_file_metadata(&path, &after)?;
    if before.dev() != after.dev() || before.ino() != after.ino() {
        return Err(LibraryError::IdentityChanged(path));
    }
    let identity = read_elf_identity(&mut file, &path)?;
    if identity.object_type != ELF_TYPE_SHARED_OBJECT {
        return Err(LibraryError::InvalidElf {
            path,
            detail: "object-is-not-et-dyn",
        });
    }

    let target_path =
        fs::canonicalize(requested_target).map_err(|source| LibraryError::TargetExecutable {
            path: requested_target.to_path_buf(),
            source,
        })?;
    let target_before =
        fs::symlink_metadata(&target_path).map_err(|source| LibraryError::TargetExecutable {
            path: target_path.clone(),
            source,
        })?;
    validate_file_metadata(&target_path, &target_before)?;
    let mut target = fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_CLOEXEC | O_NOFOLLOW)
        .open(&target_path)
        .map_err(|source| LibraryError::TargetExecutable {
            path: target_path.clone(),
            source,
        })?;
    let target_after = target
        .metadata()
        .map_err(|source| LibraryError::TargetExecutable {
            path: target_path.clone(),
            source,
        })?;
    validate_file_metadata(&target_path, &target_after)?;
    if target_before.dev() != target_after.dev() || target_before.ino() != target_after.ino() {
        return Err(LibraryError::IdentityChanged(target_path));
    }
    let target_identity = read_elf_identity(&mut target, &target_path)?;
    if (identity.class, identity.data, identity.machine)
        != (
            target_identity.class,
            target_identity.data,
            target_identity.machine,
        )
    {
        return Err(LibraryError::AbiMismatch {
            library: path,
            library_identity: identity.render(),
            target: target_path,
            target_identity: target_identity.render(),
        });
    }

    Ok(LibraryEvidence {
        path,
        device: after.dev(),
        inode: after.ino(),
        bytes: after.len(),
        mode: after.mode(),
        uid: after.uid(),
        gid: after.gid(),
        mtime_seconds: after.mtime(),
        mtime_nanoseconds: after.mtime_nsec(),
        ctime_seconds: after.ctime(),
        ctime_nanoseconds: after.ctime_nsec(),
        identity,
        target_path,
        target_device: target_after.dev(),
        target_inode: target_after.ino(),
        target_bytes: target_after.len(),
        target_mode: target_after.mode(),
        target_uid: target_after.uid(),
        target_gid: target_after.gid(),
        target_identity,
    })
}

impl LibraryEvidence {
    pub fn event_fields(&self, event: Event) -> Event {
        event
            .encoded_path_field("caught_signal_library_path_hex", &self.path)
            .field("caught_signal_library_device", self.device)
            .field("caught_signal_library_inode", self.inode)
            .field("caught_signal_library_bytes", self.bytes)
            .field("caught_signal_library_mode", format!("{:o}", self.mode))
            .field("caught_signal_library_uid", self.uid)
            .field("caught_signal_library_gid", self.gid)
            .field("caught_signal_library_mtime_seconds", self.mtime_seconds)
            .field(
                "caught_signal_library_mtime_nanoseconds",
                self.mtime_nanoseconds,
            )
            .field("caught_signal_library_ctime_seconds", self.ctime_seconds)
            .field(
                "caught_signal_library_ctime_nanoseconds",
                self.ctime_nanoseconds,
            )
            .field("caught_signal_library_elf_class", self.identity.class)
            .field("caught_signal_library_elf_data", self.identity.data)
            .field("caught_signal_library_elf_type", self.identity.object_type)
            .field("caught_signal_library_elf_machine", self.identity.machine)
            .encoded_path_field("caught_signal_target_path_hex", &self.target_path)
            .field("caught_signal_target_device", self.target_device)
            .field("caught_signal_target_inode", self.target_inode)
            .field("caught_signal_target_bytes", self.target_bytes)
            .field(
                "caught_signal_target_mode",
                format!("{:o}", self.target_mode),
            )
            .field("caught_signal_target_uid", self.target_uid)
            .field("caught_signal_target_gid", self.target_gid)
            .field("caught_signal_target_elf_class", self.target_identity.class)
            .field("caught_signal_target_elf_data", self.target_identity.data)
            .field(
                "caught_signal_target_elf_type",
                self.target_identity.object_type,
            )
            .field(
                "caught_signal_target_elf_machine",
                self.target_identity.machine,
            )
    }
}

fn validate_file_metadata(path: &Path, metadata: &Metadata) -> Result<(), LibraryError> {
    if !metadata.file_type().is_file() {
        return Err(LibraryError::NotRegularFile(path.to_path_buf()));
    }
    if metadata.mode() & 0o6022 != 0 {
        return Err(LibraryError::UnsafePermissions {
            path: path.to_path_buf(),
            mode: metadata.mode(),
        });
    }
    Ok(())
}

fn read_elf_identity(file: &mut File, path: &Path) -> Result<ElfIdentity, LibraryError> {
    let mut header = [0_u8; ELF_HEADER_BYTES];
    let mut observed = 0;
    while observed < header.len() {
        match file.read(&mut header[observed..]) {
            Ok(0) => break,
            Ok(bytes) => observed += bytes,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(source) => {
                return Err(LibraryError::Read {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }
    if observed < 20 {
        return Err(LibraryError::InvalidElf {
            path: path.to_path_buf(),
            detail: "header-shorter-than-20-bytes",
        });
    }
    if &header[..4] != b"\x7fELF" {
        return Err(LibraryError::InvalidElf {
            path: path.to_path_buf(),
            detail: "magic-mismatch",
        });
    }
    let class = header[4];
    if !matches!(class, 1 | 2) {
        return Err(LibraryError::InvalidElf {
            path: path.to_path_buf(),
            detail: "unknown-class",
        });
    }
    let data = header[5];
    let decode = match data {
        1 => u16::from_le_bytes,
        2 => u16::from_be_bytes,
        _ => {
            return Err(LibraryError::InvalidElf {
                path: path.to_path_buf(),
                detail: "unknown-byte-order",
            });
        }
    };
    Ok(ElfIdentity {
        class,
        data,
        object_type: decode([header[16], header[17]]),
        machine: decode([header[18], header[19]]),
    })
}

impl ElfIdentity {
    fn render(self) -> String {
        format!(
            "class={},data={},type={},machine={}",
            self.class, self.data, self.object_type, self.machine
        )
    }
}

impl fmt::Display for LibraryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafePath(path) => write!(
                formatter,
                "library path must be absolute and contain no LD_PRELOAD separators: {}",
                path.display()
            ),
            Self::Canonicalize { path, source } => {
                write!(formatter, "canonicalize {}: {source}", path.display())
            }
            Self::Metadata { path, source } => {
                write!(formatter, "inspect {}: {source}", path.display())
            }
            Self::NotRegularFile(path) => {
                write!(formatter, "{} is not a regular file", path.display())
            }
            Self::UnsafePermissions { path, mode } => write!(
                formatter,
                "{} has group/world-write or set-ID mode {:o}",
                path.display(),
                mode
            ),
            Self::Open { path, source } => {
                write!(
                    formatter,
                    "open {} without following symlinks: {source}",
                    path.display()
                )
            }
            Self::IdentityChanged(path) => write!(
                formatter,
                "{} changed identity between metadata and open",
                path.display()
            ),
            Self::Read { path, source } => {
                write!(
                    formatter,
                    "read ELF header from {}: {source}",
                    path.display()
                )
            }
            Self::InvalidElf { path, detail } => {
                write!(
                    formatter,
                    "{} has invalid ELF identity: {detail}",
                    path.display()
                )
            }
            Self::TargetExecutable { path, source } => write!(
                formatter,
                "inspect caught-signal target executable {}: {source}",
                path.display()
            ),
            Self::AbiMismatch {
                library,
                library_identity,
                target,
                target_identity,
            } => write!(
                formatter,
                "library {} ({library_identity}) does not match target {} ({target_identity})",
                library.display(),
                target.display()
            ),
        }
    }
}

impl std::error::Error for LibraryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Canonicalize { source, .. }
            | Self::Metadata { source, .. }
            | Self::Open { source, .. }
            | Self::Read { source, .. }
            | Self::TargetExecutable { source, .. } => Some(source),
            Self::UnsafePath(_)
            | Self::NotRegularFile(_)
            | Self::UnsafePermissions { .. }
            | Self::IdentityChanged(_)
            | Self::InvalidElf { .. }
            | Self::AbiMismatch { .. } => None,
        }
    }
}
