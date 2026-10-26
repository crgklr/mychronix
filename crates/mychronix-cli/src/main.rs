use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use aim_link::{AP_ADDRESS, Link};
use aim_xrk::{Session, format_lap_time};
use anyhow::Context;
use clap::{Parser, Subcommand};

/// Mychronix: AiM MyChron tools for Linux.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Show what's happening: -v for each step, -vv for every frame exchanged with the MyChron.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
    #[command(subcommand)]
    command: Command,
}

/// Prints log messages to the terminal (stderr), at the level -v asks for.
struct StderrLog(log::LevelFilter);

impl log::Log for StderrLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= self.0 && (m.target().starts_with("aim_") || m.target().starts_with("mychronix"))
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("{:<5} {}: {}", r.level(), r.target(), r.args());
        }
    }
    fn flush(&self) {}
}

#[derive(Subcommand)]
enum Command {
    /// Summarize a session file (.xrk/.xrz): details, laps and channels.
    Info {
        file: PathBuf,
        /// Also list every channel with its sample count and units.
        #[arg(long)]
        channels: bool,
    },
    /// List downloaded sessions from their .xri summaries.
    Sessions {
        /// Folder of sessions (default: ~/Mychronix/sessions/<each logger>).
        dir: Option<PathBuf>,
    },
    /// Talk to a MyChron: a MyChron 6 plugged in by USB, else over WiFi (join its AiM-MYC… network first).
    #[command(subcommand)]
    Device(DeviceCommand),
    /// Read a MyChron 4 data key plugged into this computer.
    #[command(subcommand)]
    Datakey(DataKeyCommand),
}

