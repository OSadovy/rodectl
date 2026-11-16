//! Commands that read or write one setting.

use anyhow::Result;
use rode_hid::{ChannelMode, GainAssist, Input, LedBrightness, OutputMode};

use crate::cli::LevelChange;
use crate::output::{Reading, Value};
use crate::select::{Context, Kind, Target, single};

const TRANSMITTERS: &[Kind] = &[Kind::Slot, Kind::Transmitter];

pub fn battery(ctx: &Context) -> Result<Vec<Reading>> {
    let kinds = &[Kind::Receiver, Kind::Slot, Kind::Transmitter];
    let mut readings = Vec::new();
    for target in ctx.targets("a battery", kinds, kinds)? {
        let raw = match &target {
            Target::Receiver(info) => ctx.receiver(info)?.battery_raw()?,
            Target::Slot { receiver, slot, .. } => {
                ctx.receiver(receiver)?
                    .transmitter(*slot)?
                    .ok_or(rode_hid::Error::NotLinked { slot: *slot })?
                    .battery_raw
            }
            Target::Transmitter(info) => ctx.transmitter(info)?.battery_raw()?,
            Target::AiMicro(_) => unreachable!("not a battery target"),
        };
        readings.push(Reading::new(&target, "battery", Value::Raw(raw)));
    }
    Ok(readings)
}

pub fn gain_assist(ctx: &Context, value: Option<GainAssist>) -> Result<Vec<Reading>> {
    const SETTING: &str = "gain-assist";
    let accepts = &[Kind::Receiver, Kind::Slot, Kind::Transmitter];
    let targets = ctx.targets(SETTING, accepts, TRANSMITTERS)?;
    let targets = match value {
        Some(_) => vec![single(SETTING, targets)?],
        None => targets,
    };
    let mut readings = Vec::new();
    for target in targets {
        let mode = match (&target, value) {
            (Target::Receiver(info), None) => ctx.receiver(info)?.gain_assist()?,
            (Target::Receiver(info), Some(mode)) => ctx.receiver(info)?.set_gain_assist(mode)?,
            (Target::Slot { receiver, slot, .. }, None) => {
                ctx.receiver(receiver)?.tx_gain_assist(*slot)?
            }
            (Target::Slot { receiver, slot, .. }, Some(mode)) => {
                ctx.receiver(receiver)?.set_tx_gain_assist(*slot, mode)?
            }
            (Target::Transmitter(info), None) => ctx.transmitter(info)?.gain_assist()?,
            (Target::Transmitter(info), Some(mode)) => {
                ctx.transmitter(info)?.set_gain_assist(mode)?
            }
            (Target::AiMicro(_), _) => unreachable!("not a GainAssist target"),
        };
        readings.push(Reading::new(&target, SETTING, Value::text(mode)));
    }
    Ok(readings)
}

pub fn led(ctx: &Context, value: Option<LedBrightness>) -> Result<Vec<Reading>> {
    const SETTING: &str = "led";
    let kinds = &[Kind::Receiver, Kind::Slot, Kind::Transmitter];
    let targets = ctx.targets(SETTING, kinds, kinds)?;
    let targets = match value {
        Some(_) => vec![single(SETTING, targets)?],
        None => targets,
    };
    let mut readings = Vec::new();
    for target in targets {
        let brightness = match (&target, value) {
            (Target::Receiver(info), None) => ctx.receiver(info)?.led()?,
            (Target::Receiver(info), Some(b)) => ctx.receiver(info)?.set_led(b)?,
            (Target::Slot { receiver, slot, .. }, None) => {
                ctx.receiver(receiver)?
                    .transmitter(*slot)?
                    .ok_or(rode_hid::Error::NotLinked { slot: *slot })?
                    .led
            }
            (Target::Slot { receiver, slot, .. }, Some(b)) => {
                ctx.receiver(receiver)?.set_tx_led(*slot, b)?
            }
            (Target::Transmitter(info), None) => ctx.transmitter(info)?.led()?,
            (Target::Transmitter(info), Some(b)) => ctx.transmitter(info)?.set_led(b)?,
            (Target::AiMicro(_), _) => unreachable!("not an LED target"),
        };
        readings.push(Reading::new(&target, SETTING, Value::text(brightness)));
    }
    Ok(readings)
}

