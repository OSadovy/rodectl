//! Finding the devices a command addresses.

use std::time::Duration;

use rode_hid::{
    AiMicro, DeviceInfo, DeviceKind, Hid, HidTransport, Power, Receiver, Slot, Transmitter,
};

use crate::cli::Selector;

/// Selection failures. `Usage` exits with status 2, the rest with 3.
#[derive(Debug, thiserror::Error)]
pub enum SelectError {
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{what} is on more than one device: {}; choose one with --device", .labels.join(", "))]
    Ambiguous { what: String, labels: Vec<String> },
    #[error("{device} has no {setting}")]
    NotApplicable { device: String, setting: String },
}

/// Something a setting can live on.
#[derive(Debug, Clone)]
pub enum Target {
    Receiver(DeviceInfo),
    Slot {
        receiver: DeviceInfo,
        slot: Slot,
        serial: Option<String>,
    },
    Transmitter(DeviceInfo),
    AiMicro(DeviceInfo),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Receiver,
    Slot,
    Transmitter,
    AiMicro,
}

impl Target {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Receiver(_) => Kind::Receiver,
            Self::Slot { .. } => Kind::Slot,
            Self::Transmitter(_) => Kind::Transmitter,
            Self::AiMicro(_) => Kind::AiMicro,
        }
    }

    /// The `--device` name that selects this target.
    pub fn label(&self) -> String {
        match self {
            Self::Receiver(_) => "rx".into(),
            Self::Slot { slot, .. } => format!("tx{slot}"),
            Self::Transmitter(_) => "tx".into(),
            Self::AiMicro(_) => "micro".into(),
        }
    }

    /// The label with what tells this target from another of the same
    /// model: its serial, or for a slot whose transmitter is unknown, the
    /// receiver's.
    pub fn qualified_label(&self) -> String {
        match self {
            Self::Slot {
                slot,
                serial: Some(serial),
                ..
            } => format!("tx{slot} {serial}"),
            Self::Slot {
                receiver,
                slot,
                serial: None,
            } => format!("rx {} tx{slot}", usb_id(receiver)),
            Self::Receiver(info) | Self::Transmitter(info) | Self::AiMicro(info) => {
                format!("{} {}", self.label(), usb_id(info))
            }
        }
    }

    pub fn serial(&self) -> Option<&str> {
        match self {
            Self::Receiver(info) | Self::Transmitter(info) | Self::AiMicro(info) => {
                info.serial.as_deref()
            }
            Self::Slot { serial, .. } => serial.as_deref(),
        }
    }
}

/// A device's serial, or its path when it reports none.
pub fn usb_id(info: &DeviceInfo) -> String {
    info.serial
        .clone()
        .unwrap_or_else(|| info.path.to_string_lossy().into_owned())
}

/// Open HID access and the global options that shape every request.
pub struct Context {
    pub hid: Hid,
    pub timeout: Duration,
    pub selector: Option<Selector>,
}

impl Context {
    pub fn receiver(&self, info: &DeviceInfo) -> rode_hid::Result<Receiver<HidTransport>> {
        Ok(Receiver::new(self.hid.link(info, self.timeout)?))
    }

    pub fn transmitter(&self, info: &DeviceInfo) -> rode_hid::Result<Transmitter<HidTransport>> {
        Ok(Transmitter::new(self.hid.link(info, self.timeout)?))
    }

    pub fn ai_micro(&self, info: &DeviceInfo) -> rode_hid::Result<AiMicro<HidTransport>> {
        Ok(AiMicro::new(self.hid.link(info, self.timeout)?))
    }

    fn of_kind(&self, kind: DeviceKind) -> Vec<DeviceInfo> {
        self.hid
            .devices()
            .into_iter()
            .filter(|d| d.kind == kind)
            .collect()
    }

    /// Transmitters linked to `receiver`, with their serials. A receiver
    /// that is switched off knows of none.
    pub fn linked_slots(&self, receiver: &DeviceInfo) -> anyhow::Result<Vec<Target>> {
        if receiver.power == Some(Power::Off) {
            return Ok(Vec::new());
        }
        let mut rx = self.receiver(receiver)?;
        let mut slots = Vec::new();
        for slot in Slot::ALL {
            if let Some(status) = rx.transmitter(slot)? {
                slots.push(Target::Slot {
                    receiver: receiver.clone(),
                    slot,
                    serial: Some(status.serial),
                });
            }
        }
        Ok(slots)
    }

    /// The targets a setting command acts on. With `--device`, the one it
    /// names, which must be of a kind in `accepts`; without, every connected
    /// target of a kind in `default`, counting only linked transmitters.
    pub fn targets(
        &self,
        setting: &str,
        accepts: &[Kind],
        default: &[Kind],
    ) -> anyhow::Result<Vec<Target>> {
        let Some(selector) = &self.selector else {
            return self.all_targets(setting, default);
        };
        let targets = self.select(selector)?;
        for target in &targets {
            if !accepts.contains(&target.kind()) {
                return Err(SelectError::NotApplicable {
                    device: target.label(),
                    setting: setting.into(),
                }
                .into());
            }
        }
        Ok(targets)
    }

