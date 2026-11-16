# RØDE vendor HID protocol: Wireless ME and AI-Micro

What rodectl relies on, and what is known beyond it. Device firmware: Wireless
ME 1.47, AI-Micro 1.22. The register map was read from RØDE Central 2.2.0's
`rode_bridge.dll`, a C++/JUCE library behind its Flutter window, and checked
against the devices. [open-rode](https://github.com/areoseek/open-rode)
documents the same framing for the Wireless GO II, whose register map is
partly shared.

Each item is marked:

- **verified**: read, or written and read back, on a device;
- **decoded**: found in RØDE Central's code and consistent with the
  devices, but its write not yet observed.

## Devices and interfaces

| device | USB id | interfaces |
|---|---|---|
| AI-Micro | 19F7:0023 | USB audio + vendor HID |
| Wireless ME receiver | 19F7:0046 | USB audio capture + vendor HID |
| Wireless ME transmitter | 19F7:0044 | vendor HID only |

Settings travel on the vendor HID interface, usage page 0xFF00. Every device
uses the operating system's class drivers. Every open handle receives a copy
of every input report, so several programs can share a device, and each sees
the replies to the others' requests.

**A Wireless ME receiver that is switched off** but on USB presents its HID
interface alone, as a plain USB device; on Windows its instance id then has
no `MI_00`. It still answers register reads such as battery. Switched on, it
is a composite device with HID on interface 0 and audio on interface 1.

## Framing

| family | request report | reply report | request length after the id |
|---|---|---|---|
| Wireless ME | 09 | 0A | 36 |
| AI-Micro | 0A | 09 | 27 |

- read: `<req> <reg> 01`
- write: `<req> <reg> 00 <value…>`
- reply: `<rep> <reg> <status> <value…>`, zero-padded.

Status `41` (`A`) acknowledges. Anything else refuses; the Wireless ME
receiver uses `4E` (`N`). RØDE Central pads requests to the device's longest
output report (1537 bytes on the Wireless ME) and waits 250 ms for a write's
acknowledgement and 2 s for a read's reply.

Match a reply by report and register, and expect stray ones: replies to other
programs' requests arrive too, and the acknowledgement of someone else's write,
`<rep> <reg> 41 00`, looks like a read returning 0.

The Wireless ME also has report pairs 01/02, 03/04, 0D/0E and 0F/10, and
unsolicited input report 0B, none of which settings need. Reports 05 and 07
carry firmware.

## Wireless ME

### Either unit, addressed directly

| reg | bytes | meaning | |
|---|---|---|---|
| 0x00 | 1 | battery, raw; scale unknown (RØDE Central showed 64 as "Around 90%") | verified read |
| 0x01 | 4 | clock, u32 little-endian Unix time, set by RØDE Central on connect | verified read |
| 0x03 | 22 | device name, UTF-8, NUL-padded | verified read |
| 0x15 | 1 (reads 2) | GainAssist of the unit's own microphone | verified on the transmitter; read on the receiver |

**Transmitter only:** 0x0C is LED brightness (verified read). The transmitter
switches between its built-in microphone and one in its 3.5 mm jack by
itself; no register shows which is active.

### Receiver only

| reg | meaning | |
|---|---|---|
| 0x06, 0x14 | output: 0x06 = 1 for merged, 0x14 its inverse; RØDE Central writes 0x06, then 0x14 | verified |
| 0x0B + i | status block of transmitter i | verified |
| 0x0D + i | relay: `09 0D+i <tx-reg> <value…>` writes a transmitter register | verified for the clock and LED |
| 0x11 | LED brightness | verified |
| 0x12 | ME Mic, the receiver's own microphone: 0 off, 1 on | verified |
| 0x16 + i | GainAssist of transmitter i | verified |
| 0x18, 0x19 | pairing state; 0x18 reads 2 whenever a transmitter is paired | read |
| 0x1A + i | refresh: writing 0 reloads the status block; refused with no transmitter linked | verified |
| 0x05 | output gain, 2 bytes, encoding unknown | decoded |
| 0x51 | pairing command, byte 1 = i + 1 | decoded |

i is 0 for TX1 and 1 for TX2.

**Split needs a second source.** With one transmitter and ME Mic off, RØDE
Central offers only Merged; with ME Mic on it offers Split too, and Split puts
the transmitter and the receiver's microphone on separate channels.

**Relayed transmitter registers have their own numbers**: LED brightness is
0x1A through the relay, `09 0D 1A <value>`, but 0x0C on the transmitter
directly. GainAssist is not relayed; it has the receiver registers 0x16 and
0x17. The relay's acknowledgement echoes the transmitter register:
`0A 0D 41 <tx-reg>`.

**LED brightness** is 0xDF bright and 0xFE dim, on all three LED registers.
A relayed LED write shows in the status block's byte 18 at once.

**No transmitter linked:** the receiver refuses the status block and 0x16 + i
with `4E`. It cannot tell a transmitter that is off from one out of range, and
refuses the same reads for a few minutes after it powers on. A link appears
within about 4 s of switching the transmitter on.

### GainAssist

Reading returns `<mode> <gain>`; writing takes one byte. 0 is Auto and 1
Dynamic, both keeping the stored gain. 2 + g, g from 0 to 30, is Off with a
manual gain of g dB. The Wireless ME ignores that gain: switching it between 0
and 30 dB with GainAssist off made no audible difference, and the Wireless ME
manual offers only Auto, Dynamic and Off. The value exists because the
GainAssist code is shared with models that use it.

A write to the receiver's 0x16 + i reaches the transmitter's 0x15 within two
seconds. A write to the transmitter's 0x15 over USB reaches the receiver's
0x16 + i within about ten seconds.

### Transmitter status block

`09 0B 01` for TX1; 28 bytes after the `0A 0B 41` header, little-endian.

| bytes | meaning |
|---|---|
| 0–3 | serial, as in the transmitter's USB instance id |
| 4–7 | transmitter clock |
| 8–9 | firmware, 0x0147 = 1.47 |
| 18 | LED brightness |
| 21 | relay register 8, probably input pad |
| 22 | battery, raw |
| 23–24 | the transmitter's USB product id |
| 25 | GainAssist, as written |
| 27 | GainAssist manual gain, dB |

Bytes 10–17 and 19–20 hold settings the Wireless ME does not offer.

**The block is the receiver's record, not a live reading.** Writes to 0x16 + i
update byte 25 at once, but the battery byte stays as it was until the
record is refreshed. With RØDE Central closed, a block read showed a battery
of 48 while the transmitter itself, on USB, read 26. Writing 0 to 0x1A + i
refreshes it: the receiver acknowledges and at once sends the new block
unasked, here with a battery of 44 against the transmitter's own 44. The
clock bytes did not change, so they are probably not a live clock. RØDE
Central sends that write every 60 status polls, about 30 s, and once when
the block first shows no link.

## AI-Micro

| reg | meaning | values | |
|---|---|---|---|
| 0 | channel mode | 0 merged, 1 split, 2 stereo | verified |
| 1 | input 1 direct monitoring | 0 off, 1 on | verified |
| 2 | input 1 monitoring level | 0–100 | verified |
| 3 | input 1 high-pass filter | 0 off, 1 on | verified |
| 4 | input 2 high-pass filter | 0 off, 1 on | verified |
| 5 | input 2 direct monitoring | 0 off, 1 on | verified |
| 6 | input 2 monitoring level | 0–100 | verified |
| 7 | written by RØDE Central at most once a second; never read | unknown | |

Factory values: 0, 1, 91, 0, 0, 1, 91.

**Stereo disables input 2.** Selecting Stereo, RØDE Central writes register 5
= 0 before register 0 = 2; leaving Stereo, it writes 5 = 1.

Input gain and headphone level are not HID settings: they are the USB audio
volumes of the AI-Micro's capture and playback endpoints.

## RØDE Central's polling

It polls the Wireless ME receiver twice a second: registers 0, 1, 5, 6, 0x0B,
0x0C, 0x0F, 0x18 and 0x19, and 0x1A now and then. It never reads 0x16 + i,
taking GainAssist from the status block. It reads ME Mic (0x12) only on connect, and writes the
value it read straight back, so it does not notice a change made elsewhere
until it reconnects. On connect it sends three report-01
reads (commands 0x36, 0x77, 0x42), sets the receiver's clock and relays the
transmitter's (`09 0D 06 <u32>`). None of that is needed to read or write
settings.

## Not found yet

Factory reset, diagnostics, a power-off command, the battery scale, and the
encoding of the receiver's output gain.

## Do not send

- A read-shaped request (`09 <reg> 01`) to a register that uses byte 2 for
  something else. On the receiver `09 0D 01` relays a write of transmitter
  register 1, its clock, and `09 51 01` is a pairing command. Read only
  registers known to hold a plain value; do not scan the receiver, or the
  transmitter above 0x3F.
- Reports 05 and 07, which carry firmware.
- Anything near the registers of RØDE Central's `factoryResetDevice` and
  `formatTxMemory` until those are found.
