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

### MyChron 6 over USB (`11cc:0110`) — verified Oct 22, 2026

Implemented in `aim-link/src/usb.rs`; checked on a MyChron 6 (firmware as
shipped Oct 2026): the folder listing, an `.xri`, the settings file and a
2.2 MB session, all byte-identical to the WiFi downloads. Wire facts first
from rodrigobrim/myoverlay's notes (GPL-3.0; facts only, no code taken),
then confirmed and corrected on the device.

| Fact | Status | Source |
|---|---|---|
| One vendor-class interface (class `0xFF`), bulk IN `0x81`, bulk OUT `0x02`, 64-byte packets, USB 1.1 full speed. Reachable from userspace with libusb/`nusb`, no kernel driver; the udev rule for vendor `11cc` covers it | observed | `lsusb -v` |
| A charge-only USB-C cable gives a USB-C "partner" but no USB device at all; a data cable is needed | observed | |
| A request is the same 64-byte body as over TCP, with `0x0038` at bytes 4–5, a sequence number at 6–7 and `0x00000a01` at 24–27; sent as a **vendor control OUT, request 1** (bmRequestType `0x40`) | observed (layout per myoverlay) | |
| The reply is a **vendor control IN, request 1**, 64 bytes: on this firmware it echoes the request, with the TCP-style phase `0x0a09` at 24 and `0xfc00` (64,512) at 20 — not the `"cpa"` prefix myoverlay describes | observed | |
| **Vendor control IN, request 2**, 8 bytes: `[state u32, length u32]`. State 0: a chunk of `length` bytes is ready on bulk IN. State `0x14`: nothing ready (busy preparing the next chunk, or idle after a transfer, keeping the last length) | observed | |
| After reading a chunk, an empty **vendor control OUT, request 2** asks for the next; chunks are at most 64,512 bytes | observed | |
| There is no end-of-file state: a file's size comes from its folder listing | observed | |
| A transfer left unfinished **carries on at the next connection, even after a new request**: the logger serves the rest of the old file. So connecting first reads out any chunks still ready | observed | |
| Speed: ~2.2 MB in ~4.5 s (~500 KB/s) | observed | |
| The logger's serial is in `1:/mem/prm_cfg.cff`, in an `SRC` record whose payload is the same `idn` record the WiFi discovery reply carries (serial 12 bytes in) | observed | |
| Never sent here: identify (`0x00010010`), which on firmware 667+ stages a settings bundle that must be read out before anything else is answered (myoverlay) | reported | myoverlay |

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
    AiM sensor code u16 at +0x40, frequency code u16 at +0x46 (see the
    rate table below), sample count u32 at +0x48, full-scale float at +0x50.
  - `GG` (256 B) per lap: lap time in ms u32 at +0x76, in logger ticks u32 at +0x04.
  - `RR`: a table of 4 KB blocks, each with its own `RR` header (one in
    small files; 10 in a 1.3 MB one; the first word gives the count, but
    RaceStudio 2 can compact the extra blocks away without changing it, so
    follow the chain of headers), then every channel's samples as u16 LE,
    channel after channel in `MM` order, to the end of the file. The last
    2–3 samples of each channel are a trailer (0x4316, 0x2620, 0x1015,
    0x7FFF…), not readings. Block headers are only searched for before `RR`:
    sample bytes can look like one.
  - `MM` calibration: two floats at +0x5C and +0x60 hold the channel's own
    (zero, gain) for accelerometers, and (min, max) for imported channels.
- Time: fast channels every 100 logger ticks, slow ones every 1000. The
  logger's clock drifts (≈1.5% on one MyChron 4); scale ticks to ms by
  Σ lap ms / Σ lap ticks, as RaceStudio 2 does.
- Values: by sensor code, from RaceStudio 2's sensor table (`FW_SENSORI`
  in `DATABASE/Configs_20800AIM.mdb`); see "Sensor scaling" below.
  0xCFF0–0xCFFF are sensor error codes (0xCFFF not connected, 0xCFFD seen
  on a faulty thermocouple), not readings.
