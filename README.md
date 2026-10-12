# Mychronix

A Linux desktop app for **AiM MyChron** kart data loggers: download sessions
over WiFi, analyse laps, and browse tracks.

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
- **Sessions:** every downloaded session by race day, with best laps and
  per-logger driver and kart names.
- **Analysis:** channels over time with a shared cursor, laps and best lap
  (matching the logger's own), a GPS track map coloured by any channel.
- **Tracks:** AiM's track database by country with venue details, outlines,
  elevation, favourites, your speed on the track with braking points, splits
  with sector times and an ideal lap, map or satellite view.

Not yet: lap comparison, events, MyChron 4 (USB), writing anything to the
logger (tracks, settings, firmware). Mychronix only ever reads from the
logger.

## Install (Linux)

Needs Rust (via [rustup](https://rustup.rs)), a C compiler, and ImageMagick for
the icons. WiFi joining uses NetworkManager (`nmcli`).

```sh
./scripts/install-desktop.sh
```

This builds Mychronix, installs `mychronix-gui` and the `mychronix`
command-line tool to `~/.local/bin`, and adds a launcher with its icon to your
app menu.

Data lives in `~/Mychronix/`: sessions under `sessions/<logger id>/`, your
names and favourites as small JSON files.

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

## Licence

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at
your option.

Third-party parts keep their own licences: Lucide icons (ISC, see
`crates/mychronix-gui/assets/icons/LICENSE.txt`), the Inter typeface bundled by
fastframe (SIL Open Font License), and the Rust dependencies (MIT, Apache-2.0
and other permissive licences).
