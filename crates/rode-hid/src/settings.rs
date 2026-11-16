//! Setting values and their register encodings.

use std::fmt;

use serde::Serialize;

use crate::{Error, Result};

macro_rules! names {
    ($ty:ty { $($variant:ident => $name:literal),+ $(,)? }) => {
        impl $ty {
            /// The setting's name as the command line and JSON spell it.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                }
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

/// GainAssist, RØDE's automatic level control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum GainAssist {
    /// Automatic.
    Auto,
    /// Dynamic.
    Dynamic,
    /// Off.
    Off,
}

names!(GainAssist { Auto => "auto", Dynamic => "dynamic", Off => "off" });

impl GainAssist {
    /// Decodes the first byte of a GainAssist register. Off is stored as
    /// 2 + a manual gain in dB, which the Wireless ME ignores.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Self {
        match byte {
            0 => Self::Auto,
            1 => Self::Dynamic,
            _ => Self::Off,
        }
    }

    /// The byte to write. Off is written with no manual gain.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        match self {
            Self::Auto => 0,
            Self::Dynamic => 1,
            Self::Off => 2,
        }
    }
}

/// How the Wireless ME receiver puts two transmitters on its output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum OutputMode {
    /// Both transmitters mixed on both channels.
    Merged,
    /// One transmitter per channel.
    Split,
}

names!(OutputMode { Merged => "merged", Split => "split" });

/// How the AI-Micro routes its two inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum ChannelMode {
    /// Both inputs mixed on both channels.
    Merged,
    /// One input per channel.
    Split,
    /// A stereo microphone on input 1; input 2 is disabled.
    Stereo,
}

names!(ChannelMode { Merged => "merged", Split => "split", Stereo => "stereo" });

impl ChannelMode {
    pub(crate) fn from_byte(register: u8, byte: u8) -> Result<Self> {
        match byte {
            0 => Ok(Self::Merged),
            1 => Ok(Self::Split),
            2 => Ok(Self::Stereo),
            _ => Err(Error::unexpected(register, format!("channel mode {byte}"))),
        }
    }

    pub(crate) const fn to_byte(self) -> u8 {
        match self {
            Self::Merged => 0,
            Self::Split => 1,
            Self::Stereo => 2,
        }
    }
}

/// Brightness of a Wireless ME unit's LEDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum LedBrightness {
    /// Bright.
    Bright,
    /// Dim.
    Dim,
}

names!(LedBrightness { Bright => "bright", Dim => "dim" });

impl LedBrightness {
    pub(crate) fn from_byte(register: u8, byte: u8) -> Result<Self> {
        match byte {
            0xDF => Ok(Self::Bright),
            0xFE => Ok(Self::Dim),
            _ => Err(Error::unexpected(
                register,
                format!("LED brightness {byte:#04x}"),
            )),
        }
    }

    pub(crate) const fn to_byte(self) -> u8 {
        match self {
            Self::Bright => 0xDF,
            Self::Dim => 0xFE,
        }
    }
}

/// A firmware version, stored as two BCD-like bytes: 0x0147 is 1.47.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Firmware(pub u16);

impl fmt::Display for Firmware {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [major, minor] = self.0.to_be_bytes();
        write!(f, "{major:x}.{minor:02x}")
    }
}

impl Serialize for Firmware {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

pub(crate) fn byte(register: u8, value: &[u8]) -> Result<u8> {
    value
        .first()
        .copied()
        .ok_or_else(|| Error::unexpected(register, "empty reply"))
}

pub(crate) fn switch(register: u8, value: &[u8]) -> Result<bool> {
    match byte(register, value)? {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(Error::unexpected(register, format!("switch value {other}"))),
    }
}

/// Decodes a NUL-padded UTF-8 name.
pub(crate) fn name(register: u8, value: &[u8]) -> Result<String> {
    let end = value.iter().position(|&b| b == 0).unwrap_or(value.len());
    String::from_utf8(value[..end].to_vec())
        .map_err(|_| Error::unexpected(register, "name is not UTF-8"))
}

/// Displays as `on` or `off`, for [`confirm`].
#[derive(PartialEq)]
pub(crate) struct Switch(pub(crate) bool);

impl fmt::Display for Switch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0 { "on" } else { "off" })
    }
}

/// Fails with [`Error::Mismatch`] unless the read-back equals what was
/// written.
pub(crate) fn confirm<V: PartialEq + fmt::Display>(written: &V, read: V) -> Result<V> {
    if *written == read {
        Ok(read)
    } else {
        Err(Error::Mismatch {
            written: written.to_string(),
            read: read.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_assist_off_carries_a_gain() {
        assert_eq!(GainAssist::from_byte(0x18), GainAssist::Off);
        assert_eq!(GainAssist::from_byte(2), GainAssist::Off);
        assert_eq!(GainAssist::from_byte(1), GainAssist::Dynamic);
    }

    #[test]
    fn firmware_reads_as_its_hex_digits() {
        assert_eq!(Firmware(0x0147).to_string(), "1.47");
        assert_eq!(Firmware(0x0122).to_string(), "1.22");
    }

    #[test]
    fn name_stops_at_padding() {
        assert_eq!(name(3, b"Lex\0\0\0").unwrap(), "Lex");
        assert_eq!(name(3, &[0, 0]).unwrap(), "");
    }
}
