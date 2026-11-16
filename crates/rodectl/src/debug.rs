//! `debug`: raw reports.

use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use rode_hid::protocol::Hex;
use rode_hid::{DeviceInfo, DeviceKind, Transport};
use serde::Serialize;

use crate::cli::{Debug, Selector};
use crate::output::Out;
use crate::select::{Context, SelectError};

pub fn run(ctx: &Context, out: &mut Out, command: &Debug) -> Result<()> {
    match command {
        Debug::Devices => devices(ctx, out),
        Debug::Listen { duration } => exchange(ctx, out, None, *duration),
        Debug::Send { bytes, listen } => {
            let report = parse_hex(bytes)?;
            exchange(ctx, out, Some(&report), Some(*listen))
        }
    }
}

#[derive(Serialize)]
struct Interface {
    kind: DeviceKind,
    product_id: String,
    release: String,
    serial: Option<String>,
    path: String,
}

fn devices(ctx: &Context, out: &mut Out) -> Result<()> {
    let interfaces: Vec<Interface> = ctx
        .hid
        .devices()
        .into_iter()
        .map(|d| Interface {
            kind: d.kind,
            product_id: format!("{:04x}", d.product_id),
            release: format!("{:04x}", d.release),
            serial: d.serial,
            path: d.path.to_string_lossy().into_owned(),
        })
        .collect();
    if out.json {
        return Ok(out.document(&interfaces)?);
    }
    for i in &interfaces {
        out.line(format_args!(
            "{}, product {}, release {}, serial {}",
            i.kind,
            i.product_id,
            i.release,
            i.serial.as_deref().unwrap_or("none")
        ))?;
        out.line(format_args!("path: {}", i.path))?;
    }
    Ok(())
}

/// The USB device `--device` names, supported or not.
fn device(ctx: &Context) -> Result<DeviceInfo> {
    let all = ctx.hid.devices();
    let kind = match &ctx.selector {
        None => None,
        Some(Selector::Rx) => Some(DeviceKind::WirelessMeRx),
        Some(Selector::Tx) => Some(DeviceKind::WirelessMeTx),
        Some(Selector::Micro) => Some(DeviceKind::AiMicro),
        Some(Selector::Tx1 | Selector::Tx2) => {
            bail!(SelectError::NotFound(
                "debug talks to devices on USB; use --device rx to reach transmitters".into()
            ))
        }
        Some(Selector::Serial(serial)) => {
            return all
                .into_iter()
                .find(|d| {
                    d.serial
                        .as_deref()
                        .is_some_and(|s| s.eq_ignore_ascii_case(serial))
                })
                .ok_or_else(|| {
                    SelectError::NotFound(format!("no device on USB has serial {serial}")).into()
                });
        }
    };
    let mut found: Vec<DeviceInfo> = all
        .into_iter()
        .filter(|d| kind.is_none_or(|k| d.kind == k))
        .collect();
    if found.len() == 1 {
        return Ok(found.remove(0));
    }
    if found.is_empty() {
        bail!(SelectError::NotFound("no RØDE device is connected".into()));
    }
    let labels = found
        .iter()
        .map(|d| {
            format!(
                "{} ({})",
                d.kind,
                d.serial.as_deref().unwrap_or("no serial")
            )
        })
        .collect();
    Err(SelectError::Ambiguous {
        what: "debug".into(),
        labels,
    }
    .into())
}

#[derive(Serialize)]
struct Traffic<'a> {
    event: &'static str,
    ms: u128,
    report: &'a str,
}

fn exchange(
    ctx: &Context,
    out: &mut Out,
    report: Option<&[u8]>,
    duration: Option<Duration>,
) -> Result<()> {
    let info = device(ctx)?;
    let mut transport = ctx.hid.open(&info)?;
    let start = Instant::now();
    let print = |out: &mut Out, event: &'static str, bytes: &[u8]| {
        let hex = Hex(bytes).to_string();
        let ms = start.elapsed().as_millis();
        if out.json {
            out.json_line(&Traffic {
                event,
                ms,
                report: &hex,
            })
        } else {
            out.line(format_args!("{ms:>6} ms {event}: {hex}"))
        }
    };
    if let Some(report) = report {
        transport.send(report)?;
        print(out, "sent", report)?;
    }
    let deadline = duration.map(|d| start + d);
    loop {
        let wait = match deadline {
            Some(deadline) => deadline.saturating_duration_since(Instant::now()),
            None => Duration::from_secs(1),
        };
        if wait.is_zero() {
            return Ok(());
        }
        if let Some(input) = transport.receive(wait)? {
            print(out, "received", &input)?;
        }
    }
}

/// Parses hex bytes given as separate or run-together pairs, with or
/// without `0x`.
fn parse_hex(words: &[String]) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for word in words {
        let digits = word.trim_start_matches("0x").trim_start_matches("0X");
        if digits.is_empty() || digits.len() % 2 != 0 {
            bail!("{word:?} is not whole hex bytes");
        }
        for pair in digits.as_bytes().chunks(2) {
            let pair = std::str::from_utf8(pair).context("not hex")?;
            bytes.push(
                u8::from_str_radix(pair, 16).with_context(|| format!("{word:?} is not hex"))?,
            );
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_accepts_pairs_and_runs() {
        let words = ["09".to_owned(), "1601".to_owned(), "0xFF".to_owned()];
        assert_eq!(parse_hex(&words).unwrap(), [0x09, 0x16, 0x01, 0xFF]);
        assert!(parse_hex(&["091".to_owned()]).is_err());
        assert!(parse_hex(&["zz".to_owned()]).is_err());
    }
}
