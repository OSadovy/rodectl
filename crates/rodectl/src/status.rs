//! `list`, `status` and `watch`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use rode_hid::{
    AiMicroStatus, DeviceInfo, DeviceKind, Power, ReceiverStatus, SlotStatus, TransmitterStatus,
};
use serde::Serialize;

use crate::output::{Out, Section, Value};
use crate::select::{Context, SelectError, Target, usb_id};

/// One USB device in `list` and `status` JSON.
#[derive(Serialize)]
struct Device {
    kind: DeviceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    serial: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    power: Option<Power>,
    #[serde(skip_serializing_if = "Option::is_none")]
    product_id: Option<String>,
    #[serde(flatten)]
    status: Option<Status>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Status {
    Receiver(ReceiverStatus),
    Transmitter(TransmitterStatus),
    AiMicro(AiMicroStatus),
    /// A receiver's transmitters alone, for `list`.
    Linked {
        transmitters: Vec<SlotStatus>,
    },
}

#[derive(Serialize)]
struct Devices {
    devices: Vec<Device>,
}

/// The USB devices to show: those `--device` names, or all. A transmitter
/// slot, or the serial of a transmitter linked to a receiver, names the
/// receiver.
fn devices(ctx: &Context) -> Result<Vec<DeviceInfo>> {
    let Some(selector) = &ctx.selector else {
        return Ok(ctx.hid.devices());
    };
    let mut found: Vec<DeviceInfo> = Vec::new();
    for target in ctx.select(selector)? {
        let info = match target {
            Target::Receiver(info) | Target::Transmitter(info) | Target::AiMicro(info) => info,
            Target::Slot { receiver, .. } => receiver,
        };
        if !found.iter().any(|f| f.path == info.path) {
            found.push(info);
        }
    }
    Ok(found)
}

/// For each device, its serial when another device of the same model is
/// connected, to tell the two apart.
fn qualifiers(devices: &[DeviceInfo]) -> Vec<Option<String>> {
    devices
        .iter()
        .map(|d| (devices.iter().filter(|o| o.kind == d.kind).count() > 1).then(|| usb_id(d)))
        .collect()
}

fn read(ctx: &Context, info: &DeviceInfo, full: bool, refresh: bool) -> Result<Device> {
    let status = match info.kind {
        DeviceKind::WirelessMeRx => {
            let power = info.power.unwrap_or(Power::Unknown);
            let mut rx = ctx.receiver(info)?;
            Some(if full {
                Status::Receiver(rx.status(power, refresh)?)
            } else if power == Power::Off {
                Status::Linked {
                    transmitters: Vec::new(),
                }
            } else {
                let mut transmitters = Vec::new();
                for slot in rode_hid::Slot::ALL {
                    let transmitter = rx.transmitter(slot)?;
                    transmitters.push(SlotStatus {
                        slot,
                        linked: transmitter.is_some(),
                        transmitter,
                    });
                }
                Status::Linked { transmitters }
            })
        }
        DeviceKind::WirelessMeTx if full => {
            Some(Status::Transmitter(ctx.transmitter(info)?.status()?))
        }
        DeviceKind::AiMicro if full => Some(Status::AiMicro(ctx.ai_micro(info)?.status()?)),
        _ => None,
    };
    Ok(Device {
        kind: info.kind,
        serial: info.serial.clone(),
        power: info.power,
        product_id: (info.kind == DeviceKind::Unsupported)
            .then(|| format!("{:04x}", info.product_id)),
        status,
    })
}

fn heading(device: &Device) -> String {
    let mut title = device.kind.name().to_owned();
    if let Some(id) = &device.product_id {
        let _ = write!(title, " {:04x}:{id}", rode_hid::protocol::VENDOR_ID);
    }
    if let Some(serial) = &device.serial {
        let _ = write!(title, ", serial {serial}");
    }
    if let Some(power) = device.power {
        let _ = write!(title, ", {power}");
    }
    title
}

fn label(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::WirelessMeRx => "rx",
        DeviceKind::WirelessMeTx => "tx",
        DeviceKind::AiMicro => "micro",
        DeviceKind::Unsupported => "unsupported",
    }
}

/// The text form of a device: its own section, then one per transmitter
/// slot. `qualifier` is added to the labels when given.
fn sections(device: &Device, qualifier: Option<&str>) -> Vec<Section> {
    let own_label = match qualifier {
        Some(q) => format!("{} {q}", label(device.kind)),
        None => label(device.kind).to_owned(),
    };
    let slot_prefix = qualifier.map(|_| format!("{own_label} "));
    let slot_prefix = slot_prefix.as_deref().unwrap_or("");
    let slot_section = |slot: &SlotStatus, full: bool| slot_section(slot_prefix, slot, full);
    let mut own = Section::new(own_label.clone(), heading(device));
    let mut slots = Vec::new();
    match &device.status {
        Some(Status::Receiver(rx)) => {
            own.fact("battery", Value::Raw(rx.battery_raw));
            if let Some(name) = rx.name.as_ref().filter(|n| !n.is_empty()) {
                own.fact("name", Value::text(name));
            }
            if let Some(on) = rx.me_mic {
                own.fact("me-mic", Value::Switch(on));
            }
            if let Some(mode) = rx.gain_assist {
                own.fact("gain-assist", Value::text(mode));
            }
            if let Some(mode) = rx.output {
                own.fact("output", Value::text(mode));
            }
            if let Some(led) = rx.led {
                own.fact("led", Value::text(led));
            }
            slots.extend(rx.transmitters.iter().map(|s| slot_section(s, true)));
        }
        Some(Status::Linked { transmitters }) => {
            slots.extend(transmitters.iter().map(|s| slot_section(s, false)));
        }
        Some(Status::Transmitter(tx)) => {
            own.fact("battery", Value::Raw(tx.battery_raw));
            if !tx.name.is_empty() {
                own.fact("name", Value::text(&tx.name));
            }
            own.fact("gain-assist", Value::text(tx.gain_assist));
            own.fact("led", Value::text(tx.led));
        }
        Some(Status::AiMicro(micro)) => {
            own.fact("channels", Value::text(micro.channels));
            for input in &micro.inputs {
                let n = input.input;
                own.fact(format!("input {n} monitor"), Value::Switch(input.monitor));
                own.fact(
                    format!("input {n} monitor-level"),
                    Value::Number(input.monitor_level),
                );
                own.fact(format!("input {n} low-cut"), Value::Switch(input.low_cut));
            }
        }
        None => {}
    }
    let mut all = vec![own];
    all.extend(slots);
    all
}

