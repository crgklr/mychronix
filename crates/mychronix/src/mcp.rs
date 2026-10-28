//! Mychronix Performance Engineer: an MCP server, so an AI assistant on this
//! computer (Claude Desktop and other apps that speak the Model Context
//! Protocol) can read the Mychronix library and coach from it.
//!
//! Started by the assistant app as `mychronix --mcp`: no window, JSON-RPC
//! 2.0 over stdin and stdout, one message per line (MCP's "stdio"
//! transport). Read-only: it never writes the library and never talks to a
//! MyChron. When it's turned off in Settings, every tool says so.
//!
//! The numbers come from the same code the screens use (the corner model,
//! lap-by-lap statistics, setups), in the units chosen in Settings.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

use crate::corner_model::{CornerModel, Slowing, Tone};
use crate::tracks::Library;

const PROTOCOL: &str = "2025-06-18";

pub fn serve() {
    log::info!("Mychronix Performance Engineer (MCP) starting");
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&msg),
            Err(e) => Some(error(Value::Null, -32700, &format!("not JSON: {e}"))),
        };
        if let Some(reply) = reply
            && writeln!(out, "{reply}").and_then(|_| out.flush()).is_err()
        {
            break;
        }
    }
    log::info!("Mychronix Performance Engineer (MCP) stopping");
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// One message in, at most one reply out (notifications get none).
fn handle(msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(json!({}));
    let id = id?; // a notification ("notifications/initialized" and the like)
    let result = match method {
        "initialize" => {
            let version = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL).to_string();
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "mychronix", "title": "Mychronix Performance Engineer", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let outcome = if crate::settings::get().mcp {
                crate::jobs::guard(&format!("MCP tool {name}"), || call(name, &args)).unwrap_or_else(|| Err("Mychronix hit an error reading that; it's in Mychronix's log.".into()))
            } else {
                Err("The Mychronix Performance Engineer is turned off. Turn it on in Mychronix, under Settings.".into())
            };
            match outcome {
                Ok(v) => json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }], "structuredContent": v, "isError": false }),
                Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
            }
        }
        "resources/list" => json!({ "resources": [] }),
        "prompts/list" => json!({ "prompts": [] }),
        _ => return Some(error(id, -32601, &format!("unknown method {method}"))),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

const INSTRUCTIONS: &str = "Mychronix holds a kart racer's sessions downloaded from AiM MyChron data loggers. \
Start with list_sessions to find sessions (by driver, track or date), then get_session for its laps, setup and conditions, \
get_corners for where a lap gains and loses time corner by corner, compare_laps to put two laps side by side, \
get_channel for one channel through a lap, and race_days for progress over time. \
A corner runs from its braking point to the next one's, so its time includes the exit. \
'Best of every corner' adds up the fastest pass through each corner from the session's laps that count. \
Speeds and distances are in the units given in each answer. Be specific: name the corner, the lap and the numbers.";

