use crate::Slot;

/// Result type of this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors from talking to a device.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The operating system's HID layer failed.
    #[error("device I/O: {0}")]
    Transport(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// No reply arrived for the register within the timeout.
    #[error("no reply for register {register:#04x}")]
    Timeout {
        /// Register asked about.
        register: u8,
    },

    /// The device refused the request.
    #[error("device refused register {register:#04x} (status {status:#04x})")]
    Refused {
        /// Register asked about.
        register: u8,
        /// Status byte in place of the acknowledgement.
        status: u8,
    },

    /// The receiver has no transmitter linked in this slot. The receiver
    /// cannot tell a transmitter that is off from one out of range, and
    /// refuses for a while after it powers on.
    #[error(
        "transmitter {slot} is not linked: switched off, out of range, or the receiver is still starting"
    )]
    NotLinked {
        /// Slot asked about.
        slot: Slot,
    },

    /// A reply too short for its register, or holding a value this crate
    /// does not know.
    #[error("unexpected value for register {register:#04x}: {detail}")]
    Unexpected {
        /// Register asked about.
        register: u8,
        /// What was wrong.
        detail: String,
    },

    /// A write was acknowledged, but reading the setting back gave another
    /// value.
    #[error("wrote {written}, but the device reads back {read}")]
    Mismatch {
        /// Value written.
        written: String,
        /// Value read back.
        read: String,
    },

    /// A value outside the setting's range.
    #[error("{0}")]
    OutOfRange(String),
}

impl Error {
    pub(crate) fn transport(err: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Transport(Box::new(err))
    }

    pub(crate) fn unexpected(register: u8, detail: impl Into<String>) -> Self {
        Self::Unexpected {
            register,
            detail: detail.into(),
        }
    }
}
