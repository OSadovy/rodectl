//! `rodectl`: control RØDE Wireless ME and AI-Micro without RØDE Central.

mod cli;
mod debug;
mod output;
mod select;
mod settings;
mod status;

use std::io;
use std::process::ExitCode;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use rode_hid::Hid;
use tracing::level_filters::LevelFilter;

use crate::cli::{Cli, Command};
use crate::output::Out;
use crate::select::{Context, SelectError};
use crate::settings::InputOp;

fn main() -> ExitCode {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .with_max_level(match cli.verbose {
            0 => LevelFilter::WARN,
            1 => LevelFilter::DEBUG,
            _ => LevelFilter::TRACE,
        })
        .init();
    let mut out = Out::new(cli.json);
    match run(cli, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) if is_broken_pipe(&err) => ExitCode::SUCCESS,
        Err(err) => {
            let code = exit_code(&err);
            eprintln!("rodectl: {err:#}");
            ExitCode::from(code)
        }
    }
}

fn run(cli: Cli, out: &mut Out) -> Result<()> {
    if let Command::Completions { shell } = cli.command {
        clap_complete::generate(shell, &mut Cli::command(), "rodectl", &mut io::stdout());
        return Ok(());
    }
    let mut ctx = Context {
        hid: Hid::new()?,
        timeout: cli.timeout,
        selector: cli.device,
    };
    let readings = match cli.command {
        Command::List => return status::show(&ctx, out, false),
        Command::Status => return status::show(&ctx, out, true),
        Command::Watch { interval, refresh } => {
            return status::watch(&mut ctx, out, interval, refresh);
        }
        Command::Debug(command) => return debug::run(&ctx, out, &command),
        Command::Completions { .. } => unreachable!("handled above"),
        Command::Battery => settings::battery(&ctx)?,
        Command::GainAssist { value } => settings::gain_assist(&ctx, value)?,
        Command::MeMic { value } => settings::me_mic(&ctx, value.map(bool::from))?,
        Command::Output { value } => settings::output(&ctx, value)?,
        Command::Led { value } => settings::led(&ctx, value)?,
        Command::Channels { value } => settings::channels(&ctx, value)?,
        Command::Monitor { input, value } => {
            settings::input_setting(&ctx, input, &InputOp::Monitor(value.map(bool::from)))?
        }
        Command::MonitorLevel { input, value } => {
            settings::input_setting(&ctx, input, &InputOp::MonitorLevel(value))?
        }
        Command::LowCut { input, value } => {
            settings::input_setting(&ctx, input, &InputOp::LowCut(value.map(bool::from)))?
        }
    };
    Ok(out.readings(&readings)?)
}

fn exit_code(err: &anyhow::Error) -> u8 {
    for cause in err.chain() {
        if let Some(err) = cause.downcast_ref::<SelectError>() {
            return match err {
                SelectError::Usage(_) => 2,
                _ => 3,
            };
        }
        if let Some(err) = cause.downcast_ref::<rode_hid::Error>() {
            return match err {
                rode_hid::Error::Refused { .. } | rode_hid::Error::NotLinked { .. } => 4,
                rode_hid::Error::Timeout { .. } => 5,
                rode_hid::Error::Mismatch { .. } => 6,
                rode_hid::Error::OutOfRange(_) => 2,
                _ => 1,
            };
        }
    }
    1
}

fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain()
        .filter_map(|cause| cause.downcast_ref::<io::Error>())
        .any(|err| err.kind() == io::ErrorKind::BrokenPipe)
}