fn slot_section(prefix: &str, slot: &SlotStatus, full: bool) -> Section {
    let label = format!("{prefix}tx{}", slot.slot);
    let Some(tx) = &slot.transmitter else {
        let mut section = Section::new(&label, format!("TX{}: not linked", slot.slot));
        section.fact("link", Value::text("not linked"));
        return section;
    };
    let mut section = Section::new(
        &label,
        format!(
            "TX{}, serial {}, firmware {}",
            slot.slot, tx.serial, tx.firmware
        ),
    );
    section.fact("link", Value::text("linked"));
    if full {
        section.fact("battery", Value::Raw(tx.battery_raw));
        section.fact("gain-assist", Value::text(tx.gain_assist));
        section.fact("led", Value::text(tx.led));
    }
    section
}

/// `list` and `status`.
pub fn show(ctx: &Context, out: &mut Out, full: bool) -> Result<()> {
    let found = devices(ctx)?;
    if found.is_empty() {
        return Err(SelectError::NotFound("no RØDE device is connected".into()).into());
    }
    let devices = found
        .iter()
        .map(|info| read(ctx, info, full, true))
        .collect::<Result<Vec<_>>>()?;
    if out.json {
        out.document(&Devices { devices })?;
    } else {
        let sections: Vec<Section> = devices
            .iter()
            .zip(qualifiers(&found))
            .flat_map(|(device, q)| sections(device, q.as_deref()))
            .collect();
        // `list` shows headings alone.
        if full {
            out.sections(&sections)?;
        } else {
            for section in sections {
                out.line(section.title)?;
            }
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct Change<'a> {
    time: u64,
    device: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<&'static str>,
}

/// `watch`: polls every device and prints what changed, starting with
/// everything.
pub fn watch(
    ctx: &mut Context,
    out: &mut Out,
    interval: Duration,
    refresh_every: Duration,
) -> Result<()> {
    let mut known: BTreeMap<(String, String), Value> = BTreeMap::new();
    let mut present: BTreeSet<String> = BTreeSet::new();
    let mut last_error = None;
    let mut last_refresh: Option<Instant> = None;
    loop {
        let refresh = last_refresh.is_none_or(|at| at.elapsed() >= refresh_every);
        if refresh {
            last_refresh = Some(Instant::now());
        }
        match poll(ctx, refresh) {
            Ok(sections) => {
                last_error = None;
                let time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                let now: BTreeSet<String> = sections.iter().map(|s| s.label.clone()).collect();
                for gone in present.difference(&now) {
                    event(out, time, gone, "disconnected")?;
                    known.retain(|(device, _), _| device != gone);
                }
                for section in &sections {
                    if !present.contains(&section.label) {
                        if out.json {
                            event(out, time, &section.label, "connected")?;
                        } else {
                            out.line(&section.title)?;
                        }
                    }
                    for fact in &section.facts {
                        let key = (section.label.clone(), fact.key.clone());
                        if known.get(&key) == Some(&fact.value) {
                            continue;
                        }
                        if out.json {
                            out.json_line(&Change {
                                time,
                                device: &section.label,
                                key: Some(&fact.key),
                                value: Some(&fact.value),
                                event: None,
                            })?;
                        } else {
                            out.line(format_args!(
                                "{} {}: {}",
                                section.label, fact.key, fact.value
                            ))?;
                        }
                        known.insert(key, fact.value.clone());
                    }
                }
                present = now;
            }
            Err(err) => {
                let message = format!("{err:#}");
                if last_error.as_ref() != Some(&message) {
                    eprintln!("rodectl: {message}");
                    last_error = Some(message);
                }
            }
        }
        thread::sleep(interval);
        ctx.hid.refresh()?;
    }
}

fn event(out: &mut Out, time: u64, device: &str, what: &'static str) -> std::io::Result<()> {
    if out.json {
        out.json_line(&Change {
            time,
            device,
            key: None,
            value: None,
            event: Some(what),
        })
    } else {
        out.line(format_args!("{device}: {what}"))
    }
}

fn poll(ctx: &Context, refresh: bool) -> Result<Vec<Section>> {
    // A device that `--device` names but is gone shows as disconnected.
    let found = match devices(ctx) {
        Err(err) if err.downcast_ref::<SelectError>().is_some() => Vec::new(),
        other => other?,
    };
    let mut sections = Vec::new();
    for (info, q) in found.iter().zip(qualifiers(&found)) {
        sections.extend(self::sections(
            &read(ctx, info, true, refresh)?,
            q.as_deref(),
        ));
    }
    Ok(sections)
}
