//! Wireless ME receiver and transmitter.

use std::fmt;

use serde::Serialize;

use crate::link::{Link, Transport};
use crate::settings::{self, Firmware, GainAssist, LedBrightness, OutputMode, Switch, confirm};
use crate::{Error, Power, Result};

/// Registers. Those marked "+ slot" take the transmitter slot's index.
mod reg {
    pub const BATTERY: u8 = 0x00;
    pub const NAME: u8 = 0x03;
    pub const MERGED: u8 = 0x06;
    /// Transmitter status block, + slot.
    pub const TX_STATUS: u8 = 0x0B;
    /// LED brightness on a transmitter addressed directly.
    pub const TX_LED: u8 = 0x0C;
    /// Relay: the request's third byte names a transmitter register to
    /// write, + slot.
    pub const RELAY: u8 = 0x0D;
    pub const RX_LED: u8 = 0x11;
    pub const ME_MIC: u8 = 0x12;
    /// Always the inverse of `MERGED`.
    pub const SPLIT: u8 = 0x14;
    /// GainAssist of the unit's own microphone.
    pub const GAIN_ASSIST: u8 = 0x15;
    /// GainAssist of a linked transmitter, + slot.
    pub const TX_GAIN_ASSIST: u8 = 0x16;
    /// The transmitter's LED brightness as numbered through the relay.
    pub const RELAY_LED: u8 = 0x1A;
    /// Writing 0 reloads the receiver's record of a transmitter, + slot.
    /// Refused when none is linked.
    pub const TX_REFRESH: u8 = 0x1A;
}

/// One of the receiver's two transmitter slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    /// TX1.
    Tx1,
    /// TX2.
    Tx2,
}

impl Slot {
    /// Both slots, in order.
    pub const ALL: [Self; 2] = [Self::Tx1, Self::Tx2];

    /// 1 or 2.
    #[must_use]
    pub const fn number(self) -> u8 {
        self.index() + 1
    }

    const fn index(self) -> u8 {
        match self {
            Self::Tx1 => 0,
            Self::Tx2 => 1,
        }
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.number().fmt(f)
    }
}

impl Serialize for Slot {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(self.number())
    }
}

/// A linked transmitter, as the receiver records it. The receiver does not
/// refresh the record by itself, so [`Receiver::transmitter`] asks it to
/// first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TxStatus {
    /// Serial number, as in the transmitter's USB instance id.
    pub serial: String,
    /// Firmware version.
    pub firmware: Firmware,
    /// Battery level, raw. Its scale is not known: RØDE Central showed 64
    /// as "Around 90%".
    pub battery_raw: u8,
    /// LED brightness.
    pub led: LedBrightness,
    /// GainAssist.
    pub gain_assist: GainAssist,
}

impl TxStatus {
    const LEN: usize = 28;

    /// Decodes the status block, the value of register 0x0B + slot.
    ///
    /// # Errors
    /// When the block is short or holds an unknown LED value.
    pub fn parse(block: &[u8]) -> Result<Self> {
        let register = reg::TX_STATUS;
        if block.len() < Self::LEN {
            return Err(Error::unexpected(
                register,
                format!("status block of {} bytes", block.len()),
            ));
        }
        let serial = u32::from_le_bytes([block[0], block[1], block[2], block[3]]);
        Ok(Self {
            serial: format!("{serial:08X}"),
            firmware: Firmware(u16::from_le_bytes([block[8], block[9]])),
            led: LedBrightness::from_byte(register, block[18])?,
            battery_raw: block[22],
            gain_assist: GainAssist::from_byte(block[25]),
        })
    }
}

/// Everything readable on the receiver. Fields it refused are `None`.
#[derive(Debug, Clone, Serialize)]
pub struct ReceiverStatus {
    /// Battery level, raw; see [`TxStatus::battery_raw`].
    pub battery_raw: u8,
    /// Device name; empty unless one was set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whether the receiver's own microphone is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub me_mic: Option<bool>,
    /// GainAssist of the receiver's own microphone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gain_assist: Option<GainAssist>,
    /// Merged or split output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<OutputMode>,
    /// LED brightness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub led: Option<LedBrightness>,
    /// Both transmitter slots; empty while the receiver is switched off.
    pub transmitters: Vec<SlotStatus>,
}

