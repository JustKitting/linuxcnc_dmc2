use std::fmt;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::raw::{c_int, c_void};

use crate::event::{hex_bytes, Event};

const AF_UNIX: c_int = 1;
const SOCK_SEQPACKET: c_int = 5;
const SOCK_NONBLOCK: c_int = 0o4000;
const SOCK_CLOEXEC: c_int = 0o2_000_000;
const SOL_SOCKET: c_int = 1;
const SO_SNDBUF: c_int = 7;
const F_GETFD: c_int = 1;
const F_SETFD: c_int = 2;
const FD_CLOEXEC: c_int = 1;
const MSG_DONTWAIT: c_int = 0x40;
const MSG_NOSIGNAL: c_int = 0x4000;
const VALIDATION_PAYLOAD: [u8; 8] = *b"DMC2CHAN";

pub struct EvidenceChannel {
    pub reader: File,
    pub writer: OwnedFd,
    pub send_buffer: SendBufferEvidence,
}

#[derive(Debug)]
pub enum SendBufferEvidence {
    Captured(i32),
    QueryFailed {
        error_kind: io::ErrorKind,
        raw_os_error: Option<i32>,
        detail: String,
    },
    Invalid {
        returned_length: u32,
        returned_value: i32,
    },
}

#[derive(Debug)]
pub enum ChannelError {
    Create(io::Error),
    GetWriterFlags(io::Error),
    ClearWriterCloseOnExec(io::Error),
    ValidationSend(io::Error),
    ValidationSendLength(isize),
    ValidationReceive(io::Error),
    ValidationReceiveMismatch {
        received_bytes: isize,
        payload: [u8; VALIDATION_PAYLOAD.len()],
    },
}

pub fn evidence_channel() -> Result<EvidenceChannel, ChannelError> {
    let mut descriptors = [-1_i32; 2];
    // SAFETY: `descriptors` has space for exactly two descriptors. On success,
    // socketpair initializes both, and ownership is immediately transferred to
    // OwnedFd values exactly once.
    if unsafe {
        socketpair(
            AF_UNIX,
            SOCK_SEQPACKET | SOCK_NONBLOCK | SOCK_CLOEXEC,
            0,
            descriptors.as_mut_ptr(),
        )
    } != 0
    {
        return Err(ChannelError::Create(io::Error::last_os_error()));
    }
    // SAFETY: socketpair succeeded, so each descriptor is uniquely owned here
    // and is converted into an OwnedFd exactly once.
    let reader = unsafe { OwnedFd::from_raw_fd(descriptors[0]) };
    // SAFETY: same ownership argument as above for the write descriptor.
    let writer = unsafe { OwnedFd::from_raw_fd(descriptors[1]) };

    let writer_flags =
        retry_fcntl(writer.as_raw_fd(), F_GETFD, 0).map_err(ChannelError::GetWriterFlags)?;
    retry_fcntl(writer.as_raw_fd(), F_SETFD, writer_flags & !FD_CLOEXEC)
        .map_err(ChannelError::ClearWriterCloseOnExec)?;
    validate_channel(reader.as_raw_fd(), writer.as_raw_fd())?;
    let send_buffer = send_buffer_evidence(writer.as_raw_fd());

    Ok(EvidenceChannel {
        reader: File::from(reader),
        writer,
        send_buffer,
    })
}

fn send_buffer_evidence(descriptor: c_int) -> SendBufferEvidence {
    let mut value = 0_i32;
    let mut length = std::mem::size_of::<c_int>() as u32;
    // SAFETY: `value` and `length` are valid writable objects of the sizes
    // supplied to getsockopt, and no pointer is retained after the call.
    if unsafe {
        getsockopt(
            descriptor,
            SOL_SOCKET,
            SO_SNDBUF,
            (&mut value as *mut c_int).cast::<c_void>(),
            &mut length,
        )
    } != 0
    {
        let error = io::Error::last_os_error();
        return SendBufferEvidence::QueryFailed {
            error_kind: error.kind(),
            raw_os_error: error.raw_os_error(),
            detail: error.to_string(),
        };
    }
    if length as usize != std::mem::size_of::<c_int>() || value <= 0 {
        return SendBufferEvidence::Invalid {
            returned_length: length,
            returned_value: value,
        };
    }
    SendBufferEvidence::Captured(value)
}

fn validate_channel(reader: c_int, writer: c_int) -> Result<(), ChannelError> {
    let sent = retry_send(writer, &VALIDATION_PAYLOAD).map_err(ChannelError::ValidationSend)?;
    if sent != VALIDATION_PAYLOAD.len() as isize {
        return Err(ChannelError::ValidationSendLength(sent));
    }

    let mut observed = [0_u8; VALIDATION_PAYLOAD.len()];
    let received = retry_receive(reader, &mut observed).map_err(ChannelError::ValidationReceive)?;
    if received != VALIDATION_PAYLOAD.len() as isize || observed != VALIDATION_PAYLOAD {
        return Err(ChannelError::ValidationReceiveMismatch {
            received_bytes: received,
            payload: observed,
        });
    }
    Ok(())
}

