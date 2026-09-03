use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::application::nml::{PollCodes, PollDisposition, StatusChannel, TransportStatus};
use crate::snapshot::NativeSnapshot;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    PathContainsNul(PathBuf),
    Open { nml_error: i32, cms_status: i32 },
    Poll { nml_error: i32, cms_status: i32 },
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PathContainsNul(path) => {
                write!(formatter, "NML path contains NUL: {}", path.display())
            }
            Self::Open {
                nml_error,
                cms_status,
            } => write!(
                formatter,
                "TASK_HEARTBEAT_CHANNEL_OPEN_FAILED: nml_error={nml_error} cms_status={cms_status}"
            ),
            Self::Poll {
                nml_error,
                cms_status,
            } => write!(
                formatter,
                "TASK_HEARTBEAT_CHANNEL_POLL_FAILED: nml_error={nml_error} cms_status={cms_status}"
            ),
        }
    }
}

pub struct Channel {
    inner: StatusChannel,
    codes: PollCodes,
}

impl Channel {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| Error::PathContainsNul(path.to_path_buf()))?;
        let codes = PollCodes::required();
        let (inner, transport) = StatusChannel::open(&path, codes);
        if !transport.healthy_after_open(codes) {
            return Err(open_error(transport));
        }
        let inner = inner.ok_or_else(|| open_error(transport))?;
        Ok(Self { inner, codes })
    }

    pub fn poll(&mut self) -> Result<Option<u32>, Error> {
        let mut snapshot = NativeSnapshot::safe();
        let outcome = self.inner.poll(&mut snapshot, self.codes);
        match outcome.disposition {
            PollDisposition::Snapshot => Ok(Some(snapshot.task.heartbeat)),
            PollDisposition::WaitingForFirstStatus => Ok(None),
            PollDisposition::Fault => Err(Error::Poll {
                nml_error: outcome.transport.nml_error,
                cms_status: outcome.transport.cms_status,
            }),
        }
    }
}

fn open_error(transport: TransportStatus) -> Error {
    Error::Open {
        nml_error: transport.nml_error,
        cms_status: transport.cms_status,
    }
}