/// One transmitter slot.
#[derive(Debug, Clone, Serialize)]
pub struct SlotStatus {
    /// Which slot.
    pub slot: Slot,
    /// Whether a transmitter is linked.
    pub linked: bool,
    /// The linked transmitter.
    #[serde(flatten)]
    pub transmitter: Option<TxStatus>,
}

/// The Wireless ME receiver, and the transmitters linked to it.
pub struct Receiver<T> {
    link: Link<T>,
}

impl<T: Transport> Receiver<T> {
    /// Wraps a link to the receiver.
    pub fn new(link: Link<T>) -> Self {
        Self { link }
    }

    /// Battery level, raw.
    ///
    /// # Errors
    /// On refusal, timeout or I/O failure.
    pub fn battery_raw(&mut self) -> Result<u8> {
        settings::byte(reg::BATTERY, &self.link.read(reg::BATTERY)?)
    }

    /// Device name.
    ///
    /// # Errors
    /// On refusal, timeout or I/O failure.
    pub fn name(&mut self) -> Result<String> {
        settings::name(reg::NAME, &self.link.read(reg::NAME)?)
    }

    /// Whether the receiver's own microphone is on.
    ///
    /// # Errors
    /// On refusal, timeout or I/O failure.
    pub fn me_mic(&mut self) -> Result<bool> {
        settings::switch(reg::ME_MIC, &self.link.read(reg::ME_MIC)?)
    }

    /// Switches the receiver's own microphone, returning the value read back.
    ///
    /// # Errors
    /// On refusal, timeout, I/O failure or a read-back that differs.
    pub fn set_me_mic(&mut self, on: bool) -> Result<bool> {
        self.link.write(reg::ME_MIC, &[u8::from(on)])?;
        confirm(&Switch(on), Switch(self.me_mic()?)).map(|s| s.0)
    }

    /// GainAssist of the receiver's own microphone.
    ///
    /// # Errors
    /// On refusal, timeout or I/O failure.
    pub fn gain_assist(&mut self) -> Result<GainAssist> {
        let value = self.link.read(reg::GAIN_ASSIST)?;
        Ok(GainAssist::from_byte(settings::byte(
            reg::GAIN_ASSIST,
            &value,
        )?))
    }

    /// Sets GainAssist of the receiver's own microphone, returning the value
    /// read back.
    ///
    /// # Errors
    /// On refusal, timeout, I/O failure or a read-back that differs.
    pub fn set_gain_assist(&mut self, mode: GainAssist) -> Result<GainAssist> {
        self.link.write(reg::GAIN_ASSIST, &[mode.to_byte()])?;
        confirm(&mode, self.gain_assist()?)
    }

    /// Merged or split output.
    ///
    /// # Errors
    /// On refusal, timeout or I/O failure.
    pub fn output(&mut self) -> Result<OutputMode> {
        let merged = settings::switch(reg::MERGED, &self.link.read(reg::MERGED)?)?;
        Ok(if merged {
            OutputMode::Merged
        } else {
            OutputMode::Split
        })
    }

    /// Sets merged or split output, returning the value read back.
    ///
    /// # Errors
    /// On refusal, timeout, I/O failure or a read-back that differs.
    pub fn set_output(&mut self, mode: OutputMode) -> Result<OutputMode> {
        let merged = mode == OutputMode::Merged;
        self.link.write(reg::MERGED, &[u8::from(merged)])?;
        self.link.write(reg::SPLIT, &[u8::from(!merged)])?;
        confirm(&mode, self.output()?)
    }

    /// The receiver's LED brightness.
    ///
    /// # Errors
    /// On refusal, timeout, I/O failure or an unknown value.
    pub fn led(&mut self) -> Result<LedBrightness> {
        let value = settings::byte(reg::RX_LED, &self.link.read(reg::RX_LED)?)?;
        LedBrightness::from_byte(reg::RX_LED, value)
    }

