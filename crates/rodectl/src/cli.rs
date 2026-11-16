use std::str::FromStr;
use std::time::Duration;

use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use rode_hid::{ChannelMode, GainAssist, LedBrightness, OutputMode};

const EXIT_CODES: &str = "\
Exit status:
  0  success
  1  any other failure
  2  usage error
  3  no such device, or --device needed to choose one
  4  the device refused, such as a transmitter that is not linked
  5  the device did not answer in time
  6  the setting reads back other than what was written";

/// Control RØDE Wireless ME and AI-Micro without RØDE Central.
///
/// A setting command with no value reads the setting. With a value it writes
/// it, reads it back, and prints what the device now reports.
#[derive(Debug, Parser)]
#[command(version, after_long_help = EXIT_CODES)]
pub struct Cli {
    /// Device to address: rx, tx1, tx2 (through the receiver), tx (plugged
    /// in by USB), micro, or a serial number. Needed only when more than one
    /// device has the setting.
    #[arg(short, long, global = true, value_name = "DEVICE")]
    pub device: Option<Selector>,

    /// Print JSON instead of text.
    #[arg(long, global = true)]
    pub json: bool,

    /// Log reports sent and received to stderr; twice to include skipped
    /// ones.
    #[arg(short, long, global = true, action = ArgAction::Count)]
    pub verbose: u8,

    /// How long to wait for each reply.
    #[arg(long, global = true, value_name = "DURATION", default_value = "2s", value_parser = humantime::parse_duration)]
    pub timeout: Duration,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List connected devices and linked transmitters.
    List,

    /// Print every readable setting.
    Status,

    /// Battery level, raw: its scale is not known.
    Battery,

    /// GainAssist of a Wireless ME transmitter, or of the receiver's own mic
    /// with --device rx.
    GainAssist { value: Option<GainAssist> },

    /// The Wireless ME receiver's own microphone.
    MeMic { value: Option<Switch> },

    /// Whether the Wireless ME receiver merges two transmitters or splits
    /// them between channels.
    Output { value: Option<OutputMode> },

    /// LED brightness of a Wireless ME receiver or transmitter.
    Led { value: Option<LedBrightness> },

    /// AI-Micro channel mode. Stereo disables input 2 and its monitoring.
    Channels { value: Option<ChannelMode> },

    /// AI-Micro direct monitoring. Without an input, reads both.
    Monitor {
        #[arg(value_parser = parse_input)]
        input: Option<u8>,
        value: Option<Switch>,
    },

    /// AI-Micro direct monitoring level, 0 to 100, or +N or -N to step it.
    MonitorLevel {
        #[arg(value_parser = parse_input)]
        input: Option<u8>,
        #[arg(allow_hyphen_values = true)]
        value: Option<LevelChange>,
    },

    /// AI-Micro high-pass filter.
    LowCut {
        #[arg(value_parser = parse_input)]
        input: Option<u8>,
        value: Option<Switch>,
    },

    /// Poll every device and print each change; with --json, one JSON
    /// object per line.
    Watch {
        /// Time between polls.
        #[arg(long, value_name = "DURATION", default_value = "1s", value_parser = humantime::parse_duration)]
        interval: Duration,

        /// Least time between refreshes of the receiver's record of each
        /// transmitter, which holds its battery and LED; between them the
        /// record is read as it stands. RØDE Central refreshes every 30s.
        #[arg(long, value_name = "DURATION", default_value = "1s", value_parser = humantime::parse_duration)]
        refresh: Duration,
    },

    /// Print a shell completion script.
    Completions { shell: clap_complete::Shell },

    /// Raw reports, for working out the protocol. Can change any setting.
    #[command(subcommand)]
    Debug(Debug),
}

#[derive(Debug, Subcommand)]
pub enum Debug {
    /// List every RØDE vendor HID interface, supported or not.
    Devices,

    /// Print every input report the device sends. Sends nothing.
    Listen {
        /// Stop after this long; by default, run until interrupted.
        #[arg(long = "for", value_name = "DURATION", value_parser = humantime::parse_duration)]
        duration: Option<Duration>,
    },

    /// Send one output report and print every input report that follows.
    ///
    /// DANGER: this sends whatever bytes it is given. Besides reading
    /// registers, reports can write settings, pair or unpair transmitters,
    /// factory-reset a device, or start a firmware transfer (reports 05 and
    /// 07) that can leave it unusable. On the Wireless ME receiver even a
    /// read-shaped request to registers 0x0D, 0x0E or 0x51 writes or pairs.
    /// Read docs/protocol.md first.
    Send {
        /// The report, report id first, as hex bytes: `09 16 01` or
        /// `091601`.
        #[arg(required = true, num_args = 1..)]
        bytes: Vec<String>,

        /// How long to print replies for.
        #[arg(long, value_name = "DURATION", default_value = "1s", value_parser = humantime::parse_duration)]
        listen: Duration,
    },
}

fn parse_input(s: &str) -> Result<u8, String> {
    match s {
        "1" => Ok(1),
        "2" => Ok(2),
        _ => Err("expected input 1 or 2 before the value, as in `monitor 2 on`".into()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Switch {
    On,
    Off,
}

impl From<Switch> for bool {
    fn from(switch: Switch) -> Self {
        switch == Switch::On
    }
}

/// A monitoring level, or a step up or down from the current one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelChange {
    To(u8),
    Up(u8),
    Down(u8),
}

impl LevelChange {
    pub fn apply(self, current: u8) -> u8 {
        match self {
            Self::To(level) => level,
            Self::Up(step) => current
                .saturating_add(step)
                .min(rode_hid::MAX_MONITOR_LEVEL),
            Self::Down(step) => current.saturating_sub(step),
        }
    }
}

impl FromStr for LevelChange {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let number = |digits: &str| {
            digits
                .parse::<u8>()
                .map_err(|_| format!("expected 0 to 100, +N or -N, not {s:?}"))
        };
        if let Some(step) = s.strip_prefix('+') {
            Ok(Self::Up(number(step)?))
        } else if let Some(step) = s.strip_prefix('-') {
            Ok(Self::Down(number(step)?))
        } else {
            Ok(Self::To(number(s)?))
        }
    }
}

/// Which device a command addresses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    Rx,
    Tx1,
    Tx2,
    Tx,
    Micro,
    Serial(String),
}

impl FromStr for Selector {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "rx" => Self::Rx,
            "tx1" => Self::Tx1,
            "tx2" => Self::Tx2,
            "tx" => Self::Tx,
            "micro" | "ai-micro" => Self::Micro,
            "" => return Err("empty device".into()),
            _ => Self::Serial(s.to_ascii_uppercase()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_steps_stay_in_range() {
        assert_eq!("+4".parse::<LevelChange>().unwrap().apply(98), 100);
        assert_eq!("-4".parse::<LevelChange>().unwrap().apply(2), 0);
        assert_eq!("60".parse::<LevelChange>().unwrap().apply(91), 60);
        assert!("loud".parse::<LevelChange>().is_err());
    }

    #[test]
    fn cli_is_consistent() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
