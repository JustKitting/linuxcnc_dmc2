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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_snapshot_and_error_channel_combination_has_one_exact_policy() {
        for snapshot_valid in [false, true] {
            for error_connected in [false, true] {
                assert_eq!(
                    publication_policy(None, snapshot_valid, error_connected),
                    PublicationPolicy::SafeKeepStatus
                );
                assert_eq!(
                    publication_policy(
                        Some(PollDisposition::WaitingForFirstStatus),
                        snapshot_valid,
                        error_connected,
                    ),
                    if error_connected {
                        PublicationPolicy::WaitForFirstStatus
                    } else {
                        PublicationPolicy::SafeKeepStatus
                    }
                );
                assert_eq!(
                    publication_policy(
                        Some(PollDisposition::Fault),
                        snapshot_valid,
                        error_connected,
                    ),
                    PublicationPolicy::SafeDropStatus
                );
                assert_eq!(
                    publication_policy(
                        Some(PollDisposition::Snapshot),
                        snapshot_valid,
                        error_connected,
                    ),
                    if !snapshot_valid {
                        PublicationPolicy::SafeDropStatus
                    } else if error_connected {
                        PublicationPolicy::Live
                    } else {
                        PublicationPolicy::SafeKeepStatus
                    }
                );
            }
        }
    }
}
