# Mychronix

A Linux desktop app for **AiM MyChron** kart data loggers: download sessions
over WiFi, analyze laps, and browse tracks.

Mychronix exists to fill a gap on systems AiM doesn't support, such as Linux.
It is not meant to replace AiM products or RaceStudio in any way on officially
supported operating systems; if you're on one of those, use RaceStudio.

> Mychronix is an independent project. It is not affiliated with, endorsed by
> or supported by AiM Tech Srl. MyChron and RaceStudio are trademarks of
> AiM Tech Srl.

## Status

Early, and built around one MyChron 6 and the features its owners use. What
works today:

- **Download** sessions from a MyChron 5/6 over its WiFi, including joining the
  logger's network from the app, with RaceStudio-style session details
  (driver, kart, championship, session type, track, comment).
- **Garage:** the home page. Your MyChrons and whether they're connected,
  seat time per driver, and each kart with the setup it's running. Add and
  edit drivers and karts there: a kart has its engine, usual driver, the
  MyChron mounted on it and a baseline setup.
- **Sessions:** every downloaded session by race day, with best laps, named
  after the kart its MyChron is on.
- **Analysis:** channels over time or distance with a shared cursor and the
  track map colored by any channel; every lap as a strip across the top (best
  lap matching the logger's own, out/in and slow laps dimmed); channels added
  from a grouped picker (engine, movement, lap timing, logger), with the
  MyChron's timing channels in seconds and unused ones hidden.
- **Lap by lap** (RaceStudio's channels report): one row per lap with chosen statistics (RPM, speed, EGT,
  lateral and braking g by default; presets for engine and driving), max/min/
  average/std dev over the laps you tick, and any column graphed by lap.
- **Splits** (RaceStudio's split times report): each lap's time through 2–8
  splits, shown as times or against the best split or best lap, each split or
  running total, shaded fastest to slowest; best theoretical and best rolling
  laps; average, median, slowest and std dev; per-split details (speed through
  it, time and distance driven by lap) and a map of every lap through it.
- **Histogram:** time spent in each band of a channel (RPM, speed, EGT, g) for
  a lap or every racing lap, as % of time or seconds, with the best lap
  outlined for comparison.
- **Scatter:** one channel against another, optionally colored by a third:
  RPM against speed with the setup sheet's gearing drawn as a line, and the
  G-G diagram on a circular grid.
- **Lap comparison:** overlay up to eight laps from any sessions, lined up by
  distance around the track, with the time gap to a reference lap and a map of
  where time is gained and lost.
- **Events:** sessions grouped into events as RaceStudio does (week, track and
  championship), plus events you name or plan ahead. Each driver's sessions,
  laps, time on track, fastest lap, peak RPM and EGT and top speed, best lap
  through the event, and a season summary per championship. Engine warm-ups
  are listed but left out of the numbers.
- **Setup sheets:** gearing, tires and pressures, weight and ballast, jetting
  and notes per session, as RaceStudio's log sheets. A session without its own
  sheet uses the previous one (the first uses the kart's baseline), changes are flagged on the session and the
  event, and the gearing is checked against RPM and GPS speed.
- **Tracks:** AiM's track database by country with venue details, outlines,
  elevation, favorites, your speed on the track with braking points, splits
  with sector times and an ideal lap, map or satellite view.

Mychronix only ever reads from the logger.

## To do

Roughly in priority order.

- [ ] **MyChron 4:** read its sessions through AiM's data key (CAN-to-USB on the 5-pin Binder connector); compare laps
      timed by the infrared beacon (no GPS) along time instead of distance.
- [ ] **Live device test:** confirm joining, listing, downloading and
      disconnecting from the Device page end to end on a MyChron 6.
- [ ] **Session details after download:** edit driver, kart, championship,
      session type and comment for one or many sessions; filter by session
      type.
- [ ] **Import, export and delete sessions:** add `.xrk`/`.xrz` files or folders
      from elsewhere (e.g. a friend's data); export copies; remove sessions.
- [ ] **Laps preview** on the Sessions page: lap times for the selected session
      and a lap-time-by-lap graph, without opening it.
- [ ] **Analysis by distance:** a time/distance switch for the Session tab's
      graphs; quick lap picks (best 1/3/5, first or last laps); drag a range to
      read the time and channel change across it; min/max/average per
      channel.
- [ ] **Lap comparison, more:** the time-change map for every compared lap, not
      just the second; a "differential" view beside the gap; a time-based mode
      for laps without GPS.
- [ ] **Split divisions:** by corners and straights, or the track's official
      splits; rename, divide, merge and drag splits; saved per track. Split
      bands over the distance graphs, double-click to zoom into one.
- [ ] **Tracks:** make a custom track from a session's lap; import and export
      `.tkk` files; show the track's local time and time zone.
- [ ] **Device information:** firmware version, the tracks stored on the
      logger, and its counters (odometers), all read-only.
- [ ] **Export:** sessions and laps to CSV.
- [ ] **Writing to the logger** (custom tracks, settings), only once that part
      of the protocol is properly understood. Firmware updates are out of scope.
- [ ] **MyChron 5:** confirm it works the same as the 6 over WiFi.
- [ ] **GPS timing on older loggers:** RaceStudio places GPS ~70 ms apart from
      the other channels on an early MyChron 5 sample (0 ms on a MyChron 6).
      Probably re-timing by the GPS receiver's clock; see "GPS timing against
      the other channels" in `docs/protocol-notes.md`.
- [ ] **Packaging:** a Flatpak or similar so installing doesn't need Rust.

## Install (Linux)

Each release on GitHub has downloads for most Linux systems:

| Download | For |
|---|---|
| `Mychronix-<version>-x86_64.AppImage` | Any distro, no install: mark it executable and double-click. |
| `mychronix_<version>-1_amd64.deb` | Ubuntu, Debian, Mint, Pop!_OS: `sudo apt install ./mychronix_*.deb` |
| `mychronix-<version>-1.x86_64.rpm` | Fedora, openSUSE: `sudo dnf install ./mychronix-*.rpm` |
| `mychronix-<version>-x86_64.tar.gz` | Anything else: unpack and run `bin/mychronix-gui`. |

They're built on Ubuntu 22.04, so they run on that and newer (glibc 2.35+:
Debian 12, Fedora 36, Mint 21 and later). Joining the MyChron's WiFi from the
app uses NetworkManager, which nearly every desktop distro has; without it,
join the MyChron's network yourself and everything else works.

`scripts/package-linux.sh` builds the same downloads locally (see the script
for what it needs); `.github/workflows/release.yml` runs it for each `v*` tag.

## Install (Mac)

For Apple Silicon Macs (M1 and later) on macOS 11 or newer: download
`Mychronix-<version>-arm64.dmg` from a release, open it and drag Mychronix into
Applications.

Mychronix isn't signed with an Apple Developer ID, so the first launch shows
"Apple could not verify…". Click Done, open System Settings → Privacy &
Security, and click **Open Anyway** next to Mychronix. Once per version.

On a Mac, join the MyChron's WiFi (AiM-MYC…) from the menu bar yourself (macOS
doesn't let apps list WiFi networks without Location access); Mychronix finds
the logger once you're on it. If macOS asks to let Mychronix find devices on
your local network, allow it. The `mychronix` command-line tool is inside the
app at `Mychronix.app/Contents/MacOS/mychronix`. Logs go to
`~/Library/Logs/Mychronix`; your data is in `~/Mychronix` as on Linux.

### Build from source

Needs Rust (via [rustup](https://rustup.rs)), a C compiler, and ImageMagick for
the icons. WiFi joining uses NetworkManager (`nmcli`).

```sh
./scripts/install-desktop.sh
```

This builds Mychronix, installs `mychronix-gui` and the `mychronix`
command-line tool to `~/.local/bin`, and adds a launcher with its icon to your
app menu.

Data lives in `~/Mychronix/`: sessions under `sessions/<logger id>/`, your
names, favorites, events and setups as small JSON files; each session's setup
sheet sits beside it as `a_NNNN.setup.json`. A file that can't be read is kept as
`<name>.damaged-<time>.json` rather than overwritten, and if Mychronix ever
crashes the details go to `~/.cache/mychronix/crash.log`.

### Logs

Mychronix keeps a log at `~/.cache/mychronix/mychronix.log` (on a Mac,
`~/Library/Logs/Mychronix/mychronix.log`; rolled over at
1 MB): startup, library loads, every MyChron scan, WiFi change, listing and
download, and every error shown on screen. Start it with `MYCHRONIX_LOG=debug`
for more (each session it lists, each WiFi command), or `MYCHRONIX_LOG=trace`
to record every frame exchanged with the MyChron. The command-line tool takes
`-v` / `-vv` for the same, printed to the terminal.

### AiM track database

The track list comes from RaceStudio 3's own database, which Mychronix doesn't
ship. It reads it from a RaceStudio 3 install under Wine at `~/.rs3-wine`, or
from any `.ria` and `.tkk` files you copy into `~/Mychronix/tracks/`. Custom
tracks learned on your MyChron are copied from the logger automatically.

### Maps

Map tiles come from OpenStreetMap and satellite imagery from Esri World
Imagery (free for personal, non-commercial use with attribution). Both are
cached in `~/.cache/mychronix/tiles`.

## Command line

```sh
mychronix info session.xrz        # laps and channels
mychronix sessions                # downloaded sessions
mychronix device probe            # is a MyChron answering?
mychronix device pull             # download every new session
```

## How it works

The MyChron's WiFi protocol was worked out from captures of RaceStudio 3
talking to a MyChron 6, for interoperability with hardware we own. What is
known, and how sure we are of each part, is written up in
[docs/protocol-notes.md](docs/protocol-notes.md).

| Crate | What it does |
| --- | --- |
| `aim-link` | Finding a logger, its framing and checksums, listing and downloading files |
| `aim-xrk` | Session files (`.xrk`/`.xrz`) via [libxrk](https://github.com/m3rlin45/libxrk), and session summaries (`.xri`) |
| `aim-track` | AiM track files (`.ria` database, `.tkk` tracks) |
| `mychronix-cli` | The `mychronix` command |
| `mychronix-gui` | The app, built with [egui](https://github.com/emilk/egui) and [fastframe](https://github.com/crmne/fastframe) |

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at
your option.

Third-party parts keep their own licenses: Lucide icons (ISC, see
`crates/mychronix-gui/assets/icons/LICENSE.txt`), the Inter typeface bundled by
fastframe (SIL Open Font License), and the Rust dependencies (MIT, Apache-2.0
and other permissive licenses).