    /// Sets the receiver's LED brightness, returning the value read back.
    ///
    /// # Errors
    /// On refusal, timeout, I/O failure or a read-back that differs.
    pub fn set_led(&mut self, brightness: LedBrightness) -> Result<LedBrightness> {
        self.link.write(reg::RX_LED, &[brightness.to_byte()])?;
        confirm(&brightness, self.led()?)
    }

    /// The transmitter linked in `slot`, or `None` when none is, after
    /// refreshing the receiver's record of it.
    ///
    /// # Errors
    /// On timeout, I/O failure or a malformed status block.
    pub fn transmitter(&mut self, slot: Slot) -> Result<Option<TxStatus>> {
        if self.refresh(slot)? {
            self.recorded_transmitter(slot)
        } else {
            Ok(None)
        }
    }

    /// Has the receiver reload its record of the transmitter in `slot`, as
    /// RØDE Central does every 30 s. Returns `false` when none is linked.
    ///
    /// # Errors
    /// On timeout or I/O failure.
    pub fn refresh(&mut self, slot: Slot) -> Result<bool> {
        match self.link.write(reg::TX_REFRESH + slot.index(), &[0]) {
            Ok(()) => Ok(true),
            Err(Error::Refused { .. }) => Ok(false),
            Err(err) => Err(err),
        }
    }

    /// The transmitter linked in `slot` as the receiver last recorded it,
    /// or `None` when none is. Only GainAssist is current; see
    /// [`Receiver::refresh`].
    ///
    /// # Errors
    /// On timeout, I/O failure or a malformed status block.
    pub fn recorded_transmitter(&mut self, slot: Slot) -> Result<Option<TxStatus>> {
        let mut status = match self.link.read(reg::TX_STATUS + slot.index()) {
            Ok(block) => TxStatus::parse(&block)?,
            Err(Error::Refused { .. }) => return Ok(None),
            Err(err) => return Err(err),
        };
        status.gain_assist = self.tx_gain_assist(slot)?;
        Ok(Some(status))
    }

    /// GainAssist of the transmitter in `slot`.
    ///
    /// # Errors
    /// [`Error::NotLinked`] when no transmitter is linked; otherwise on
    /// timeout or I/O failure.
    pub fn tx_gain_assist(&mut self, slot: Slot) -> Result<GainAssist> {
        let register = reg::TX_GAIN_ASSIST + slot.index();
        let value = self.link.read(register).map_err(|e| not_linked(e, slot))?;
        Ok(GainAssist::from_byte(settings::byte(register, &value)?))
    }

    /// Sets GainAssist of the transmitter in `slot`, returning the value
    /// read back. The receiver passes it on to the transmitter within two
    /// seconds.
    ///
    /// # Errors
    /// [`Error::NotLinked`] when no transmitter is linked; otherwise on
    /// timeout, I/O failure or a read-back that differs.
    pub fn set_tx_gain_assist(&mut self, slot: Slot, mode: GainAssist) -> Result<GainAssist> {
        let register = reg::TX_GAIN_ASSIST + slot.index();
        self.link
            .write(register, &[mode.to_byte()])
            .map_err(|e| not_linked(e, slot))?;
        confirm(&mode, self.tx_gain_assist(slot)?)
    }

    /// Sets the LED brightness of the transmitter in `slot`, returning the
    /// value read back from its status block.
    ///
    /// # Errors
    /// [`Error::NotLinked`] when no transmitter is linked; otherwise on
    /// timeout, I/O failure or a read-back that differs.
    pub fn set_tx_led(&mut self, slot: Slot, brightness: LedBrightness) -> Result<LedBrightness> {
        self.link
            .command(
                reg::RELAY + slot.index(),
                reg::RELAY_LED,
                &[brightness.to_byte()],
            )
            .map_err(|e| not_linked(e, slot))?;
        let status = self.transmitter(slot)?.ok_or(Error::NotLinked { slot })?;
        confirm(&brightness, status.led)
    }