fn tools() -> Value {
    let session = json!({ "type": "string", "description": "A session id from list_sessions" });
    let lap = json!({ "type": "integer", "description": "Lap number as the logger numbers it (the out-lap is 1). Leave out for the session's best lap." });
    json!([
        {
            "name": "list_sessions",
            "title": "List sessions",
            "description": "The library's sessions, newest first: driver, kart, track, date, laps and best lap. Filter by driver, track or date.",
            "inputSchema": { "type": "object", "properties": {
                "driver": { "type": "string", "description": "Part of a driver's name" },
                "track": { "type": "string", "description": "Part of a track's name" },
                "since": { "type": "string", "description": "Only sessions on or after this date, YYYY-MM-DD" },
                "until": { "type": "string", "description": "Only sessions on or before this date, YYYY-MM-DD" },
                "timed_only": { "type": "boolean", "description": "Leave out sessions without a timed lap (warm-ups)" },
                "limit": { "type": "integer", "description": "At most this many (default 40)" }
            } },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "get_session",
            "title": "Get a session",
            "description": "One session: every lap's time (and whether it counts as a racing lap), the kart's setup, the weather, patterns across the laps (consistency, trends) and peak readings per lap.",
            "inputSchema": { "type": "object", "properties": { "session": session }, "required": ["session"] },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "get_corners",
            "title": "Corner by corner",
            "description": "A lap corner by corner against the best the driver has driven each corner this session: braking point, entry and apex speed, speed down the next straight, braking time and peak, trail braking, steering, the corner's time and time lost, and what stands out. Also the best of every corner (ideal lap). Needs GPS.",
            "inputSchema": { "type": "object", "properties": { "session": session, "lap": lap }, "required": ["session"] },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "compare_laps",
            "title": "Compare two laps",
            "description": "Two laps corner by corner (from one session, or two sessions at the same track): where each gains and loses, with braking points and speeds side by side.",
            "inputSchema": { "type": "object", "properties": {
                "session": session, "lap": lap,
                "other_session": { "type": "string", "description": "The other lap's session (leave out for the same session)" },
                "other_lap": { "type": "integer", "description": "The other lap's number (leave out for that session's best lap)" }
            }, "required": ["session"] },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "get_channel",
            "title": "One channel through a lap",
            "description": "One recorded channel (e.g. RPM, GPS Speed, Exhaust Temp, Forward g, Steering) through a lap, by distance when there's GPS (else by time), thinned to about 200 points.",
            "inputSchema": { "type": "object", "properties": {
                "session": session, "lap": lap,
                "channel": { "type": "string", "description": "Channel name, as in get_session's channel list" }
            }, "required": ["session", "channel"] },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "race_days",
            "title": "Race days",
            "description": "Race days (a date at a track) with each driver's best lap, laps and sessions that day, oldest first: progress over a season.",
            "inputSchema": { "type": "object", "properties": {
                "driver": { "type": "string", "description": "Part of a driver's name" },
                "track": { "type": "string", "description": "Part of a track's name" }
            } },
            "annotations": { "readOnlyHint": true }
        }
    ])
}

fn call(name: &str, args: &Value) -> Result<Value, String> {
    let lib = Library::load();
    let profiles = crate::profiles::Profiles::load();
    match name {
        "list_sessions" => list_sessions(&lib, &profiles, args),
        "get_session" => get_session(&lib, &profiles, args),
        "get_corners" => get_corners(&lib, &profiles, args),
        "compare_laps" => compare_laps(&lib, &profiles, args),
        "get_channel" => get_channel(&lib, &profiles, args),
        "race_days" => race_days(&lib, &profiles, args),
        _ => Err(format!("No tool called {name}")),
    }
}

// ---------------------------------------------------------------------------
// Helpers

fn units() -> Value {
    json!({ "speed": crate::units::speed_unit(), "distance": crate::units::distance_unit(), "temperature": crate::units::temp_unit(), "time": "s" })
}

fn round(v: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (v * f).round() / f
}

fn speed(kmh: f64) -> f64 {
    round(crate::units::speed(kmh), 1)
}

fn distance(m: f64) -> f64 {
    round(crate::units::distance(m), 0)
}

/// A session's id: its path under the sessions folder.
fn session_id(lib: &Library, i: usize) -> String {
    let root = crate::paths::sessions_dir().unwrap_or_default();
    let p = &lib.sessions[i].path;
    p.strip_prefix(&root).unwrap_or(p).display().to_string()
}

fn find_session(lib: &Library, id: &str) -> Result<usize, String> {
    lib.sessions.iter().enumerate().position(|(i, _)| session_id(lib, i) == id).ok_or_else(|| format!("No session {id}; list_sessions gives the ids"))
}

fn date_text(d: Option<aim_xrk::summary::Date>) -> Option<String> {
    d.map(crate::events::iso)
}

/// Opens session `i`, with speed worked out from RPM when there's no GPS (as the app does).
fn open(lib: &Library, profiles: &crate::profiles::Profiles, i: usize) -> Result<aim_xrk::Session, String> {
    let s = &lib.sessions[i];
    let mut session = crate::jobs::open_session(&s.path).map_err(|e| format!("Couldn't open that session: {e}"))?;
    if session.channel("GPS Speed").is_none() {
        let resolved = crate::setup::resolve(lib, &crate::setup::Baselines::load());
        let kart = s.kart.and_then(|id| profiles.karts.iter().find(|k| k.id == id));
        let engine = kart.map(|k| k.engine.clone()).unwrap_or_default();
        let clutch = kart.and_then(|k| crate::setup::number(&k.clutch_rpm));
        if let Some(ch) = resolved.get(i).and_then(|r| crate::setup::speed_from_rpm(&session, &r.setup, &engine, clutch)) {
            session.channels.push(ch);
        }
    }
    Ok(session)
}

