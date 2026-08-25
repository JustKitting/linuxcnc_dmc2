use dmc2_serial_bridge::{BridgeState, ProtocolError, MAX_SERIAL_LINE_BYTES};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::application) enum LineEvent {
    Pending,
    Empty,
    Accepted,
    Rejected(ProtocolError),
    DiscardedAfterOverlong,
}

impl LineEvent {
    pub(in crate::application) const fn requires_publish(self) -> bool {
        !matches!(self, Self::Pending)
    }
}

pub(in crate::application) struct LineAssembler {
    // The extra byte can hold the optional CR in a maximum-length CRLF frame.
    bytes: [u8; MAX_SERIAL_LINE_BYTES + 1],
    length: usize,
    discarding_overlong: bool,
}

impl LineAssembler {
    pub(in crate::application) const fn new() -> Self {
        Self {
            bytes: [0; MAX_SERIAL_LINE_BYTES + 1],
            length: 0,
            discarding_overlong: false,
        }
    }

    pub(in crate::application) fn consume(
        &mut self,
        byte: u8,
        state: &mut BridgeState,
        now_ns: u64,
    ) -> LineEvent {
        if byte == b'\n' {
            return self.finish(state, now_ns);
        }
        if self.discarding_overlong {
            return LineEvent::Pending;
        }

        // At MAX bytes, only a terminal CR can still produce a valid frame.
        if self.length == MAX_SERIAL_LINE_BYTES && byte != b'\r' {
            state.note_protocol_error();
            self.discarding_overlong = true;
            return LineEvent::Rejected(ProtocolError::OverlongLine);
        }
        if self.length < self.bytes.len() {
            self.bytes[self.length] = byte;
            self.length += 1;
            return LineEvent::Pending;
        }

        state.note_protocol_error();
        self.discarding_overlong = true;
        LineEvent::Rejected(ProtocolError::OverlongLine)
    }

    fn finish(&mut self, state: &mut BridgeState, now_ns: u64) -> LineEvent {
        let event = if self.discarding_overlong {
            LineEvent::DiscardedAfterOverlong
        } else {
            let payload_length =
                self.length - usize::from(self.length > 0 && self.bytes[self.length - 1] == b'\r');
            if payload_length > MAX_SERIAL_LINE_BYTES {
                state.note_protocol_error();
                LineEvent::Rejected(ProtocolError::OverlongLine)
            } else if payload_length == 0 {
                LineEvent::Empty
            } else {
                match state.accept_line(&self.bytes[..payload_length], now_ns) {
                    Ok(()) => LineEvent::Accepted,
                    Err(error) => LineEvent::Rejected(error),
                }
            }
        };
        self.length = 0;
        self.discarding_overlong = false;
        event
    }
}

#[cfg(test)]
mod tests;