fn retry_send(descriptor: c_int, payload: &[u8]) -> io::Result<isize> {
    loop {
        // SAFETY: `descriptor` is the live writer endpoint, and `payload`
        // remains readable for the duration of this call.
        let result = unsafe {
            send(
                descriptor,
                payload.as_ptr().cast::<c_void>(),
                payload.len(),
                MSG_DONTWAIT | MSG_NOSIGNAL,
            )
        };
        if result >= 0 {
            return Ok(result);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn retry_receive(descriptor: c_int, payload: &mut [u8]) -> io::Result<isize> {
    loop {
        // SAFETY: `descriptor` is the live reader endpoint, and `payload`
        // remains writable for the duration of this call.
        let result = unsafe {
            recv(
                descriptor,
                payload.as_mut_ptr().cast::<c_void>(),
                payload.len(),
                MSG_DONTWAIT,
            )
        };
        if result >= 0 {
            return Ok(result);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn retry_fcntl(descriptor: c_int, command: c_int, argument: c_int) -> io::Result<c_int> {
    loop {
        // SAFETY: `descriptor` is live for this call. These fcntl commands
        // accept an integer third argument and retain no pointers.
        let result = unsafe { fcntl(descriptor, command, argument) };
        if result >= 0 {
            return Ok(result);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

impl fmt::Display for ChannelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create(error) => write!(
                formatter,
                "create nonblocking Unix sequenced-packet evidence channel: {error}"
            ),
            Self::GetWriterFlags(error) => {
                write!(formatter, "read evidence-writer descriptor flags: {error}")
            }
            Self::ClearWriterCloseOnExec(error) => write!(
                formatter,
                "make evidence-writer descriptor inheritable for one exec: {error}"
            ),
            Self::ValidationSend(error) => {
                write!(formatter, "send evidence-channel validation packet: {error}")
            }
            Self::ValidationSendLength(bytes) => write!(
                formatter,
                "evidence-channel validation send returned {bytes} bytes, expected {}",
                VALIDATION_PAYLOAD.len()
            ),
            Self::ValidationReceive(error) => {
                write!(formatter, "receive evidence-channel validation packet: {error}")
            }
            Self::ValidationReceiveMismatch {
                received_bytes,
                payload,
            } => write!(
                formatter,
                "evidence-channel validation receive mismatch: bytes={received_bytes}, payload_hex={}",
                hex_bytes(payload)
            ),
        }
    }
}

impl std::error::Error for ChannelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Create(error)
            | Self::GetWriterFlags(error)
            | Self::ClearWriterCloseOnExec(error)
            | Self::ValidationSend(error)
            | Self::ValidationReceive(error) => Some(error),
            Self::ValidationSendLength(_) | Self::ValidationReceiveMismatch { .. } => None,
        }
    }
}

impl SendBufferEvidence {
    pub fn event_fields(&self, event: Event) -> Event {
        match self {
            Self::Captured(bytes) => event
                .field("caught_signal_channel_send_buffer_state", "captured")
                .field("caught_signal_channel_send_buffer_bytes", *bytes),
            Self::QueryFailed {
                error_kind,
                raw_os_error,
                detail,
            } => event
                .field("caught_signal_channel_send_buffer_state", "query-failed")
                .field(
                    "caught_signal_channel_send_buffer_error_kind",
                    format!("{error_kind:?}"),
                )
                .field(
                    "caught_signal_channel_send_buffer_raw_os_error",
                    raw_os_error.map_or_else(|| "NONE".to_owned(), |error| error.to_string()),
                )
                .field(
                    "caught_signal_channel_send_buffer_error_hex",
                    hex_bytes(detail.as_bytes()),
                ),
            Self::Invalid {
                returned_length,
                returned_value,
            } => event
                .field("caught_signal_channel_send_buffer_state", "invalid-result")
                .field(
                    "caught_signal_channel_send_buffer_returned_length",
                    *returned_length,
                )
                .field(
                    "caught_signal_channel_send_buffer_returned_value",
                    *returned_value,
                ),
        }
    }
}

unsafe extern "C" {
    fn socketpair(domain: c_int, socket_type: c_int, protocol: c_int, sockets: *mut c_int)
        -> c_int;
    fn getsockopt(
        socket: c_int,
        level: c_int,
        option: c_int,
        value: *mut c_void,
        length: *mut u32,
    ) -> c_int;
    fn send(socket: c_int, buffer: *const c_void, length: usize, flags: c_int) -> isize;
    fn recv(socket: c_int, buffer: *mut c_void, length: usize, flags: c_int) -> isize;
    fn fcntl(descriptor: c_int, command: c_int, ...) -> c_int;
}

#[cfg(test)]
mod tests {
    use std::thread;

    #[test]
    fn validates_real_channels_under_concurrent_creation() {
        let workers = (0..8)
            .map(|_| {
                thread::spawn(|| {
                    for _ in 0..128 {
                        let channel = super::evidence_channel()
                            .expect("create and loopback-validate real evidence channel");
                        drop(channel);
                    }
                })
            })
            .collect::<Vec<_>>();

        for worker in workers {
            worker.join().expect("channel-validation worker survives");
        }
    }
}
