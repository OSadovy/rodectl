//! Request and reply over a transport.

use std::time::{Duration, Instant};

use tracing::{debug, trace};

use crate::protocol::{self, Family, Hex, Reply};
use crate::{Error, Result};

/// Moves raw reports to and from one device.
pub trait Transport {
    /// Sends one output report, report id first.
    ///
    /// # Errors
    /// When the operating system fails the write.
    fn send(&mut self, report: &[u8]) -> Result<()>;

    /// Waits up to `timeout` for the next input report, report id first.
    /// Returns `None` when none arrived.
    ///
    /// # Errors
    /// When the operating system fails the read.
    fn receive(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>>;
}

impl<T: Transport + ?Sized> Transport for &mut T {
    fn send(&mut self, report: &[u8]) -> Result<()> {
        (**self).send(report)
    }

    fn receive(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        (**self).receive(timeout)
    }
}

/// Reads and writes registers of one device.
pub struct Link<T> {
    transport: T,
    family: Family,
    timeout: Duration,
}

/// Input reports discarded before a request, at most. The Windows HID driver
/// buffers 32 per handle by default.
const DRAIN_LIMIT: usize = 256;

impl<T: Transport> Link<T> {
    /// Wraps `transport`, waiting up to `timeout` for each reply.
    pub fn new(transport: T, family: Family, timeout: Duration) -> Self {
        Self {
            transport,
            family,
            timeout,
        }
    }

    /// The device family this link frames requests for.
    pub fn family(&self) -> Family {
        self.family
    }

    /// Reads `register`, returning its zero-padded value.
    ///
    /// # Errors
    /// On refusal, timeout or I/O failure.
    pub fn read(&mut self, register: u8) -> Result<Vec<u8>> {
        let request = protocol::read_request(self.family, register);
        self.exchange(register, &request)
    }

    /// Writes `value` to `register` and waits for the acknowledgement.
    ///
    /// # Errors
    /// On refusal, timeout or I/O failure.
    pub fn write(&mut self, register: u8, value: &[u8]) -> Result<()> {
        let request = protocol::write_request(self.family, register, value);
        self.exchange(register, &request).map(drop)
    }

    /// Sends a request whose third byte is `argument`, as the receiver's
    /// relay registers take, and waits for the acknowledgement.
    ///
    /// # Errors
    /// On refusal, timeout or I/O failure.
    pub fn command(&mut self, register: u8, argument: u8, value: &[u8]) -> Result<()> {
        let request = protocol::command_request(self.family, register, argument, value);
        self.exchange(register, &request).map(drop)
    }

    fn exchange(&mut self, register: u8, request: &[u8]) -> Result<Vec<u8>> {
        self.drain()?;
        debug!("send {}", Hex(request));
        self.transport.send(request)?;
        let deadline = Instant::now() + self.timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout { register });
            }
            let Some(report) = self.transport.receive(left)? else {
                continue;
            };
            match protocol::parse_reply(self.family, register, &report) {
                Some(Reply::Ack(value)) => {
                    debug!("recv {}", Hex(&report));
                    return Ok(value.to_vec());
                }
                Some(Reply::Refused(status)) => {
                    debug!("recv {}", Hex(&report));
                    return Err(Error::Refused { register, status });
                }
                None => trace!("skip {}", Hex(&report)),
            }
        }
    }

    /// Discards reports already queued, such as replies to RØDE Central's
    /// polls, so that a stale reply for the same register is not taken for
    /// ours.
    fn drain(&mut self) -> Result<()> {
        for _ in 0..DRAIN_LIMIT {
            match self.transport.receive(Duration::ZERO)? {
                Some(report) => trace!("drop {}", Hex(&report)),
                None => break,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::VecDeque;

    use super::*;

    /// Plays back input reports; each `send` releases the next batch.
    #[derive(Default)]
    pub(crate) struct Script {
        pub(crate) queued: VecDeque<Vec<u8>>,
        pub(crate) batches: VecDeque<Vec<Vec<u8>>>,
        pub(crate) sent: Vec<Vec<u8>>,
    }

    impl Script {
        pub(crate) fn new(batches: &[&[&[u8]]]) -> Self {
            Self {
                batches: batches
                    .iter()
                    .map(|batch| batch.iter().map(|r| r.to_vec()).collect())
                    .collect(),
                ..Self::default()
            }
        }

        pub(crate) fn sent_headers(&self) -> Vec<Vec<u8>> {
            self.sent
                .iter()
                .map(|r| {
                    let end = r
                        .iter()
                        .rposition(|&b| b != 0)
                        .map_or(3, |i| (i + 1).max(3));
                    r[..end].to_vec()
                })
                .collect()
        }
    }

    impl Transport for Script {
        fn send(&mut self, report: &[u8]) -> Result<()> {
            self.sent.push(report.to_vec());
            if let Some(batch) = self.batches.pop_front() {
                self.queued.extend(batch);
            }
            Ok(())
        }

        fn receive(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>> {
            let report = self.queued.pop_front();
            if report.is_none() {
                std::thread::sleep(timeout);
            }
            Ok(report)
        }
    }

    pub(crate) fn link(script: &mut Script, family: Family) -> Link<&mut Script> {
        Link::new(script, family, Duration::from_millis(20))
    }

    #[test]
    fn skips_replies_for_other_registers() {
        let mut script = Script::new(&[&[
            &[0x0A, 0x00, 0x41, 0x64],
            &[0x0B, 0x01, 0x02],
            &[0x0A, 0x16, 0x41, 0x18, 0x16],
        ]]);
        let value = link(&mut script, Family::Wireless).read(0x16).unwrap();
        assert_eq!(value, [0x18, 0x16]);
    }

    #[test]
    fn drops_reports_queued_before_the_request() {
        let mut script = Script::new(&[&[&[0x0A, 0x16, 0x41, 0x01, 0x16]]]);
        script.queued.push_back(vec![0x0A, 0x16, 0x41, 0x00, 0x16]);
        let value = link(&mut script, Family::Wireless).read(0x16).unwrap();
        assert_eq!(value[0], 0x01);
    }

    #[test]
    fn refusal_is_an_error() {
        let mut script = Script::new(&[&[&[0x0A, 0x0C, 0x4E, 0x00]]]);
        let err = link(&mut script, Family::Wireless).read(0x0C).unwrap_err();
        assert!(matches!(
            err,
            Error::Refused {
                register: 0x0C,
                status: 0x4E
            }
        ));
    }

    #[test]
    fn silence_times_out() {
        let mut script = Script::new(&[]);
        let err = link(&mut script, Family::AiMicro).read(0x02).unwrap_err();
        assert!(matches!(err, Error::Timeout { register: 0x02 }));
    }
}