    fn all_targets(&self, setting: &str, kinds: &[Kind]) -> anyhow::Result<Vec<Target>> {
        let mut targets = Vec::new();
        let receivers = self.of_kind(DeviceKind::WirelessMeRx);
        for rx in &receivers {
            if kinds.contains(&Kind::Receiver) {
                targets.push(Target::Receiver(rx.clone()));
            }
            if kinds.contains(&Kind::Slot) {
                targets.extend(self.linked_slots(rx)?);
            }
        }
        if kinds.contains(&Kind::Transmitter) {
            let direct = self.of_kind(DeviceKind::WirelessMeTx);
            // A transmitter on USB that is also linked is listed once, by
            // the direct route: the receiver's record of it can be stale.
            targets.retain(|t| {
                t.kind() != Kind::Slot
                    || !direct
                        .iter()
                        .any(|d| d.serial.is_some() && d.serial.as_deref() == t.serial())
            });
            targets.extend(direct.into_iter().map(Target::Transmitter));
        }
        if kinds.contains(&Kind::AiMicro) {
            targets.extend(
                self.of_kind(DeviceKind::AiMicro)
                    .into_iter()
                    .map(Target::AiMicro),
            );
        }
        if targets.is_empty() {
            if kinds.contains(&Kind::Slot) && !receivers.is_empty() {
                return Err(rode_hid::Error::NotLinked { slot: Slot::Tx1 }.into());
            }
            return Err(
                SelectError::NotFound(format!("no device with {setting} is connected")).into(),
            );
        }
        Ok(targets)
    }

    /// The targets `selector` names; one, unless two devices of the same
    /// model are connected.
    pub fn select(&self, selector: &Selector) -> anyhow::Result<Vec<Target>> {
        let not_found = |what: &str| SelectError::NotFound(format!("no {what} is connected"));
        let found = match selector {
            Selector::Rx => self
                .of_kind(DeviceKind::WirelessMeRx)
                .into_iter()
                .map(Target::Receiver)
                .collect(),
            Selector::Tx1 | Selector::Tx2 => {
                let slot = if *selector == Selector::Tx1 {
                    Slot::Tx1
                } else {
                    Slot::Tx2
                };
                self.of_kind(DeviceKind::WirelessMeRx)
                    .into_iter()
                    .map(|receiver| Target::Slot {
                        receiver,
                        slot,
                        serial: None,
                    })
                    .collect()
            }
            Selector::Tx => self
                .of_kind(DeviceKind::WirelessMeTx)
                .into_iter()
                .map(Target::Transmitter)
                .collect(),
            Selector::Micro => self
                .of_kind(DeviceKind::AiMicro)
                .into_iter()
                .map(Target::AiMicro)
                .collect(),
            Selector::Serial(serial) => self.by_serial(serial)?,
        };
        let found: Vec<Target> = found;
        if found.is_empty() {
            let what = match selector {
                Selector::Rx | Selector::Tx1 | Selector::Tx2 => "Wireless ME receiver".into(),
                Selector::Tx => "Wireless ME transmitter on USB".into(),
                Selector::Micro => "AI-Micro".into(),
                Selector::Serial(serial) => format!("device with serial {serial}"),
            };
            return Err(not_found(&what).into());
        }
        Ok(found)
    }

    fn by_serial(&self, serial: &str) -> anyhow::Result<Vec<Target>> {
        let matches = |s: Option<&str>| s.is_some_and(|s| s.eq_ignore_ascii_case(serial));
        let mut found = Vec::new();
        for info in self.hid.devices() {
            if !matches(info.serial.as_deref()) {
                continue;
            }
            found.push(match info.kind {
                DeviceKind::WirelessMeRx => Target::Receiver(info),
                DeviceKind::WirelessMeTx => Target::Transmitter(info),
                DeviceKind::AiMicro => Target::AiMicro(info),
                DeviceKind::Unsupported => continue,
            });
        }
        if found.is_empty() {
            for rx in self.of_kind(DeviceKind::WirelessMeRx) {
                found.extend(
                    self.linked_slots(&rx)?
                        .into_iter()
                        .filter(|t| matches(t.serial())),
                );
            }
        }
        Ok(found)
    }
}

/// The one target a write goes to.
pub fn single(setting: &str, mut targets: Vec<Target>) -> anyhow::Result<Target> {
    if targets.len() == 1 {
        return Ok(targets.remove(0));
    }
    let labels = targets
        .iter()
        .map(|t| match t.serial() {
            Some(serial) => format!("{} (serial {serial})", t.label()),
            None => t.label(),
        })
        .collect();
    Err(SelectError::Ambiguous {
        what: setting.into(),
        labels,
    }
    .into())
}
