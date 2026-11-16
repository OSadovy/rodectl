//! AI-Micro interface. Input gain and headphone level are ordinary USB audio
//! volumes, not HID settings, so they are not here.

use std::fmt;

use serde::Serialize;

use crate::link::{Link, Transport};
use crate::settings::{self, ChannelMode, Switch, confirm};
use crate::{Error, Result};

const CHANNELS: u8 = 0;

/// One of the two microphone inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Input {
    /// Input 1.
    One,
    /// Input 2.
    Two,
}

impl Input {
    /// Both inputs, in order.
    pub const ALL: [Self; 2] = [Self::One, Self::Two];

    /// 1 or 2.
    #[must_use]
    pub const fn number(self) -> u8 {
        match self {
            Self::One => 1,
            Self::Two => 2,
        }
    }

    /// The input numbered 1 or 2.
    #[must_use]
    pub const fn from_number(number: u8) -> Option<Self> {
        match number {
            1 => Some(Self::One),
            2 => Some(Self::Two),
            _ => None,
        }
    }

    const fn monitor(self) -> u8 {
        match self {
            Self::One => 1,
            Self::Two => 5,
        }
    }

    const fn monitor_level(self) -> u8 {
        match self {
            Self::One => 2,
            Self::Two => 6,
        }
    }

    const fn low_cut(self) -> u8 {
        match self {
            Self::One => 3,
            Self::Two => 4,
        }
    }
}

impl fmt::Display for Input {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.number().fmt(f)
    }
}

impl Serialize for Input {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(self.number())
    }
}

/// The highest direct monitoring level.
pub const MAX_MONITOR_LEVEL: u8 = 100;

/// Everything readable on the AI-Micro.
#[derive(Debug, Clone, Serialize)]
pub struct AiMicroStatus {
    /// Channel mode.
    pub channels: ChannelMode,
    /// Both inputs.
    pub inputs: Vec<InputStatus>,
}

/// One input's settings.
#[derive(Debug, Clone, Serialize)]
pub struct InputStatus {
    /// Which input.
    pub input: Input,
    /// Whether direct monitoring is on.
    pub monitor: bool,
    /// Direct monitoring level, 0 to 100.
    pub monitor_level: u8,
    /// Whether the high-pass filter is on.
    pub low_cut: bool,
}

/// The AI-Micro.
pub struct AiMicro<T> {
    link: Link<T>,
}

impl<T: Transport> AiMicro<T> {
    /// Wraps a link to the AI-Micro.
    pub fn new(link: Link<T>) -> Self {
        Self { link }
    }

    /// Channel mode.
    ///
    /// # Errors
    /// On refusal, timeout, I/O failure or an unknown value.
    pub fn channels(&mut self) -> Result<ChannelMode> {
        let value = settings::byte(CHANNELS, &self.link.read(CHANNELS)?)?;
        ChannelMode::from_byte(CHANNELS, value)
    }

    /// Sets the channel mode, returning the value read back. Stereo disables
    /// input 2, so entering it turns input 2's monitoring off first and
    /// leaving it turns that back on, as RØDE Central does.
    ///
    /// # Errors
    /// On refusal, timeout, I/O failure or a read-back that differs.
    pub fn set_channels(&mut self, mode: ChannelMode) -> Result<ChannelMode> {
        let current = self.channels()?;
        let stereo = ChannelMode::Stereo;
        if mode == stereo && current != stereo {
            self.link.write(Input::Two.monitor(), &[0])?;
        }
        self.link.write(CHANNELS, &[mode.to_byte()])?;
        if current == stereo && mode != stereo {
            self.link.write(Input::Two.monitor(), &[1])?;
        }
        confirm(&mode, self.channels()?)
    }

    /// Whether direct monitoring of `input` is on.
    ///
    /// # Errors
    /// On refusal, timeout, I/O failure or an unknown value.
    pub fn monitor(&mut self, input: Input) -> Result<bool> {
        self.switch(input.monitor())
    }

    /// Switches direct monitoring of `input`, returning the value read back.
    ///
    /// # Errors
    /// On refusal, timeout, I/O failure or a read-back that differs.
    pub fn set_monitor(&mut self, input: Input, on: bool) -> Result<bool> {
        self.set_switch(input.monitor(), on)
    }

