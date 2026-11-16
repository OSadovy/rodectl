//! Report framing, as pure functions over bytes.
//!
//! Every setting is a register. A request is one output report,
//! `<id> <register> 01` to read or `<id> <register> 00 <value…>` to write;
//! the reply arrives on another report as `<id> <register> <status>
//! <value…>`, where status `41` acknowledges and anything else refuses.

use std::fmt;

/// RØDE's USB vendor id.
pub const VENDOR_ID: u16 = 0x19F7;

/// The usage page of the vendor HID interface that carries settings.
pub const USAGE_PAGE: u16 = 0xFF00;

/// The status byte of an acknowledged request (`A`).
pub const ACK: u8 = 0x41;

/// The status byte the Wireless ME receiver gives when no transmitter is
/// linked to the slot asked about (`N`).
pub const REFUSED: u8 = 0x4E;

const READ: u8 = 0x01;
const WRITE: u8 = 0x00;

/// Device families with their own report numbers and lengths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Wireless ME receiver and transmitter, which share the Wireless GO II
    /// framing.
    Wireless,
    /// AI-Micro interface.
    AiMicro,
}

impl Family {
    /// The output report a request goes out on.
    #[must_use]
    pub const fn request_id(self) -> u8 {
        match self {
            Self::Wireless => 0x09,
            Self::AiMicro => 0x0A,
        }
    }

    /// The input report replies arrive on.
    #[must_use]
    pub const fn reply_id(self) -> u8 {
        match self {
            Self::Wireless => 0x0A,
            Self::AiMicro => 0x09,
        }
    }

    /// Length of the request report, report id included. Windows pads a
    /// write to the device's longest output report anyway; Linux sends what
    /// it is given.
    const fn request_len(self) -> usize {
        match self {
            Self::Wireless => 37,
            Self::AiMicro => 28,
        }
    }
}

/// Builds the request that reads `register`.
#[must_use]
pub fn read_request(family: Family, register: u8) -> Vec<u8> {
    request(family, &[register, READ])
}

/// Builds the request that writes `value` to `register`.
#[must_use]
pub fn write_request(family: Family, register: u8, value: &[u8]) -> Vec<u8> {
    request(family, &[&[register, WRITE], value].concat())
}

/// Builds a request whose third byte is an argument, not the read or write
/// flag: the receiver's relay registers take a transmitter register there.
#[must_use]
pub fn command_request(family: Family, register: u8, argument: u8, value: &[u8]) -> Vec<u8> {
    request(family, &[&[register, argument], value].concat())
}

fn request(family: Family, body: &[u8]) -> Vec<u8> {
    let mut report = vec![0; family.request_len()];
    report[0] = family.request_id();
    report[1..=body.len()].copy_from_slice(body);
    report
}

/// A device's answer to a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply<'a> {
    /// Acknowledged; the register's value follows, zero-padded.
    Ack(&'a [u8]),
    /// Refused with this status byte.
    Refused(u8),
}

/// Reads `report` as the reply to a request for `register`. Returns `None`
/// for any other report: every open handle receives every input report, so
/// replies to other programs' requests arrive too.
#[must_use]
pub fn parse_reply(family: Family, register: u8, report: &[u8]) -> Option<Reply<'_>> {
    match report {
        [id, reg, status, value @ ..] if *id == family.reply_id() && *reg == register => {
            Some(if *status == ACK {
                Reply::Ack(value)
            } else {
                Reply::Refused(*status)
            })
        }
        _ => None,
    }
}

/// Formats bytes as spaced hex, dropping the zero padding at the end but
/// keeping the three header bytes.
pub struct Hex<'a>(pub &'a [u8]);

impl fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let end = self
            .0
            .iter()
            .rposition(|&b| b != 0)
            .map_or(0, |i| i + 1)
            .max(self.0.len().min(3));
        for (i, byte) in self.0[..end].iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{byte:02X}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_padded_to_the_report_length() {
        let read = read_request(Family::Wireless, 0x16);
        assert_eq!(read.len(), 37);
        assert_eq!(read[..4], [0x09, 0x16, 0x01, 0x00]);

        let write = write_request(Family::AiMicro, 0x02, &[60]);
        assert_eq!(write.len(), 28);
        assert_eq!(write[..5], [0x0A, 0x02, 0x00, 60, 0x00]);

        let relay = command_request(Family::Wireless, 0x0D, 0x1A, &[0xFE]);
        assert_eq!(relay[..5], [0x09, 0x0D, 0x1A, 0xFE, 0x00]);
    }

    #[test]
    fn replies_match_only_their_register() {
        let report = [0x0A, 0x16, 0x41, 0x18, 0x16, 0x00];
        assert_eq!(
            parse_reply(Family::Wireless, 0x16, &report),
            Some(Reply::Ack(&[0x18, 0x16, 0x00]))
        );
        assert_eq!(parse_reply(Family::Wireless, 0x15, &report), None);
        assert_eq!(parse_reply(Family::AiMicro, 0x16, &report), None);
    }

    #[test]
    fn refusal_carries_its_status() {
        let report = [0x0A, 0x17, 0x4E, 0x00];
        assert_eq!(
            parse_reply(Family::Wireless, 0x17, &report),
            Some(Reply::Refused(REFUSED))
        );
    }

    #[test]
    fn hex_trims_padding() {
        assert_eq!(
            Hex(&[0x0A, 0x16, 0x41, 0x18, 0, 0]).to_string(),
            "0A 16 41 18"
        );
        assert_eq!(Hex(&[0x09, 0x00, 0x41, 0x00, 0x00]).to_string(), "09 00 41");
        assert_eq!(Hex(&[0x0A]).to_string(), "0A");
    }
}
