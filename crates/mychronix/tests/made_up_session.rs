//! A made-up MyChron 4 session, put where a download would put it, read back
//! through the assistant server (`mychronix --mcp`) as an AI app would: the
//! library, the lap rules and the channels found by what they measure, end to
//! end, with no real data needed.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// A run as the data key saves it (`.mc4`): five laps of 30.00, 29.50,
/// 29.00, 29.40 and 31.00 s, the engine at 100 ms steps peaking once a lap.
fn run() -> Vec<u8> {
    let mut h = vec![0u8; 256];
    h[0..2].copy_from_slice(&7u16.to_le_bytes());
    h[0x0E] = 0x02; // the temperature input on
    h[0x76..0x78].copy_from_slice(&[0x59, 0x32]); // firmware
    h[0x78..0x7C].copy_from_slice(&123456u32.to_le_bytes()); // logger
    h[0x7C..0x7E].copy_from_slice(&106u16.to_le_bytes()); // a MyChron 4
    h[0x9A..0xA1].copy_from_slice(&[0x00, 0x30, 0x09, 0x19, 0x05, 0x09, 0x26]); // 09/19/2026 09:30:00
    let laps_cs = [3000u32, 2950, 2900, 2940, 3100];
    let laps: Vec<u8> = laps_cs
        .iter()
        .flat_map(|cs| {
            let mut r = vec![0xFFu8; 64];
            r[4..8].copy_from_slice(&cs.to_le_bytes());
            r
        })
        .collect();
    let frames: u32 = laps_cs.iter().sum::<u32>() / 10;
    let mut words: Vec<u16> = Vec::new();
    for f in 0..frames {
        // Through each 30 s lap: 6000 rpm out of the corners, 9000 at the end of the straight.
        let phase = (f % 300) as f64 / 300.0 * std::f64::consts::TAU;
        words.extend([(7500.0 - 1500.0 * phase.cos()) as u16, 0xCFFF]);
        if f % 10 == 0 {
            words.extend([2400, 1500]); // battery, temperature
        }
    }
    words.extend([0xD202, 1, 200, 0, 0, 0x1234]);
    let data: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let mut file = b"MC4R".to_vec();
    file.extend(1u32.to_le_bytes());
    for part in [&h, &laps, &data] {
        file.extend((part.len() as u32).to_le_bytes());
        file.extend(part);
    }
    file
}

/// The assistant server, talked to over its standard input and output.
struct Server {
    child: std::process::Child,
    out: BufReader<std::process::ChildStdout>,
    id: u32,
}

impl Server {
    fn start(home: &std::path::Path) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mychronix"))
            .arg("--mcp")
            .env_clear()
            .env("HOME", home)
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the app runs");
        let out = BufReader::new(child.stdout.take().unwrap());
        let mut s = Server { child, out, id: 0 };
        s.call(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}),
        );
        s
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let line = json!({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params});
        writeln!(self.child.stdin.as_mut().unwrap(), "{line}").unwrap();
        let mut reply = String::new();
        self.out.read_line(&mut reply).unwrap();
        serde_json::from_str(&reply).unwrap()
    }

    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let r = self.call("tools/call", json!({"name": name, "arguments": arguments}));
        let text = r["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: {r}"));
        serde_json::from_str(text).unwrap_or_else(|_| panic!("{name}: {text}"))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn a_made_up_session_reads_back_through_the_assistant_server() {
    let home = std::env::temp_dir().join(format!("mychronix-made-up-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let dir = home.join("Mychronix/sessions/123456");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("20260919-093000_007.mc4"), run()).unwrap();
    // The assistant server is off until it's turned on in Settings.
    std::fs::write(home.join("Mychronix/settings.json"), r#"{"mcp": true}"#).unwrap();

    let mut server = Server::start(&home);
    let list = server.tool("list_sessions", json!({}));
    let sessions = list["sessions"]
        .as_array()
        .unwrap_or_else(|| panic!("{list}"));
    assert_eq!(sessions.len(), 1, "{list}");
    let session = server.tool("get_session", json!({"session": sessions[0]["session"]}));
    assert_eq!(session["logger"], "MyChron 4");
    assert_eq!(
        (session["date"].as_str(), session["start"].as_str()),
        (Some("2026-09-19"), Some("09:30"))
    );
    // The MyChron 4 calls it Engine; it's found as the RPM, as a MyChron 6's would be.
    let channels = session["channels"].as_array().unwrap();
    assert!(channels.iter().any(|c| c == "RPM"), "{channels:?}");
    let laps = session["laps"]
        .as_array()
        .unwrap_or_else(|| panic!("{session}"));
    let times: Vec<f64> = laps.iter().map(|l| l["time_s"].as_f64().unwrap()).collect();
    assert_eq!(times, [30.0, 29.5, 29.0, 29.4, 31.0]);
    // The out-lap and in-lap don't count; the best is lap 3.
    let counts: Vec<bool> = laps
        .iter()
        .map(|l| l["counts"].as_bool().unwrap())
        .collect();
    assert_eq!(counts, [false, true, true, true, false]);
    let best: Vec<u64> = laps
        .iter()
        .filter(|l| l["best"] == true)
        .map(|l| l["lap"].as_u64().unwrap())
        .collect();
    assert_eq!(best, [3]);
    for l in laps {
        let (lo, hi) = (
            l["min_rpm"].as_f64().unwrap(),
            l["max_rpm"].as_f64().unwrap(),
        );
        assert!(
            (6000.0..=9000.0).contains(&lo) && (6000.0..=9000.0).contains(&hi) && lo < hi,
            "{l}"
        );
    }
    drop(server);
    std::fs::remove_dir_all(&home).unwrap();
}
