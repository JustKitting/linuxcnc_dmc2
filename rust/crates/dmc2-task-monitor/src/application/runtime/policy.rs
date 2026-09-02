use crate::application::nml::PollDisposition;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PublicationPolicy {
    Live,
    WaitForFirstStatus,
    SafeKeepStatus,
    SafeDropStatus,
}

pub(super) fn publication_policy(
    status: Option<PollDisposition>,
    snapshot_valid: bool,
    error_channel_connected: bool,
) -> PublicationPolicy {
    match status {
        Some(PollDisposition::Snapshot) if snapshot_valid && error_channel_connected => {
            PublicationPolicy::Live
        }
        Some(PollDisposition::WaitingForFirstStatus) if error_channel_connected => {
            PublicationPolicy::WaitForFirstStatus
        }
        Some(PollDisposition::Snapshot) if snapshot_valid => PublicationPolicy::SafeKeepStatus,
        None | Some(PollDisposition::WaitingForFirstStatus) => PublicationPolicy::SafeKeepStatus,
        Some(PollDisposition::Snapshot | PollDisposition::Fault) => {
            PublicationPolicy::SafeDropStatus
        }
    }
}