    /// Everything readable, refreshing the transmitters' records first if
    /// `refresh`. A receiver that is switched off but on USB is asked only
    /// for its battery, and lists no transmitters.
    ///
    /// # Errors
    /// On timeout, I/O failure or an unknown value.
    pub fn status(&mut self, power: Power, refresh: bool) -> Result<ReceiverStatus> {
        let battery_raw = self.battery_raw()?;
        if power == Power::Off {
            return Ok(ReceiverStatus {
                battery_raw,
                name: None,
                me_mic: None,
                gain_assist: None,
                output: None,
                led: None,
                transmitters: Vec::new(),
            });
        }
        let mut transmitters = Vec::new();
        for slot in Slot::ALL {
            let transmitter = if refresh {
                self.transmitter(slot)?
            } else {
                self.recorded_transmitter(slot)?
            };
            transmitters.push(SlotStatus {
                slot,
                linked: transmitter.is_some(),
                transmitter,
            });
        }
        Ok(ReceiverStatus {
            battery_raw,
            name: unless_refused(self.name())?,
            me_mic: unless_refused(self.me_mic())?,
            gain_assist: unless_refused(self.gain_assist())?,
            output: unless_refused(self.output())?,
            led: unless_refused(self.led())?,
            transmitters,
        })
    }
}

/// Everything readable on a transmitter plugged in by USB.
#[derive(Debug, Clone, Serialize)]
pub struct TransmitterStatus {
    /// Battery level, raw; see [`TxStatus::battery_raw`].
    pub battery_raw: u8,
    /// Device name; empty unless one was set.
    pub name: String,
    /// LED brightness.
    pub led: LedBrightness,
    /// GainAssist.
    pub gain_assist: GainAssist,
}

/// A Wireless ME transmitter plugged in by USB.
pub struct Transmitter<T> {
    link: Link<T>,
}

impl<T: Transport> Transmitter<T> {
    /// Wraps a link to the transmitter.
    pub fn new(link: Link<T>) -> Self {
        Self { link }
    }

    /// Battery level, raw.
    ///
    /// # Errors
    /// On refusal, timeout or I/O failure.
    pub fn battery_raw(&mut self) -> Result<u8> {
        settings::byte(reg::BATTERY, &self.link.read(reg::BATTERY)?)
    }

    /// GainAssist.
    ///
    /// # Errors
    /// On refusal, timeout or I/O failure.
    pub fn gain_assist(&mut self) -> Result<GainAssist> {
        let value = self.link.read(reg::GAIN_ASSIST)?;
        Ok(GainAssist::from_byte(settings::byte(
            reg::GAIN_ASSIST,
            &value,
        )?))
    }

    /// Sets GainAssist, returning the value read back.
    ///
    /// # Errors
    /// On refusal, timeout, I/O failure or a read-back that differs.
    pub fn set_gain_assist(&mut self, mode: GainAssist) -> Result<GainAssist> {
        self.link.write(reg::GAIN_ASSIST, &[mode.to_byte()])?;
        confirm(&mode, self.gain_assist()?)
    }

    /// LED brightness.
    ///
    /// # Errors
    /// On refusal, timeout, I/O failure or an unknown value.
    pub fn led(&mut self) -> Result<LedBrightness> {
        let value = settings::byte(reg::TX_LED, &self.link.read(reg::TX_LED)?)?;
        LedBrightness::from_byte(reg::TX_LED, value)
    }

    /// Sets the LED brightness, returning the value read back.
    ///
    /// # Errors
    /// On refusal, timeout, I/O failure or a read-back that differs.
    pub fn set_led(&mut self, brightness: LedBrightness) -> Result<LedBrightness> {
        self.link.write(reg::TX_LED, &[brightness.to_byte()])?;
        confirm(&brightness, self.led()?)
    }

    /// Everything readable.
    ///
    /// # Errors
    /// On refusal, timeout, I/O failure or an unknown value.
    pub fn status(&mut self) -> Result<TransmitterStatus> {
        Ok(TransmitterStatus {
            battery_raw: self.battery_raw()?,
            name: settings::name(reg::NAME, &self.link.read(reg::NAME)?)?,
            led: self.led()?,
            gain_assist: self.gain_assist()?,
        })
    }
}

fn not_linked(err: Error, slot: Slot) -> Error {
    match err {
        Error::Refused { .. } => Error::NotLinked { slot },
        other => other,
    }
}

