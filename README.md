# rodectl

Control RØDE devices from the command line, without RØDE Central. Supported
so far: the Wireless ME and the AI-Micro.

Output is plain text, one `name: value` per line, or JSON with `--json`.

This is an independent project, not affiliated with or endorsed by RØDE. The
protocol was worked out from RØDE Central's behaviour on the wire; see
[docs/protocol.md](docs/protocol.md).

## Devices

| device | USB id | tested firmware |
|---|---|---|
| Wireless ME receiver | 19F7:0046 | 1.47 |
| Wireless ME transmitter, on USB or through the receiver | 19F7:0044 | 1.47 |
| AI-Micro | 19F7:0023 | 1.22 |

Other RØDE devices show up in `rodectl list` as unsupported.

## Usage

A setting command with no value reads the setting. With a value it writes it,
reads it back, and prints what the device now reports.

```
$ rodectl gain-assist
gain-assist: off
$ rodectl gain-assist auto
gain-assist: auto
$ rodectl status
Wireless ME RX, serial 80012345, on
battery: 100 raw
me-mic: off
gain-assist: off
output: merged
led: bright
TX1, serial 80054321, firmware 1.47
link: linked
battery: 48 raw
gain-assist: auto
led: bright
TX2: not linked
link: not linked
```

| command | values | device |
|---|---|---|
| `list` | | every connected device, and linked transmitters |
| `status` | | every readable setting |
| `battery` | read only, raw | receiver, transmitters |
| `gain-assist` | `auto`, `dynamic`, `off` | transmitter; receiver's own mic with `-d rx` |
| `me-mic` | `on`, `off` | receiver |
| `output` | `merged`, `split` | receiver |
| `led` | `bright`, `dim` | receiver, transmitters |
| `channels` | `merged`, `split`, `stereo` | AI-Micro |
| `monitor [INPUT]` | `on`, `off` | AI-Micro |
| `monitor-level [INPUT]` | 0 to 100, `+N`, `-N` | AI-Micro |
| `low-cut [INPUT]` | `on`, `off` | AI-Micro |
| `watch` | | prints each change; `--interval` sets the poll |
| `completions SHELL` | | shell completion script |
| `debug` | | raw reports; see below |

On the AI-Micro the first number after the command is the input:
`monitor-level 2 60` sets input 2, `monitor-level 2 +4` steps it up, and
`monitor-level` alone reads both inputs.

**Choosing a device.** Without `--device` (`-d`), a read covers every
connected device that has the setting, and a write needs exactly one. Name
one with `-d rx`, `-d tx1` or `-d tx2` (transmitters through the receiver),
`-d tx` (a transmitter plugged in by USB), `-d micro`, or a serial number.

**Transmitters through the receiver.** The receiver keeps a record of each
linked transmitter, holding its battery and LED, and updates it only when
asked. Every command asks first. `watch` asks on every poll by default;
`--refresh 30s` asks at most every 30 seconds, as RØDE Central does, and
reads the record as it stands in between.

**Battery** levels are the raw values the devices report. Their scale is not
known yet: RØDE Central showed a transmitter's 64 as "Around 90%".

**JSON.** `--json` prints setting commands as an array of
`{"device", "serial", "input", "setting", "value"}` objects, `list` and
`status` as `{"devices": [...]}`, and `watch` as one object per line. Errors
are one line of text on standard error, with or without `--json`.

**Exit status:** 0 success; 1 any other failure; 2 usage error; 3 no such
device, or `--device` needed to choose one; 4 the device refused, such as a
transmitter that is not linked; 5 no answer in time; 6 the setting reads back
other than what was written.

### debug

`rodectl debug devices` lists every RØDE vendor HID interface.
`rodectl debug listen` prints every input report a device sends, and sends
nothing. `rodectl debug send 09 16 01` sends one raw report and prints what
follows.

`debug send` sends whatever it is given. Some reports write settings, pair or
unpair transmitters, or start a firmware transfer that can leave a device
unusable. Read "Do not send" in [docs/protocol.md](docs/protocol.md) first.

## Running alongside RØDE Central

Both can have a device open at once. Each sees the other's replies, and
rodectl skips those that are not for its own requests.

## Building

```
cargo build --release
```

Windows needs nothing beyond Rust: HID access is pure Rust. To build the
Windows binary from Linux, use [cargo-xwin](https://github.com/rust-cross/cargo-xwin):

```
cargo xwin build --release --target x86_64-pc-windows-msvc
```

On Linux, install `udev/70-rode.rules` into `/etc/udev/rules.d/` so that you
can open the devices without root. Linux builds but has not been tried with
the devices; whether a receiver is switched on is reported only on Windows.

The `rode-hid` library crate holds the protocol and can be used on its own.

## License

MIT
