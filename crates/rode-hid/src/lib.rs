//! Control RØDE Wireless ME and AI-Micro devices over USB HID, without
//! RØDE Central.
//!
//! [`Hid`] finds devices and opens a [`Link`] to one; [`Receiver`],
//! [`Transmitter`] and [`AiMicro`] read and write its settings. Every write
//! is read back and compared. [`protocol`] builds and parses the raw
//! reports.
#![warn(missing_docs)]

mod ai_micro;
mod device;
mod error;
#[cfg(any(windows, target_os = "linux"))]
mod hid;
mod link;
pub mod protocol;
mod settings;
mod wireless;

pub use ai_micro::{AiMicro, AiMicroStatus, Input, InputStatus, MAX_MONITOR_LEVEL};
pub use device::{DeviceKind, Power};
pub use error::{Error, Result};
#[cfg(any(windows, target_os = "linux"))]
pub use hid::{DeviceInfo, Hid, HidTransport};
pub use link::{Link, Transport};
pub use settings::{ChannelMode, Firmware, GainAssist, LedBrightness, OutputMode};
pub use wireless::{
    Receiver, ReceiverStatus, Slot, SlotStatus, Transmitter, TransmitterStatus, TxStatus,
};