    /// Direct monitoring level of `input`, 0 to 100.
    ///
    /// # Errors
    /// On refusal, timeout or I/O failure.
    pub fn monitor_level(&mut self, input: Input) -> Result<u8> {
        let register = input.monitor_level();
        settings::byte(register, &self.link.read(register)?)
    }

    /// Sets the direct monitoring level of `input`, returning the value read
    /// back.
    ///
    /// # Errors
    /// [`Error::OutOfRange`] above 100; otherwise on refusal, timeout, I/O
    /// failure or a read-back that differs.
    pub fn set_monitor_level(&mut self, input: Input, level: u8) -> Result<u8> {
        if level > MAX_MONITOR_LEVEL {
            return Err(Error::OutOfRange(format!(
                "monitoring level {level} is above {MAX_MONITOR_LEVEL}"
            )));
        }
        self.link.write(input.monitor_level(), &[level])?;
        confirm(&level, self.monitor_level(input)?)
    }

    /// Whether the high-pass filter of `input` is on.
    ///
    /// # Errors
    /// On refusal, timeout, I/O failure or an unknown value.
    pub fn low_cut(&mut self, input: Input) -> Result<bool> {
        self.switch(input.low_cut())
    }

    /// Switches the high-pass filter of `input`, returning the value read
    /// back.
    ///
    /// # Errors
    /// On refusal, timeout, I/O failure or a read-back that differs.
    pub fn set_low_cut(&mut self, input: Input, on: bool) -> Result<bool> {
        self.set_switch(input.low_cut(), on)
    }

    /// Everything readable.
    ///
    /// # Errors
    /// On refusal, timeout, I/O failure or an unknown value.
    pub fn status(&mut self) -> Result<AiMicroStatus> {
        let channels = self.channels()?;
        let mut inputs = Vec::new();
        for input in Input::ALL {
            inputs.push(InputStatus {
                input,
                monitor: self.monitor(input)?,
                monitor_level: self.monitor_level(input)?,
                low_cut: self.low_cut(input)?,
            });
        }
        Ok(AiMicroStatus { channels, inputs })
    }

    fn switch(&mut self, register: u8) -> Result<bool> {
        settings::switch(register, &self.link.read(register)?)
    }

    fn set_switch(&mut self, register: u8, on: bool) -> Result<bool> {
        self.link.write(register, &[u8::from(on)])?;
        confirm(&Switch(on), Switch(self.switch(register)?)).map(|s| s.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::tests::{Script, link};
    use crate::protocol::Family;

    #[test]
    fn entering_stereo_turns_input_2_monitoring_off_first() {
        let mut script = Script::new(&[
            &[&[0x09, 0x00, 0x41, 0x01]],
            &[&[0x09, 0x05, 0x41]],
            &[&[0x09, 0x00, 0x41]],
            &[&[0x09, 0x00, 0x41, 0x02]],
        ]);
        let mode = AiMicro::new(link(&mut script, Family::AiMicro))
            .set_channels(ChannelMode::Stereo)
            .unwrap();
        assert_eq!(mode, ChannelMode::Stereo);
        assert_eq!(
            script.sent_headers(),
            [
                vec![0x0A, 0x00, 0x01],
                vec![0x0A, 0x05, 0x00],
                vec![0x0A, 0x00, 0x00, 0x02],
                vec![0x0A, 0x00, 0x01],
            ]
        );
    }

    #[test]
    fn leaving_stereo_turns_input_2_monitoring_back_on() {
        let mut script = Script::new(&[
            &[&[0x09, 0x00, 0x41, 0x02]],
            &[&[0x09, 0x00, 0x41]],
            &[&[0x09, 0x05, 0x41]],
            &[&[0x09, 0x00, 0x41, 0x00]],
        ]);
        AiMicro::new(link(&mut script, Family::AiMicro))
            .set_channels(ChannelMode::Merged)
            .unwrap();
        assert_eq!(
            script.sent_headers()[1..3],
            [vec![0x0A, 0x00, 0x00], vec![0x0A, 0x05, 0x00, 0x01]]
        );
    }

    #[test]
    fn monitoring_level_above_100_is_not_sent() {
        let mut script = Script::new(&[]);
        let err = AiMicro::new(link(&mut script, Family::AiMicro))
            .set_monitor_level(Input::Two, 101)
            .unwrap_err();
        assert!(matches!(err, Error::OutOfRange(_)));
        assert!(script.sent.is_empty());
    }
}
