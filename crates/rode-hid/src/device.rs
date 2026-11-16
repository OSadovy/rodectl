//! Device models.

use std::fmt;

use serde::Serialize;

use crate::protocol::Family;

/// A RØDE device model, by USB product id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceKind {
    /// Wireless ME receiver.
    WirelessMeRx,
    /// Wireless ME transmitter plugged in by USB.
    WirelessMeTx,
    /// AI-Micro interface.
    AiMicro,
    /// A RØDE device this crate has no register map for.
    Unsupported,
}

impl DeviceKind {
    /// The model with this USB product id.
    #[must_use]
    pub const fn from_product_id(product_id: u16) -> Self {
        match product_id {
            0x0046 => Self::WirelessMeRx,
            0x0044 => Self::WirelessMeTx,
            0x0023 => Self::AiMicro,
            _ => Self::Unsupported,
        }
    }

    /// The framing the model speaks, if supported.
    #[must_use]
    pub const fn family(self) -> Option<Family> {
        match self {
            Self::WirelessMeRx | Self::WirelessMeTx => Some(Family::Wireless),
            Self::AiMicro => Some(Family::AiMicro),
            Self::Unsupported => None,
        }
    }

    /// The model's name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::WirelessMeRx => "Wireless ME RX",
            Self::WirelessMeTx => "Wireless ME TX",
            Self::AiMicro => "AI-Micro",
            Self::Unsupported => "unsupported RØDE device",
        }
    }
}

impl fmt::Display for DeviceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Whether a Wireless ME receiver on USB is switched on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Power {
    /// Switched on: it is an audio device too.
    On,
    /// Switched off and charging: only its HID interface is present.
    Off,
    /// This platform does not tell.
    Unknown,
}

impl fmt::Display for Power {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Unknown => "unknown",
        })
    }
}