fn unless_refused<V>(result: Result<V>) -> Result<Option<V>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(Error::Refused { .. }) => Ok(None),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::tests::{Script, link};
    use crate::protocol::Family;

    /// TX1's status block as a receiver returned it, with the serial changed.
    const BLOCK: &[u8] = &[
        0x0A, 0x0B, 0x41, 0x45, 0x23, 0x01, 0x80, 0x2B, 0x01, 0xFB, 0x6A, 0x47, 0x01, 0x01, 0x00,
        0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0xDF, 0x00, 0x01, 0x01, 0x30, 0x44, 0x00, 0x18, 0x00,
        0x16,
    ];

    #[test]
    fn status_block_decodes() {
        let status = TxStatus::parse(&BLOCK[3..]).unwrap();
        assert_eq!(
            status,
            TxStatus {
                serial: "80012345".into(),
                firmware: Firmware(0x0147),
                battery_raw: 0x30,
                led: LedBrightness::Bright,
                gain_assist: GainAssist::Off,
            }
        );
    }

    #[test]
    fn transmitter_gain_assist_comes_from_its_register() {
        let mut script = Script::new(&[
            &[&[0x0A, 0x1A, 0x41]],
            &[BLOCK],
            &[&[0x0A, 0x16, 0x41, 0x00, 0x16]],
        ]);
        let status = Receiver::new(link(&mut script, Family::Wireless))
            .transmitter(Slot::Tx1)
            .unwrap()
            .unwrap();
        assert_eq!(status.gain_assist, GainAssist::Auto);
        assert_eq!(
            script.sent_headers(),
            [
                vec![0x09, 0x1A, 0x00],
                vec![0x09, 0x0B, 0x01],
                vec![0x09, 0x16, 0x01]
            ]
        );
    }

    #[test]
    fn refused_slot_is_unlinked() {
        let mut script = Script::new(&[&[&[0x0A, 0x1B, 0x4E]], &[&[0x0A, 0x17, 0x4E]]]);
        let mut rx = Receiver::new(link(&mut script, Family::Wireless));
        assert_eq!(rx.transmitter(Slot::Tx2).unwrap(), None);
        assert!(matches!(
            rx.tx_gain_assist(Slot::Tx2),
            Err(Error::NotLinked { slot: Slot::Tx2 })
        ));
    }

    #[test]
    fn gain_assist_write_is_read_back() {
        let mut script = Script::new(&[&[&[0x0A, 0x16, 0x41]], &[&[0x0A, 0x16, 0x41, 0x01, 0x16]]]);
        let mode = Receiver::new(link(&mut script, Family::Wireless))
            .set_tx_gain_assist(Slot::Tx1, GainAssist::Dynamic)
            .unwrap();
        assert_eq!(mode, GainAssist::Dynamic);
        assert_eq!(
            script.sent_headers(),
            [vec![0x09, 0x16, 0x00, 0x01], vec![0x09, 0x16, 0x01]]
        );
    }

    #[test]
    fn read_back_that_differs_is_an_error() {
        let mut script = Script::new(&[&[&[0x0A, 0x16, 0x41]], &[&[0x0A, 0x16, 0x41, 0x18, 0x16]]]);
        let err = Receiver::new(link(&mut script, Family::Wireless))
            .set_tx_gain_assist(Slot::Tx1, GainAssist::Auto)
            .unwrap_err();
        assert!(matches!(err, Error::Mismatch { .. }));
    }

    #[test]
    fn transmitter_led_goes_through_the_relay() {
        let mut script = Script::new(&[&[&[0x0A, 0x0E, 0x41, 0x1A]], &[&[0x0A, 0x1B, 0x4E]]]);
        let err = Receiver::new(link(&mut script, Family::Wireless))
            .set_tx_led(Slot::Tx2, LedBrightness::Dim)
            .unwrap_err();
        assert!(matches!(err, Error::NotLinked { slot: Slot::Tx2 }));
        assert_eq!(script.sent_headers()[0], [0x09, 0x0E, 0x1A, 0xFE]);
    }
}
