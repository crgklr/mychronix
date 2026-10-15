# AiM logger protocol notes

What is known about how RaceStudio 3 (RS3) talks to MyChron loggers, where each
fact came from, and how sure we are. **Observed** = seen in captures or bytes;
**reported** = documented by another project's captures; **inferred** = our reading.

## WiFi (MyChron 5 / 6)

| Fact | Status | Source |
|---|---|---|
| MyChron 6 runs an open access point named `AiM-MYC6-<serial>` (e.g. `AiM-MYC6-345678` for serial 12345678); MyChron 5 uses `AiM-MYC5-` | observed | Oct 5 Fedora test; RS3 strings |
| Logger is `10.0.0.1`; DHCP hands the PC `10.0.0.2` | observed | Oct 5 test |
| Logger answers ARP but not ICMP ping | observed | Oct 5 test |
| Logger's DHCP offers **no router/gateway** (only server id, DNS = 10.0.0.1, mask /24, 2 h lease), so Linux gets no default route via 10.0.0.1 | observed | Oct 11 test (`aim-rs3-test.sh`) |
| **TCP 2000 accepts connections from Linux with no prior UDP keepalive**; TCP 80/23/21 closed | observed | Oct 11 test |
| Logger resets TCP connections to port 53 (it advertises itself as DNS but doesn't serve it over TCP) | observed | Oct 11 capture |
| Discovery/keepalive: UDP, "aim-ka", port **36002**; RS3 sends *from* local port 36002 | reported | aim-racestudio3-mac `installer/bridge/wine-patch/README.md` |
| Data: "STCP" over TCP port **2000** on the logger | reported | aim-racestudio3-mac bridge docs |
| RS3 unicasts to the logger's gateway address; no broadcast/multicast in the connect flow | reported (MXS dash pcap) | aim-racestudio3-mac `docs/plans/2026-06-05-wifi-loopback-bridge.md` |
| TCP:2000 is closed by the logger ~1 s after the last UDP keepalive; keepalives every ~500 ms keep it open | reported | same plan, citing a `KeepaliveMaintainer` from a project named `promiscuous-slurp` (not found publicly) |
| RS3 ignores aim-ka replies that don't come from the logger's address | reported | aim-racestudio3-mac `CLAUDE.md` |
| Under Wine, RS3's per-interface broadcast resolves to `0.0.0.0`, so discovery goes to `0.0.0.0:36002` | reported (macOS Wine) | aim-racestudio3-mac wine-patch README |
| Dash subnets seen: `10.0.0.0/24`, `11.0.0.0/24`, `12.0.0.0/24` | reported | aim-racestudio3-mac `CLAUDE.md` |
| RS3 imports `wlanapi` (WlanScan/WlanConnect/WlanSetProfile/…) and recognizes loggers by SSID prefix | observed (imports, strings) | RS3 `AiMRS3-64-ReleaseU.exe` |

### Why RS3 under Wine on Fedora sent nothing (Oct 5)

Update Oct 11: the logger's DHCP gives no gateway, and RS3 derives the logger
address from the gateway, which is a second likely cause. The Oct 11 test added a
temporary gateway plus the redirect below, but was inconclusive: RS3 was only
started about a minute before the recording ended and was still loading.

Inferred: on Linux a datagram to `0.0.0.0` is delivered locally rather than sent
out of `wlp3s0`, so RS3's discovery never reached the air, and with no aim-ka
reply RS3 never opened TCP:2000. Candidate fix to test: an nftables output-NAT
rule rewriting `0.0.0.0:36002` to `10.0.0.1:36002`. Unlike macOS there is no
privacy gate on Linux, so TCP:2000 should then work directly.

### First successful capture (Oct 11, 23:50)

RS3 under Fedora Wine (with Direct3D switched to wined3d) found the MyChron 6 and
downloaded sessions while `aim-rs3-test.sh` recorded. Captures, rebuilt files and the
parsing scripts are in `samples/private/` (git-ignored).

**Discovery (observed).** RS3 sends the 6 ASCII bytes `aim-ka` from UDP 36002 every
2 s. Sent from Linux to `10.0.0.1:36002`, the logger answers within ~3 ms with a
236-byte binary status packet (starts `ec 00 00 00`, length 0xec = 236; contains
`0a 00 00 01` = its own IP, and the bytes `69 64 6e` "idn" around offset 84). Under
Fedora's Wine, RS3 addresses `aim-ka` to the PC's own address (`10.0.0.2`), not
`0.0.0.0` as on macOS.

**Framing (observed).** TCP 2000 carries messages framed like `.xrk` records:

```
'<' 'h' TAG[4] LEN:u32le X:u8 '>'   payload[LEN]   '<' TAG[4] CK:u16 '>'
```

Tags seen: `STCP` (both directions) and `STNC` (PC → logger requests). Every
connection opens with an 8-byte `STCP` hello: PC `00 00 00 00 06 08 00 00`, logger
`… 06 09 00 00`. **The trailer checksum is the u16 wrapping sum of the payload
bytes** (verified on every frame, including nested ones). The same framing nests
inside replies, with NUL-padded short tags (`SZ\0\0`) and FLAG `0x01`/`'a'`.

**Requests (observed).** 64-byte `STNC` bodies: bytes 8–11 look like a command and
sub-command (`10 00 01 00`, `02 00 04 00` file read, `05 00 04 00` directory/tree
read, `03 00 02 00`), and bytes 32–63 hold a NUL-terminated path. Paths seen:
`1:/mem/a_0033.xrz`, `1:/mem/a_0034.xrz`, `1:/mem/a_0035.xrz` (sessions, drive 1),
`0:/tkk`, `0:/tkk/al.ria`, `0:/tkk/ll.ria`, `0:/tkk/al-2-ll.idx`, `0:/tkk/ll-2-al.idx`,
`0:/tkk/dev.ria`, `0:/tkk/o/nl2amuco.tkk` (track database, drive 0).

**File read (observed).** The logger answers a read with a 64-byte `STCP` header
echoing the request, with the file size as u32le at bytes 16–19 and `c0 ff 00 00`
at 20–23, then the file in `STCP` chunks of up to 65,476 bytes, each with a 4-byte
prefix before the data (`00 00 00 00` or `c0 ff 00 00`). Concatenating the chunk data
rebuilt `a_0033.xrz` and `a_0034.xrz` byte-exact to the advertised size, and both
open in `mychronix info` (track, date, laps and channels all read correctly). `.xrz` is the
zlib-compressed session (`78 01` header).

**libxrk on MyChron 6 files (observed).** Channels and laps decode, but libxrk warns
about hundreds of `(S…)`/`(M…)` records it doesn't recognize; MyChron 6 writes
record types its MXP/MXm test data lacks. Worth reporting upstream with a sample.

**Exchange shape (observed, interleaved by time).** PC `STNC` request → logger
`STCP`(64) ack (bytes 24–25 = phase `0x0a09`) → logger `STCP`(64) with phase
`0x0a11` and the reply size at 16–19 → **PC pulls by byte offset**: it sends a
4-byte `STCP` holding the offset, the logger answers with that offset plus up to
65,472 bytes. The "acks" are offsets (65,472 = `0xffc0`). When RS3's download of
`a_0035` failed it re-sent the same offset five times and gave up. Writes start
with phase `0x0a01` (the logger asking the PC for `size` bytes), e.g. command
`10/01`, where RS3 sends 64 bytes containing the year 2026 (likely PC time).

**Directory listing (observed).** Command `05/04` on a folder returns `FLE`
records, each holding `TMD` (date, `MM/DD/YYYY`), `TMT` (`HH:MM:SS`), `SZ`
(decimal size) and `N` (name).

**Device info (observed).** Command `10/01` returns ~4.3 KB of nested records:
`iMST` (ids), `iHW` (`WiFi=ESP32|Reg=usa|LSM6DSV16X|Led=PI33TB|MYC68B|M103`),
`iUSR` (on-logger `pilota=`/`veicolo=`/`campionato=`, empty on ours), `iPTH` (the
path table below), `iLCK` (locks), `iSST` (feature flags), `iLTS` (logger clock),
`iPRL` (settings as CSV: date/time format, WiFi power, backlight color, …). Path
table excerpt: `settings=0:/set`, `channels=0:/ch`, `tracks=0:/gps`, `tkk=0:/tkk`,
**`recorded=1:/mem`**, `lapinfo=1:/mem/linfo`, `logs=1:/logs`, `software=0:/sw`,
`font=0:/font`, `languages=0:/lang`.

**aim-ka reply (observed).** The logger id is u32le at byte 96 (the serial,
whose last six digits form the SSID, e.g. 12345678 → `AiM-MYC6-345678`), inside a record starting `idn` at 84.

**Implemented** in `crates/aim-link` (read-only): discovery, framing with checksum
checks, connect + hello, folder listing, file download by offset with retry and
stale-chunk skipping. Tested against a fake logger that replays these formats.

**What RS3 adds on save (observed).** RS3 keeps the logger's `.xrz` byte-for-byte
and also writes `.xrk`/`.drk`/`.rrk` plus a CSV folder under
`user/data/<date>/<driver>/<track>/`; its `.xrk` carries the driver and vehicle
typed in at download.

**What's in `1:/mem` (observed).** Per session `a_NNNN.xrz` (the recording) and
`a_NNNN.xri` (~720 B summary), plus `summary1.csv` (all sessions, one row each,
newest first), `recofile.db`/`.chk`, `remrinfo.db` (binary index naming each
`A_NNNN.XRZ`), and `prm_cfg.cff`/`.chk`. Listing times are UTC file times; the
`.xri`/CSV `hour` is the local start time.

**`.xri` format (observed, parsed in `aim-xrk::summary`).** Text records framed like
the binary ones but with decimal fields:
`<hTAG` + 10-digit length + flag `a` + `>`, a body of CRLF `key=value|` lines,
CRLF, then `<TAG` + 6-digit decimal sum of the body bytes + `>`. `IMEM` (length 0)
wraps the file; its trailer is the last thing in it. Records: `USR` (`pilota`,
`veicolo`, `campionato`, …: empty on the logger), `TINF` (`sess_num`, `date`
**day-first** `DD/MM/YYYY`, `hour`), `TRK` (`track_name`, `NKA` when unknown;
`track_lat_0..5`/`track_lon_0..5` in 1e-7 degrees; `track_guid`), `RACM` (`mode`,
`trk_type`), `TDUR` (`test_dur` ms), `LAPS` (`nlap`, `motivolap`, `maxrpm`,
`maxvel`, `maxlacc`, `nbest` **1-based**, `best` ms). All 35 of ours verify.

**Lap numbering and best lap (observed).** The logger numbers laps from 1 (the
out-lap is lap 1) and its `best` only considers complete laps, never the first
(out) or last (in) lap. libxrk numbers from 0 and includes both, which made a
partial in-lap look like the best lap; `aim-xrk` now follows the logger, and its
best lap matches the logger's on all 35 sessions.

**`summary1.csv` (observed).** Columns `name,size,date,hour,nlap,nbest,best,
pilota,track_name,veicolo,campionato,venue_type,mode,trk_type,motivolap,maxvel,
device,track_lat,track_lon,test_dur,pname,ptype,ptime,pdist,pmaxv,valid`. Dates
day-first. `maxvel` here is not the `.xri` speed (58.6 in the `.xri`, 1078807756 in
the CSV; as an f32 that is 3.21, so the encoding is still unknown).

**Open questions:** meaning of the `aim-ka` reply fields; the `X` byte and trailer
checksum; commands `10/01` and `03/02` (the latter returns small blocks of 8, 496,
2,724 and 12,812 bytes: device info or settings?); why `a_0035.xrz` (2.2 MB)
came back short in the capture (capture loss vs. RS3 stopping early).

## USB

| Fact | Status | Source |
|---|---|---|
| AiM USB vendor ID `0x11CC` | observed | RS3 driver INFs |
| Custom-driver product IDs `0110`, `0120`, `0130` (likely MyChron 4 and other older loggers) | observed IDs; device mapping inferred | `Driver_USB`, `Driver_USB_Std` |
| RNDIS (USB networking) product IDs `2002`, `3003` (likely newer loggers, so USB may be the same IP protocol as WiFi) | observed IDs; mapping inferred | `Driver_RNDIS` |
| Newer AiM USB devices are vendor-class WinUSB (`Class=USBDevice`); PID `0130` = "AiM Device 1.3". On Linux they are reachable from userspace (libusb/`nusb`) with no kernel driver | reported | aim-racestudio3-mac `installer/wine-patch/README.md` |
| aim-racestudio3-mac patches Wine's USB layer and has a USB reinit plan | reported | `installer/wine-patch/build-wineusb-so.sh`, `docs/plans/2026-06-18-usb-prefix-reinit.md` |

## Session files

`.xrk`/`.xrz` parsing uses [libxrk](https://github.com/m3rlin45/libxrk) (MIT),
whose `spec/xrk_format.py` is an executable spec of the format. Its test data
comes from MXP/MXm loggers; MyChron files still need checking against it.

### GPS timing against the other channels (open question, Oct 13 2026)

**Observed.** RaceStudio 3's "Race Studio 2 .csv" export was compared sample by
sample with Mychronix (`cargo run -p aim-xrk --example vs_rs3 -- <file> <csv>`):

- On AiM's sample `test.xrk` (Adria Kart, Jan 2016; an early MyChron 5 by its
  channels: WiFi, backlight, internal battery, 6-axis IMU, two temperatures),
  RaceStudio places GPS **70 ms differently relative to RPM and the IMU** than
  libxrk's timestamps do. Every non-GPS channel lines up with RaceStudio at
  0 ms; GPS lines up at 70 ms (RaceStudio's GPS later relative to RPM).
- On a MyChron 6 session the offset is **0 ms** for all channels.

**Why the loggers differ (measured with `--example gps_clock`).** Each GPS
record holds the logger's timestamp and the receiver's own time of week
(u-blox NAV-SOL iTOW/fTOW).

- MyChron 6: logger time minus receiver time is constant to 0.3 ms over 18,411
  fixes (25 Hz). Its clock is locked to GPS; fixes are stamped when measured.
- The older logger: the same difference wanders. Fixes are stamped **when they
  arrive**, 0–50 ms late (median 18 ms above the earliest arrivals), 8 Hz.

**Partly tested hypothesis.** RaceStudio re-times GPS by the receiver's clock.
Re-timing our GPS that way brings its speed closer to RaceStudio's export
(0.279 → 0.249 km/h rms), which supports it. It does **not** explain the fixed
70 ms, whose direction (GPS later against RPM) is the opposite of removing a
delay.

**Leads in RaceStudio (Ghidra project `rs3`, read-only scripts).**
- `FUN_141d8c780` builds a channel named **"GPS CLOCK SYNCH"** from (time × 1000,
  integer) pairs and registers it with the other channels: the most likely
  place the logger and GPS clocks are tied together.
- `FUN_141f8bb30` registers `ChanLatency` / `ChanAvgLatency` by name among many
  classes; it looks like the math-channel function catalog rather than an
  automatic correction (unconfirmed).
- `FID_SPEC_GPS_AIM_TIME` exists as a string; its reference wasn't inside a
  function Ghidra had defined.

**Not conclusive.** GPS speed vs RPM (`--example gps_vs_rpm`) trails by ~105 ms
even on the MyChron 6, where RaceStudio applies no shift. That measures the
receiver's own speed filtering, so it can't isolate RaceStudio's correction.

**Other loggers checked (Oct 14).** Three more public sessions all stamp GPS
when it's measured (logger minus receiver time steady to about 1 ms): a 2026
MyChron 6 (25 Hz), a 2022 AiM car logger (25 Hz) and a 2021 AiM car logger
(10 Hz). So far only the 2016 early-MyChron-5 sample stamps GPS on arrival.

**To close it.** (1) RaceStudio CSV exports from a second older (pre-MyChron 6)
logger, to see whether the 70 ms is fixed or varies; (2) trace how
`FUN_141d8c780`'s "GPS CLOCK SYNCH" channel is consumed when GPS samples are
placed. Until then Mychronix uses libxrk's timestamps unchanged; this only
matters for loggers that stamp GPS on arrival (not the MyChron 6).

## MyChron 4 data key (USB 11cc:0101) — static findings, Oct 15, 2026

From RaceStudio 2 (`RaceStudio2.exe`, `Download.exe`, driver
`AIM_USB_DRIVER_2008/AIM_USBdrv10_01.sys`), read with Ghidra; nothing has been
sent to a key yet.

- **Not a USB drive.** The key enumerates as 11cc:0101, class 0xDC
  (diagnostic), one interface with EP1 interrupt IN/OUT (16 bytes), EP2 bulk
  IN (64 bytes) and a fourth endpoint. Only AiM's software talks to it.
- **Driver:** Microsoft's BulkUsb sample with an AiM IOCTL added. RS2 sends
  every transfer as `DeviceIoControl(0x22000c)` with a 48-byte transfer
  record: +0x10 buffer pointer, +0x14 byte count, +0x20 an 8-byte USB setup
  packet. `0x220000` returns the configuration descriptor.
- **Commands are vendor control transfers** (bmRequestType 0x40) with a
  small data stage, set up in `CUSBHardware32`'s constructor:
  - bRequest 0x0F booter (6 bytes) — firmware loader, never send;
  - bRequest 0x0E memory (6 bytes): `[opcode, addr & 0xffff (LE), slave,
    len (LE u16)]`, `wIndex = addr >> 16`; opcode 1 = MEM_READ;
  - bRequest 0x0D "Evo3" (9 bytes): `[cmd, ...]`; cmd 0x1B = CMD_GETNETTABLE;
  - bRequest 0x0C bulk (6 bytes): `[addr (3 bytes LE), len (LE u16), op]`,
    `wIndex = 0x471`; op 1 = BULK_READ, then read `len` bytes from the bulk
    IN pipe (`ReadMEM_MainBuff`);
  - bRequest 0x02 CFG, 0x01, 0x00 — not yet identified.
- `CConnectionDataKey::DumpStrumento` is a diagnostic dump of the key's own
  microcontroller memory (MEM_READ 128 bytes at a time over 0xE0000–0xFFFFF,
  0xF000–0xFFFF and 0x40000000 + 0–0x8FFFF), **not** the session download.
- `CConnectionDataKey::LeggiNetTable` sends CMD_GETNETTABLE and reads a
  256-byte table of up to 31 "slaves" (u16 logger code each, 8-byte
  entries): the key acting as a USB-to-CAN bridge to a live logger.
- **Unresolved:** which RS2 code path downloads runs stored on the key
  (likely `Download.exe`'s run list → `.imd`/`.dat` → `.drk`), and the opcodes
  for it. Next step: record RS2 downloading from a key that holds a dump
  (Windows VM + usbmon), then decode.
- **Never send:** booter (0x0F), MEM_WRITE / SLAVE_BULKWRITE, GPS writes, the
  key-erase command, firmware updates.

### Recorded: RaceStudio 2 downloading from the key (Oct 15, 2026)

RS2 ran under Wine with `tools/aimkey-bridge` (a stand-in msimg32.dll that
forwards AiM's IOCTLs to a libusb helper; Wine's out-of-process driver can't
follow the pointer to the command bytes). The full exchange is logged; the
wire format, confirmed:

- AiM's driver sends RS2's vendor requests with **bmRequestType 0x42**
  (vendor, endpoint recipient). bRequest 0x0C uses `wIndex = 0x0471`.
- Transfer record (64-bit layout, 56 bytes): +0x04 endpoint, +0x08 read
  flag, +0x10 buffer pointer, +0x18 length, +0x20 bytes transferred,
  +0x24 setup packet, +0x2C OK flag. IOCTL 0x22000c = control,
  0x220010 = bulk.
- Read pattern: a command (0x0E memory or 0x0D "Evo3"), then
  `0x0C [addr:3][len:2][op=1]` (BULK_READ), then read `len` bytes from bulk
  IN 0x82.
- Identify: 0x0E `07 00 00 00 00 00` → 128-byte block (key info).
- Memory reads: 0x0E `01 lo lo slave len len`, `wIndex = addr >> 16`.
- The download proper is 0x0D commands `1d 11 …` / `1d 12 nn 00 …` (cmd 0x1D
  with sub-commands; `1d 12 nn` steps through stored items), each followed by
  bulk reads of up to ~40 KB.
- At the end RS2 sends 0x0F `0f 00 00 00 00 00` (booter family, probably the
  key's restart). Mychronix shouldn't need it.
- After a 0x0C, the key waits for its data to be read from 0x82; if it never
  is, it ignores further commands until unplugged.

## RaceStudio 2 `.drk` files (MyChron 4), format version 6.00

Read by `aim_xrk::drk`; checked against RaceStudio 2 on a MyChron 4 session
(16 laps identical to the millisecond; a lap's RPM min/max/mean identical).

- A run of blocks, each a 32-byte header — two capital letters, the format
  version as u16 LE (600), a checksum — then data:
  - `RD` ×2: the first is a bare header; the second holds the session name
    and, at +0x0c, `DD-MM-YY HH:MM:SS` (the logger's clock).
  - `PP` (1 KB): track name at +0x28, driver at +0x50, session name at +0x78.
  - `II` (128 B).
  - `MM` (1 KB) per channel: name at +0, code (e.g. `\t11040`) at +0x14,
    sample count u32 at +0x48, full-scale float at +0x50.
  - `GG` (256 B) per lap: lap time in ms u32 at +0x76, in logger ticks u32 at +0x04.
  - `RR`: a 4 KB table, then every channel's samples as u16 LE, channel after
    channel in `MM` order. The last 2–3 samples of each channel are a
    trailer (0x4316, 0x2620, 0x1015, 0x7FFF…), not readings.
- Time: fast channels every 100 logger ticks, slow ones every 1000. The
  logger's clock drifts (≈1.5% on one MyChron 4); scale ticks to ms by
  Σ lap ms / Σ lap ticks, as RaceStudio 2 does.
- Values: Engine = RPM as is; Datalogger_Temp = raw / 100 °C (≈1 °C above
  RS2's display — formula not exact yet); Battery = raw × 0.0055 V
  (RS2: 8.2 V); 0xCFFF = nothing connected.
- Older versions (RaceStudio 2's own samples) use other layouts — not read yet.