#[derive(Subcommand)]
enum DataKeyCommand {
    /// List the runs on the key.
    List,
    /// Download every run that isn't already saved (as .drk or .mc4).
    Pull {
        /// Folder to save into (default: ~/Mychronix/sessions/<logger serial>).
        #[arg(long)]
        to: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum DeviceCommand {
    /// Check that a logger answers, and show its id.
    Probe,
    /// Show the sessions on the logger without downloading them.
    Sessions,
    /// List a folder on the logger (default: recorded sessions, 1:/mem).
    Ls { dir: Option<String> },
    /// Download one file from the logger, e.g. `1:/mem/a_0034.xrz`.
    Get {
        path: String,
        /// Where to save it (default: the file name, in the current folder).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Download every session (and its .xri summary) that isn't already saved.
    Pull {
        /// Folder to save into (default: ~/Mychronix/sessions/<logger id>).
        #[arg(long)]
        to: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let level = match cli.verbose {
        0 => log::LevelFilter::Warn,
        1 => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
    };
    if log::set_boxed_logger(Box::new(StderrLog(level))).is_ok() {
        log::set_max_level(level);
    }
    match cli.command {
        Command::Info { file, channels } => info(&file, channels),
        Command::Sessions { dir } => local_sessions(dir),
        Command::Device(cmd) => device(cmd),
        Command::Datakey(cmd) => datakey(cmd),
    }
}

fn sessions_root() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Mychronix").join("sessions")
}

fn datakey(cmd: DataKeyCommand) -> anyhow::Result<()> {
    use aim_link::datakey::DataKey;
    let mut key = DataKey::open()?;
    let runs = key.runs()?;
    match cmd {
        DataKeyCommand::List => {
            println!("{} runs on the key", runs.len());
            for r in &runs {
                let (date, time) = r.start().unwrap_or_default();
                let best = r.best_lap_cs.map(|cs| format_lap_time(cs as i64 * 10)).unwrap_or_else(|| "-".into());
                println!("  run {:>3}  {date} {time}  {:>2} laps  best {best:>9}  {:>6} bytes  logger {}", r.number, r.laps(), r.data_len, r.serial);
            }
        }
        DataKeyCommand::Pull { to } => {
            let serial = runs.first().map(|r| r.serial.to_string()).unwrap_or_else(|| "mychron4".into());
            let dir = to.unwrap_or_else(|| sessions_root().join(&serial));
            std::fs::create_dir_all(&dir)?;
            // Runs already here (from RaceStudio 2's .drk files or an earlier pull), by start time.
            let have: Vec<(Option<String>, Option<String>)> = std::fs::read_dir(&dir)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "drk" || x == "mc4"))
                .filter_map(|p| Session::open(&p).ok())
                .map(|s| (s.info.date, s.info.time))
                .collect();
            let mut saved = 0;
            for r in &runs {
                let Some((date, time)) = r.start() else { continue };
                if have.iter().any(|(d, t)| d.as_deref() == Some(&date) && t.as_deref() == Some(&time)) {
                    continue;
                }
                let run = key.download(r, |_, _| {})?;
                let path = dir.join(r.file_name());
                std::fs::write(&path, run.to_file())?;
                println!("saved {}", path.display());
                saved += 1;
            }
            println!("{saved} new of {} runs; {} already here", runs.len(), runs.len() - saved);
        }
    }
    Ok(())
}

fn info(file: &PathBuf, list_channels: bool) -> anyhow::Result<()> {
    let session = Session::open(file).with_context(|| format!("reading {}", file.display()))?;
    let i = &session.info;
    let field = |label: &str, value: &Option<String>| {
        if let Some(v) = value.as_deref().filter(|v| !v.is_empty()) {
            println!("{label:<9}{v}");
        }
    };
    field("Driver", &i.driver);
    field("Vehicle", &i.vehicle);
    field("Track", &i.venue);
    field("Session", &i.session);
    field("Date", &i.date);
    field("Time", &i.time);
    field("Device", &i.device);
    if let Some(id) = i.logger_id {
        println!("{:<9}{id}", "Logger");
    }

    let best = session.best_lap(10_000).map(|l| l.number);
    println!("\nLaps ({}):", session.laps.len());
    for lap in &session.laps {
        let mark = if Some(lap.number) == best { "  best" } else { "" };
        println!("  {:>3}  {}{mark}", lap.number, format_lap_time(lap.duration_ms()));
    }

    match &session.gps {
        Some(g) => println!("\nGPS: {} points", g.len()),
        None => println!("\nGPS: none"),
    }
    println!("Channels: {}", session.channels.len());
    if list_channels {
        for c in &session.channels {
            println!("  {:<28} {:>8} samples  {}", c.name, c.values.len(), c.units);
        }
    }
    Ok(())
}

fn device(cmd: DeviceCommand) -> anyhow::Result<()> {
    match cmd {
        DeviceCommand::Probe => {
            if aim_link::usb::is_present() {
                let id = aim_link::usb::UsbLogger::open().and_then(|mut l| l.logger_id()).context("a MyChron is plugged in by USB but didn't answer")?;
                println!("MyChron on USB");
                if let Some(id) = id {
                    println!("Logger id  {id}");
                }
                return Ok(());
            }
            let beacon = probe()?;
            println!("Logger at {} answered in {} ms", beacon.address, beacon.round_trip.as_millis());
            if let Some(id) = beacon.logger_id {
                println!("Logger id  {id}");
            }
        }
        DeviceCommand::Sessions => {
            let csv = connect()?
                .read_file("1:/mem/summary1.csv", |_, _| {})
                .context("reading the logger's session summary")?;
            let mut rows = aim_xrk::parse_summary_csv(&String::from_utf8_lossy(&csv));
            rows.sort_by_key(|s| s.number);
            print_summaries(&rows);
        }
        DeviceCommand::Ls { dir } => {
            let dir = dir.as_deref().unwrap_or(aim_link::SESSIONS_DIR);
            let entries = connect()?.list(dir).with_context(|| format!("listing {dir}"))?;
            for e in &entries {
                println!("{:<24} {:>10}  {} {}", e.name, e.size, e.date, e.time);
            }
            println!("{} entries in {dir}", entries.len());
        }
        DeviceCommand::Get { path, output } => {
            let output = output.unwrap_or_else(|| PathBuf::from(path.rsplit('/').next().unwrap_or("download")));
            let data = connect()?.read_file(&path, progress_bar(&path)).with_context(|| format!("reading {path}"))?;
            eprintln!();
            std::fs::write(&output, &data)?;
            println!("Saved {} ({} bytes)", output.display(), data.len());
        }
        DeviceCommand::Pull { to } => pull(to)?,
    }
    Ok(())
}

fn probe() -> anyhow::Result<aim_link::Beacon> {
    aim_link::probe(AP_ADDRESS, Duration::from_secs(3)).context(
        "no MyChron answered at 10.0.0.1. Is its WiFi on, and is this computer on its AiM-MYC… network?",
    )
}

/// USB when a MyChron 6 is plugged in, else its WiFi.
fn connect() -> anyhow::Result<Link> {
    Link::connect().context("couldn't open a connection to the MyChron (USB, or WiFi at 10.0.0.1:2000)")
}

fn pull(to: Option<PathBuf>) -> anyhow::Result<()> {
    let logger_id = if aim_link::usb::is_present() {
        aim_link::usb::UsbLogger::open().and_then(|mut l| l.logger_id()).context("a MyChron is plugged in by USB but didn't answer")?
    } else {
        probe()?.logger_id
    };
    let id = logger_id.map(|id| id.to_string()).unwrap_or_else(|| "unknown".into());
    let dir = match to {
        Some(dir) => dir,
        None => dirs_home()?.join("Mychronix").join("sessions").join(&id),
    };
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let mut logger = connect()?;
    let listing = logger.list(aim_link::SESSIONS_DIR).context("listing sessions")?;
    let sessions: Vec<_> = listing.iter().filter(|e| e.is_session()).collect();
    // Each session has a small `.xri` companion holding its summary; keep it alongside.
    let mut wanted: Vec<_> = listing.iter().filter(|e| e.is_session() || e.name.to_ascii_lowercase().ends_with(".xri")).collect();
    wanted.sort_by(|a, b| a.name.cmp(&b.name));
    println!("Logger {id}: {} sessions; saving to {}", sessions.len(), dir.display());
    let (mut saved, mut skipped) = (0, 0);
    for entry in wanted {
        // The name comes from the logger: never let it point outside this folder.
        if !aim_link::is_safe_file_name(&entry.name) {
            eprintln!("Skipping a file with an unusable name: {:?}", entry.name);
            continue;
        }
        let target = dir.join(&entry.name);
        if already_saved(&target, entry.size) {
            skipped += 1;
            continue;
        }
        let path = format!("{}/{}", aim_link::SESSIONS_DIR, entry.name);
        let data = logger.read_file(&path, progress_bar(&entry.name)).with_context(|| format!("reading {path}"))?;
        eprintln!();
        // Write to a temporary name first so an interrupted download never looks complete.
        let partial = target.with_extension("part");
        std::fs::write(&partial, &data)?;
        std::fs::rename(&partial, &target)?;
        saved += 1;
    }
    println!("Saved {saved} new files, {skipped} already here.");
    Ok(())
}

fn already_saved(path: &Path, size: u64) -> bool {
    std::fs::metadata(path).map(|m| m.len() == size).unwrap_or(false)
}

fn dirs_home() -> anyhow::Result<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).context("HOME is not set")
}