- Older versions (RaceStudio 2's own samples) use other layouts — not read yet.

### Downloading runs from the key (decoded from the recording, Oct 15, 2026)

All download commands are bRequest 0x0D (bmRequestType 0x42) with 9 data
bytes `[1D][sub][00][addr: 3 bytes LE][00][len: u16 LE]`, followed by a bulk
read of `len` bytes from 0x82. RS2 first sends the identify sequence (0x0E
`07 …` + 0x0C BULK_READ 128) and two 128-byte memory reads; then:

- `1D 11`, addr 0, len 4096: the **directory**, 16 bytes per run: model u16
  (106), logger serial u32, a u32 (timestamp?), `ffffffff`, run index u16.
- `1D 12`, addr = run index, len 256: the **run header**. +0x00 run number
  (u16); +136 lap-records address, +140 data address, +144 end address,
  +150 data length (u32, bytes); serial, model and firmware (`59 32`, BCD)
  near +120; driver name near +0x13, track near +0x56; best lap in
  hundredths at +8.
- `1D 13`, lap-records address, len = data address − lap address:
  **one 64-byte record per lap**, lap time in hundredths of a second at +4.
- `1D 14`, data address, len = data length rounded up to 64: the **samples**,
  u16 LE words, laid out as below.

Decoded this way, all 50 runs on a key give exactly the samples RaceStudio
2 wrote into the matching .drk files (whose last few "samples" are the end
record misread).

### Which channels a run holds, and their order (from Download.exe, Oct 15, 2026)

RaceStudio 2's MyChron 4 converter (`CAIMConversioneImdDrk_M4`; layout
built at 0x41F230, slots placed at 0x41EF20, per-channel flags at 0x49B110,
stream parsed at 0x498F10) decides the layout from the run header:

- Model at +0x7C (101 or 106 MyChron 4, 104 or 107 MyChron 4 2T), an
  expansion box code at +0x7E (102 Gold, 103 Extreme; 0 or 0xFFFF none),
  firmware as `swap16(u16 at +0x76)` (0x5932 on one logger).