/// The lap asked for (by number), else the best.
fn pick_lap(session: &aim_xrk::Session, number: Option<i64>) -> Result<aim_xrk::Lap, String> {
    match number {
        Some(n) => session.laps.iter().find(|l| l.number as i64 == n).copied().ok_or_else(|| format!("No lap {n}; this session has laps 1 to {}", session.laps.len())),
        None => session.best_lap(crate::app::MIN_LAP_MS).copied().ok_or_else(|| "This session has no complete timed lap".to_string()),
    }
}

/// The MyChron's model ("MyChron 6"): as its summary says, else as learned
/// when it was connected (a MyChron 5/6 summary doesn't name its model).
fn logger_model(s: &crate::tracks::LocalSession, profiles: &crate::profiles::Profiles) -> Option<String> {
    s.summary.model.clone().or_else(|| profiles.loggers.get(&s.logger).map(|l| l.model.clone()).filter(|m| !m.is_empty()))
}

fn kart_name(s: &crate::tracks::LocalSession, profiles: &crate::profiles::Profiles) -> Option<String> {
    s.kart.and_then(|id| profiles.karts.iter().find(|k| k.id == id)).map(|k| k.name.clone())
}

// ---------------------------------------------------------------------------
// Tools

fn list_sessions(lib: &Library, profiles: &crate::profiles::Profiles, args: &Value) -> Result<Value, String> {
    let want = |key: &str| args.get(key).and_then(Value::as_str).map(str::to_lowercase).filter(|s| !s.is_empty());
    let (driver, track, since, until) = (want("driver"), want("track"), want("since"), want("until"));
    let timed_only = args.get("timed_only").and_then(Value::as_bool).unwrap_or(false);
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(40) as usize;
    let mut rows: Vec<(String, Value)> = Vec::new();
    for (i, s) in lib.sessions.iter().enumerate() {
        let d = crate::app::session_driver(s, profiles);
        let t = crate::app::session_track(lib, i).unwrap_or_default();
        let date = date_text(s.summary.date).unwrap_or_default();
        if driver.as_ref().is_some_and(|w| !d.to_lowercase().contains(w))
            || track.as_ref().is_some_and(|w| !t.to_lowercase().contains(w))
            || since.as_ref().is_some_and(|w| date.as_str() < w.as_str())
            || until.as_ref().is_some_and(|w| date.as_str() > w.as_str())
            || (timed_only && s.summary.best_lap_ms.is_none())
        {
            continue;
        }
        rows.push((
            s.start(),
            json!({
                "session": session_id(lib, i),
                "number": s.summary.number,
                "date": date,
                "start": s.summary.start_time.as_deref().map(crate::events::clock_time),
                "driver": d,
                "kart": kart_name(s, profiles),
                "track": t,
                "championship": s.meta.as_ref().map(|m| m.championship.clone()).filter(|c| !c.is_empty()),
                "laps": s.summary.laps,
                "best_lap_s": s.summary.best_lap_ms.map(|ms| ms as f64 / 1000.0),
                "max_rpm": s.summary.max_rpm,
                "logger": logger_model(s, profiles),
            }),
        ));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    let total = rows.len();
    Ok(json!({ "sessions": rows.into_iter().take(limit).map(|r| r.1).collect::<Vec<_>>(), "matching": total }))
}

fn get_session(lib: &Library, profiles: &crate::profiles::Profiles, args: &Value) -> Result<Value, String> {
    let i = find_session(lib, args.get("session").and_then(Value::as_str).unwrap_or(""))?;
    let s = &lib.sessions[i];
    let session = open(lib, profiles, i)?;
    let slow = crate::app::slow_laps(&session);
    let best = session.best_lap(crate::app::MIN_LAP_MS).copied();
    let peak = |name: &str, lap: &aim_xrk::Lap| {
        let c = crate::report::channel(&session, name)?;
        let (lo, hi) = (c.times_ms.partition_point(|&t| t < lap.start_ms), c.times_ms.partition_point(|&t| t <= lap.end_ms));
        c.values[lo..hi].iter().copied().filter(|v| v.is_finite()).reduce(f64::max)
    };
    let low = |name: &str, lap: &aim_xrk::Lap| {
        let c = crate::report::channel(&session, name)?;
        let (lo, hi) = (c.times_ms.partition_point(|&t| t < lap.start_ms), c.times_ms.partition_point(|&t| t <= lap.end_ms));
        c.values[lo..hi].iter().copied().filter(|v| v.is_finite() && *v > 1.0).reduce(f64::min)
    };
    let laps: Vec<Value> = session
        .laps
        .iter()
        .enumerate()
        .map(|(k, l)| {
            json!({
                "lap": l.number,
                "time_s": l.duration_ms() as f64 / 1000.0,
                "counts": !slow[k],
                "best": Some(*l) == best,
                "max_rpm": peak("RPM", l).map(|v| v.round()),
                "min_rpm": low("RPM", l).map(|v| v.round()),
                "top_speed": peak("GPS Speed", l).map(speed),
                "slowest_speed": low("GPS Speed", l).map(speed),
                "max_exhaust_temp": peak("Exhaust Temp", l).map(|c| round(crate::units::temp(c), 0)),
            })
        })
        .collect();
    let notes: Vec<Value> = aim_xrk::notes::session_notes(&session)
        .into_iter()
        .map(|n| match n {
            aim_xrk::notes::Note::LapTrend { per_lap_s, first_lap, last_lap } => json!({ "lap_time_trend_s_per_lap": round(per_lap_s, 3), "over_laps": [first_lap, last_lap] }),
            aim_xrk::notes::Note::Consistency { sd_s, laps } => json!({ "lap_time_std_dev_s": round(sd_s, 3), "racing_laps": laps }),
            aim_xrk::notes::Note::TempTrend { channel, from, to, per_lap, .. } => json!({ "temperature_trend": channel, "from_c": round(from, 0), "to_c": round(to, 0), "per_lap_c": round(per_lap, 1) }),
        })
        .collect();
    let setup = crate::setup::resolve(lib, &crate::setup::Baselines::load()).get(i).map(|r| json!({ "setup": r.setup, "changed_from_previous": r.changes }));
    let channels: Vec<&str> = session.channels.iter().filter(|c| c.values.len() > 1).map(|c| c.name.as_str()).collect();
    Ok(json!({
        "session": session_id(lib, i),
        "date": date_text(s.summary.date),
        "start": s.summary.start_time.as_deref().map(crate::events::clock_time),
        "driver": crate::app::session_driver(s, profiles),
        "kart": kart_name(s, profiles),
        "track": crate::app::session_track(lib, i),
        "logger": logger_model(s, profiles).or_else(|| session.info.logger_model_id.and_then(aim_xrk::mychron_model).map(str::to_string)),
        "gps": session.gps.as_ref().is_some_and(|g| g.len() > 10),
        "speed_calculated_from_rpm": session.channel("GPS Speed").is_none() && session.channel(crate::setup::SPEED_FROM_RPM).is_some(),
        "weather": s.weather.as_ref().map(|w| w.summary()),
        "comment": s.meta.as_ref().map(|m| m.comment.clone()).filter(|c| !c.is_empty()),
        "units": units(),
        "laps": laps,
        "patterns": notes,
        "kart_setup": setup,
        "channels": channels,
    }))
}

/// The corner model for session `i`, and the trace of `lap` in it.
fn corners_of(lib: &Library, profiles: &crate::profiles::Profiles, i: usize, lap: Option<i64>) -> Result<(aim_xrk::Session, CornerModel, usize), String> {
    let session = open(lib, profiles, i)?;
    let best = session.best_lap(crate::app::MIN_LAP_MS).copied().ok_or("This session has no complete timed lap")?;
    let counts: Vec<bool> = crate::app::slow_laps(&session).into_iter().map(|s| !s).collect();
    let model = CornerModel::build(&session, &best, &counts).ok_or("Corners need GPS (this session has none, or no corner to brake for)")?;
    let lap = pick_lap(&session, lap)?;
    let index = session.laps.iter().position(|l| *l == lap).unwrap_or(0);
    let trace = model.trace_of(index).ok_or_else(|| format!("Lap {} doesn't go all the way round (an out-lap or in-lap)", lap.number))?;
    Ok((session, model, trace))
}

fn corner_row(model: &CornerModel, trace: usize, k: usize) -> Value {
    let a = &model.stats[trace][k];
    let b = &model.stats[model.best_of[k]][k];
    let share = |s: Slowing| a.slowing.iter().filter(|&&x| x == s).count() as f64 * crate::corner_model::STEP_M;
    json!({
        "corner": k + 1,
        "time_s": round(a.time_s, 3),
        "lost_s": round(model.loss(trace, k), 3),
        "best_time_s": round(b.time_s, 3),
        "best_on_lap": model.traces[model.best_of[k]].number,
        "brakes_at": distance(a.brake_m),
        "entry_speed": speed(a.entry_kmh),
        "apex_speed": speed(a.min_kmh),
        "speed_down_next_straight": speed(a.exit_kmh),
        "slowing": { "distance": distance(a.slowing_m()), "on_brakes": distance(share(Slowing::Brake) + share(Slowing::Trail)), "lift_or_slide": distance(share(Slowing::Lift)) },
        "on_brakes_s": round(a.brake_s, 2),
        "peak_braking_g": round(a.peak_g, 2),
        "trail_braking_s": a.trail_s.map(|t| round(t, 2)),
        "steering": a.steering.map(|(angle, corr)| json!({ "largest_angle_deg": angle.round(), "corrections": corr })),
        "stands_out": model.notes(trace, k).into_iter().map(|(t, text)| json!({ "note": text, "kind": match t { Tone::Good => "good", Tone::Attention => "attention", Tone::Plain => "info" } })).collect::<Vec<_>>(),
    })
}

fn get_corners(lib: &Library, profiles: &crate::profiles::Profiles, args: &Value) -> Result<Value, String> {
    let i = find_session(lib, args.get("session").and_then(Value::as_str).unwrap_or(""))?;
    let (session, model, trace) = corners_of(lib, profiles, i, args.get("lap").and_then(Value::as_i64))?;
    let lap_s: f64 = model.stats[trace].iter().map(|s| s.time_s).sum();
    Ok(json!({
        "session": session_id(lib, i),
        "lap": model.traces[trace].number,
        "lap_counts_toward_bests": model.counts[trace],
        "best_lap": model.traces[model.best].number,
        "best_of_every_corner_s": round(model.ideal_s(), 3),
        "left_on_this_lap_s": round((lap_s - model.ideal_s()).max(0.0), 3),
        "imu_braking": model.has_trail,
        "speed_calculated_from_rpm": session.channel("GPS Speed").is_none(),
        "units": units(),
        "how_corners_are_measured": "Each corner runs from its braking point to the next corner's braking point; brakes_at is the distance from the start of the lap.",
        "corners": (0..model.segments.len()).map(|k| corner_row(&model, trace, k)).collect::<Vec<_>>(),
    }))
}

fn compare_laps(lib: &Library, profiles: &crate::profiles::Profiles, args: &Value) -> Result<Value, String> {
    let i = find_session(lib, args.get("session").and_then(Value::as_str).unwrap_or(""))?;
    let j = match args.get("other_session").and_then(Value::as_str) {
        Some(id) => find_session(lib, id)?,
        None => i,
    };
    let (_, a_model, a) = corners_of(lib, profiles, i, args.get("lap").and_then(Value::as_i64))?;
    let (_, b_model, b) = corners_of(lib, profiles, j, args.get("other_lap").and_then(Value::as_i64))?;
    if a_model.segments.len() != b_model.segments.len() {
        return Err(format!(
            "These laps don't have the same corners ({} against {}): different tracks or layouts can't be compared corner by corner.",
            a_model.segments.len(),
            b_model.segments.len()
        ));
    }
    let corners: Vec<Value> = (0..a_model.segments.len())
        .map(|k| {
            let (x, y) = (&a_model.stats[a][k], &b_model.stats[b][k]);
            json!({
                "corner": k + 1,
                "time_s": [round(x.time_s, 3), round(y.time_s, 3)],
                "difference_s": round(x.time_s - y.time_s, 3),
                "brakes_at": [distance(x.brake_m), distance(y.brake_m)],
                "entry_speed": [speed(x.entry_kmh), speed(y.entry_kmh)],
                "apex_speed": [speed(x.min_kmh), speed(y.min_kmh)],
                "speed_down_next_straight": [speed(x.exit_kmh), speed(y.exit_kmh)],
                "on_brakes_s": [round(x.brake_s, 2), round(y.brake_s, 2)],
                "trail_braking_s": [x.trail_s.map(|t| round(t, 2)), y.trail_s.map(|t| round(t, 2))],
            })
        })
        .collect();
    let total = |m: &CornerModel, t: usize| m.stats[t].iter().map(|s| s.time_s).sum::<f64>();
    Ok(json!({
        "laps": [
            { "session": session_id(lib, i), "lap": a_model.traces[a].number, "time_s": round(total(&a_model, a), 3) },
            { "session": session_id(lib, j), "lap": b_model.traces[b].number, "time_s": round(total(&b_model, b), 3) }
        ],
        "note": "Pairs are [first lap, second lap]; difference_s is first minus second (negative: the first lap was quicker there).",
        "units": units(),
        "corners": corners,
    }))
}

fn get_channel(lib: &Library, profiles: &crate::profiles::Profiles, args: &Value) -> Result<Value, String> {
    let i = find_session(lib, args.get("session").and_then(Value::as_str).unwrap_or(""))?;
    let session = open(lib, profiles, i)?;
    let name = args.get("channel").and_then(Value::as_str).unwrap_or("");
    let mut channel = crate::report::channel(&session, name).cloned().ok_or_else(|| format!("No channel {name}; get_session lists this session's channels"))?;
    crate::units::convert_channel(&mut channel);
    let lap = pick_lap(&session, args.get("lap").and_then(Value::as_i64))?;
    let (lo, hi) = (channel.times_ms.partition_point(|&t| t < lap.start_ms), channel.times_ms.partition_point(|&t| t <= lap.end_ms));
    if hi <= lo {
        return Err("That channel has no readings on that lap".into());
    }
    let step = ((hi - lo) / 200).max(1);
    let profile = crate::compare::DistanceProfile::from_gps(&session, &lap);
    let points: Vec<Value> = (lo..hi)
        .step_by(step)
        .map(|k| {
            let t = channel.times_ms[k];
            let v = round(channel.values[k], 2);
            match &profile {
                Some(p) => json!([distance(p.dist_at(t as f64)), v]),
                None => json!([round((t - lap.start_ms) as f64 / 1000.0, 2), v]),
            }
        })
        .collect();
    Ok(json!({
        "session": session_id(lib, i),
        "lap": lap.number,
        "channel": channel.name,
        "units": channel.units,
        "calculated_from_rpm": crate::app::is_calculated(&channel.name),
        "points_are": if profile.is_some() { format!("[distance from the start of the lap ({}), value]", crate::units::distance_unit()) } else { "[seconds into the lap, value]".to_string() },
        "points": points,
    }))
}

fn race_days(lib: &Library, profiles: &crate::profiles::Profiles, args: &Value) -> Result<Value, String> {
    let want = |key: &str| args.get(key).and_then(Value::as_str).map(str::to_lowercase).filter(|s| !s.is_empty());
    let (driver, track) = (want("driver"), want("track"));
    // (date, track, driver) -> (sessions, laps, best ms)
    type Day = (String, String, String);
    let mut days: std::collections::BTreeMap<Day, (usize, u32, Option<u64>)> = Default::default();
    for (i, s) in lib.sessions.iter().enumerate() {
        let (Some(date), d, t) = (date_text(s.summary.date), crate::app::session_driver(s, profiles), crate::app::session_track(lib, i).unwrap_or_default()) else { continue };
        if driver.as_ref().is_some_and(|w| !d.to_lowercase().contains(w)) || track.as_ref().is_some_and(|w| !t.to_lowercase().contains(w)) {
            continue;
        }
        let e = days.entry((date, t, d)).or_default();
        e.0 += 1;
        e.1 += s.summary.laps.unwrap_or(0);
        if let Some(b) = s.summary.best_lap_ms {
            e.2 = Some(e.2.map_or(b, |x| x.min(b)));
        }
    }
    Ok(json!({
        "race_days": days.into_iter().map(|((date, track, driver), (sessions, laps, best))| json!({
            "date": date, "track": track, "driver": driver, "sessions": sessions, "laps": laps, "best_lap_s": best.map(|ms| ms as f64 / 1000.0)
        })).collect::<Vec<_>>()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaks_mcp() {
        let init = handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } })).unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "mychronix");
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        // Notifications get no reply.
        assert!(handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        let list = handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).unwrap();
        let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["list_sessions", "get_session", "get_corners", "compare_laps", "get_channel", "race_days"]);
        assert_eq!(handle(&json!({ "jsonrpc": "2.0", "id": 3, "method": "nope" })).unwrap()["error"]["code"], -32601);
    }
}