pub fn me_mic(ctx: &Context, value: Option<bool>) -> Result<Vec<Reading>> {
    const SETTING: &str = "me-mic";
    receiver_setting(ctx, SETTING, value.is_some(), |ctx, target| {
        let Target::Receiver(info) = target else {
            unreachable!("not a receiver")
        };
        let mut rx = ctx.receiver(info)?;
        let on = match value {
            Some(on) => rx.set_me_mic(on)?,
            None => rx.me_mic()?,
        };
        Ok(Value::Switch(on))
    })
}

pub fn output(ctx: &Context, value: Option<OutputMode>) -> Result<Vec<Reading>> {
    const SETTING: &str = "output";
    receiver_setting(ctx, SETTING, value.is_some(), |ctx, target| {
        let Target::Receiver(info) = target else {
            unreachable!("not a receiver")
        };
        let mut rx = ctx.receiver(info)?;
        let mode = match value {
            Some(mode) => rx.set_output(mode)?,
            None => rx.output()?,
        };
        Ok(Value::text(mode))
    })
}

fn receiver_setting(
    ctx: &Context,
    setting: &'static str,
    write: bool,
    mut each: impl FnMut(&Context, &Target) -> Result<Value>,
) -> Result<Vec<Reading>> {
    let kinds = &[Kind::Receiver];
    let mut targets = ctx.targets(setting, kinds, kinds)?;
    if write {
        targets = vec![single(setting, targets)?];
    }
    targets
        .iter()
        .map(|target| Ok(Reading::new(target, setting, each(ctx, target)?)))
        .collect()
}

pub fn channels(ctx: &Context, value: Option<ChannelMode>) -> Result<Vec<Reading>> {
    const SETTING: &str = "channels";
    let kinds = &[Kind::AiMicro];
    let mut targets = ctx.targets(SETTING, kinds, kinds)?;
    if value.is_some() {
        targets = vec![single(SETTING, targets)?];
    }
    let mut readings = Vec::new();
    for target in targets {
        let Target::AiMicro(info) = &target else {
            unreachable!("not an AI-Micro")
        };
        let mut micro = ctx.ai_micro(info)?;
        let mode = match value {
            Some(mode) => micro.set_channels(mode)?,
            None => micro.channels()?,
        };
        readings.push(Reading::new(&target, SETTING, Value::text(mode)));
    }
    Ok(readings)
}

/// What a per-input command does to one input.
pub enum InputOp {
    Monitor(Option<bool>),
    MonitorLevel(Option<LevelChange>),
    LowCut(Option<bool>),
}

impl InputOp {
    fn setting(&self) -> &'static str {
        match self {
            Self::Monitor(_) => "monitor",
            Self::MonitorLevel(_) => "monitor-level",
            Self::LowCut(_) => "low-cut",
        }
    }

    fn writes(&self) -> bool {
        match self {
            Self::Monitor(v) | Self::LowCut(v) => v.is_some(),
            Self::MonitorLevel(v) => v.is_some(),
        }
    }
}

pub fn input_setting(ctx: &Context, input: Option<u8>, op: &InputOp) -> Result<Vec<Reading>> {
    let setting = op.setting();
    let kinds = &[Kind::AiMicro];
    let mut targets = ctx.targets(setting, kinds, kinds)?;
    let inputs = match input.and_then(Input::from_number) {
        Some(input) => vec![input],
        None if op.writes() => {
            anyhow::bail!(crate::select::SelectError::Usage(format!(
                "name the input to change: {setting} 1 or {setting} 2"
            )))
        }
        None => Input::ALL.to_vec(),
    };
    if op.writes() {
        targets = vec![single(setting, targets)?];
    }
    let mut readings = Vec::new();
    for target in &targets {
        let Target::AiMicro(info) = target else {
            unreachable!("not an AI-Micro")
        };
        let mut micro = ctx.ai_micro(info)?;
        for &input in &inputs {
            let value = match op {
                InputOp::Monitor(None) => Value::Switch(micro.monitor(input)?),
                InputOp::Monitor(Some(on)) => Value::Switch(micro.set_monitor(input, *on)?),
                InputOp::MonitorLevel(None) => Value::Number(micro.monitor_level(input)?),
                InputOp::MonitorLevel(Some(change)) => {
                    let level = match change {
                        LevelChange::To(level) => *level,
                        step => step.apply(micro.monitor_level(input)?),
                    };
                    Value::Number(micro.set_monitor_level(input, level)?)
                }
                InputOp::LowCut(None) => Value::Switch(micro.low_cut(input)?),
                InputOp::LowCut(Some(on)) => Value::Switch(micro.set_low_cut(input, *on)?),
            };
            readings.push(Reading::new(target, setting, value).input(input));
        }
    }
    Ok(readings)
}
