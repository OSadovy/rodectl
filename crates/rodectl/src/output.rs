//! Text and JSON rendering.

use std::fmt::{self, Write as _};
use std::io::{self, Write};

use serde::Serialize;

use crate::select::Target;

/// A setting's value.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Value {
    Text(String),
    Switch(bool),
    Number(u8),
    /// A battery level whose scale is not known.
    Raw(u8),
}

impl Value {
    pub fn text(value: impl fmt::Display) -> Self {
        Self::Text(value.to_string())
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => f.write_str(text),
            Self::Switch(on) => f.write_str(if *on { "on" } else { "off" }),
            Self::Number(n) => n.fmt(f),
            Self::Raw(n) => write!(f, "{n} raw"),
        }
    }
}

/// One setting of one device, as a setting command prints it.
#[derive(Debug, Serialize)]
pub struct Reading {
    pub device: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<u8>,
    pub setting: &'static str,
    pub value: Value,
    #[serde(skip)]
    qualified: String,
}

impl Reading {
    pub fn new(target: &Target, setting: &'static str, value: Value) -> Self {
        Self {
            device: target.label(),
            serial: target.serial().map(str::to_owned),
            input: None,
            setting,
            value,
            qualified: target.qualified_label(),
        }
    }

    pub fn input(mut self, input: rode_hid::Input) -> Self {
        self.input = Some(input.number());
        self
    }
}

/// A device's heading and settings, as `status` and `watch` print them.
pub struct Section {
    /// The `--device` name.
    pub label: String,
    /// The heading line.
    pub title: String,
    pub facts: Vec<Fact>,
}

pub struct Fact {
    pub key: String,
    pub value: Value,
}

impl Section {
    pub fn new(label: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            title: title.into(),
            facts: Vec::new(),
        }
    }

    pub fn fact(&mut self, key: impl Into<String>, value: Value) {
        self.facts.push(Fact {
            key: key.into(),
            value,
        });
    }
}

/// Standard output, as text or JSON.
pub struct Out {
    pub json: bool,
    stdout: io::StdoutLock<'static>,
}

impl Out {
    pub fn new(json: bool) -> Self {
        Self {
            json,
            stdout: io::stdout().lock(),
        }
    }

    pub fn line(&mut self, line: impl fmt::Display) -> io::Result<()> {
        writeln!(self.stdout, "{line}")?;
        self.stdout.flush()
    }

    /// One JSON document, pretty-printed.
    pub fn document(&mut self, value: &impl Serialize) -> io::Result<()> {
        serde_json::to_writer_pretty(&mut self.stdout, value)?;
        self.line("")
    }

    /// One JSON object on one line.
    pub fn json_line(&mut self, value: &impl Serialize) -> io::Result<()> {
        serde_json::to_writer(&mut self.stdout, value)?;
        self.line("")
    }

    /// Setting readings: a JSON array, or one line each. A line names the
    /// device and input only when the readings cover more than one, and
    /// adds the serial when two devices of the same model are among them.
    pub fn readings(&mut self, readings: &[Reading]) -> io::Result<()> {
        if self.json {
            return self.document(&readings);
        }
        let several = |key: fn(&Reading) -> Option<String>| {
            readings
                .iter()
                .any(|r| key(r) != readings.first().and_then(key))
        };
        let name_device = several(|r| Some(r.qualified.clone()));
        let name_input = several(|r| r.input.map(|i| i.to_string()));
        for reading in readings {
            let mut line = String::new();
            if name_device {
                let same_model = readings
                    .iter()
                    .any(|o| o.device == reading.device && o.qualified != reading.qualified);
                line.push_str(if same_model {
                    &reading.qualified
                } else {
                    &reading.device
                });
                line.push(' ');
            }
            if let (true, Some(input)) = (name_input, reading.input) {
                let _ = write!(line, "input {input} ");
            }
            let _ = write!(line, "{}: {}", reading.setting, reading.value);
            self.line(line)?;
        }
        Ok(())
    }

    /// Sections as text: the heading, then a line per setting.
    pub fn sections(&mut self, sections: &[Section]) -> io::Result<()> {
        for section in sections {
            self.line(&section.title)?;
            for fact in &section.facts {
                self.line(format_args!("{}: {}", fact.key, fact.value))?;
            }
        }
        Ok(())
    }
}