- Channels per code (RaceStudio 2's `DEFAULT_CONF_CANALI_LOGGER` table):
  - 101/106: 0 Engine, 1 Temperature, 2 Datalogger_Temp, 3 Battery,
    13 Lambda, 15 Lambda_Temp, 20 SHAFT (second RPM).
  - 104/107: as above with 1 Temp_1 and 19 Temp_2.
  - 102: 4 Speed, 5 Calculated_Gear, 6 LatAcc, 7 EBoxTemp1, 8 EBoxTemp2;
    103 adds 10–12 EboxAnalog1–3.
  - 108 (channels expansion): 96–99 M4ChExp1–4.
  - All 10 Hz (frequency code 2) except Datalogger_Temp and Battery, 1 Hz (code 0).
- Recorded or not (firmware ≥ 0x3125; older ones record channels 0–3 only):
  0, 2, 3 always; 1 if byte 0x0E bit 1; 19 if byte 0x0F bit 0 (2T); 20 if
  u16 at 0x0E bit 9; 13 and 15 if byte 0x10 bit 1 (firmware > 0x3126);
  with an expansion box: 4 if 0x0E bit 0, 5 bit 7, 6 always, 7 bit 2,
  8 bit 3, 10–12 bits 4–6; byte 0x10 bit 4 adds code 108, its inputs
  from byte 0xC7 bits 0–3.
- Order: the recorded channels sorted by number, placed over a one-second
  cycle of 500 slots (`FREQ_LOGGER` of every MyChron 4 code). Each channel
  takes the first free slot, then one every `500 / rate` slots (positions
  as f32, truncated); a taken slot moves it to the nearer free slot within
  a step either way (ahead when equally near); samples that still don't fit
  go in the first free slots afterwards. The samples are the used slots in
  order. A plain MyChron 4 gives `[RPM, temp, logger temp, battery]` then
  nine `[RPM, temp]`.
- Rate codes: 0 = 1 Hz, 1 = 2, 9 = 5, 11 = 4, 12 = 8, 2 = 10, 3 = 20,
  4 = 50, 5 = 100, 6 = 200, 7 = 500, 8 = 1000; 10 and 110 not stored.
- Markers are words with a high byte 0xD0–0xDE: `D0` starts a cycle (the
  slot count restarts), `D1` + 3 words, `D2` a lap record (6 words), `D3`
  a record ending at `D300` 7 words on (or before a `D203`), `D4` a
  9-word record (marker + 16 bytes with a checksum) announcing a CAN
  plug-and-play device. RaceStudio 2 registers the device but keeps the
  layout it built from the header (built once, at 0x498FB2; its device list
  only ever gains code 108 from the header), so reading carries on. 256 bytes of 0xFF end the data.
- The key holds no GPS: RaceStudio 2's MyChron 4 file is just the run
  headers, lap records and samples.

### Sensor scaling (RaceStudio 2's `FW_SENSORI` table)

| Code | Sensor | Value |
|---|---|---|
| 0 | RPM | raw rpm |
| 77 | second RPM (SHAFT) | raw × 2 rpm |
| 1 | speed | raw × 0.1 km/h |
| 96 | thermocouple / water temp | raw (signed) × 0.1 °C |
| 45 | logger temperature | raw × 0.1220703125 − 273.15 °C |
| 68 | battery | raw × 0.0054931640625 V |
| 501 | lambda | raw × 0.001 |
| 91 | lambda temperature | raw °C |
| 405, 413, 414 | accelerometer | raw (signed) × 0.01 g |
| 600 | number (gear, analog inputs) | raw |
| 3001 | GPS speed | raw × 0.1 km/h |
| 3002 | GPS satellites | raw |
| 3003, 3004 | GPS lateral/longitudinal accel | raw × 0.00625 − 100 m/s² |
| 3005 | GPS slope | raw × 0.0001 − π/2 rad |
| 3006 | GPS heading | raw × 0.0002 − π rad |
| 3007 | GPS yaw rate | raw × 0.0004 − 2π rad/s |

With these, one MyChron 4's logger temperature reads 73.2 °F where
RaceStudio 2 shows 74 °F (raw / 100 had read 75.6 °F).

### Scaling any sensor (Oct 15, 2026)

`crates/aim-xrk/src/sensor_table.rs` is RaceStudio 2's whole `FW_SENSORI`
table (277 sensors), generated by `tools/gen-sensor-table.py`. Kind 1 is
`A1 × raw + A0`; kind 2 (pulse inputs: RPM, wheel speed) `A1 × raw`; kind 0
is calibrated per channel or a fixed scale (battery); 101–104 are lambda
curves (not done). Beyond the table:

- Analog accelerometers (codes 10, 11, 78, 79, 198, 401–404): `40 × (raw −
  zero) / gain` g with the channel's own zero and gain (fitted against GPS
  lateral g on a sample: r = 0.94; one sample's longitudinal axis reads
  with the opposite sign, presumably as mounted). With no gain stored,
  `(raw − zero) / 440` g, signed like the gain (from landonh12/Lapline's
  notes on Solo DL files; fits a TCR car's `.xrk` at slope 0.98, r 0.994).
- Code 999, "imported data": raw 0–65535 spans the channel's own (min, max);
  the unit is written as text in the `MM` code field ("rpm", "°C", "bar").
- Linear potentiometers (7, 8, 30: dampers) and custom sensors (86, 98, 1000)
  stay raw counts: their per-channel calibration isn't worked out.
- Readings 0xCFF0–0xCFFF are sensor error codes.

Checked on RaceStudio 2's samples and a public set of three `.drk` files:
wheel speeds agree with GPS speed within a few km/h on every car and bike,
temperatures and pressures land in sensible ranges, vertical g reads −1.00.
Formats before 6.00 (`RD` version 400/500, RaceStudio 2's oldest samples)
are not read.

### Files RaceStudio 2 keeps beside a `.drk`

- `.imd` (MyChron 4): the raw key download: `IMD\0` + version, the run's own
  256-byte header at 0x14000 (the key's other headers after it), its 64-byte
  lap records at 0x20000 (count = (data address − lap address) / 64 from the
  header) and its samples at 0x50000 (length at header +150). Decoded like a
  `.mc4`, all 50 of one key's `.imd` files equal their `.drk` sample for sample.
- `.rrk` (GPS module): a `HEAD` section, then an `NSOL` section of 64-byte
  records: `NSOL`, the logger's tick count (u32), a u-blox NAV-SOL payload
  (iTOW ms, fTOW, week, fix type, flags, earth-centered X/Y/Z in cm, position
  accuracy, velocity X/Y/Z in cm/s, speed accuracy, pDOP, satellites), 4
  spare bytes. About 4 a second.
- `.gpk` (GPS module): a `PROV` record with an earth-centered reference point
  (doubles at +14, +22, +30), then 144-byte `PSOL` records: `PSOL`, the
  tick count, week, iTOW, then east/north/up offsets in meters from the
  reference (doubles at +16, +24, +32). RaceStudio 2's processed track, about
  8 a second; matches `.rrk` to 2 cm.
- Alignment: both carry the logger's tick count, so positions go straight
  onto the session's clock (scaled by the laps' ms per tick). Matching their
  speed to the `.drk`'s GPS Speed channel moves them by 25–75 ms at most
  (correlation 0.999+ on five samples), so that's only a fine-tune.

### MyChron 6 notes from another project (rodrigobrim/myoverlay, GPL-3.0; facts only)

- USB download exists (device `11cc:0110`, AiM's driver; same 64-byte
  command bodies as WiFi). Implemented and verified Oct 22, 2026: see
  "MyChron 6 over USB" above.
- Firmware 667 stages an identify bundle that must be read before the
  listing is served, and a staged payload can outlive the connection that
  asked for it. Mychronix never sends identify; when a reply announces a
  payload for a command it didn't send, it reads that out and waits for its
  own reply (replies echo the command at bytes 8–11).
- Some sessions are stored as `.hrz` (format unknown); listed and kept as downloaded.
- Destructive commands (never sent): `0x00040006` delete file, `0x00040007`
  remove dir, `0x00040008` empty dir, `0x00040009` format media,
  `0x0004000d` format data, `0x00051000` erase firmware, `0x000c0005` reset WiFi.

### Checked against the same session as `.xrk` (Oct 15, 2026)

bmc-labs/xdrk's test data has a TCR race car session as both `.drk` and
`.xrk`. Fitting the `.drk`'s values against the `.xrk`'s (libxrk, real
units) over the same time: imported channels (code 999) and GPS come out at
slope 1.00 and intercept ≈ 0 (wheel speeds, temperatures, fuel, gear, logger
temperature, GPS speed), pressures at ×1000 (mbar against bar). The `.drk`
holds 17 laps to the `.xrk`'s 2.

### Other files (LapLift's guide, citing AiM's file-type reference)

`.bak` is RaceStudio 2 Analysis's backup of the original `.drk` (read when
the `.drk` won't open); `.dat` is the raw memory dump of other older loggers
(EVO3/4, MyChron 3, MXL), like the MyChron 4's `.imd` (not read).

### WiFi keepalive

RS3 sends `aim-ka` about once a second for as long as it's connected, and
loggers are reported (TheAngryRaven/mychron-wifi-spec, MyChron 5S 2T; the
aim-racestudio3-mac bridge notes) to drop or silently stall the TCP
connection without it. Mychronix now does the same for every connection; a
run of unanswered keepalives is logged as the first sign of a hung logger.