fn progress_bar(label: &str) -> impl FnMut(u64, u64) + '_ {
    move |done, total| {
        // No total yet: nothing left to wait for.
        let pct = (done * 100).checked_div(total).unwrap_or(100);
        eprint!("\r{label:<24} {pct:>3}%  {:>9} / {total} bytes", done);
        let _ = std::io::stderr().flush();
    }
}

fn local_sessions(dir: Option<PathBuf>) -> anyhow::Result<()> {
    let dirs = match dir {
        Some(d) => vec![d],
        None => {
            let root = dirs_home()?.join("Mychronix").join("sessions");
            let mut found: Vec<_> = std::fs::read_dir(&root)
                .with_context(|| format!("reading {}", root.display()))?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            found.sort();
            found
        }
    };
    for dir in dirs {
        let mut rows = Vec::new();
        let mut files: Vec<_> = std::fs::read_dir(&dir)?.flatten().map(|e| e.path()).collect();
        files.sort();
        for path in files.iter().filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("xri"))) {
            let mut summary = aim_xrk::parse_xri(&std::fs::read(path)?).with_context(|| format!("reading {}", path.display()))?;
            summary.file = path.with_extension("xrz").file_name().map(|n| n.to_string_lossy().into_owned());
            rows.push(summary);
        }
        println!("{}  ({} sessions)", dir.display(), rows.len());
        print_summaries(&rows);
        println!();
    }
    Ok(())
}

fn print_summaries(rows: &[aim_xrk::SessionSummary]) {
    println!("{:>4}  {:<10}  {:<8}  {:<10}  {:>4}  {:>9}  {:>9}  {:>6}  {:>6}", "#", "Date", "Start", "Track", "Laps", "Best", "On track", "RPM", "Speed");
    for s in rows {
        let dash = || "—".to_string();
        let best = match (s.best_lap_ms, s.best_lap_number) {
            (Some(ms), _) => format_lap_time(ms as i64),
            _ => dash(),
        };
        let track = s.track.clone().filter(|_| s.has_track()).unwrap_or_else(|| "no track".into());
        println!(
            "{:>4}  {:<10}  {:<8}  {:<10}  {:>4}  {:>9}  {:>9}  {:>6}  {:>6}",
            s.number.map(|n| n.to_string()).unwrap_or_else(dash),
            s.date.map(|d| d.to_string()).unwrap_or_else(dash),
            s.start_time.clone().unwrap_or_else(dash),
            track,
            s.laps.map(|n| n.to_string()).unwrap_or_else(dash),
            best,
            s.duration_ms.map(|ms| format_lap_time(ms as i64)).unwrap_or_else(dash),
            s.max_rpm.map(|n| n.to_string()).unwrap_or_else(dash),
            s.max_speed.map(|v| format!("{v:.1}")).unwrap_or_else(dash),
        );
    }
}
